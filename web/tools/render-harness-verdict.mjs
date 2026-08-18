// The verdict half of the WebGPU parity gate, and the only thing in it that
// decides an exit status.
//
// The gate has two halves that each know half of what a scene's outcome is:
// `web/tools/render-harness-e2e.mjs` knows whether the browser backend got the
// scene through at all, and `compare-readback` knows whether the pixels it
// produced are the golden's. Neither can answer for the other, and neither
// exit code names a *scene* — they say that something failed. So the two exit
// codes were the whole verdict, and "these two scenes fail here because
// SwiftShader is not the rasteriser the goldens came from" had nowhere to live
// except a sentence in a backlog. This is where it lives instead.
//
//   node web/tools/render-harness-verdict.mjs --driver-json <path>
//        --compare-log <path> --driver-exit N --compare-exit N
//        [--expect-fail cube,ssr]
//
// THE EXPECTED-FAIL LIST IS EXACT IN BOTH DIRECTIONS, which is the entire
// design and is copied deliberately from `web/tools/probe-e2e.mjs`:
//
//   * A listed scene that fails is excused, and **named in the output as
//     excused** — a run that quietly swallowed it would be a gate reporting a
//     stronger claim than it made.
//   * A listed scene that **passes fails the run**, spelled "the list is
//     stale". Without this the list rots into a blanket suppression: the day
//     SwiftShader starts matching, the entry goes on covering whatever breaks
//     in that scene next, and nothing says so.
//   * A listed name that is not a scene at all fails the run too. It is the
//     same defect wearing a typo.
//   * Anything unlisted that fails still fails.
//   * **The list cannot empty the run.** A run where no scene both passed and
//     was un-excused gated nothing, and fails whatever the list says.
//
// IT CROSS-CHECKS ITSELF AGAINST BOTH EXIT CODES. The comparator's table is
// read out of its stdout, which is a text parse, and a text parse that silently
// stops matching is a gate that silently passes everything. So the two exit
// codes are handed in and held against what was parsed: a comparator that
// exited 0 while the table has a `no` in it, or exited 1 with none, means the
// parse no longer describes the tool, and that is exit 2 — the gate did not
// run — rather than a verdict.
//
// EXIT CODES
//   0  every scene rendered and matched, but for the excused ones.
//   1  a scene failed unexpectedly, an excused one has started passing, the
//      list names something that is not a scene, or nothing was gated.
//   2  it could not reach a verdict: an unreadable input, or a parse that
//      disagrees with the exit code the tool it parsed actually returned.

import { readFileSync } from 'node:fs';

/** Scene names are the `golden_names()` basenames: lower snake case. */
const SCENE_NAME = /^[a-z][a-z0-9_]*$/;

/** The comparator's table header, `print_table` in compare-readback.rs. */
const TABLE_HEADER = /^scene\s+matched\s+detail\s*$/;

/** One row of it: `{:name_width$}  {:7}  {}`. */
const TABLE_ROW = /^(\S+)\s{2,}(yes|no)\s{2,}(.*)$/;

function bail(message) {
  console.error(`render-harness-verdict: ${message}`);
  process.exit(2);
}

function parseArgs(argv) {
  const args = {
    driverJson: null,
    compareLog: null,
    driverExit: null,
    compareExit: null,
    expectFail: [],
  };
  const numeric = new Set(['--driver-exit', '--compare-exit']);
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const value = argv[i + 1];
    switch (flag) {
      case '--driver-json':
      case '--compare-log':
      case '--driver-exit':
      case '--compare-exit':
      case '--expect-fail': {
        if (value === undefined) bail(`${flag} needs a value`);
        i += 1;
        if (numeric.has(flag)) {
          const parsed = Number(value);
          if (!Number.isInteger(parsed)) bail(`${flag} takes an integer`);
          args[flag === '--driver-exit' ? 'driverExit' : 'compareExit'] =
            parsed;
        } else if (flag === '--expect-fail') {
          args.expectFail = value.split(/[\s,]+/).filter(Boolean);
        } else {
          args[flag === '--driver-json' ? 'driverJson' : 'compareLog'] = value;
        }
        break;
      }
      default:
        bail(`unknown option ${flag}`);
    }
  }
  if (!args.driverJson) bail('--driver-json is required');
  if (!args.compareLog) bail('--compare-log is required');
  if (args.driverExit === null) bail('--driver-exit is required');
  if (args.compareExit === null) bail('--compare-exit is required');
  // Shape only. Whether a listed name is a scene *of this run* is decided at
  // the verdict, by the same rule that catches one whose scene has started
  // passing, and with a message that says which.
  const misshapen = args.expectFail.filter((name) => !SCENE_NAME.test(name));
  if (misshapen.length) {
    bail(
      `--expect-fail takes scene names like "ssr,ui", got "${misshapen.join('", "')}"`
    );
  }
  return args;
}

