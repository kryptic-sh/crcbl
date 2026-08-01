// The canvas half of the shim: size, DPI, focus, keyboard, pointer.
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

import { writeUtf8 } from './wasm.js';

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
]);

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
    if (!force && width === lastWidth && height === lastHeight && scale === lastScale) return;
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
    return [(event.clientX - rect.left) * scale, (event.clientY - rect.top) * scale];
  }

  /** @param {KeyboardEvent} event */
  function onKey(event) {
    const down = event.type === 'keydown';
    // `code` is the physical key and is what the engine binds to; `key` is what
    // it produced and is only used for the keysym.
    const codeLen = writeUtf8(memory, scratchPtr, scratchCapacity, event.code);
    if (codeLen === null) return;
    const keyLen = writeUtf8(memory, scratchPtr + codeLen, scratchCapacity - codeLen, event.key);
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
        (event.repeat ? STATE_REPEAT : 0),
    );
    if (SWALLOWED.has(event.code)) event.preventDefault();
  }

  /** @param {PointerEvent} event */
  function onPointerMove(event) {
    const [x, y] = position(event);
    exports.__crcbl_web_pointer_motion(canvasId, event.timeStamp, x, y);
  }

  /** @param {PointerEvent} event */
  function onPointerButton(event) {
    const [x, y] = position(event);
    const down = event.type === 'pointerdown';
    // Clicking the canvas is how a player expects to give it the keyboard, and
    // is also the user gesture an `AudioContext` needs before it will start.
    if (down) canvas.focus();
    exports.__crcbl_web_pointer_button(
      canvasId,
      event.timeStamp,
      x,
      y,
      event.button,
      modifiers(event) | (down ? STATE_EDGE : 0),
    );
  }

  /** @param {WheelEvent} event */
  function onWheel(event) {
    const [x, y] = position(event);
    // `deltaMode` is lines or pages on some platforms; the ABI is pixels, and
    // the conventional conversions are 16 px per line and a viewport per page.
    const factor = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? canvas.clientHeight : 1;
    exports.__crcbl_web_pointer_wheel(
      canvasId,
      event.timeStamp,
      x,
      y,
      event.deltaX * factor,
      event.deltaY * factor,
      modifiers(event),
    );
    event.preventDefault();
  }

  /** @param {PointerEvent} event */
  function onPointerFocus(event) {
    const [x, y] = position(event);
    exports.__crcbl_web_pointer_focus(
      canvasId,
      event.timeStamp,
      x,
      y,
      event.type === 'pointerenter' ? STATE_EDGE : 0,
    );
  }

  /** @param {FocusEvent} event */
  function onFocus(event) {
    exports.__crcbl_web_focus(canvasId, event.type === 'focus' ? STATE_EDGE : 0);
  }

  /** The listeners, so `dispose` removes exactly what was added. */
  const listeners = [
    [canvas, 'keydown', onKey, undefined],
    [canvas, 'keyup', onKey, undefined],
    [canvas, 'pointermove', onPointerMove, undefined],
    [canvas, 'pointerdown', onPointerButton, undefined],
    [canvas, 'pointerup', onPointerButton, undefined],
    [canvas, 'pointerenter', onPointerFocus, undefined],
    [canvas, 'pointerleave', onPointerFocus, undefined],
    // `passive: false` or `preventDefault` is ignored and the page scrolls.
    [canvas, 'wheel', onWheel, { passive: false }],
    [canvas, 'focus', onFocus, undefined],
    [canvas, 'blur', onFocus, undefined],
    // The browser's own context menu on a right-click would eat the button.
    [canvas, 'contextmenu', (/** @type {Event} */ e) => e.preventDefault(), undefined],
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
  const dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
  const onDprChange = () => syncSize();
  dprQuery.addEventListener('change', onDprChange);

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
      observer.disconnect();
      dprQuery.removeEventListener('change', onDprChange);
    },
  };
}
