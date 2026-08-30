// The canvas half of the shim: size, DPI, focus, fullscreen, keyboard, pointer,
// touch.
//
// Implements the JS side of `crcbl-shell`'s Web/canvas backend
// (`crates/crcbl-shell/src/web/mod.rs`). Every browser event becomes one
// `__crcbl_web_*` call; the backend queues it and the engine drains the queue
// once per frame from `Shell::pump`.
//
// TIMESTAMPS. Every input call carries the DOM event's own `event.timeStamp`,
// never the frame's. Quantising every event in a frame to one instant is
// exactly what makes a fast double-tap indistinguishable from a slow one, and
// the backend's docs call it out. `timeStamp` and `performance.now()` share the
// page's time origin, so the engine's `align_event_clock` turns one into the
// other with a single subtraction — which is why `__crcbl_web_frame` must be
// called once per `requestAnimationFrame` with `performance.now()`.
//
// STRINGS. `__crcbl_web_key` takes pointers, and a browser cannot invent an
// address inside wasm memory. `__crcbl_web_key_scratch_ptr` hands one out; the
// two strings a key event carries are written into it back to back.
//
// CURSOR. The other pointer axis, and polled by the same loop for the same
// reason: `Shell::set_cursor` publishes a CSS keyword — `none` when the engine
// wants the cursor hidden — and this shim writes it to `canvas.style.cursor`.
// Nothing here needs a gesture, so unlike the lock below the request always
// lands on the frame after it is made.
//
// POINTER LOCK. The engine asks for it and this shim takes it, because a
// browser grants `requestPointerLock` only from inside a user gesture and wasm
// never is — the same split `requestFullscreen` has below. The request is
// *polled*, not pushed: a browser artifact imports nothing (the engine ABI is
// exports-plus-polling, and `web/tools/check-exports.mjs` fails the build over
// one import), so `__crcbl_web_pointer_lock_wanted` is read once a frame and
// the answer comes back through `__crcbl_web_pointer_lock`.

import { readUtf8, writeUtf8 } from './wasm.js';

/** `event.ctrlKey`. */
export const STATE_CTRL = 1 << 0;
/** `event.shiftKey`. */
export const STATE_SHIFT = 1 << 1;
/** `event.altKey`. */
export const STATE_ALT = 1 << 2;
/** `event.metaKey`. */
export const STATE_SUPER = 1 << 3;
/** The positive edge: down, entered, focused. */
export const STATE_EDGE = 1 << 4;
/** `KeyboardEvent.repeat`. */
export const STATE_REPEAT = 1 << 5;

/**
 * A contact landed — a touch `pointerdown`.
 *
 * The four phases `__crcbl_web_touch` takes, and they are an enumeration rather
 * than bits in the `state` word above because a contact is in exactly one of
 * them: a mask would let this shim describe a finger that both began and ended.
 * They must match the `TOUCH_*` constants in `crates/crcbl-shell/src/web/mod.rs`.
 */
export const TOUCH_BEGAN = 0;
/** A contact moved — a touch `pointermove`. */
export const TOUCH_MOVED = 1;
/** A contact was lifted — a touch `pointerup`. */
export const TOUCH_ENDED = 2;
/** A contact was taken away — a touch `pointercancel`. */
export const TOUCH_CANCELLED = 3;

/**
 * Keys the page must not act on itself while the canvas has focus.
 *
 * Arrows and space scroll the document, and `/` opens quick-find in some
 * browsers. A game bound to them and a page that also scrolls is the single
 * most common browser-game complaint.
 */
const SWALLOWED = new Set([
  'ArrowLeft',
  'ArrowRight',
  'ArrowUp',
  'ArrowDown',
  'Space',
  'Tab',
  'Slash',
  // The browser's own F11 makes the *browser window* fullscreen, which is a
  // different thing from the canvas filling the screen and leaves the page
  // laid out exactly as it was. The demo binds F11 to its own toggle below.
  'F11',
]);

/**
 * Keys the page must not act on, but only when no Ctrl or Meta is held.
 *
 * `Backquote` is `crcbl::engine::CONSOLE_KEY`: bare, it opens the engine's
 * debug console in every demo, and everything typed at that console arrives as
 * a `keydown` the page has no business acting on. Held with Ctrl or Meta it is
 * a browser shortcut instead, and the engine deliberately leaves it alone —
 * `docs/plan/52-debug-console.md` decision 5, which is the same test
 * `Pending::observe` applies on the engine's own side. So the swallow has to
 * carry the same condition: unconditional would take the visitor's devtools
 * shortcut away, and none at all would let the page act on a keystroke the
 * console has already eaten.
 */
