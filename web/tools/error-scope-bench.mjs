#!/usr/bin/env node
// Measures what `pushErrorScope`/`popErrorScope` around replay would cost, in a
// real browser on a real device, so that the granularity
// `docs/plan/41-webgpu-stream.md` leaves open can be decided from numbers
// instead of from taste.
//
//   node web/tools/error-scope-bench.mjs <site-dir> [--adapter hardware]
//                                                   [--runs 5] [--frames 120]
//
// THE QUESTION. WebGPU reports its own validation and out-of-memory failures on
// `uncapturederror`, after `replay` has returned, with no currently-executing
// command to name — so `web/engine/gpu-replay.js` records them as the device's
// and unattributed. An error *scope* would name something instead: one pair per
// flush attributes a failure to the sequence range that flush covered, one pair
// per command attributes it to the command. The second is precise and the first
// is cheap, and the plan says which is affordable is a measurement nobody has
// taken. This takes it.
//
// TWO PHASES, BECAUSE ONE NUMBER IS NOT ENOUGH.
//
//   1. THE REAL STREAM. `apps/render-harness` is the only thing in this
//      repository that drives `crcbl-render`'s scenes through `crcbl-webgpu`
//      for real, so its page is what the arms are measured on: this file serves
//      a page that patches `Replayer.prototype.replay` and then imports
//      `harness/main.js` unchanged, so every command still crosses the real
//      transport and is replayed against the real `GPUDevice`. That phase says
//      what a *real* frame of crcbl commands costs under each arm, and — the
//      number the plan is missing — how many commands a real frame actually
//      carries.
//   2. THE SWEEP. The harness renders one frame per scene, so its volume is
//      whatever those scenes are; the plan's budget is "a few thousand commands
//      a frame". So the second phase varies the pair count per frame across
//      orders of magnitude on a device of its own, with a fixed submit each
//      frame so the wire is never idle, and reports the cost as a function of
//      the count. That is what lets the answer be read off at a volume no scene
//      here happens to have.
//
// WHY THE PATCH IS A PAGE AND NOT AN EDIT TO THE REPLAYER. Measuring a thing by
// first shipping it is how a measurement stops being able to say "no". The arms
// live in the page this file serves, `harness/main.js` is imported unmodified,
// and nothing in `web/engine/` knows this file exists.
//
// WHAT IT IS NOT. Not a gate: it asserts nothing and always exits 0 unless it
// could not run at all. Numbers, not verdicts.
//
// USAGE
//   <site-dir>    A directory with `harness/index.html` and the loader-plus-
//                 output beside it, exactly as `web/run-render-harness-e2e.sh`
//                 assembles one.
//   --adapter     `hardware` (default) or `swiftshader`. Both are worth having:
//                 CI has no real GPU, and a cost measured on lavapipe is not
//                 the cost a player pays.
//   --runs        How many times each arm is repeated. The spread across runs
//                 is reported, because a single sample is not a measurement.
//   --frames      How many frames each sweep point is driven for.
//
// EXIT CODES
//   0  it ran and printed its numbers.
//   2  it could not run at all — no browser, no adapter, the wasm failed.

import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';

import {
  evaluate,
  findBrowser,
  launch,
  openPage,
  pause,
  stopEverything,
} from './browser-launch.mjs';
import { serve } from './serve.mjs';

/** How long one harness run is given before it is called a failure. */
const RUN_TIMEOUT_MS = 300_000;

/** How long one sweep is given. */
const SWEEP_TIMEOUT_MS = 300_000;

