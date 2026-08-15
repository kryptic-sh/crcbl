#!/usr/bin/env node
// The other half of the reply stream's drift check.
//
// `web/engine/gpu-reply.js` encodes replies in JavaScript and that encoder is
// the production one — the Rust `ReplyWriter` exists to test the encoding and to
// produce the fixture, not to run in a browser. Two hand-written halves of one
// format will drift, and no compiler anywhere reads both.
//
// So the two are pinned to a third thing they both have to agree with:
// `crates/crcbl-webgpu/tests/fixture.rs` freezes the canonical replies into
// `tests/fixtures/canonical-replies.bin`, and this file encodes the same replies
// here and asserts the bytes are identical. Change a tag, a field order or a cap
// in Rust and the fixture test goes red; make the same change only in JavaScript
// and this one does.
//
// THE DIRECTION IS THE OPPOSITE OF `stream-decode.mjs`, and so is what each
// check protects. There, Rust encodes and JS decodes, so the node tool decodes
// the committed bytes and asserts every field. Here, JS encodes and Rust
// decodes, so the node tool *re-encodes* and asserts every byte — a decode would
// prove nothing about the writer, which is the half that ships.
//
// The replies below are therefore deliberately spelled out rather than read from
// anything. A list derived from the fixture would agree with the fixture by
// construction.
//
// It also asserts what the writer must refuse: a payload past the field cap, an
// array past the element cap, a handle with no generation, and a sequence handed
// in as a number rather than a `BigInt`. Each has to throw a typed
// `ReplyEncodeError` where the mistake is, rather than producing a buffer wasm
// rejects a frame later.
//
// Usage:
//   node web/tools/reply-encode.mjs [path-to-fixture.bin]

import { readFile } from 'node:fs/promises';

import { ReplyEncodeError, ReplyWriter } from '../engine/gpu-reply.js';

/** The fixture `crcbl-webgpu`'s `fixture.rs` writes. */
const FIXTURE = new URL(
  '../../crates/crcbl-webgpu/tests/fixtures/canonical-replies.bin',
  import.meta.url
);

/** `tag::REPLY_HEADER_BYTES`: the magic and the version word. No base sequence:
 * every reply carries the number it answers. */
const REPLY_HEADER_BYTES = 8 + 2;

/** `tag::MAX_FIELD_BYTES`. */
const MAX_FIELD_BYTES = 1 << 20;

/** `tag::MAX_ELEMENT_COUNT`. */
const MAX_ELEMENT_COUNT = 1 << 16;

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
 * @param {number} index
 * @param {number} generation
 */
function handle(index, generation) {
  return { index, generation };
}

/**
 * The payload `replies::growth_payload` generates: big enough to force this
 * writer past its initial buffer and double it, with a value per byte.
 *
 * The one part of either corpus that is computed rather than written out, and
 * the rule is duplicated here on purpose — a payload imported from the decoder
 * would agree with it by construction, and a growth that wrote through the view
 * it held *before* growing is precisely what this catches. It did catch one.
 */
function growthPayload() {
  const bytes = new Uint8Array(512);
  for (let i = 0; i < bytes.length; i += 1) bytes[i] = (i * 7) % 251;
  return bytes;
}

/**
 * The canonical replies, in the order `replies::every_reply` holds them.
 *
 * Two things about this list are load-bearing beyond the field values:
 *
 *   * **The sequences are not in order and not contiguous.** A writer that
 *     numbered replies by position would produce a buffer that decodes and is
 *     wrong; these numbers are what make that a byte difference.
 *   * **One sequence and one value are past 2^32.** Both are `BigInt` here for
 *     that reason; as numbers they would round to something that still looks
 *     plausible.
 *
 * @param {ReplyWriter} replies
 */
function encodeCanonicalReplies(replies) {
  replies.adapter(9n, 3, 'Apple M2 — ✱');
  replies.adapter(2n, 0, '');
  replies.readbackPending(17n, handle(51, 52));
  replies.readbackReady(
    5n,
    handle(53, 54),
    new Uint8Array([0x0b, 0xad, 0xf0, 0x0d])
  );
  replies.readbackReady(23n, handle(55, 56), new Uint8Array());
  replies.readbackReady(31n, handle(61, 62), growthPayload());
  replies.queryResults(0x0000_0001_0000_002an, handle(57, 58), 4, [
    0xffff_ffff_ffff_ffffn,
    0n,
    1_234_567_890_123n,
  ]);
  replies.queryResults(11n, handle(59, 60), 0, []);
  // Appended rather than filed beside the two adapters, where they read better:
  // `replies::every_reply` explains why, and the two lists have to be in the
  // same order for the same bytes to come out.
  replies.noAdapter(13n, 'requestAdapter() resolved null — ✱');
  replies.noAdapter(1n, '');
}

