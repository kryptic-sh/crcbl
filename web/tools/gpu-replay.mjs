#!/usr/bin/env node
// The replayer, driven under node against a `navigator.gpu` that is not one.
//
// `web/engine/gpu-replay.js` is the half of this seam that actually calls the
// browser, and calling the browser is exactly what no check in CI can do. What
// *can* be checked without one, and is checked here, is everything around that
// call: which command it answers, which sequence the answer names, that the
// answer is not available on the frame that asked for it, and that every way of
// getting no adapter still produces a reply rather than silence.
//
// WHY THAT LAST ONE IS THE POINT. A dropped reply is a command wasm waits on for
// ever — the one unrecoverable bug this channel can have. There are four ways to
// fail to get an adapter: the promise resolves `null`, the promise rejects,
// `navigator.gpu` is absent altogether, and the reason string is too long for
// the writer's cap. Each is driven below, and each has to end with a reply.
//
// AND THE DEVICE REQUEST HAS FIVE MORE, which is why it has a section of its
// own: an adapter id nothing enumerated, a `compatible_surface` this replayer
// has no table for, a required feature with no `GPUFeatureName` at all, a
// required feature whose name exists and whose adapter does not have it, and a
// `requestDevice` that simply rejects. The first four are refused *before* the
// browser is asked — each is something WebGPU cannot be told — and every one of
// the five still answers.
//
// THE OTHER CLAIM THIS SECTION MAKES is that a device's capabilities are the
// device's. The stub adapter's limits and the stub device's differ in every
// member and their feature sets differ too, so a replayer that built its reply
// from the adapter it opened — the obvious mistake, since the adapter is right
// there — produces different numbers rather than plausible ones.
//
// WHAT THIS DOES *NOT* CHECK, so nobody reads more into a green run than is
// there. The expected bytes below are built with the same `ReplyWriter` the
// replayer uses, so this says nothing about the reply *format* — a tag or a
// field order wrong in `gpu-reply.js` would produce matching bytes on both
// sides here. That is `reply-encode.mjs`'s job, against the fixture Rust
// commits. What is not circular is everything this file is actually about: the
// replayer's choice of reply, of sequence, and of when.
//
// The command stream it replays is the committed fixture rather than a
// hand-built buffer, so the command it dispatches on is one the Rust encoder
// really wrote.
//
// Usage:
//   node web/tools/gpu-replay.mjs [path-to-fixture.bin]

import { readFile } from 'node:fs/promises';
import { deepStrictEqual } from 'node:assert/strict';

import { DEVICE_TYPE, ReplyWriter } from '../engine/gpu-reply.js';
import {
  ReplayError,
  Replayer,
  halAdapterInfoFor,
  halDeviceCapsFor,
  halFeaturesFor,
  halLimitsFor,
  webgpuFeaturesFor,
} from '../engine/gpu-replay.js';
import { decodeStream } from '../engine/gpu-stream.js';

/** The fixture `crcbl-webgpu`'s `fixture.rs` writes. */
const FIXTURE = new URL(
  '../../crates/crcbl-webgpu/tests/fixtures/canonical-stream.bin',
  import.meta.url
);

/**
 * `NO_ADAPTER_REASON` in `web/engine/gpu-replay.js`, restated rather than
 * imported: a value taken from the thing under test agrees with it by
 * construction, and this string is what a page ends up showing a person.
 */
const NO_ADAPTER_REASON = 'navigator.gpu.requestAdapter() granted no adapter';

/** `MAX_REASON_CHARS` in `web/engine/gpu-replay.js`, restated for that reason. */
const MAX_REASON_CHARS = 512;

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
 * Lets every queued microtask and promise callback run.
 *
 * A `setTimeout` rather than an `await Promise.resolve()`: the replayer's chain
 * is several `.then`s deep and a macrotask boundary drains all of them however
 * many that becomes.
 */
function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/**
 * A `navigator.gpu` that answers however the test needs.
 *
 * @param {() => Promise<object | null>} answer
 */
function stubGpu(answer) {
  const stub = {
    calls: 0,
    requestAdapter() {
      stub.calls += 1;
      return answer();
    },
  };
  return stub;
}

/**
 * A `GPUSupportedLimits` with a **distinct** value in every member.
 *
 * Distinct on purpose and not plausible on purpose: the mapping is nineteen
 * assignments between two sets of names that mostly resemble each other, so two
 * members swapped is the likeliest way to get it wrong, and a stub full of
 * realistic numbers — where several members legitimately agree — would let that
 * pass. Every number here is its own.
 */