function fail(message) {
  console.error(`error-scope-bench: ${message}`);
  stopEverything();
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/** @type {Record<string, string>} */
const args = {};
let siteArg = null;
for (let i = 2; i < process.argv.length; i += 1) {
  const arg = process.argv[i];
  if (!arg.startsWith('--')) {
    if (siteArg === null) siteArg = arg;
    else fail(`unexpected argument ${arg}`);
    continue;
  }
  const [name, inline] = arg.slice(2).split('=', 2);
  args[name] = inline ?? process.argv[++i] ?? '';
}

if (!siteArg) fail('usage: error-scope-bench.mjs <site-dir> [--adapter …]');
const SITE = resolve(siteArg);
const ADAPTER = args.adapter ?? 'hardware';
if (!['hardware', 'swiftshader'].includes(ADAPTER)) {
  fail(`--adapter must be hardware or swiftshader, got "${ADAPTER}"`);
}
const RUNS = Number(args.runs ?? 5);
const FRAMES = Number(args.frames ?? 120);
if (!Number.isInteger(RUNS) || RUNS < 1) fail('--runs must be a positive int');
if (!Number.isInteger(FRAMES) || FRAMES < 1) {
  fail('--frames must be a positive int');
}
if (!existsSync(join(SITE, 'harness', 'index.html'))) {
  fail(`${SITE}/harness/index.html not found — assemble the site first`);
}

/**
 * The three arms, named as the plan names them.
 *
 * `none` is the behaviour before per-flush attribution was built — the baseline
 * every other number is a delta from. `flush` is what `Replayer#replay` ships:
 * one scope per `GPUErrorFilter` around the whole call. `command` is one pair
 * around each command, which the page reaches by replaying each command as a
 * frame of its own — the sequence arithmetic is positional, so a frame of one
 * command at `base + n` is exactly the command the whole frame would have run at
 * that position.
 *
 * How an arm gets the replayer to do that, given the replayer opens scopes of
 * its own, is `BENCH_JS`'s business and is argued there.
 */
const ARMS = ['none', 'flush', 'command'];

/**
 * How many scope pairs per frame the sweep tries.
 *
 * Zero is the baseline for the same reason `none` is: the fixed submit each
 * frame costs what it costs, and only the difference is the scope's. The top of
 * the range is past `docs/plan/41-webgpu-stream.md`'s "a few thousand commands a
 * frame", so per-command attribution at a realistic volume is inside the
 * measured range rather than extrapolated to.
 */
const SWEEP_POINTS = [0, 1, 4, 16, 64, 256, 1024, 4096];

// ---------------------------------------------------------------------------
// The pages
// ---------------------------------------------------------------------------

// Served as routes rather than written into the site, so the bench adds no file
// to a directory a gate assembles and nothing it measures can be left behind in
// one.

const BENCH_HTML = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>crcbl error-scope bench</title>
  </head>
  <body>
    <pre id="log">starting…</pre>
    <script type="module" src="./bench.js"></script>
  </body>
</html>
`;

// The patch, and then `harness/main.js` unmodified. Both import
// `../engine/gpu-replay.js` by the same URL, so they share one module instance
// and one `Replayer` class — which is what makes patching the prototype here
// take effect inside a file this page does not touch.
const BENCH_JS = `
import { Replayer } from '../engine/gpu-replay.js';

const arm = new URLSearchParams(location.search).get('arm') ?? 'none';

/**
 * What the run cost, read out of the page by the driver.
 *
 * \`replayMs\` is the synchronous half: the time inside \`replay\`, which is what
 * a frame pays before it can go on. \`settleMs\` is the asynchronous half: from a
 * pop being issued to it resolving, which is when the attribution actually
 * exists. They are reported separately because they are not the same cost and
 * only the first is on the frame's critical path.
 */
const stats = {
  arm,
  frames: 0,
  commands: 0,
  replayMs: 0,
  pops: 0,
  popsResolved: 0,
  settleMs: 0,
  maxSettleMs: 0,
  captured: 0,
  startedAt: 0,
  finishedAt: 0,
};
window.benchStats = stats;

const original = Replayer.prototype.replay;

/**
 * THE REPLAYER ALREADY OPENS SCOPES, SO EVERY ARM HAS TO SAY WHAT IT DOES WITH
 * THEM.
 *
 * \`Replayer#replay\` wraps each flush that carries commands in one scope per
 * \`GPUErrorFilter\`. That is the *shipped* behaviour and one of the three things
 * being compared, which means the other two cannot simply call through: an arm
 * that meant "no scopes" would in fact be measuring the shipped ones, and an arm
 * that meant "one per command" would be measuring those on top of them.
 *
 * So each arm installs its own \`pushErrorScope\` / \`popErrorScope\` as own
 * properties on the device, shadowing the prototype's for as long as the flush
 * runs:
 *
 *   none      no-ops. Nothing is pushed and every pop answers \`null\` without a
 *             round trip, which is what the replayer did before per-flush
 *             attribution was built. The baseline.
 *   flush     pass-throughs that time and count. The replayer's own scopes are
 *             what is measured; nothing extra is pushed.
 *   command   no-ops for the inner call, with this file pushing and popping a
 *             real scope around each command instead.
 *
 * Own properties rather than a patched prototype, and \`delete\`d rather than
 * reassigned, so the device is left exactly as it was found.
 */
function install(device, push, pop) {
  device.pushErrorScope = push;
  device.popErrorScope = pop;
}

function uninstall(device) {
  delete device.pushErrorScope;
  delete device.popErrorScope;
}

const NO_PUSH = () => {};
const NO_POP = () => Promise.resolve(null);

/**
 * Records what a pop cost and whether it caught anything.
 *
 * The count of captures is the arm's own proof that it was measuring scopes a
 * browser honoured rather than calls it quietly ignored — the sweep's
 * provocation makes the same point where there is no real error to wait for.
 */
function measured(promise, at) {
  stats.pops += 1;
  promise.then(
    (error) => {
      const settle = performance.now() - at;
      stats.popsResolved += 1;
      stats.settleMs += settle;
      if (settle > stats.maxSettleMs) stats.maxSettleMs = settle;
      if (error) stats.captured += 1;
    },
    () => {
      stats.popsResolved += 1;
    }
  );
  return promise;
}

Replayer.prototype.replay = function benchReplay(frame) {
  if (frame === null || frame === undefined) return original.call(this, frame);
  const count = frame.commands.length;
  // Frames that carry nothing are not counted. The harness pumps on every rAF
  // tick and almost every tick finds an empty stream, so counting those would
  // divide a real frame's command count by the number of times nothing
  // happened.
  if (count > 0) stats.frames += 1;
  stats.commands += count;
  const device = this.device;
  if (device === null || count === 0) return original.call(this, frame);

  const realPush = device.pushErrorScope.bind(device);
  const realPop = device.popErrorScope.bind(device);
  const started = performance.now();
  if (arm === 'none') {
    install(device, NO_PUSH, NO_POP);
    try {
      original.call(this, frame);
    } finally {
      uninstall(device);
    }
  } else if (arm === 'flush') {
    install(device, realPush, () => measured(realPop(), performance.now()));
    try {
      original.call(this, frame);
    } finally {
      uninstall(device);
    }
  } else if (arm === 'command') {
    install(device, NO_PUSH, NO_POP);
    try {
      for (let i = 0; i < count; i += 1) {
        const one = {
          baseSequence: BigInt.asUintN(64, frame.baseSequence + BigInt(i)),
          commands: [frame.commands[i]],
        };
        realPush('validation');
        try {
          original.call(this, one);
        } finally {
          measured(realPop(), performance.now());
        }
      }
    } finally {
      uninstall(device);
    }
  } else {
    throw new Error('unknown arm ' + arm);
  }
  stats.replayMs += performance.now() - started;
};

stats.startedAt = performance.now();
await import('./main.js');
// \`main.js\` sets \`window.harnessDone\` when it has driven every scene; the
// import above only starts it.
const doneAt = new Promise((resolve) => {
  const tick = () => {
    if (window.harnessDone) resolve(performance.now());
    else requestAnimationFrame(tick);
  };
  tick();
});
stats.finishedAt = await doneAt;
window.benchDone = true;
`;

/**
 * The sweep, driven on a device of its own.
 *
 * Its own so that the harness's device — which may have been through a scene
 * that failed — cannot decide the numbers, and so that the sweep is a thing
 * this file can run whether or not the harness ran at all.
 *
 * THE FIXED SUBMIT IS NOT DECORATION. `popErrorScope` resolves when the wire has
 * carried the answer back from the GPU process, and a wire with nothing else on
 * it is not the wire a frame has. Every point pays the same submit, so the
 * difference between points is the scopes and only the scopes.
 */
const SWEEP_JS = (points, frames) => `(async () => {
  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) return { fatal: 'no adapter' };
  const device = await adapter.requestDevice();
  const target = device.createTexture({
    size: [256, 192],
    format: 'rgba8unorm',
    usage: GPUTextureUsage.RENDER_ATTACHMENT,
  });
  const view = target.createView();

  /** One frame's worth of real GPU work, identical at every point. */
  function submitOne() {
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [
        {
          view,
          loadOp: 'clear',
          storeOp: 'store',
          clearValue: { r: 0.1, g: 0.2, b: 0.3, a: 1 },
        },
      ],
    });
    pass.end();
    device.queue.submit([encoder.finish()]);
  }

  const nextFrame = () => new Promise((ok) => requestAnimationFrame(ok));

  // THE PROVOCATION, RUN FIRST AND REPORTED BESIDE THE NUMBERS. A sweep of
  // scopes that captured nothing is a sweep that might have been measuring a
  // browser quietly ignoring the calls, and every point would look the same
  // either way. So one deliberately invalid create is made inside a scope of
  // its own, and the pop it produces must be a \`GPUError\`. If this line says
  // \`none\`, the timings below are measuring nothing and must not be believed.
  device.pushErrorScope('validation');
  device.createBuffer({ size: 16, usage: 0 });
  const provoked = await device.popErrorScope();
  const provocation = provoked
    ? provoked.constructor.name + ': ' + String(provoked.message).slice(0, 120)
    : 'none';

  async function point(pairs) {
    const sync = [];
    const settle = [];
    // One warm-up frame per point, discarded: the first submit after a texture
    // is created pays for allocation the rest do not.
    for (let f = 0; f < ${frames} + 1; f += 1) {
      submitOne();
      const pending = [];
      const startedScopes = performance.now();
      for (let i = 0; i < pairs; i += 1) {
        device.pushErrorScope('validation');
        pending.push(device.popErrorScope());
      }
      const syncMs = performance.now() - startedScopes;
      const issued = performance.now();
      if (pending.length) await Promise.all(pending);
      const settleMs = performance.now() - issued;
      if (f > 0) {
        sync.push(syncMs);
        settle.push(settleMs);
      }
      await nextFrame();
    }
    return { pairs, sync, settle };
  }

  const out = [];
  for (const pairs of ${JSON.stringify(points)}) out.push(await point(pairs));
  device.destroy();
  return { points: out, provocation };
})()`;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/** The value at `q` of a sorted copy of `values`, by nearest rank. */
function quantile(values, q) {
  const sorted = [...values].sort((a, b) => a - b);
  if (sorted.length === 0) return NaN;
  const at = Math.min(sorted.length - 1, Math.floor(q * sorted.length));
  return sorted[at];
}

/** Median, min and max, which is the spread this file reports everywhere. */
function spread(values) {
  return {
    n: values.length,
    min: Math.min(...values),
    median: quantile(values, 0.5),
    p95: quantile(values, 0.95),
    max: Math.max(...values),
  };
}

const ms = (value) => (Number.isFinite(value) ? value.toFixed(3) : '—');

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

const routes = {
  '/harness/bench.html': {
    contentType: 'text/html; charset=utf-8',
    body: BENCH_HTML,
  },
  '/harness/bench.js': {
    contentType: 'text/javascript; charset=utf-8',
    body: BENCH_JS,
  },
};

const server = await serve(SITE, { host: '127.0.0.1', routes });
const binary = findBrowser(fail);
let browser = null;

try {
  console.log(`error-scope-bench: browser ${binary}`);
  console.log(`error-scope-bench: serving ${SITE} at ${server.origin}`);
  console.log(`error-scope-bench: adapter ${ADAPTER}`);
  console.log(
    `error-scope-bench: ${RUNS} run(s) per arm, ${FRAMES} frame(s) per sweep point`
  );

  browser = await launch({
    binary,
    mode: ADAPTER,
    profilePrefix: 'crcbl-error-scope-bench-',
    fail,
  });

  // -------------------------------------------------------------------------
  // Phase 1: the real stream
  // -------------------------------------------------------------------------

  /** @type {Record<string, object[]>} */
  const armRuns = { none: [], flush: [], command: [] };

  for (let run = 0; run < RUNS; run += 1) {
    for (const arm of ARMS) {
      const page = await openPage(browser);
      await page.send('Runtime.enable');
      await page.send('Page.enable');
      const pageErrors = [];
      page.on('Runtime.exceptionThrown', ({ exceptionDetails }) => {
        pageErrors.push(
          exceptionDetails.exception?.description ?? exceptionDetails.text
        );
      });
      await page.send('Page.navigate', {
        url: `${server.origin}/harness/bench.html?arm=${arm}`,
      });

      const deadline = Date.now() + RUN_TIMEOUT_MS;
      let done = false;
      while (Date.now() < deadline) {
        done = await evaluate(page, 'Boolean(window.benchDone)');
        if (done) break;
        await pause(100);
      }
      if (!done) {
        console.error(pageErrors.slice(0, 3).join('\n'));
        fail(`arm "${arm}" did not finish within ${RUN_TIMEOUT_MS} ms`);
      }
      // The pops of the last flush are still in flight when the harness stops,
      // so the settle numbers are read after they have had somewhere to land.
      // Reported as `popsResolved` beside `pops`, so a run where they did not
      // all land says so rather than averaging over a smaller set silently.
      await pause(500);
      const stats = await evaluate(page, 'window.benchStats');
      const result = await evaluate(
        page,
        '({ rendered: (window.harnessResult?.scenes ?? []).filter((s) => s.rendered).length,' +
          '  scenes: (window.harnessResult?.scenes ?? []).length,' +
          '  deviceErrors: (window.harnessResult?.scenes ?? [])' +
          '    .reduce((n, s) => n + s.deviceErrors.length, 0) })'
      );
      armRuns[arm].push({ ...stats, ...result });
      page.close();
    }
  }

  // -------------------------------------------------------------------------
  // Phase 2: the sweep
  // -------------------------------------------------------------------------

  const sweepPage = await openPage(browser);
  await sweepPage.send('Runtime.enable');
  await sweepPage.send('Page.enable');
  await sweepPage.send('Page.navigate', {
    url: `${server.origin}/harness/bench.html?arm=none&sweep=1`,
  });
  // The page's own module races nothing here — the sweep runs against a device
  // it opens itself — but it must have loaded before an evaluate lands in it.
  await pause(1_000);
  const sweep = await Promise.race([
    evaluate(sweepPage, SWEEP_JS(SWEEP_POINTS, FRAMES)),
    new Promise((_, reject) =>
      setTimeout(
        () => reject(new Error('the sweep did not finish')),
        SWEEP_TIMEOUT_MS
      )
    ),
  ]);
  sweepPage.close();

  // -------------------------------------------------------------------------
  // The report
  // -------------------------------------------------------------------------

  console.log('\nerror-scope-bench: the real stream (apps/render-harness)');
  console.log(
    '  arm      runs  scenes  cmds/run  cmds/frame  replay ms/run              run ms                        pops/run  settle ms'
  );
  for (const arm of ARMS) {
    const runs = armRuns[arm];
    const replay = spread(runs.map((r) => r.replayMs));
    const commands = runs[0]?.commands ?? 0;
    const frames = runs[0]?.frames ?? 0;
    const pops = spread(runs.map((r) => r.pops));
    const settle = runs.map((r) =>
      r.popsResolved > 0 ? r.settleMs / r.popsResolved : NaN
    );
    // END TO END AS WELL AS INSIDE `replay`. The synchronous cost is what a
    // frame pays before it can go on, but a flood of pending pops can slow a run
    // without ever showing up there — so the wall time of driving all eleven
    // scenes is reported beside it.
    const run = spread(runs.map((r) => r.finishedAt - r.startedAt));
    console.log(
      `  ${arm.padEnd(8)} ${String(runs.length).padStart(4)}  ` +
        `${String(runs[0]?.rendered ?? 0).padStart(2)}/${String(runs[0]?.scenes ?? 0).padEnd(3)} ` +
        `${String(commands).padStart(8)}  ` +
        `${(frames ? commands / frames : 0).toFixed(1).padStart(10)}  ` +
        `${ms(replay.min)}–${ms(replay.max)} (med ${ms(replay.median)})  ` +
        `${ms(run.min)}–${ms(run.max)} (med ${ms(run.median)})  ` +
        `${String(pops.median).padStart(8)}  ` +
        `mean ${ms(quantile(settle.filter(Number.isFinite), 0.5))} max ${ms(
          spread(runs.map((r) => r.maxSettleMs)).max
        )}`
    );
  }
  for (const arm of ARMS) {
    const runs = armRuns[arm];
    const unresolved = runs.filter((r) => r.pops !== r.popsResolved).length;
    if (unresolved > 0) {
      console.log(
        `  note: ${unresolved}/${runs.length} "${arm}" run(s) still had pops in flight when read`
      );
    }
    const captured = runs.reduce((n, r) => n + r.captured, 0);
    const errors = runs.reduce((n, r) => n + r.deviceErrors, 0);
    console.log(
      `  ${arm.padEnd(8)} captured ${captured} scope error(s); the harness's own log saw ${errors}`
    );
  }

  console.log(
    '\nerror-scope-bench: the sweep — one submit per frame, plus N scope pairs'
  );
  if (sweep.provocation) {
    // First, and before any timing is read: a browser that had quietly ignored
    // the scope calls would produce the same table as one that honoured them,
    // and this is the line that tells the two apart.
    console.log(
      `  a deliberately invalid create inside a scope popped: ${sweep.provocation}`
    );
  }
  if (sweep.fatal) {
    console.log(`  the sweep could not run: ${sweep.fatal}`);
  } else {
    // HOW TO READ `pairs` AGAINST A COMMAND COUNT. A pair here is one
    // `pushErrorScope`/`popErrorScope` on the `'validation'` filter. Covering
    // what a flush covers takes one pair per `GPUErrorFilter`, so a per-command
    // arm over a frame of C commands is C times as many pairs as that filter
    // list is long — read the row for that number, not for C.
    console.log(
      '  pairs   sync ms/frame (min/med/p95/max)      settle ms/frame (min/med/p95/max)'
    );
    console.log(
      '  (a pair is one validation scope; per-command coverage needs one per GPUErrorFilter)'
    );
    for (const p of sweep.points) {
      const sync = spread(p.sync);
      const settle = spread(p.settle);
      console.log(
        `  ${String(p.pairs).padStart(5)}   ` +
          `${ms(sync.min)}/${ms(sync.median)}/${ms(sync.p95)}/${ms(sync.max)}`.padEnd(
            36
          ) +
          `${ms(settle.min)}/${ms(settle.median)}/${ms(settle.p95)}/${ms(settle.max)}`
      );
    }
  }
} catch (error) {
  console.error(`error-scope-bench: ${error.message}`);
  browser?.stop();
  await server.close();
  process.exit(2);
}

browser?.stop();
await server.close();
process.exit(0);
