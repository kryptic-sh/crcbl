# Topic 61 — `crcbl-image` with no external crates

Written 2026-11-28, the feasibility write-up the owner asked for on 2026-11-26:
can `crcbl-image` be entirely this project's own code? Workspace crates
(`crcbl-error`, `crcbl-font`) do not count as external. This is a plan, nothing
here is built.

## Where this is

`crcbl-image`'s manifest names four external crates, all optional:

- `png`, behind the `png` feature (and `ico`, which needs it). It covers
  `png::decode_stored`, `png::dimensions` (through `png::StreamingDecoder`) and
  `png::encode`. `png::orientation` already walks the chunks itself.
- `skrifa`, `harfrust` and `unicode-bidi`, behind `svg-text`, for font outlines
  and metrics, shaping, and UAX #9 levels.

`thiserror` was the fifth and is gone: the crate derives its errors with
`crcbl-error`. Every other format the crate decodes (BMP, GIF, ICO, JPEG, WebP,
XPM) is already written here, under the owner's direction of 2026-10-16 recorded
in `docs/backlog.md` under S28. The GIF decoder (LZW) and WebP lossless
(canonical Huffman codes, LZ77 back-references) are the nearest relatives of
what PNG needs.

## `png`: feasible, and the next step

**What the decoder has to do**, from what `png::decode_stored` asks of the crate
today (`EXPAND | ALPHA | STRIP_16`, then RGBA8 or grey-alpha to RGBA8):

- The chunk layer: signature, `IHDR` first, CRC-32 on every chunk, `PLTE`,
  `tRNS`, the `IDAT` run, `IEND`, unknown ancillary chunks skipped and unknown
  critical ones refused. For APNG, `acTL`/`fcTL`/`fdAT` only so far as to decode
  the default image, which is what the module docs promise.
- Inflate (RFC 1951) under the zlib wrapper (RFC 1950): stored, fixed-Huffman
  and dynamic-Huffman blocks, a 32 KiB window, Adler-32. Bounded output: the
  decoder knows the exact filtered size from `IHDR`, so it inflates into a
  buffer of that size and refuses anything past it, which is a tighter bound
  than today's `png::Limits` with `DECODER_OVERHEAD`.
- The five filters (None, Sub, Up, Average, Paeth) per scanline, and Adam7's
  seven passes.
- Every bit depth and colour type `IHDR` allows: grey at 1, 2, 4, 8 and 16 bits,
  RGB at 8 and 16, palette at 1 to 8, grey-alpha and RGBA at 8 and 16. Each one
  expands to RGBA8, with `tRNS` for grey, RGB and palette, and 16-bit samples
  keeping their high byte, as `STRIP_16` does.
- `png::dimensions`'s contract: the header read alone, with the same refusals as
  a full decode.

**What the encoder has to do:** `png::encode` writes RGBA8 only. A stored-only
deflate (uncompressed blocks) is a valid stream and is a few dozen lines, but
its files are larger than the source pixels. A compressor with fixed Huffman
codes and a hash-chain LZ77 is the honest minimum for a file anyone keeps. That
is also where the choice of filter per row starts to matter for size.

**The oracle is the crate being replaced.** Keep `png` as a dev-dependency of
`crcbl-image` only:

- Every decode test compares pixels with it.
- Every encode test round-trips through it.
- A fuzz target in `crates/crcbl-net/fuzz` decodes the same bytes with both and
  requires the same pixels, or a refusal from both.

PngSuite (the standard conformance corpus, covering every colour type, depth,
interlace and the known-broken files) is the fixed set. libpng's `pngtest` is
not needed.

**Risks:**

- Inflate is where hand-written decoders go wrong: distance codes past the
  window, a dynamic header whose code lengths over-subscribe, and a stored
  block's `LEN`/`NLEN` mismatch. The oracle and PngSuite cover these; a review
  should be pointed at them by name.
- Speed: the `png` crate decodes through `fdeflate`, a tuned inflater. Its
  default encode, `Compression::Balanced`, which `png::encode` and
  `crcbl-sprite`'s bake both use, goes through `flate2` (`miniz_oxide`) with an
  adaptive filter per row. A first inflate here will be slower. Measure it in a
  quiet window against the crate before replacing anything on a load path.

### Decisions this needs

1. **Where CRC-32 comes from.** The workspace's one copy is
   `crcbl_store::crc32`. `crcbl-image` cannot take `crcbl-store` for it, since
   that crate brings `serde`, `toml`, `dirs` and `crcbl-net`. The options:
   - Move it to a crate both can take: `crcbl-core`, which `crcbl-image` does
     not depend on today either.
   - Move it to a new leaf crate.
   - Put a second copy in `crcbl-image`.

   The second copy is the drift the code-style rules warn about, so it would
   have to be a move.

2. **The workspace's other `png` users.** `crcbl-golden` decodes and encodes its
   goldens with `png` directly. `crcbl-sprite` decodes in `load.rs` and encodes
   with `Compression::Balanced` in `bake.rs`. `crcbl-scene`'s glTF fixtures and
   `crcbl`'s `gltf_e2e` test write PNG fixtures with it. Each can keep the crate
   or move to `crcbl_image::png`.
   - Moving all of them leaves `png` only as `crcbl-image`'s test oracle.
     `fdeflate`, `flate2`, `crc32fast` and the 0.8 `miniz_oxide` come in through
     `png` alone, so they leave the shipped dependency graph but stay in the
     lockfile for as long as the oracle does.
   - Moving only `crcbl-image` leaves two PNG decoders in the tree.
   - `crcbl-sprite`'s baked atlases are written once and loaded often, so they
     are the case where encoder quality matters.
3. **The encoder's quality bar**: stored-only, fixed Huffman with LZ77, or
   dynamic Huffman. This depends on decision 2: if only tests and goldens write
   through it, a simple compressor is enough.

### Order, once those are taken

1. The CRC-32 move.
2. Inflate and zlib, as their own module with their own tests: RFC 1951's
   examples, the oracle, and a fuzz target with an output bound.
3. Chunks, filters, Adam7 and expansion behind `png::decode_stored`, with the
   `png` crate moved to `[dev-dependencies]` as the oracle. PngSuite runs, and
   the existing tests keep passing unchanged.
4. Deflate and `png::encode` at the bar decision 3 sets.
5. The other crates, as decision 2 says.

Each step is a slice with its own gate and push.

## `skrifa`, `harfrust`, `unicode-bidi`: not feasible inside `crcbl-image` alone

These are a TrueType/CFF outline reader, an OpenType shaper (GSUB/GPOS) and the
bidi algorithm. `crcbl-ui` lays out all of its own text with the same three, so
replacing them only inside `crcbl-image` would leave two text stacks to keep in
agreement. That is worse than either alternative. The options for the owner:

- **Keep them behind `svg-text`.** The crate is then self-implemented in its
  default and codec features, and SVG text keeps the workspace's shared stack.
  This costs nothing beyond the `png` work.
- **Move the whole workspace's text stack in-house.** That is its own topic,
  with `crcbl-ui` as the first consumer. Bidi is the bounded part: UAX #9 is a
  specification with its own conformance files (`BidiTest.txt`,
  `BidiCharacterTest.txt`). Outlines are next: `glyf`, `CFF` and `CFF2`, plus
  variations. The shaper is the large one: GSUB/GPOS lookups and the per-script
  shaping engines.

This needs the owner's call and is not scheduled.
