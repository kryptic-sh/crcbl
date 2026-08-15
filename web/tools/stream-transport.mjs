#!/usr/bin/env node
// The transport's own suite: `web/engine/gpu-transport.js` against a real
// stream in a synthetic wasm memory.
//
// `stream-decode.mjs` beside this one is the *format*'s drift check — it
// decodes the committed fixture and asserts every field, and it is what goes
// red when a tag or a field order changes on one side and not the other. This
// file asserts something else entirely: that the JS half drives the ABI in
// `crates/crcbl-webgpu/src/web.rs` the way that module's docs say to. Different
// contract, different failure, so a separate file rather than a second half of
// that one.
//
// There is no wasm module here and there does not need to be. The ABI is three
// integer-returning exports and one `WebAssembly.Memory`, so the fake below is
// the whole of it: a real `Memory`, the committed fixture copied into it at a
// known offset, and three functions that answer what the Rust exports would.
// The bytes are genuinely a stream `crcbl_webgpu::StreamWriter` produced —
// `crates/crcbl-webgpu/tests/fixture.rs` writes them — so nothing here is
// asserting against a stream this file invented.
//
// THE NOT-READY CASE IS DRIVEN, NOT ASSUMED. Every export answers `0` before
// the engine installs a channel, and a shim that read that as a failure — or,
// worse, as a zero-length frame at address zero — would break on the first
// `requestAnimationFrame` of every page. It gets its own fake and its own
// checks below, because a suite that only ever ran the ready path would be the
// guard whose scope matches nothing.
//
// Usage:
//   node web/tools/stream-transport.mjs [path-to-fixture.bin]

import { readFile } from 'node:fs/promises';
import { deepStrictEqual } from 'node:assert/strict';

import { takeCommandStream } from '../engine/gpu-transport.js';
import { StreamDecodeError } from '../engine/gpu-stream.js';

/** The fixture `crcbl-webgpu`'s `fixture.rs` writes. */
const FIXTURE = new URL(
  '../../crates/crcbl-webgpu/tests/fixtures/canonical-stream.bin',
  import.meta.url
);

/** `tag::HEADER_BYTES`: the magic, the version word, and the base sequence. */
const HEADER_BYTES = 8 + 2 + 8;

/**
 * Where the fixture is written inside the synthetic memory.
 *
 * Deliberately not zero. A transport that mixed up "no channel" (`ptr === 0`)
 * with a real address would still work if the stream happened to live at the
 * bottom of the heap, and no real allocation ever does.
 */
const STREAM_PTR = 4096;

/** @type {string[]} */
const failures = [];

/**
 * @param {boolean} condition
 * @param {string} what
 */
function check(condition, what) {
  if (condition) console.log(`  ok   ${what}`);
  else {
    console.log(`  FAIL ${what}`);
    failures.push(what);
  }
}

/**
 * @param {unknown} actual
 * @param {unknown} expected
 * @param {string} what
 */
function checkEqual(actual, expected, what) {
  try {
    deepStrictEqual(actual, expected);
    check(true, what);
  } catch (error) {
    const detail = String(error instanceof Error ? error.message : error)
      .split('\n')
      .map((line) => `       ${line}`)
      .join('\n');
    check(false, `${what}\n${detail}`);
  }
}

/**
 * A wasm instance that has a channel installed and a frame waiting.
 *
 * The three exports are what `crates/crcbl-webgpu/src/web.rs` documents:
 * `len` is the readiness test and is never zero while a channel is installed,
 * `ptr` is stable across all three calls, and `release` clears the frame. This
 * one records its calls so the obligations can be asserted rather than assumed.
 *
 * @param {Uint8Array} stream
 */
function readyInstance(stream) {
  const memory = new WebAssembly.Memory({ initial: 1 });
  new Uint8Array(memory.buffer, STREAM_PTR, stream.length).set(stream);
  const calls = { len: 0, ptr: 0, release: 0 };
  /** Cleared by `release`, as `StreamWriter::clear` does. */
  let live = stream.length;
  const exports = {
    __crcbl_web_gpu_stream_len: () => {
      calls.len += 1;
      return live;
    },
    __crcbl_web_gpu_stream_ptr: () => {
      calls.ptr += 1;
      return STREAM_PTR;
    },
    __crcbl_web_gpu_stream_release: () => {
      calls.release += 1;
      live = HEADER_BYTES;
      return 1;
    },
  };
  return { memory, exports, calls };
}

