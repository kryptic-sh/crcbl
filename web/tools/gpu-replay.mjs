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
  halFeaturesFor,
  halLimitsFor,
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

/** A one-command frame carrying `command` at `sequence`. */
function frameOf(command, sequence) {
  return { baseSequence: sequence, commands: [command] };
}

/**
 * @param {number} index
 * @param {number} generation
 */
function handle(index, generation) {
  return { index, generation };
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
    for (const [index, command] of commands.entries()) {
      if (command.name === 'EnumerateAdapters') continue;
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
        `every command this slice cannot replay throws a ReplayError naming it and its sequence (${commands.length - 1} of them)`
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