const SWALLOWED_BARE = new Set(['Backquote']);

/**
 * The key that asks for fullscreen. Must match `FULLSCREEN_KEY` in each
 * sample's `app.rs`.
 *
 * The gesture has to be handled *here* rather than in the engine: a browser
 * grants `requestFullscreen` only from inside a user-gesture handler, and the
 * engine reads a key one `requestAnimationFrame` after the `keydown` that
 * carried it — by which time the gesture is over and the promise rejects. So
 * the shim makes the call and reports the outcome through
 * `__crcbl_web_fullscreen`; the engine's own F11 handling records the *request*
 * and then reads back what actually happened.
 */
const FULLSCREEN_KEY = 'F11';

/**
 * @param {KeyboardEvent | MouseEvent | PointerEvent | WheelEvent} event
 * @returns {number}
 */
function modifiers(event) {
  return (
    (event.ctrlKey ? STATE_CTRL : 0) |
    (event.shiftKey ? STATE_SHIFT : 0) |
    (event.altKey ? STATE_ALT : 0) |
    (event.metaKey ? STATE_SUPER : 0)
  );
}

/**
 * Wires `canvas` to the wasm instance's shell backend.
 *
 * Call before `__crcbl_breakout_boot`: the backend must know its canvas id
 * before it opens, and the first size has to be on its way or start-up parks
 * waiting for one.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports the wasm instance's exports
 * @param {WebAssembly.Memory} options.memory
 * @param {HTMLCanvasElement} options.canvas
 * @param {number} options.canvasId a non-zero id for this canvas
 * @returns {{ syncSize: () => void, requestClose: () => void, dispose: () => void }}
 */