/**
 * A wasm instance the engine has not installed a channel into yet.
 *
 * Every export answers `0`, which is the documented "a shim that started before
 * the engine did" case and not a failure to report.
 */
function notReadyInstance() {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const calls = { len: 0, ptr: 0, release: 0 };
  const exports = {
    __crcbl_web_gpu_stream_len: () => {
      calls.len += 1;
      return 0;
    },
    __crcbl_web_gpu_stream_ptr: () => {
      calls.ptr += 1;
      return 0;
    },
    __crcbl_web_gpu_stream_release: () => {
      calls.release += 1;
      return 0;
    },
  };
  return { memory, exports, calls };
}

/**
 * @param {number} index
 * @param {number} generation
 */
function handle(index, generation) {
  return { index, generation };
}

/**
 * The first and last commands of the fixture, spelled out rather than taken
 * from a decode.
 *
 * `stream-decode.mjs` is where every field of every command is pinned; the two
 * here are enough to say the transport handed the decoder the right bytes at
 * the right offset, and they are the two a wrong offset breaks first — an
 * off-by-one at the front loses the header, an off-by-one at the back truncates
 * the tail.
 */
const FIRST_COMMAND = {
  name: 'CreateBuffer',
  buffer: handle(11, 12),
  label: 'instances',
  size: 4096n,
  usage: ['TRANSFER_DST', 'STORAGE'],
  memory: 'DeviceLocal',
};

const LAST_COMMAND = {
  name: 'Draw',
  vertices: { start: 6, end: 9 },
  instances: { start: 1, end: 5 },
};

/** How many commands `corpus::every_command` holds. */
const COMMAND_COUNT = 12;