/**
 * The offset of the first byte the two disagree on, or `-1` when they are
 * identical. The same figure `fixture.rs` prints on the other side.
 *
 * @param {Uint8Array} committed
 * @param {Uint8Array} encoded
 */
function firstDifference(committed, encoded) {
  const shared = Math.min(committed.length, encoded.length);
  for (let at = 0; at < shared; at += 1) {
    if (committed[at] !== encoded[at]) return at;
  }
  return committed.length === encoded.length ? -1 : shared;
}

/**
 * Runs `encode` and reports whether it threw the `ReplyEncodeError` it should.
 *
 * @param {string} what
 * @param {string} kind
 * @param {() => void} encode
 */
function checkRefused(what, kind, encode) {
  let thrown = null;
  try {
    encode();
  } catch (error) {
    thrown = error;
  }
  check(
    thrown instanceof ReplyEncodeError && thrown.kind === kind,
    `${what} (got ${thrown === null ? 'no throw' : String(thrown)})`
  );
}

async function main() {
  const override = process.argv.slice(2).find((arg) => !arg.startsWith('--'));
  const path = override === undefined ? FIXTURE : override;
  const committed = new Uint8Array(await readFile(path));

  console.log(
    `reply-encode: ${override ?? FIXTURE.pathname} (${committed.length} bytes)`
  );

  // ---- the canonical replies encode to exactly the committed bytes ---------
  const replies = new ReplyWriter();
  check(
    replies.bytes.length === REPLY_HEADER_BYTES,
    `a fresh writer is a header and nothing else (${replies.bytes.length} bytes)`
  );
  encodeCanonicalReplies(replies);
  const encoded = replies.bytes;

  const at = firstDifference(committed, encoded);
  check(
    at === -1,
    at === -1
      ? `the canonical replies encode to the committed fixture (${encoded.length} bytes)`
      : `the canonical replies encode to the committed fixture — first difference at byte ${at}` +
          ` (committed ${committed.length} bytes, encoded ${encoded.length});` +
          ' both halves of this format are hand-written, so a change in web/engine/gpu-reply.js' +
          ' is a change crates/crcbl-webgpu/src/{tag,reply}.rs has to make too'
  );

  // ---- a cleared writer starts again from a header ------------------------
  // The per-frame path: a writer is reused across frames, and a `clear` that
  // left the old replies behind would send every answer twice — which the far
  // side reports as an unexpected sequence, a frame after the mistake.
  replies.clear();
  check(
    replies.bytes.length === REPLY_HEADER_BYTES,
    `a cleared writer is a header again (${replies.bytes.length} bytes)`
  );
  encodeCanonicalReplies(replies);
  check(
    firstDifference(committed, replies.bytes) === -1,
    'and encodes the same bytes the second time round'
  );

  // ---- what the writer must refuse ----------------------------------------
  // Each of these is a cap the Rust reader enforces, so encoding past one would
  // produce a buffer wasm refuses. Throwing here names the call that did it.
  checkRefused('a payload past the field cap is refused', 'InvalidLength', () =>
    new ReplyWriter().readbackReady(
      1n,
      handle(1, 1),
      new Uint8Array(MAX_FIELD_BYTES + 1)
    )
  );
  checkRefused(
    'an array past the element cap is refused',
    'InvalidLength',
    () =>
      new ReplyWriter().queryResults(
        1n,
        handle(1, 1),
        0,
        new BigUint64Array(MAX_ELEMENT_COUNT + 1)
      )
  );
  checkRefused(
    'a handle with no generation is refused rather than sent as a null handle',
    'NullHandle',
    () => new ReplyWriter().readbackPending(1n, handle(1, 0))
  );
  checkRefused(
    'a sequence handed in as a number is refused rather than rounded',
    'NotABigInt',
    // @ts-expect-error — the mistake this check exists for.
    () => new ReplyWriter().readbackPending(1, handle(1, 1))
  );
  checkRefused(
    'a query value handed in as a number is refused too',
    'NotABigInt',
    // @ts-expect-error — as above, one level down.
    () => new ReplyWriter().queryResults(1n, handle(1, 1), 0, [7])
  );

  // ---- a name is capped by its bytes, not its code points -----------------
  // The cap the far side enforces is on the encoded UTF-8, so a name that is
  // under the cap in characters and over it in bytes has to be refused. A
  // writer that checked `String.length` would send it and be rejected by wasm.
  checkRefused(
    'a name under the cap in characters and over it in bytes is refused',
    'InvalidLength',
    () => new ReplyWriter().adapter(1n, 0, '✱'.repeat(MAX_FIELD_BYTES / 2))
  );

  if (failures.length > 0) {
    console.error(`\nreply-encode: FAILED (${failures.length})`);
    process.exit(1);
  }
  console.log('\nreply-encode: OK');
}

await main();