function stubLimits() {
  return {
    maxTextureDimension2D: 101,
    maxTextureDimension3D: 102,
    maxTextureArrayLayers: 103,
    maxStorageBufferBindingSize: 104,
    maxUniformBufferBindingSize: 105,
    maxBindGroups: 106,
    maxColorAttachments: 107,
    maxComputeWorkgroupSizeX: 108,
    maxComputeWorkgroupSizeY: 109,
    maxComputeWorkgroupSizeZ: 110,
    maxComputeInvocationsPerWorkgroup: 111,
    maxComputeWorkgroupsPerDimension: 112,
    minUniformBufferOffsetAlignment: 113,
    minStorageBufferOffsetAlignment: 114,
    // Members WebGPU has that the seam does not read. Present so the stub is
    // shaped like the real thing, and so a mapping that reached for one of them
    // by mistake would land on a number nothing expects.
    maxBufferSize: 115,
    maxVertexBuffers: 116,
    maxBindingsPerBindGroup: 117,
  };
}

/**
 * An adapter shaped like the browser's, with every `info` field filled.
 *
 * `features` and `limits` are what a real `GPUAdapter` always has; the replayer
 * reads both to build its reply, so a stub without them is not one.
 *
 * @param {object} info
 * @param {string[]} [features] `GPUFeatureName`s the adapter reports.
 */
function stubAdapter(info, features = []) {
  return { info, features: new Set(features), limits: stubLimits() };
}

/**
 * The two commands this file dispatches on, taken from the committed fixture by
 * `main` before anything below runs.
 *
 * From the fixture rather than written out here, so what is replayed is a
 * command the Rust encoder really wrote — the same reason the expected fields
 * in `stream-decode.mjs` are *not* taken from a decoder.
 *
 * @type {{ enumerate: object, requestDevice: object }}
 */
const FROM_FIXTURE = { enumerate: null, requestDevice: null };

/** A one-command frame carrying `command` at `sequence`. */
function frameOf(command, sequence) {
  return { baseSequence: sequence, commands: [command] };
}

/**
 * A `GPUSupportedLimits` for a **device**, distinct in every member from
 * `stubLimits`.
 *
 * The point of the whole device section: WebGPU gives a device the limits it
 * was created with — the specification's defaults, since a `DeviceDesc` asks
 * for none — and those are not the adapter's ceilings. A replayer that read a
 * device's capabilities off its adapter would produce `stubLimits`'s numbers
 * here, and every one of them differs.
 */
function deviceLimits() {
  return {
    maxTextureDimension2D: 201,
    maxTextureDimension3D: 202,
    maxTextureArrayLayers: 203,
    maxStorageBufferBindingSize: 204,
    maxUniformBufferBindingSize: 205,
    maxBindGroups: 206,
    maxColorAttachments: 207,
    maxComputeWorkgroupSizeX: 208,
    maxComputeWorkgroupSizeY: 209,
    maxComputeWorkgroupSizeZ: 210,
    maxComputeInvocationsPerWorkgroup: 211,
    maxComputeWorkgroupsPerDimension: 212,
    minUniformBufferOffsetAlignment: 213,
    minStorageBufferOffsetAlignment: 214,
  };
}

/**
 * A `GPUDevice` as `requestDevice()` resolves one: its own features, its own
 * limits, and nothing else this replayer reads.
 *
 * @param {string[]} [features]
 */
function stubDevice(features = []) {
  return { features: new Set(features), limits: deviceLimits() };
}

/**
 * An adapter that opens devices and records what it was asked for.
 *
 * `requests` is what says the descriptor this replayer built is the one WebGPU
 * would want — the feature *names*, which is the half of the mapping the reply
 * comparisons cannot see.
 *
 * @param {object} [options]
 * @param {string[]} [options.features] `GPUFeatureName`s the adapter has.
 * @param {object} [options.device] What `requestDevice` resolves to.
 * @param {unknown} [options.refuse] What it rejects with instead.
 */
function openingAdapter({ features = [], device, refuse } = {}) {
  const adapter = {
    info: { vendor: 'crcbl', device: 'stub' },
    features: new Set(features),
    limits: stubLimits(),
    /** @type {object[]} */
    requests: [],
    async requestDevice(desc) {
      adapter.requests.push(desc);
      if (refuse !== undefined) throw refuse;
      return device ?? stubDevice();
    },
  };
  return adapter;
}

/**
 * Enumerates `adapter`, then replays `command` and lets the answer settle.
 *
 * The two-step is the seam's own shape and not test scaffolding: a device
 * request names an adapter by the id an *enumeration* gave it, so a replayer
 * that had never enumerated has nothing to open. The enumeration's own reply is
 * cleared in between so what comes back is the device answer alone — a `clear`
 * a page would only ever do once wasm had taken the bytes.
 *
 * @param {object} adapter
 * @param {object} command
 * @param {bigint} sequence
 */