async function main() {
  const override = process.argv.slice(2).find((arg) => !arg.startsWith('--'));
  const path = override === undefined ? FIXTURE : override;
  const fixture = new Uint8Array(await readFile(path));

  console.log(
    `stream-transport: ${override ?? FIXTURE.pathname} (${fixture.length} bytes)`
  );

  // ---- a frame is read out of wasm memory and decoded ----------------------
  // The throw is caught rather than left to node, for `stream-decode.mjs`'s
  // reason: a transport that hands the decoder the wrong window fails on the
  // very first call, and that should land as a named check rather than as a
  // stack trace out of a file this suite is not testing.
  const ready = readyInstance(fixture);
  /** @type {{ baseSequence: bigint, commands: object[] } | null} */
  let frame = null;
  let threw = false;
  try {
    frame = takeCommandStream(ready);
  } catch (error) {
    threw = true;
    check(false, `the waiting frame is taken at all (threw ${String(error)})`);
  }
  if (frame === null) {
    if (!threw) check(false, 'a waiting frame is taken (got null instead)');
    console.error(`\nstream-transport: FAILED (${failures.length})`);
    process.exit(1);
  }
  check(
    frame.commands.length === COMMAND_COUNT,
    `the frame holds ${COMMAND_COUNT} commands (took ${frame.commands.length})`
  );
  checkEqual(
    frame.commands[0],
    FIRST_COMMAND,
    'the first command is decoded from the byte after the header'
  );
  checkEqual(
    frame.commands.at(-1),
    LAST_COMMAND,
    'the last command is decoded from the last byte of the frame'
  );
  check(
    frame.baseSequence === 0n,
    `the header's base sequence is carried out (got ${frame.baseSequence})`
  );

  // ---- …and it is read from the header, not assumed to be zero ------------
  // The fixture's base is zero, so the check above passes for a transport that
  // simply says zero. A frame recorded after any earlier one has a base that is
  // not, and that is the only number connecting a replayer's failure back to
  // the Rust that encoded the command — so it is worth a header that says so.
  const resumed = fixture.slice();
  new DataView(
    resumed.buffer,
    resumed.byteOffset,
    resumed.byteLength
  ).setBigUint64(HEADER_BYTES - 8, 1234567890123n, true);
  const resumedFrame = takeCommandStream(readyInstance(resumed));
  check(
    resumedFrame?.baseSequence === 1234567890123n,
    `a non-zero base sequence is carried out verbatim (got ${resumedFrame?.baseSequence})`
  );

  // ---- the buffer is handed back, exactly once ----------------------------
  // The obligation the Rust module docs state: a frame that is never released
  // is one the next frame appends to, and the buffer only grows.
  checkEqual(
    ready.calls,
    { len: 1, ptr: 1, release: 1 },
    'one length, one pointer and one release per frame'
  );
  check(
    ready.exports.__crcbl_web_gpu_stream_len() === HEADER_BYTES,
    'the release left an empty frame behind, not a full one'
  );

  // ---- nothing decoded is a view onto wasm memory -------------------------
  // The detached-view rule, proved rather than asserted: growing the memory
  // detaches every view over its old buffer, so a command still readable
  // afterwards is one that copied its bytes out. `PushConstants.data` is the
  // only byte payload in the fixture and therefore the only thing that could
  // have been left as a window on the heap.
  const push = frame.commands.find((c) => c.name === 'PushConstants');
  ready.memory.grow(1);
  checkEqual(
    push?.data,
    new Uint8Array([0xde, 0xad, 0xbe, 0xef, 0x00, 0x01]),
    'a decoded payload survives a memory.grow that detaches every view'
  );
  checkEqual(frame.commands[0], FIRST_COMMAND, 'so does the rest of the frame');

  // ---- an engine that has not booted reads as not-ready, not as failure ----
  const early = notReadyInstance();
  let earlyFrame;
  let earlyThrew = null;
  try {
    earlyFrame = takeCommandStream(early);
  } catch (error) {
    earlyThrew = error;
  }
  check(
    earlyThrew === null,
    earlyThrew === null
      ? 'a shim that starts before the engine does not throw'
      : `a shim that starts before the engine threw ${String(earlyThrew)}`
  );
  check(
    earlyFrame === null,
    `no channel installed answers null (got ${JSON.stringify(earlyFrame)})`
  );
  // The length is the readiness test, so nothing else may be called: a pointer
  // read at that moment is address zero, and releasing a frame that does not
  // exist is a call wasm has to answer for no reason.
  checkEqual(
    early.calls,
    { len: 1, ptr: 0, release: 0 },
    'not-ready is settled by the length alone'
  );

  // ---- an empty frame is not the same fact as no frame --------------------
  // Both are "no commands to replay", and a transport that collapsed them would
  // make a booted engine indistinguishable from one that never started.
  const idle = readyInstance(fixture.subarray(0, HEADER_BYTES));
  const idleFrame = takeCommandStream(idle);
  checkEqual(
    idleFrame,
    { baseSequence: 0n, commands: [] },
    'an installed channel with nothing recorded is an empty frame, not null'
  );
  checkEqual(
    idle.calls,
    { len: 1, ptr: 1, release: 1 },
    'and it is released like any other'
  );

  // ---- a frame that will not decode is still handed back ------------------
  // Otherwise the corrupt bytes stay in the buffer, the next frame appends to
  // them, and the shim meets the same failure forever.
  const corrupt = fixture.slice();
  corrupt[HEADER_BYTES] = 0xff; // a tag no command claims
  const broken = readyInstance(corrupt);
  let thrown = null;
  try {
    takeCommandStream(broken);
  } catch (error) {
    thrown = error;
  }
  check(
    thrown instanceof StreamDecodeError && thrown.kind === 'UnknownTag',
    `a corrupt frame throws the decoder's own error (got ${String(thrown)})`
  );
  check(
    broken.calls.release === 1,
    'a frame that threw mid-decode is released anyway'
  );

  if (failures.length > 0) {
    console.error(`\nstream-transport: FAILED (${failures.length})`);
    process.exit(1);
  }
  console.log('\nstream-transport: OK');
}

await main();