/**
 * Reads the comparator's per-scene table out of its stdout.
 *
 * Bounded by the header and the blank line that follows the last row rather
 * than by matching rows anywhere in the text: the summary lines below the table
 * are also `<word> <word> <rest>`, and a row regex loose enough to be applied
 * to the whole file is one that can pick them up. A header that is not there,
 * or a line inside the table that does not parse, is a hard stop — the
 * comparator's output changed shape and this tool is reading fiction.
 */
function parseCompareTable(text) {
  const lines = text.split('\n');
  const header = lines.findIndex((line) => TABLE_HEADER.test(line));
  if (header < 0) {
    bail(
      "the comparator's output has no `scene  matched  detail` table in it — " +
        'either it did not run, or its output changed shape and this parse is stale'
    );
  }
  const rows = [];
  // +2 steps over the header and the rule of dashes under it.
  for (let i = header + 2; i < lines.length; i += 1) {
    if (lines[i].trim() === '') break;
    const match = TABLE_ROW.exec(lines[i]);
    if (!match) {
      bail(
        `line ${i + 1} of the comparator's table did not parse: ${JSON.stringify(lines[i])}`
      );
    }
    rows.push({
      scene: match[1],
      matched: match[2] === 'yes',
      detail: match[3].trim(),
    });
  }
  return rows;
}

/**
 * Joins one scene's two half-verdicts into one, and says why when it fails.
 *
 * A scene is a pass only if the browser got it through, the device refused
 * nothing while it did, and the pixels are the golden's. All three are
 * failures of the same thing — "this backend draws that scene right in a
 * browser" — and the expected-fail list is per scene, so they have to reduce to
 * one answer per scene before it can be applied.
 */
function verdict(entry) {
  const why = [];
  if (!entry.driver) {
    why.push('the browser driver never reported it');
  } else {
    if (!entry.driver.rendered) {
      const detail =
        entry.driver.fatal ??
        entry.driver.replayFailure ??
        entry.driver.error ??
        (entry.driver.timedOut
          ? `timed out after ${entry.driver.frames} frames`
          : `state ${entry.driver.stateName}`);
      why.push(`did not render: ${String(detail).split('\n')[0]}`);
    }
    const refused = entry.driver.deviceErrors ?? [];
    if (refused.length) {
      // A refused command does not throw, so a scene can reach `rendered` with
      // every draw in it rejected. Counted as a failure for that reason.
      why.push(
        `the device refused ${refused.length} command(s): ${refused[0].split('\n')[0]}`
      );
    }
  }
  if (!entry.compare) {
    why.push('the comparator never compared it');
  } else if (!entry.compare.matched) {
    why.push(entry.compare.detail);
  }
  return { scene: entry.scene, ok: why.length === 0, why };
}

const args = parseArgs(process.argv.slice(2));

let driver;
try {
  driver = JSON.parse(readFileSync(args.driverJson, 'utf8'));
} catch (error) {
  bail(`could not read ${args.driverJson}: ${error.message}`);
}
let compareText;
try {
  compareText = readFileSync(args.compareLog, 'utf8');
} catch (error) {
  bail(`could not read ${args.compareLog}: ${error.message}`);
}

const driverScenes = Array.isArray(driver.scenes) ? driver.scenes : [];
const rows = parseCompareTable(compareText);

// --- the parse held against the exit codes it claims to explain --------------
//
// Both directions, because both are a false verdict. A comparator that exited 0
// with a `no` parsed out of its table means this tool is about to fail a green
// run; one that exited 1 with none means it is about to pass a red one, which
// is the direction that ends up shipping.
const anyMismatch = rows.some((row) => !row.matched);
if ((args.compareExit === 0) === anyMismatch) {
  bail(
    `the comparator exited ${args.compareExit} while its table shows ` +
      `${anyMismatch ? 'a mismatch' : 'no mismatch'} — the parse no longer describes the tool`
  );
}
const driverClean =
  driverScenes.length > 0 &&
  driverScenes.every(
    (scene) => scene.rendered && (scene.deviceErrors ?? []).length === 0
  );
if ((args.driverExit === 0) !== driverClean) {
  bail(
    `the driver exited ${args.driverExit} while its result says ` +
      `${driverClean ? 'every scene rendered cleanly' : 'a scene did not'} — ` +
      `${args.driverJson} does not describe that run`
  );
}

