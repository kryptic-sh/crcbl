// Queued keyboard input in the real puppet page, after group C's course walk.
// Keep the page alive: group I owns its Stop and GPU teardown receipts.
import { Cdp, evaluate, say } from './browser-launch.mjs';

const FORWARD = { code: 'KeyW', text: 'w', virtualKeyCode: 87 };
const BACKWARD = { code: 'KeyS', text: 's', virtualKeyCode: 83 };
const ESCAPE = { code: 'Escape', text: 'Escape', virtualKeyCode: 27 };
// Between the course wall and spawn in apps/puppet/src/map.rs. Choose the
// direction with room, rather than pushing W into the wall group C reached.
const TURN_Z = 8;

export async function queuedKeyChecks({
  page,
  browser,
  hud,
  until,
  check,
  sendKey,
  loopFrames,
  focus,
  heartbeats,
  advance,
  beats,
}) {
  const started = performance.now();
  const control = await Cdp.connect(browser.endpoint);
  const ensure = (ok, detail) => {
    if (!ok) throw new Error(detail);
  };
  const readings = (from) =>
    hud()
      .slice(from)
      .flatMap((line) => {
        const tick = line.match(/\btick: (\d+)/);
        const z = line.match(/\bpz: (-?[\d.]+)/);
        return tick && z ? [{ tick: Number(tick[1]), z: Number(z[1]) }] : [];
      });
  const sample = async () => {
    const from = hud().length;
    const value = await until(async () => readings(from)[0] ?? null);
    ensure(value !== null, 'no fresh puppet HUD tick');
    return value;
  };
  const status = async (wanted) => {
    const got = await until(async () =>
      (await evaluate(page, 'crcbl.status()')) === wanted ? wanted : null
    );
    ensure(
      got === wanted,
      `expected status ${wanted}, got ${await evaluate(page, 'crcbl.status()')}`
    );
  };
  const moving = async (key, base) => {
    const from = hud().length;
    await sendKey(page, key, 'keyDown');
    const direction = key === BACKWARD ? 1 : -1;
    const moved = await until(async () => {
      const rows = readings(from);
      const last = rows.at(-1);
      const prior = rows.at(-2) ?? base;
      return last &&
        last.tick > prior.tick &&
        direction * (last.z - prior.z) >= advance
        ? last
        : null;
    });
    ensure(
      moved !== null,
      `${key.code} did not move from ${JSON.stringify(base)}; fresh readings ${JSON.stringify(readings(from))}`
    );
    return moved;
  };
  const names = {
    blur: 'a held walk key is cleared across blur and needs a fresh press',
    tab: 'a held walk key is cleared across a real tab switch',
    visibility:
      'the visibility edge clears held input without a canvas blur event',
  };
  let recovered = true;
  try {
    for (const kind of Object.keys(names)) {
      if (!recovered) {
        check(
          'E',
          names[kind],
          false,
          'not executed: queued-input recovery failed'
        );
        continue;
      }
      let otherTarget = null;
      let key = null;
      try {
        await status(3);
        ensure(
          await evaluate(
            page,
            "document.activeElement?.id === 'canvas' && document.visibilityState === 'visible'"
          ),
          'precondition: canvas must be focused and visible'
        );
        const base = await sample();
        key = base.z < TURN_Z ? BACKWARD : FORWARD;
        const held = await moving(key, base);
        if (kind === 'blur') {
          await evaluate(page, "document.getElementById('stop').focus()");
        } else if (kind === 'tab') {
          // Creating a foreground target can itself blur: only do it AFTER the
          // held-key movement witness, never during fixture setup.
          otherTarget = (
            await control.send('Target.createTarget', { url: 'about:blank' })
          ).targetId;
          await control.send('Target.activateTarget', {
            targetId: otherTarget,
          });
          ensure(
            await until(async () =>
              (await evaluate(page, 'document.visibilityState')) === 'hidden'
                ? true
                : null
            ),
            'real tab switch never hid the puppet document'
          );
          await control.send('Target.activateTarget', {
            targetId: page.targetId,
          });
          ensure(
            await until(async () =>
              (await evaluate(page, 'document.visibilityState')) === 'visible'
                ? true
                : null
            ),
            'puppet document never became visible again'
          );
        } else {
          // Isolate the production visibility listener from canvas blur. The
          // property override is synchronous and restored even if dispatch fails.
          await evaluate(
            page,
            `(() => {
            const canvas = document.getElementById('canvas');
            const events = [];
            const log = event => events.push(event.type);
            canvas.addEventListener('blur', log);
            canvas.addEventListener('focus', log);
            window.__crcblQueuedFocus = { events, dispose() {
              canvas.removeEventListener('blur', log);
              canvas.removeEventListener('focus', log);
            } };
            const descriptor = Object.getOwnPropertyDescriptor(document, 'visibilityState');
            try {
              Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
              document.dispatchEvent(new Event('visibilitychange'));
            } finally {
              if (descriptor) Object.defineProperty(document, 'visibilityState', descriptor);
              else delete document.visibilityState;
            }
          })()`
          );
        }
        await status(6);
        ensure(
          (await heartbeats()) === 0,
          'simulation ticks advanced while paused'
        );
        if (kind === 'visibility') {
          const isolated = await evaluate(
            page,
            `({
            active: document.activeElement?.id,
            events: window.__crcblQueuedFocus.events,
            visible: document.visibilityState,
          })`
          );
          ensure(
            isolated.active === 'canvas' &&
              isolated.events.length === 0 &&
              isolated.visible === 'visible',
            `visibility isolation failed: ${JSON.stringify(isolated)}`
          );
        }
        await focus();
        ensure(
          (await loopFrames(page)) !== null,
          'engine did not process the focusing click'
        );
        ensure(
          await evaluate(
            page,
            "document.activeElement?.id === 'canvas' && crcbl.status() === 6"
          ),
          'focus alone resumed the demo or missed the canvas'
        );
        const from = hud().length;
        for (const type of ['keyDown', 'keyUp'])
          await sendKey(page, ESCAPE, type);
        await status(3);
        const resumed = await until(async () => {
          const rows = readings(from);
          return rows.length >= beats ? rows.slice(0, beats) : null;
        });
        ensure(resumed !== null, 'no advancing HUD ticks after resume');
        // Observe the interval between the first resumed HUD samples. There is
        // no paused-position reading here to measure displacement to the first.
        // Fresh same-direction movement below rules out a stationary wall.
        ensure(
          resumed.every(
            (row, index) =>
              row.z === resumed[0].z &&
              (index === 0 || row.tick > resumed[index - 1].tick)
          ),
          `movement continued without fresh input: ${JSON.stringify(resumed)}`
        );
        await sendKey(page, key, 'keyUp');
        const freshBase = await sample();
        const fresh = await moving(key, freshBase);
        check(
          'E',
          names[kind],
          true,
          `${key.code}: held ${JSON.stringify(held)}, resumed ${JSON.stringify(resumed)}, fresh ${JSON.stringify(freshBase)} -> ${JSON.stringify(fresh)}`
        );
      } catch (error) {
        check('E', names[kind], false, error.message);
      } finally {
        // A failed cleanup must not prevent independent releases or group I.
        const recover = async (name, action) => {
          try {
            await action();
          } catch (error) {
            recovered = false;
            check('E', `${kind} recovery: ${name}`, false, error.message);
          }
        };
        await recover('dispose focus observer', () =>
          evaluate(
            page,
            `(() => {
            window.__crcblQueuedFocus?.dispose();
            delete window.__crcblQueuedFocus;
          })()`
          )
        );
        await recover('close other tab', async () => {
          if (otherTarget) {
            const result = await control.send('Target.closeTarget', {
              targetId: otherTarget,
            });
            ensure(result.success, 'Target.closeTarget did not close the tab');
          }
        });
        await recover('activate puppet', () =>
          control.send('Target.activateTarget', { targetId: page.targetId })
        );
        // Restore the real focus pair after the isolated visibility edge.
        await recover('blur canvas', () =>
          evaluate(page, "document.getElementById('stop').focus()")
        );
        await recover('focus canvas', focus);
        await recover('release walk key', async () => {
          if (key) await sendKey(page, key, 'keyUp');
        });
        await recover('release Escape', () => sendKey(page, ESCAPE, 'keyUp'));
        await recover('drain input', async () => {
          ensure((await loopFrames(page)) !== null, 'input did not drain');
        });
        await recover('resume', async () => {
          if ((await evaluate(page, 'crcbl.status()')) === 6) {
            await sendKey(page, ESCAPE, 'keyDown');
          }
        });
        await recover('release resume key', () =>
          sendKey(page, ESCAPE, 'keyUp')
        );
        await recover('running focused visible state', async () => {
          await status(3);
          ensure(
            await evaluate(
              page,
              "document.activeElement?.id === 'canvas' && document.visibilityState === 'visible'"
            ),
            'canvas is not focused and visible'
          );
        });
      }
    }
    return recovered;
  } finally {
    control.close();
    say(
      `web e2e: queued-input checks took ${Math.round(performance.now() - started)} ms`
    );
  }
}