async function openDevice(adapter, command, sequence) {
  const replayer = new Replayer({ gpu: stubGpu(async () => adapter) });
  replayer.replay(frameOf(FROM_FIXTURE.enumerate, 0n));
  await settle();
  replayer.clear();
  replayer.replay(frameOf(command, sequence));
  await settle();
  return replayer;
}

/**
 * @param {number} index
 * @param {number} generation
 */
function handle(index, generation) {
  return { index, generation };
}

/**
 * The one reply in `bytes`, as far as the checks here need to read it.
 *
 * A READER FOR THIS FILE, AND NOT A CODEC. The reply format has exactly two
 * implementations and both are held to the committed fixture — Rust writes it,
 * `reply-encode.mjs` re-encodes it byte for byte — so nothing here defines
 * anything. What this exists for is the refusals: their reason is a string the
 * replayer *composes*, and asserting on whole buffers would mean restating that
 * wording verbatim, which pins prose rather than behaviour. So the two fields
 * that are behaviour — which reply came back, and the feature bits it blames —
 * are read out and asserted, and the wording is only ever checked for the part
 * that has to be actionable.
 *
 * Only the two device replies are understood; anything else is a bug in the
 * check that called this. Of a `Device` it reads the feature word and the first
 * limit, which are the two the checks below are about — and reading them *out
 * of the buffer* is what makes those checks about the replayer rather than
 * about the mapping function they would otherwise call twice.
 *
 * @param {Uint8Array} bytes A whole reply buffer, header included.
 * @returns {{ tag: string, sequence: bigint, features?: bigint,
 *             maxImage2d?: number, reason?: string, unsupported?: bigint }}
 */
function decodeOneReply(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  // The header is the magic and the version word; every reply then opens with
  // its tag and the sequence it answers.
  let at = 10;
  const tag = view.getUint8(at);
  const sequence = view.getBigUint64(at + 1, true);
  at += 9;
  if (tag === 0x02) {
    return {
      tag: 'Device',
      sequence,
      // `DeviceCaps` is the feature word and then `Limits` in declaration
      // order, whose first field is `max_image_2d`.
      features: view.getBigUint64(at, true),
      maxImage2d: view.getUint32(at + 8, true),
    };
  }
  if (tag !== 0x03) {
    throw new Error(`decodeOneReply does not read tag 0x${tag.toString(16)}`);
  }
  const length = view.getUint32(at, true);
  const reason = new TextDecoder('utf-8', { fatal: true }).decode(
    bytes.subarray(at + 4, at + 4 + length)
  );
  return {
    tag: 'DeviceFailed',
    sequence,
    reason,
    unsupported: view.getBigUint64(at + 4 + length, true),
  };
}

/**
 * One decoded reply as a line a failure message can carry.
 *
 * Hand-written rather than `JSON.stringify`, which throws on a `BigInt` — and
 * both interesting fields here are one.
 *
 * @param {ReturnType<typeof decodeOneReply> | undefined} reply
 */
function shown(reply) {
  if (reply === undefined) return 'nothing';
  return (
    `${reply.tag} answering ${reply.sequence}` +
    `, unsupported 0x${(reply.unsupported ?? 0n).toString(16)}` +
    `, reason ${JSON.stringify(reply.reason ?? '')}`
  );
}

/**
 * What a `ReplyWriter` produces for one reply, for comparing against what the
 * replayer produced. See the header for what this does and does not prove.
 *
 * @param {(replies: ReplyWriter) => void} encode
 */
function expectedReplies(encode) {
  const replies = new ReplyWriter();
  encode(replies);
  return replies.bytes;
}