// --- one row per scene, from either half ------------------------------------
//
// Keyed by name and unioned rather than zipped: the two halves each walk their
// own copy of the scene list, and a scene that appears in one and not the other
// is a discrepancy worth failing on rather than an index to line up.
const merged = new Map();
for (const scene of driverScenes) {
  merged.set(scene.scene, { scene: scene.scene, driver: scene, compare: null });
}
for (const row of rows) {
  const entry = merged.get(row.scene) ?? {
    scene: row.scene,
    driver: null,
    compare: null,
  };
  entry.compare = row;
  merged.set(row.scene, entry);
}
const scenes = [...merged.values()].map(verdict);

const expectFail = args.expectFail;
const failing = scenes.filter((scene) => !scene.ok);
const excused = failing.filter((scene) => expectFail.includes(scene.scene));
const unexpected = failing.filter((scene) => !expectFail.includes(scene.scene));
const known = new Set(scenes.map((scene) => scene.scene));
const absent = expectFail.filter((name) => !known.has(name));
const stale = expectFail.filter((name) =>
  scenes.some((scene) => scene.scene === name && scene.ok)
);
// The scenes this run actually gated: passed, and not covered by the list.
const gated = scenes.filter(
  (scene) => scene.ok && !expectFail.includes(scene.scene)
);

// --- the report --------------------------------------------------------------

const nameWidth = Math.max(5, ...scenes.map((scene) => scene.scene.length));
const pad = (text) => String(text).padEnd(nameWidth);
console.log(`\n${pad('scene')}  verdict   detail`);
console.log('-'.repeat(nameWidth + 2 + 9 + 2 + 40));
for (const scene of scenes) {
  const state = scene.ok
    ? 'pass'
    : expectFail.includes(scene.scene)
      ? 'excused'
      : 'FAIL';
  console.log(
    `${pad(scene.scene)}  ${state.padEnd(9)}  ${scene.why.join('; ')}`
  );
}

console.log(
  `\nrender-harness-verdict: ${scenes.length - failing.length}/${scenes.length} ` +
    'scene(s) rendered through the browser backend and matched their golden'
);

// Said on a green run too, and scene by scene rather than only as a list of
// names: "the run passed" and "these two claims were not made" have to be
// readable off the same log, or the second one is not really said.
if (expectFail.length) {
  console.log(
    `render-harness-verdict: expected to fail on this platform: ${expectFail.join(' ')}`
  );
  for (const scene of excused) {
    console.log(`  excused ${scene.scene}: ${scene.why.join('; ')}`);
  }
}

if (driver.fatal) {
  // Never excusable, and separate from any scene: the wasm module aborted under
  // the page, so every scene after it was driven against a poisoned module and
  // no per-scene verdict from this run means what it says.
  console.error(
    `\nrender-harness-verdict: the harness reported a fatal: ${String(driver.fatal).split('\n')[0]}`
  );
}

if (unexpected.length) {
  console.error('\nrender-harness-verdict: FAILED');
  for (const scene of unexpected) {
    console.error(`  ${scene.scene}: ${scene.why.join('; ')}`);
  }
}

// After the real failures rather than instead of them: a stale entry and a
// regression arriving together is exactly the run where seeing only one sends
// the reader to the wrong half.
if (stale.length || absent.length) {
  console.error(
    '\nrender-harness-verdict: THE EXPECTED-FAIL LIST NO LONGER DESCRIBES THIS RUN'
  );
  for (const name of stale) {
    console.error(
      `  ${name}: it rendered and matched its golden, and --expect-fail names it. ` +
        'Drop it from the list — this platform draws that scene right now.'
    );
  }
  for (const name of absent) {
    console.error(
      `  ${name}: --expect-fail names it and this run has no such scene. ` +
        'Either it is not a golden scene name, or the run never reached it.'
    );
  }
}

if (scenes.length === 0) {
  console.error(
    '\nrender-harness-verdict: ZERO SCENES WERE COMPARED — the gate is not gating.'
  );
  process.exit(1);
}
if (gated.length === 0) {
  // The trap the whole mechanism has to be immune to: a list long enough to
  // cover everything left standing turns a run that proved nothing into a pass.
  console.error(
    '\nrender-harness-verdict: NOTHING WAS GATED — every scene either failed or ' +
      'is on the expected-fail list, so this run made no claim at all.'
  );
  process.exit(1);
}

if (unexpected.length || stale.length || absent.length || driver.fatal) {
  process.exit(1);
}

if (expectFail.length) {
  console.log(
    `render-harness-verdict: ${gated.length} scene(s) drew the golden picture in a browser; ` +
      `${expectFail.join(' ')} are excused here, and this run would have failed had any of them passed`
  );
} else {
  console.log(
    'render-harness-verdict: every golden scene drew the golden picture in a browser'
  );
}
process.exit(0);