export function attachShell({ exports, memory, canvas, canvasId }) {
  // `crcbl-wgpu` builds a `WebWindowHandle(canvas_id)` and raw-window-handle
  // resolves it by querying `[data-raw-handle="<id>"]`. Without this attribute
  // the surface cannot be created and the failure is a bare "no surface".
  canvas.dataset.rawHandle = String(canvasId);
  // A canvas is not focusable by default, so it would never receive a key
  // event and the game would look dead.
  if (!canvas.hasAttribute('tabindex')) canvas.setAttribute('tabindex', '0');

  exports.__crcbl_web_canvas(canvasId);

  const scratchPtr = exports.__crcbl_web_key_scratch_ptr();
  const scratchCapacity = exports.__crcbl_web_key_scratch_capacity();

  let lastWidth = 0;
  let lastHeight = 0;
  let lastScale = 0;

  /**
   * Reports the canvas's size in physical pixels, if it changed.
   *
   * The backing store is sized in device pixels and the element is left to CSS,
   * which is what makes the render sharp on a HiDPI display instead of being
   * upscaled by the compositor. `devicePixelRatio` is passed through verbatim,
   * including values below 1 on a zoomed-out page — the backend clamps only
   * non-positive values, because a fractional scale is a real thing and
   * rounding it up is a wrong answer.
   *
   * `force` re-sends an unchanged size, and the page needs it exactly once.
   * Everything here has to be wired *before* the engine's window exists — the
   * backend must know its canvas id before it opens — but a `resize` that
   * arrives before the window is created has nowhere to go and the backend
   * drops it. Without a forced call after the window exists, the first size the
   * engine ever hears about would be the *second* time the canvas changed size,
   * which for a page that is never resized is never, and start-up would park
   * forever waiting for a configure.
   *
   * @param {boolean} [force]
   */
  function syncSize(force = false) {
    const scale = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    const width = Math.max(1, Math.round(rect.width * scale));
    const height = Math.max(1, Math.round(rect.height * scale));
    if (
      !force &&
      width === lastWidth &&
      height === lastHeight &&
      scale === lastScale
    )
      return;
    lastWidth = width;
    lastHeight = height;
    lastScale = scale;
    canvas.width = width;
    canvas.height = height;
    exports.__crcbl_web_resize(canvasId, width, height, scale);
  }

  /** @param {PointerEvent | MouseEvent | WheelEvent} event */
  function position(event) {
    const rect = canvas.getBoundingClientRect();
    const scale = window.devicePixelRatio || 1;
    return [
      (event.clientX - rect.left) * scale,
      (event.clientY - rect.top) * scale,
    ];
  }

  /**
   * How far the pointer moved, in the same device pixels `position` reports.
   *
   * `movementX`/`movementY` are CSS pixels like `clientX`/`clientY`, so they
   * take the same `devicePixelRatio` factor — a delta left unscaled would turn
   * the camera at half speed on a HiDPI display, which reads as a sensitivity
   * setting nobody set. Only meaningful under the lock, and the backend is what
   * decides that: it drops the delta while the pointer is free, where
   * `movementX` is a difference of accelerated positions rather than the raw
   * device motion `unadjustedMovement` asks for.
   *
   * `|| 0` because a synthesized event — a test double, a `dispatchEvent` from
   * the page — may carry no movement at all, and `undefined` crossing into wasm
   * as a delta is `NaN` in the camera.
   *
   * @param {PointerEvent} event
   */
  function movement(event) {
    const scale = window.devicePixelRatio || 1;
    return [(event.movementX || 0) * scale, (event.movementY || 0) * scale];
  }

  /** @param {KeyboardEvent} event */
  function onKey(event) {
    const down = event.type === 'keydown';
    // `code` is the physical key and is what the engine binds to; `key` is what
    // it produced and is only used for the keysym.
    const codeLen = writeUtf8(memory, scratchPtr, scratchCapacity, event.code);
    if (codeLen === null) return;
    const keyLen = writeUtf8(
      memory,
      scratchPtr + codeLen,
      scratchCapacity - codeLen,
      event.key
    );
    if (keyLen === null) return;
    exports.__crcbl_web_key(
      canvasId,
      scratchPtr,
      codeLen,
      scratchPtr + codeLen,
      keyLen,
      event.timeStamp,
      modifiers(event) |
        (down ? STATE_EDGE : 0) |
        (event.repeat ? STATE_REPEAT : 0)
    );
    const bare = !event.ctrlKey && !event.metaKey;
    if (SWALLOWED.has(event.code) || (bare && SWALLOWED_BARE.has(event.code)))
      event.preventDefault();
    // After the forward, so the engine sees the press either way, and only on
    // a real press: `keyup` would toggle straight back, and a held key's
    // repeats would toggle once per repeat.
    if (down && !event.repeat && event.code === FULLSCREEN_KEY)
      toggleFullscreen();
  }

  /**
   * Puts the canvas in or out of the document's fullscreen element.
   *
   * Both calls return promises that reject when the browser refuses — no
   * gesture, a `fullscreen` permission policy that excludes the frame — and a
   * rejection is not an error the page can do anything about. It is swallowed
   * rather than thrown because the engine learns the truth from
   * `fullscreenchange`, which simply does not fire: `mode_request_honoured()`
   * stays false and the sample is expected to say so.
   */
  function toggleFullscreen() {
    if (document.fullscreenElement === canvas) {
      void document.exitFullscreen?.().catch(() => {});
    } else {
      void canvas.requestFullscreen?.().catch(() => {});
    }
  }

  /**
   * Tells the engine what the document's fullscreen element actually is.
   *
   * Fires for changes the page asked for *and* for ones it did not — Escape
   * leaves fullscreen without delivering a key event anywhere — which is why
   * this is the engine's only source of truth about the mode.
   */
  function onFullscreenChange() {
    const on = document.fullscreenElement === canvas;
    exports.__crcbl_web_fullscreen(canvasId, on ? STATE_EDGE : 0);
    // The element's box changed; `ResizeObserver` will say so too, but the
    // forced call means the swapchain is resized on the very next frame rather
    // than one observation later.
    syncSize(true);
  }

  /**
   * Whether the canvas currently holds the pointer lock.
   *
   * Asked of the document every time rather than remembered: the browser
   * releases the lock on its own — Escape, a tab switch, the element leaving
   * the DOM — and a cached flag would go on saying "locked" through all of it.
   */
  function isLocked() {
    return document.pointerLockElement === canvas;
  }

  /** Whether the gesture listener below is installed. */
  let lockArmed = false;

  /** Starts waiting for the gesture that can take the lock. */
  function armLock() {
    if (lockArmed) return;
    lockArmed = true;
    canvas.addEventListener('pointerdown', takeLock);
  }

  /** Stops waiting — the lock was taken, given up, or is no longer wanted. */
  function disarmLock() {
    if (!lockArmed) return;
    lockArmed = false;
    canvas.removeEventListener('pointerdown', takeLock);
  }

  /**
   * Takes the lock, from inside the gesture that armed this.
   *
   * `unadjustedMovement: true` is the whole reason the engine sets
   * `RAW_POINTER_MOTION` for this backend: it asks the OS for raw device motion
   * instead of the accelerated pointer, which is what a first-person camera
   * needs and what differencing cursor positions can never give.
   *
   * **Two return shapes, and both are handled.** The promise-returning form is
   * the newer one: it rejects with `NotSupportedError` where the option is
   * unavailable — Firefox for Android reports it exactly that way — and the
   * retry without the option is a lock with OS-adjusted deltas, which is worth
   * having and is what `ShellCaps::RAW_POINTER_MOTION` says may happen. A
   * browser that predates it returns `undefined` having already started the
   * request, so there is nothing to chain onto and nothing to retry. Every
   * rejection is caught either way: an unhandled one is a console error on a
   * page whose gate reads the console, and the outcome that matters arrives at
   * `onPointerLockChange` regardless. A request made with no gesture left
   * rejects `NotAllowedError` and is left alone here for that reason.
   *
   * **A mouse and nothing else takes it.** A browser grants Pointer Lock from
   * *any* gesture, a finger's `pointerdown` included, and a locked page is one
   * where `clientX`/`clientY` stop being a position — Chromium reports them as
   * `0`, so `position` answers `-rect.left` for every event after the grant. A
   * contact reported there is a contact where the finger is not, which is how
   * the browser gate's touch group went red on the two demos that ask for the
   * lock. There is nothing to pin either: a finger has no cursor. A pen is
   * excluded for the same second reason, and that one is a decision rather
   * than an oversight — `docs/backlog.md` carries it.
   *
   * A gesture this declines leaves the listener **armed**, which is why the
   * arming is not `{ once: true }`: a one-shot is burned by the touch that
   * ignores it, and the mouse press it was waiting for would then never find
   * it. `disarmLock` below is what spends it, on the gesture that is really
   * taking the lock.
   *
   * @param {PointerEvent} event
   */
  function takeLock(event) {
    if (event.pointerType !== 'mouse') return;
    disarmLock();
    const asked = canvas.requestPointerLock?.({ unadjustedMovement: true });
    if (!asked || typeof asked.then !== 'function') return;
    asked.catch((/** @type {unknown} */ error) => {
      const name =
        error && typeof error === 'object' && 'name' in error
          ? error.name
          : undefined;
      // Anything else — no gesture left, a `pointer-lock` permission policy —
      // is refused for a reason a second identical call cannot fix, and
      // `pointerlockerror` has already told the engine.
      if (name !== 'NotSupportedError') return;
      const plain = canvas.requestPointerLock?.();
      if (plain && typeof plain.then === 'function') plain.catch(() => {});
    });
  }

  /**
   * What was last written to `canvas.style.cursor`, so an unchanged request
   * touches no style.
   *
   * `null` and not `''` for the first frame: an empty string is a value the
   * engine really does publish — "no request, leave the page's own cursor" —
   * and starting there would skip the write that clears an inline style some
   * earlier window left behind.
   */
  let appliedCursor = null;

  /**
   * Draws the cursor the engine asked for.
   *
   * The keyword comes out of wasm memory rather than a table here, because
   * `CursorIcon`'s variants are CSS keywords already: see `set_cursor` in
   * `crates/crcbl-shell/src/web/mod.rs`. A table would be a second copy of that
   * enum, in a file no compiler checks against it.
   *
   * A zero length is the engine saying it has no window on this canvas, which
   * is not the same as any keyword: the inline style is cleared and the page's
   * own cursor comes back, rather than the canvas being pinned to an arrow
   * nobody asked for.
   */
  function syncCursor() {
    const len = exports.__crcbl_web_cursor_len(canvasId);
    const wanted =
      len === 0
        ? ''
        : readUtf8(memory, exports.__crcbl_web_cursor_ptr(canvasId), len);
    if (wanted === appliedCursor) return;
    appliedCursor = wanted;
    canvas.style.cursor = wanted;
  }

  /**
   * Brings the browser's pointer lock in line with what the engine asked for.
   *
   * Polled once a frame rather than driven by a call from wasm, because a
   * browser artifact imports nothing — see the note at the top of this file.
   * Polled from `requestAnimationFrame` rather than from the input handlers
   * because a *release* must not wait for input: a game that frees the pointer
   * during a cutscene would otherwise leave the cursor hidden until the player
   * moved the mouse to find out it was.
   *
   * Taking the lock is the half that cannot be done here — a rAF callback is
   * not a user gesture — so this only arms the click that can. Disarming on
   * release is what stops a stale request being granted by a click the player
   * meant for something else, minutes later.
   */
  function syncPointerLock() {
    const wanted = exports.__crcbl_web_pointer_lock_wanted(canvasId) !== 0;
    if (wanted) {
      if (!isLocked()) armLock();
      return;
    }
    disarmLock();
    if (isLocked()) document.exitPointerLock?.();
  }

  /**
   * Tells the engine whether the pointer is actually pinned to the canvas.
   *
   * The engine's only source of truth about it, for the reason
   * `onFullscreenChange` is about the mode: the browser lets go of the lock on
   * its own — Escape does it, and delivers no key anywhere — and a request that
   * was refused arrives here as `pointerlockerror` and nowhere else. While this
   * says locked the backend reports relative motion and no position; the moment
   * it says otherwise, positions come back.
   */
  function onPointerLockChange() {
    exports.__crcbl_web_pointer_lock(canvasId, isLocked() ? STATE_EDGE : 0);
  }

  /**
   * Whether this pointer is the one the browser emulates a mouse for.
   *
   * `isPrimary` is the first contact of a gesture and is always true for a mouse
   * or a pen, and the browser's own rule is that only that contact produces the
   * compatibility mouse events. The engine's pointer seam is that emulated
   * mouse: one position, one button, no contact id — so a second finger must not
   * reach it, or a two-finger fumble would move the paddle to the wrong finger
   * and release the button under the first.
   *
   * The second finger is not dropped any more; it goes to `__crcbl_web_touch`
   * below, which is a *different* stream with room for it. Keeping the pointer
   * one primary-only is what lets a game bound to a mouse button carry on
   * working with a finger, unchanged, while a game that wants two fingers reads
   * contacts.
   *
   * @param {PointerEvent} event
   */
  function isPrimary(event) {
    // `undefined` on the PointerEvent-less paths a test double can take; only an
    // explicit `false` is a secondary contact.
    return event.isPrimary !== false;
  }

  /**
   * Forwards one contact of a touch gesture, whether or not it is primary.
   *
   * Only for `pointerType === 'touch'`. A mouse and a pen are one contact by
   * construction and are already fully described by the pointer stream; giving
   * them a contact id as well would hand a game two ways to read the same
   * finger and no way to tell they were the same one.
   *
   * `pointerId` is passed through as the contact id: the browser's guarantee —
   * distinct while two contacts are down together, constant for a gesture,
   * reused only afterwards — is exactly what `ContactId` documents, and
   * renumbering it here would be a second identity to keep in step.
   *
   * @param {PointerEvent} event
   * @param {number} phase one of the `TOUCH_*` constants
   */
  function forwardContact(event, phase) {
    if (event.pointerType !== 'touch') return;
    const [x, y] = position(event);
    exports.__crcbl_web_touch(
      canvasId,
      event.timeStamp,
      x,
      y,
      event.pointerId >>> 0,
      phase
    );
  }

  /** @param {PointerEvent} event */
  function onPointerMove(event) {
    // Before the primary filter, and this order is the whole change: a second
    // finger's move is a real event about a real contact, and only the *pointer*
    // half of it has nowhere to go.
    forwardContact(event, TOUCH_MOVED);
    if (!isPrimary(event)) return;
    const [x, y] = position(event);
    const [dx, dy] = movement(event);
    // Both pairs, every time. Which one becomes the event is the backend's
    // call, because a click and a wheel have to drop their positions under the
    // same lock and only it sees all three.
    exports.__crcbl_web_pointer_motion(canvasId, event.timeStamp, x, y, dx, dy);
  }

  /**
   * The button the primary pointer last went down with.
   *
   * `pointercancel` reports `button: -1` — no button changed state, says the
   * spec — and the engine's seam has to be told which one came up. The gesture
   * being cancelled is the one that pressed this.
   */
  let heldButton = 0;

  /** @param {PointerEvent} event */
  function onPointerButton(event) {
    const down = event.type === 'pointerdown';
    forwardContact(event, down ? TOUCH_BEGAN : TOUCH_ENDED);
    if (!isPrimary(event)) return;
    const [x, y] = position(event);
    if (down) {
      heldButton = event.button;
      // Clicking the canvas is how a player expects to give it the keyboard,
      // and is also the user gesture an `AudioContext` needs before it starts.
      canvas.focus();
    }
    exports.__crcbl_web_pointer_button(
      canvasId,
      event.timeStamp,
      x,
      y,
      event.button,
      modifiers(event) | (down ? STATE_EDGE : 0)
    );
  }

  /**
   * A gesture the browser took away, reported as the button coming up.
   *
   * `pointercancel` fires **instead of** `pointerup` — the OS claimed the touch
   * for a system gesture, or the contact count changed — so without this the
   * engine is left holding the button down for good. A held button raises no
   * press *edge*, so the symptom is not a stuck control: it is a game whose tap
   * silently stops working, on touch only, and only after something else on the
   * phone interrupted it.
   *
   * `heldButton` and not `event.button`, which this event reports as `-1`: the
   * engine reads that as a button it has never heard of, drops the release, and
   * the handler is inert in exactly the way it was written to prevent.
   *
   * The *contact* keeps the distinction the pointer stream cannot express. A
   * pointer has one way to come up, so a cancelled gesture has to arrive as an
   * ordinary release and a game bound to the button fires as if the player had
   * let go; `TOUCH_CANCELLED` says what really happened, and a game reading
   * contacts can undo instead of committing.
   *
   * @param {PointerEvent} event
   */
  function onPointerCancel(event) {
    forwardContact(event, TOUCH_CANCELLED);
    if (!isPrimary(event)) return;
    const [x, y] = position(event);
    exports.__crcbl_web_pointer_button(
      canvasId,
      event.timeStamp,
      x,
      y,
      heldButton,
      modifiers(event)
    );
  }

  /** @param {WheelEvent} event */
  function onWheel(event) {
    const [x, y] = position(event);
    // `deltaMode` is lines or pages on some platforms; the ABI is pixels, and
    // the conventional conversions are 16 px per line and a viewport per page.
    const factor =
      event.deltaMode === 1
        ? 16
        : event.deltaMode === 2
          ? canvas.clientHeight
          : 1;
    exports.__crcbl_web_pointer_wheel(
      canvasId,
      event.timeStamp,
      x,
      y,
      event.deltaX * factor,
      event.deltaY * factor,
      modifiers(event)
    );
    event.preventDefault();
  }

  /**
   * A pointer crossing the canvas's edge — for a **cursor**, not a contact.
   *
   * A touch pointer is created at `pointerdown` and destroyed once the gesture
   * ends, and the browser fires `pointerenter` and `pointerleave` around it: the
   * enter arrives with the press, and the leave arrives immediately after the
   * release, in the same batch the engine pumps. The leave says "the pointer is
   * nowhere", so the position the engine hit-tests that release against is gone
   * by the time it looks — and a tap on a menu button fires nothing, while the
   * identical tap with a mouse works. A finger between contacts is not hovering
   * anywhere, so there is no state here for touch to report.
   *
   * @param {PointerEvent} event
   */
  function onPointerFocus(event) {
    if (!isPrimary(event) || event.pointerType === 'touch') return;
    const [x, y] = position(event);
    exports.__crcbl_web_pointer_focus(
      canvasId,
      event.timeStamp,
      x,
      y,
      event.type === 'pointerenter' ? STATE_EDGE : 0
    );
  }

  /** @param {FocusEvent} event */
  function onFocus(event) {
    exports.__crcbl_web_focus(
      canvasId,
      event.type === 'focus' ? STATE_EDGE : 0
    );
  }

  /**
   * A hidden tab is a canvas that has lost focus, whatever `activeElement` says.
   *
   * Only the losing edge is synthesized. Coming back visible does not hand the
   * keyboard back — the element may or may not still be focused, and a real
   * `focus` event says so if it is — and inventing one would tell the engine
   * every key it thought was held is live again.
   */
  function onVisibility() {
    if (document.visibilityState === 'hidden')
      exports.__crcbl_web_focus(canvasId, 0);
  }

  /** The listeners, so `dispose` removes exactly what was added. */
  const listeners = [
    [canvas, 'keydown', onKey, undefined],
    [canvas, 'keyup', onKey, undefined],
    [canvas, 'pointermove', onPointerMove, undefined],
    [canvas, 'pointerdown', onPointerButton, undefined],
    [canvas, 'pointerup', onPointerButton, undefined],
    [canvas, 'pointercancel', onPointerCancel, undefined],
    [canvas, 'pointerenter', onPointerFocus, undefined],
    [canvas, 'pointerleave', onPointerFocus, undefined],
    // `passive: false` or `preventDefault` is ignored and the page scrolls.
    [canvas, 'wheel', onWheel, { passive: false }],
    [canvas, 'focus', onFocus, undefined],
    [canvas, 'blur', onFocus, undefined],
    // On the *document*, not the canvas: `fullscreenchange` is fired at the
    // element in the standard and at the document in WebKit's prefixed history,
    // and the document form is the one every engine ships.
    [document, 'fullscreenchange', onFullscreenChange, undefined],
    // Pointer Lock fires both of these at the document by specification, and
    // the error one is not optional: a refused request changes nothing about
    // `pointerLockElement`, so without it a lock that never happened would look
    // to the engine exactly like one it is still waiting for.
    [document, 'pointerlockchange', onPointerLockChange, undefined],
    [document, 'pointerlockerror', onPointerLockChange, undefined],
    // A tab switch does not always blur the focused element — the element stays
    // `document.activeElement` while the document loses focus — so `blur` alone
    // leaves a game running with keys it will never see released. rAF stops in a
    // hidden tab, which freezes the picture but not the engine's idea of what is
    // held; the synthesized focus loss is what clears it.
    [document, 'visibilitychange', onVisibility, undefined],
    // The browser's own context menu on a right-click would eat the button.
    [
      canvas,
      'contextmenu',
      (/** @type {Event} */ e) => e.preventDefault(),
      undefined,
    ],
  ];
  for (const [target, type, handler, options] of listeners) {
    target.addEventListener(type, handler, options);
  }

  // Wrapped rather than passed directly: `ResizeObserver` hands its callback
  // `(entries, observer)`, and `observer` is truthy — which would make every
  // observation a forced one.
  const observer = new ResizeObserver(() => syncSize());
  observer.observe(canvas);
  // `ResizeObserver` does not fire when only `devicePixelRatio` changes — which
  // is what happens when the window is dragged to a monitor with a different
  // scale, or the page is zoomed. This media query does.
  const dprQuery = window.matchMedia(
    `(resolution: ${window.devicePixelRatio}dppx)`
  );
  const onDprChange = () => syncSize();
  dprQuery.addEventListener('change', onDprChange);

  // A loop of this shim's own, and a small one: two integer calls into wasm per
  // frame, plus a third only on the frame the cursor changes. It is separate
  // from the demo's frame loop because the demo's is the *engine's* — it runs
  // `api.frame` and stops when the engine does — and the pointer has to be
  // given back when it stops.
  let lockPoll = requestAnimationFrame(function poll() {
    syncPointerLock();
    syncCursor();
    lockPoll = requestAnimationFrame(poll);
  });

  // Sizes the backing store now. The engine does not hear about it yet — it has
  // no window — which is why `syncSize(true)` after boot is not optional.
  syncSize();

  return {
    syncSize,
    requestClose() {
      exports.__crcbl_web_close(canvasId);
    },
    dispose() {
      for (const [target, type, handler, options] of listeners) {
        target.removeEventListener(type, handler, options);
      }
      cancelAnimationFrame(lockPoll);
      // The armed click is not in `listeners` — it is added and removed as the
      // engine changes its mind — so it comes down through the same pair that
      // put it up, and a lock this shim is no longer watching is released
      // rather than left on a canvas nothing is driving.
      disarmLock();
      if (isLocked()) document.exitPointerLock?.();
      observer.disconnect();
      dprQuery.removeEventListener('change', onDprChange);
    },
  };
}