async function main() {
  const override = process.argv.slice(2).find((arg) => !arg.startsWith('--'));
  const path = override === undefined ? FIXTURE : override;
  const fixture = new Uint8Array(await readFile(path));
  // Caught for `stream-decode.mjs`'s reason: a decoder that has drifted from
  // the fixture refuses it outright, and nothing below this line means anything
  // once it has. That is that tool's failure to report, not this one's, so it
  // lands as one named check rather than as a stack trace out of a file this
  // suite is not testing.
  /** @type {object[]} */
  let commands;
  try {
    commands = decodeStream(fixture);
  } catch (error) {
    check(false, `the fixture decodes at all (threw ${String(error)})`);
    console.error(`\ngpu-replay: FAILED (${failures.length})`);
    process.exit(1);
  }

  console.log(
    `gpu-replay: ${override ?? FIXTURE.pathname} (${commands.length} commands)`
  );

  // ---- the command this replays is one the Rust encoder writes -------------
  const enumerateAt = commands.findIndex(
    (command) => command.name === 'EnumerateAdapters'
  );
  check(
    enumerateAt >= 0,
    `the committed stream carries an EnumerateAdapters (at ${enumerateAt})`
  );
  const enumerate = commands[enumerateAt];
  FROM_FIXTURE.enumerate = enumerate;

  // The device request the fixture carries with an adapter id an enumeration
  // can actually have granted. The corpus holds two; the other names adapter 3
  // and a surface, and is used below for exactly those refusals.
  const requestDevice = commands.find(
    (command) => command.name === 'RequestDevice' && command.adapter === 0
  );
  check(
    requestDevice !== undefined,
    'the committed stream carries a RequestDevice for adapter 0'
  );
  FROM_FIXTURE.requestDevice = requestDevice;

  // ---- the answer is not available on the frame that asked ----------------
  // The whole shape of this seam. `replay` returns having *started* the work;
  // the browser answers on its own schedule, and the sequence number is what
  // lets the reply arrive whenever that turns out to be.
  {
    const gpu = stubGpu(async () =>
      stubAdapter({ vendor: 'crcbl', device: 'stub', description: 'a stub' })
    );
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 7n));
    check(
      gpu.calls === 0 && !replayer.hasReplies && replayer.inFlight === 1,
      `replay returns without an answer (calls ${gpu.calls}, queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );

    await settle();
    check(
      gpu.calls === 1 && replayer.hasReplies && replayer.inFlight === 0,
      `the answer lands later (calls ${gpu.calls}, queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.adapter(
          7n,
          halAdapterInfoFor(
            stubAdapter({
              vendor: 'crcbl',
              device: 'stub',
              description: 'a stub',
            })
          )
        )
      ),
      'the reply is an Adapter naming the command that asked, id 0, every info field joined'
    );
  }

  // ---- the sequence is positional, and 64-bit ------------------------------
  // Nothing per command is on the wire: the nth command's number is the
  // buffer's base plus n. A base past 2^53 is where a replayer that did that
  // arithmetic in `Number` stops being exact — and stops in a way that still
  // looks like a plausible sequence.
  {
    const base = 9_007_199_254_740_993n; // 2^53 + 1
    const gpu = stubGpu(async () => stubAdapter({ device: 'positional' }));
    const replayer = new Replayer({ gpu });
    replayer.replay({ baseSequence: base, commands: [enumerate] });
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.adapter(
          base,
          halAdapterInfoFor(stubAdapter({ device: 'positional' }))
        )
      ),
      'a command at base + 0 is answered with exactly that number, past 2^53'
    );
  }

  // ---- an adapter that will not name itself is still an adapter -----------
  {
    const gpu = stubGpu(async () => stubAdapter({}));
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 4n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.adapter(4n, halAdapterInfoFor(stubAdapter({})))
      ),
      'an adapter with no info is granted with an empty name rather than refused'
    );
  }

  // ---- every way of getting no adapter still answers ----------------------
  {
    const gpu = stubGpu(async () => null);
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 5n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) => replies.noAdapter(5n, NO_ADAPTER_REASON)),
      'a promise resolving null is a NoAdapter with a reason a person can read'
    );
  }
  {
    const gpu = stubGpu(async () => {
      throw new Error('the GPU process is gone');
    });
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 6n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.noAdapter(6n, 'Error: the GPU process is gone')
      ),
      'a rejected promise is a NoAdapter carrying what it threw'
    );
  }
  {
    // No `gpu` at all — a browser without WebGPU, where the property is simply
    // absent. Nothing may be silent here: wasm has registered a wait.
    const replayer = new Replayer({ gpu: undefined });
    replayer.replay(frameOf(enumerate, 8n));
    await settle();
    check(
      replayer.hasReplies,
      'a browser with no navigator.gpu is answered rather than left waiting'
    );
  }
  {
    // A reason past the writer's cap would throw *inside a promise callback*,
    // where the throw becomes an unhandled rejection and the reply is simply
    // never queued. Truncation is the guard, and this is what holds it.
    const shout = 'x'.repeat(4 * 1024 * 1024);
    const gpu = stubGpu(async () => {
      throw new Error(shout);
    });
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 9n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.noAdapter(9n, `Error: ${shout}`.slice(0, MAX_REASON_CHARS))
      ),
      'a reason past the cap is truncated rather than dropping the reply'
    );
  }

  // ---- replies accumulate across frames -----------------------------------
  // One writer for the life of the page: two enumerations started on different
  // frames land in one buffer, and `clear` is what empties it — after wasm has
  // taken them, never before.
  {
    const gpu = stubGpu(async () => stubAdapter({ device: 'both' }));
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 1n));
    await settle();
    replayer.replay(frameOf(enumerate, 2n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) => {
        replies.adapter(1n, halAdapterInfoFor(stubAdapter({ device: 'both' })));
        replies.adapter(2n, halAdapterInfoFor(stubAdapter({ device: 'both' })));
      }),
      'two frames of answers share one buffer, in the order they settled'
    );
    replayer.clear();
    check(
      !replayer.hasReplies && replayer.replies.length === 10,
      `clear leaves a header and nothing else (${replayer.replies.length} bytes)`
    );
  }

  // ---- a command with no implementation says so, loudly -------------------
  // Skipping it would be a draw that never happened and a frame that renders
  // almost right. Every command in the corpus that is not the one this slice
  // implements has to throw, and the error has to name the sequence — the
  // number wasm's own error attribution is keyed on.
  {
    const replayer = new Replayer({ gpu: stubGpu(async () => null) });
    /** @type {string[]} */
    const wrong = [];
    const implemented = ['EnumerateAdapters', 'RequestDevice'];
    for (const [index, command] of commands.entries()) {
      if (implemented.includes(command.name)) continue;
      let thrown = null;
      try {
        replayer.replay(frameOf(command, BigInt(index)));
      } catch (error) {
        thrown = error;
      }
      if (
        !(thrown instanceof ReplayError) ||
        thrown.command !== command.name ||
        thrown.sequence !== BigInt(index)
      ) {
        wrong.push(`${command.name} at ${index}: ${String(thrown)}`);
      }
    }
    check(
      wrong.length === 0,
      wrong[0] ??
        `every command this slice cannot replay throws a ReplayError naming it and its sequence (${commands.length - 3} of them)`
    );
  }

  // ---- the device request --------------------------------------------------
  //
  // The second call that makes the round trip, and the first that has a
  // descriptor to get wrong. Four things are checked here and nowhere else: the
  // reply carries the **device's** capabilities rather than its adapter's, the
  // `requiredFeatures` list handed to WebGPU is the inverse of the mapping the
  // adapter direction uses, every refusal still answers, and a refusal carries
  // the bits that caused it.
  {
    // The answer is not available on the frame that asked, exactly as an
    // enumeration's is not.
    const adapter = openingAdapter();
    const replayer = new Replayer({ gpu: stubGpu(async () => adapter) });
    replayer.replay(frameOf(enumerate, 0n));
    await settle();
    replayer.clear();
    replayer.replay(frameOf(requestDevice, 4n));
    check(
      adapter.requests.length === 0 &&
        !replayer.hasReplies &&
        replayer.inFlight === 1,
      `replay returns without a device (asked ${adapter.requests.length}, queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );
    await settle();
    check(
      adapter.requests.length === 1 &&
        replayer.hasReplies &&
        replayer.inFlight === 0,
      `the device lands later (asked ${adapter.requests.length}, queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );
  }
  {
    // **The capabilities are the device's own.** The adapter here has four
    // mapped features and `stubLimits`'s ceilings; the device it opens has one
    // feature and `deviceLimits`'s numbers, and every one of them differs.
    const device = stubDevice(['timestamp-query']);
    const adapter = openingAdapter({
      features: [
        'depth-clip-control',
        'texture-compression-bc',
        'timestamp-query',
        'indirect-first-instance',
      ],
      device,
    });
    const replayer = await openDevice(adapter, requestDevice, 4n);
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.device(4n, halDeviceCapsFor(device))
      ),
      'the reply is a Device naming the command that asked'
    );

    // …and what that record actually holds, **read back out of the buffer**
    // rather than recomputed. A check that called `halDeviceCapsFor` again
    // would agree with the replayer whichever object the replayer had passed
    // it, which is exactly the mistake this section exists to catch. CORE is
    // the four flags core WebGPU grants; `timestamp-query` is bit 5 and is the
    // only optional one this *device* has.
    const CORE = (1n << 8n) | (1n << 7n) | (1n << 14n) | (1n << 18n);
    const sent = decodeOneReply(replayer.replies);
    checkEqual(
      sent.features,
      CORE | (1n << 5n),
      "the feature word that crossed is the device's own, not the adapter's"
    );
    checkEqual(
      sent.maxImage2d,
      201,
      "the limit that crossed is the device's own, not the adapter's"
    );
    check(
      halFeaturesFor(adapter) !== sent.features &&
        halLimitsFor(adapter).maxImage2d !== sent.maxImage2d,
      'the adapter and the device disagree, so a copy of either would be visible'
    );
    check(
      replayer.device === device,
      'the replayer holds the device it reported, so nothing may collect it'
    );
  }
  {
    // **The descriptor WebGPU is handed.** The fixture's request asks for every
    // optional feature the seam has; only the mapped names this adapter
    // actually reports may reach `requiredFeatures`, because `requestDevice`
    // fails the whole request over one it lacks — which would turn "optional"
    // into fatal.
    const adapter = openingAdapter({
      features: ['timestamp-query', 'texture-compression-bc'],
    });
    await openDevice(adapter, requestDevice, 4n);
    const asked = adapter.requests[0] ?? {};
    checkEqual(
      [...(asked.requiredFeatures ?? [])].sort(),
      ['texture-compression-bc', 'timestamp-query'],
      'only the optional features this adapter has are asked for'
    );
    check(
      !('requiredLimits' in asked),
      'no limits are requested, so the device gets the specification defaults'
    );
    check(
      !('label' in asked),
      'a descriptor with no label passes none rather than an empty one'
    );
  }
  {
    // A label that *is* present crosses, and lands on the descriptor.
    const adapter = openingAdapter();
    await openDevice(
      adapter,
      { ...requestDevice, label: 'engine', optionalFeatures: 0n },
      4n
    );
    checkEqual(
      adapter.requests[0]?.label,
      'engine',
      "the descriptor's label reaches requestDevice"
    );
  }
  {
    // **A required feature with no WebGPU name at all.** `TIMELINE_SEMAPHORE`
    // is bit 9, WebGPU has no semaphores, and this is the case that must fail
    // loudly rather than open a device without it — which is what dropping the
    // bit from the list would do.
    const adapter = openingAdapter();
    const replayer = await openDevice(
      adapter,
      { ...requestDevice, requiredFeatures: 1n << 9n },
      6n
    );
    check(
      adapter.requests.length === 0,
      'the browser is never asked for a feature it cannot express'
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' && answered.unsupported === 1n << 9n,
      `the refusal names the bits that caused it (${shown(answered)})`
    );
    check(
      String(answered?.reason).includes('bit 9'),
      `and says which flag, in a message a person can act on (${answered?.reason})`
    );
  }
  {
    // **A required feature the browser does not have.** The name exists —
    // `timestamp-query` — and this adapter simply does not report it. Refused
    // here rather than handed to `requestDevice`, which would reject with a
    // message that names no seam flag at all.
    const adapter = openingAdapter({ features: [] });
    const replayer = await openDevice(
      adapter,
      { ...requestDevice, requiredFeatures: 1n << 5n },
      7n
    );
    check(
      adapter.requests.length === 0,
      'a required feature the adapter lacks is refused before the request'
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' && answered.unsupported === 1n << 5n,
      `the refusal carries the same shape as an unmappable one (${shown(answered)})`
    );
  }
  {
    // A rejected `requestDevice`. The browser was asked and said no, so the
    // reason is the browser's and there is no feature gap to report.
    const adapter = openingAdapter({
      refuse: new Error('device creation failed'),
    });
    const replayer = await openDevice(
      adapter,
      { ...requestDevice, optionalFeatures: 0n },
      8n
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' &&
        answered.unsupported === 0n &&
        String(answered.reason).includes('device creation failed'),
      `a rejected requestDevice is answered rather than dropped (${shown(answered)})`
    );
  }
  {
    // A reason past the writer's cap would throw inside a promise callback and
    // strand the command; truncation is the guard here as it is for an adapter.
    const adapter = openingAdapter({
      refuse: new Error('x'.repeat(4 * 1024 * 1024)),
    });
    const replayer = await openDevice(
      adapter,
      { ...requestDevice, optionalFeatures: 0n },
      9n
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' &&
        String(answered.reason).length === MAX_REASON_CHARS,
      `a reason past the cap is truncated rather than dropping the reply (${String(answered?.reason).length} chars)`
    );
  }
  {
    // A device request with no enumeration behind it. The id is a position in a
    // list this replayer never answered with, so there is nothing to open — and
    // it still has to answer, because wasm is waiting on that sequence.
    const replayer = new Replayer({
      gpu: stubGpu(async () => openingAdapter()),
    });
    replayer.replay(frameOf(requestDevice, 3n));
    await settle();
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' &&
        String(answered.reason).includes('no adapter 0'),
      `an unenumerated adapter is refused and answered (${shown(answered)})`
    );
  }
  {
    // The fixture's *other* request: adapter 3, and a `compatible_surface`.
    // Both are things this replayer cannot honour, and the surface is the one
    // that must not be honoured silently — a headless device handed to a caller
    // that asked for a presentable one renders nothing and reports nothing.
    const withSurface = commands.find(
      (command) =>
        command.name === 'RequestDevice' && command.compatibleSurface !== null
    );
    check(
      withSurface !== undefined,
      'the committed stream carries a RequestDevice naming a surface'
    );
    const adapter = openingAdapter();
    const replayer = await openDevice(
      adapter,
      { ...withSurface, adapter: 0 },
      5n
    );
    check(
      adapter.requests.length === 0,
      'a request naming a surface never reaches the browser'
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' &&
        String(answered.reason).includes('surface'),
      `a compatible_surface this replayer cannot resolve is refused (${shown(answered)})`
    );
  }

  // ---- the WebGPU → seam mapping, field by field --------------------------
  //
  // NOT CIRCULAR, unlike the reply comparisons above, and deliberately kept
  // apart from them for that reason. Those build their expectation with the
  // same `halAdapterInfoFor` the replayer calls, which is fine for what they
  // assert — the choice of reply, its sequence, and when it arrives — and says
  // nothing about the mapping. Everything below spells the expected value out.
  {
    const bare = stubAdapter({});

    // The four flags core WebGPU grants with no feature name behind them:
    // COMPUTE (1<<8), OCCLUSION_QUERY (1<<7), DEPTH_BIAS_CLAMP (1<<14),
    // DEBUG_MARKERS (1<<18).
    const CORE = (1n << 8n) | (1n << 7n) | (1n << 14n) | (1n << 18n);
    checkEqual(
      halFeaturesFor(bare),
      CORE,
      'an adapter with no optional features still reports what core WebGPU grants'
    );

    // Each mapped name, one at a time, so a table row wired to the wrong bit
    // names itself instead of hiding inside a union.
    for (const [name, bit, flag] of [
      ['depth-clip-control', 1n << 13n, 'DEPTH_CLAMP'],
      ['texture-compression-bc', 1n << 16n, 'TEXTURE_COMPRESSION_BC'],
      ['timestamp-query', 1n << 5n, 'TIMESTAMP_QUERY'],
      ['indirect-first-instance', 1n << 4n, 'INDIRECT_FIRST_INSTANCE'],
    ]) {
      checkEqual(
        halFeaturesFor(stubAdapter({}, [name])),
        CORE | bit,
        `${name} maps to Features::${flag} and to nothing else`
      );
    }

    // **The other direction's loss.** Most `GPUFeatureName`s have no seam flag,
    // and a device with them has to report exactly what a device without them
    // does — otherwise a bit is being set by a name nobody chose.
    checkEqual(
      halFeaturesFor(
        stubAdapter({}, [
          'shader-f16',
          'float32-filterable',
          'float32-blendable',
          'dual-source-blending',
          'clip-distances',
          'subgroups',
          'texture-compression-etc2',
          'texture-compression-astc',
          'depth32float-stencil8',
          'rg11b10ufloat-renderable',
          'bgra8unorm-storage',
          'core-features-and-limits',
        ])
      ),
      CORE,
      'a WebGPU feature with no seam flag is dropped rather than setting one'
    );

    // The limits, against the distinct stub. Written out rather than looped
    // over the stub, so a mapping that read the wrong member has to disagree
    // with a number here.
    checkEqual(
      halLimitsFor(stubAdapter({})),
      {
        maxImage2d: 101,
        maxImage3d: 102,
        maxImageArrayLayers: 103,
        maxStorageBufferRange: 104n,
        maxUniformBufferRange: 105n,
        maxBindGroups: 106,
        // No bindless model and no push constants in WGSL, so the `0` that
        // `crcbl_hal::Limits` documents for each of their absences.
        maxBindlessDescriptors: 0,
        maxPushConstantSize: 0,
        maxColorAttachments: 107,
        // `sampleCount` is specified to be 1 or 4 and no limit reports it.
        maxSampleCount: 4,
        // Without a count buffer, one indirect call emits one draw.
        maxDrawIndirectCount: 1,
        maxComputeWorkgroupSize: [108, 109, 110],
        maxComputeInvocationsPerWorkgroup: 111,
        maxComputeWorkgroupsPerDimension: 112,
        minUniformBufferOffsetAlignment: 113n,
        minStorageBufferOffsetAlignment: 114n,
        // A spec constant — `bytesPerRow` must be a multiple of 256 — and not a
        // member of `GPUSupportedLimits` at all.
        optimalBufferCopyOffsetAlignment: 256n,
        // The floor, because WebGPU reports no anisotropy ceiling to promise.
        maxSamplerAnisotropy: 1,
        // No timestamp queries on this stub, so no tick period.
        timestampPeriodNs: 0,
      },
      'every GPUSupportedLimits member lands in the seam field that names it'
    );

    checkEqual(
      halLimitsFor(stubAdapter({}, ['timestamp-query'])).timestampPeriodNs,
      1,
      "a browser's timestamps are nanoseconds, so the period is 1 once they exist"
    );

    // **The same table read backwards**, which is what a device request needs.
    // Spelled out here too: the two directions share a table precisely so they
    // cannot disagree, and this is what says the sharing works rather than
    // that it happened.
    checkEqual(
      webgpuFeaturesFor(CORE),
      { names: [], unsatisfiable: 0n },
      'the core flags need no GPUFeatureName and ask for none'
    );
    for (const [name, bit] of [
      ['depth-clip-control', 1n << 13n],
      ['texture-compression-bc', 1n << 16n],
      ['timestamp-query', 1n << 5n],
      ['indirect-first-instance', 1n << 4n],
    ]) {
      checkEqual(
        webgpuFeaturesFor(bit),
        { names: [name], unsatisfiable: 0n },
        `${name} is what that bit asks WebGPU for, and nothing else`
      );
      // …and the round trip: what the name maps to is the bit it came from.
      checkEqual(
        halFeaturesFor(stubAdapter({}, [name])) & ~CORE,
        bit,
        `${name} survives both directions of the mapping`
      );
    }
    // Nineteen of the seam's twenty-seven flags have nothing behind them in
    // WebGPU. Each comes back as unsatisfiable rather than as silence, which is
    // what lets a *required* one fail the request.
    const unmappable =
      (1n << 9n) | (1n << 0n) | (1n << 22n) | (1n << 24n) | (1n << 12n);
    checkEqual(
      webgpuFeaturesFor(unmappable),
      { names: [], unsatisfiable: unmappable },
      'TIMELINE_SEMAPHORE, DESCRIPTOR_INDEXING, MESH_SHADER, RAY_QUERY and PUSH_CONSTANTS have no WebGPU name'
    );
    checkEqual(
      webgpuFeaturesFor(CORE | (1n << 5n) | (1n << 9n)),
      { names: ['timestamp-query'], unsatisfiable: 1n << 9n },
      'a mixed word is split into what WebGPU can be asked for and what it cannot'
    );

    // **The fields WebGPU cannot supply.** Each is the value that means absent
    // rather than one that looks real — a fabricated vendor id would be
    // indistinguishable downstream from a true one.
    const info = halAdapterInfoFor(
      stubAdapter({ vendor: 'crcbl', architecture: 'stub-1', device: 'gpu' })
    );
    check(
      info.vendorId === 0 &&
        info.deviceId === 0 &&
        info.deviceType === DEVICE_TYPE.OTHER &&
        info.driver === '' &&
        info.id === 0,
      'the fields WebGPU has no answer for are the documented absences' +
        ` (vendor ${info.vendorId}, device ${info.deviceId},` +
        ` type ${info.deviceType}, driver ${JSON.stringify(info.driver)})`
    );
    check(
      info.name === 'crcbl stub-1 gpu',
      `the name is every GPUAdapterInfo string the browser filled (${JSON.stringify(info.name)})`
    );

    // A `GPUAdapter` whose limits are not numbers is a browser lying to us, and
    // it has to be named rather than arriving as `BigInt(undefined)` from three
    // frames away — or, worse, as a zero.
    let thrown = null;
    try {
      halLimitsFor({ info: {}, features: new Set(), limits: {} });
    } catch (error) {
      thrown = error;
    }
    check(
      thrown instanceof TypeError &&
        thrown.message.includes('maxStorageBufferBindingSize'),
      `an adapter with no limits names the member it was missing (${String(thrown)})`
    );
  }

  // ---- a reply that will not encode leaves no bytes behind ----------------
  // The failure this guards is specific and silent: `#adapter` writes a record
  // field by field, its caller catches a throw and answers `NoAdapter` instead,
  // and without a rollback the buffer would hold half an adapter followed by a
  // refusal. wasm refuses that whole frame's replies as undecodable, which
  // strands every other answer in it.
  {
    const replies = new ReplyWriter();
    replies.readbackPending(1n, handle(1, 1));
    const good = replies.bytes.slice();
    let thrown = null;
    try {
      // A record with a missing field, which `putAdapterInfo` refuses part-way
      // through — after the tag and the sequence have been written.
      replies.adapter(2n, {
        id: 0,
        name: 'half',
        vendorId: 0,
        deviceId: 0,
        deviceType: DEVICE_TYPE.OTHER,
        driver: '',
        features: 0n,
        limits: {},
      });
    } catch (error) {
      thrown = error;
    }
    check(
      thrown !== null,
      `the half-written reply was refused (${String(thrown)})`
    );
    checkEqual(
      replies.bytes,
      good,
      'and the buffer is byte for byte what it was before the refused call'
    );
    // …and the writer still works afterwards, which is what the caller does
    // next: it answers the same sequence with a refusal instead.
    replies.noAdapter(2n, 'could not encode this adapter');
    check(
      replies.bytes.length > good.length,
      'a reply written after a refused one still lands'
    );
  }

  // ---- a frame with nothing in it is not an error -------------------------
  // `takeCommandStream` answers `null` when no channel is installed, and an
  // empty `commands` array when a frame encoded nothing. Both are ordinary.
  {
    const replayer = new Replayer({ gpu: stubGpu(async () => null) });
    let thrown = null;
    try {
      replayer.replay(null);
      replayer.replay({ baseSequence: 0n, commands: [] });
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null && !replayer.hasReplies,
      `no channel and an empty frame are both nothing to do (${String(thrown)})`
    );
  }

  if (failures.length > 0) {
    console.error(`\ngpu-replay: FAILED (${failures.length})`);
    process.exit(1);
  }
  console.log('\ngpu-replay: OK');
}

await main();
