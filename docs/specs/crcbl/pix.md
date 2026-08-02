# `.crpix` — pixel art as text

**Status:** implemented, version 1. `crates/crcbl-sprite/src/crpix.rs` is the
reference parser and `bake.rs` the reference baker; where this document and the
code disagree, it is a bug in one of them and this document is the intent.

**Scope.** A build-time authoring format for pixel art written by hand in this
repository. It is **not** a runtime format: nothing in the engine loads a
`.crpix`. One `.crpix` bakes to a PNG and, when it has anything to say beyond a
single still image, an Aseprite-schema JSON sidecar — and those two are all the
engine ever reads.

```text
.crpix ──bake──▶ PNG + Aseprite JSON ─┐
Aseprite ─export▶ PNG + Aseprite JSON ─┴──▶ crcbl_sprite::Sheet ──▶ renderer
```

---

## 1. Why this exists

Two jobs get conflated when people say "a text format for pixel art", and they
want different answers.

**Interchange** — moving art between tools, and letting an artist bring real
work. That is settled: **Aseprite** is the industry standard, and its
`--sheet x.png --data x.json --format json-array` export already carries frames
with durations, `frameTags` for named animations, and `slices[].keys[].center`,
which is nine-slice under another name. The engine reads that schema and nothing
else, so art from Aseprite needs no engine change.

**Authoring by hand**, in a repository, in a diff. Nothing standard covers it
adequately:

| Candidate                 | Why not                                                                                                                                                                           |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **XPM** (X11)             | The right shape, and the closest thing to a standard — a palette of characters and rows of them. Holds exactly one image, with nowhere to put frames, clips or a nine-slice rect. |
| Netpbm plain (P1/P2/P3)   | Open and universal, but ASCII _numbers_, not characters. No palette, no alpha in P3. Not readable as art.                                                                         |
| XBM                       | One bit per pixel.                                                                                                                                                                |
| SVG                       | A real standard, but pixel art as `<rect>` elements is worse than XPM in every respect.                                                                                           |
| Sixel                     | An actual DEC/ANSI standard and printable-ASCII encoded, but a terminal transmission format, not something written by hand.                                                       |
| ANSI/ASCII art with SAUCE | Character-cell art. A different medium.                                                                                                                                           |
| `.piskel`, Tiled `.tmx`   | Open and text, but `.piskel` wraps base64 PNGs and `.tmx` references images rather than containing them.                                                                          |

So `.crpix` is bespoke, and the mitigation is structural rather than a promise:
it is a **build input**, it is converted to the standard pair, and nothing
downstream knows it exists. Deleting it costs one build script.

Its design is XPM's, deliberately — see §6.

---

## 2. Lexical structure

A `.crpix` file is UTF-8 text, read line by line.

- **Comments.** Everything from the first `#` on a line to the end of the line
  is discarded, _except_ on palette-entry lines, which are read from the raw
  line (§4.2). A consequence, and a rule: **`#` cannot be a palette key**, and
  therefore never appears in a row.
- **Blank lines** — empty after comment removal — are ignored anywhere.
- **Indentation is not significant.** Rows and palette entries are
  conventionally indented two spaces for readability; the parser trims both
  ends.
- **A space is never a pixel.** Rows are trimmed at both ends, so a file that
  looks aligned is aligned. Write empty as `.` by convention (any key will do —
  the convention is not enforced).

## 3. Grammar

```ebnf
file        = header , { blank | comment | section } ;

header      = "crpix:" , uint , uint , uint , uint , uint ;
              (* width height frames colours chars-per-pixel *)

section     = palette | frame | clip | nine | sample ;

palette     = "palette:" , newline , { entry } ;
entry       = key , [ context , value ] , { context , value } ;
key         = <exactly `chars` characters, followed by whitespace> ;
context     = "c" | "m" | "g" | "s" ;
colour      = "None" | "transparent" | "#" hex3 | "#" hex6 | "#" hex8 ;

frame       = "frame" , name , ":" , newline , row * height ;
row         = <exactly `width * chars` characters> ;

clip        = "clip" , name , ":" , { frame-name } ,
              [ "@" , uint ] , [ "loop" ] ,
              [ "reverse" | "pingpong" ] ;

nine        = "nine:" , uint , uint , uint , uint ;   (* left right top bottom *)
sample      = "sample:" , ( "pixel" | "smooth" ) ;
```

The header must be the first non-blank, non-comment line and may appear once.

## 4. Semantics

### 4.1 Header

`crpix: <width> <height> <frames> <colours> <chars>`

Five positive integers. Width and height are **per frame**, not of the baked
sheet. Every one is checked against the file's contents (§5) — that is the
entire reason they are declared rather than inferred.

### 4.2 Palette

One entry per line, inside a `palette:` section.

The key is the **first `chars` characters of the line** after indentation, and
must be followed by whitespace. Taking it positionally rather than by splitting
on whitespace is what allows a key to contain a space when `chars > 1`; the
required separator is what catches a one-character key written under a
`chars: 2` header, which would otherwise swallow the following space and leave
every row unmatched.

After the key come XPM colour _contexts_. `c` is the colour one and the only one
this engine uses; `m`, `g` and `s` (monochrome, greyscale, symbolic) are parsed
and skipped. A file that omits the context entirely — `k #241c1c` — is accepted,
and is the shorter form to write by hand.

| Colour                | Meaning                                                       |
| --------------------- | ------------------------------------------------------------- |
| `None`, `transparent` | Fully transparent, and **all four channels zeroed**           |
| `#rgb`                | Each digit expanded by ×17, as CSS does: `#fff` is `#ffffff`  |
| `#rrggbb`             | Opaque                                                        |
| `#rrggbbaa`           | Straight (non-premultiplied) alpha                            |
| `#rrrrggggbbbb`       | XPM's 16-bit form, truncated to the high byte of each channel |
| a colour name         | One of the 234 names `magick -list color` marks XPM-compliant |

Colour names are matched **case-insensitively, ignoring spaces and
underscores**, as X11's own lookup is: `AliceBlue`, `aliceblue` and `alice blue`
are one colour. A name that is not in the table is refused with an error naming
what a colour may be — it is never guessed at.

The 16-bit form is truncated rather than rounded, so `#ffffffffffff` lands on
`#ffffff` exactly; rounding the low byte in would carry past it.

Zeroing all four channels for transparency is not cosmetic: a transparent texel
carrying leftover RGB shows as a halo the moment a sampler blends across it.

### 4.3 Frames

`frame <name>:` opens one; the next `height` rows are its art, and it closes
automatically once it has them. Each row is exactly `width * chars` characters,
read in `chars`-sized cells, each of which must be a palette key.

Frames appear in sheet order. Names are used by clips and are written into the
sidecar's `filename`.

### 4.4 Clips

`clip <name>: <frame> ... [@ <ticks>] [loop] [reverse|pingpong]`

- The frames must be a **contiguous ascending run** in sheet order. An Aseprite
  frame tag is a `from`/`to` range, so this is the model being baked to. Play
  order is `reverse` and `pingpong`'s job, not the list's.
- `@ N` sets how long **each** frame of the clip is held, in **simulation
  ticks**, and applies to the frames the clip names. Default 1. Where two clips
  name the same frame at different rates, the last one parsed wins.
- `loop` repeats forever. Without it the clip is one-shot and holds its last
  frame — which is what a death or a button press wants.
- `pingpong` runs forward then back without repeating either end: four frames
  play `0 1 2 3 2 1`.

Ticks rather than milliseconds because a sample's animation must advance
identically at 20 fps and 240 fps, for the same reason its physics must, and the
fixed tick is the only clock that is true of.

### 4.5 Nine-slice

`nine: <left> <right> <top> <bottom>`, in pixels, insetting from the frame's
edges. Applies to every frame of the sheet.

The four corners never scale, the top and bottom edges stretch horizontally
only, the left and right stretch vertically only, and the centre stretches both
ways. Insets are stored rather than Aseprite's centre rect because insets are
what the geometry needs and what a human can write; §7 gives the conversion.

### 4.6 Sample mode

`sample: pixel` (the default) or `sample: smooth`. A hint carried through to the
renderer, not something the format itself acts on. `pixel` preserves the art's
pixels as exactly as the output allows even at non-integer scale; `smooth` is
ordinary filtering.

---

## 5. Validation

Every rule below is a **build failure naming a line**. The failure being avoided
is specific: art whose rows are one character short renders as a sheared sprite,
and a sheared sprite is blamed on the renderer for an afternoon before anybody
counts the characters.

| Rule                                                   | Error               |
| ------------------------------------------------------ | ------------------- |
| File opens with a header                               | `MissingHeader`     |
| Header is five positive integers                       | `BadHeader`         |
| Header appears once                                    | `RepeatedHeader`    |
| Every row is exactly `width * chars` characters        | `RowWrongWidth`     |
| Every frame has exactly `height` rows                  | `FrameWrongHeight`  |
| The file holds exactly `frames` frames                 | `CountMismatch`     |
| The palette holds exactly `colours` entries            | `CountMismatch`     |
| A palette key is followed by whitespace                | `KeyNotSeparated`   |
| No two entries share a key                             | `DuplicateKey`      |
| Every cell is a palette key                            | `UnknownPixel`      |
| A colour is `None` or hex                              | `BadColour`         |
| A clip names only defined frames                       | `UnknownFrame`      |
| A clip's frames are a contiguous ascending run         | `ClipNotContiguous` |
| A hold is a positive integer                           | `BadHold`           |
| `nine:` is four integers, and the insets fit the frame | `BadNine`, `Sheet`  |
| `sample:` is `pixel` or `smooth`                       | `BadSample`         |

Parsing stops at the first failure: a build error needs the first cause, not the
noise downstream of it.

**Why the counts are declared.** They look like redundancy that will rot. They
are a checksum, and the checking is only possible because they exist. A row one
character short is detectable _at all_ only against a declared width — and a
frame with two rows wrong in compensating directions is invisible to any check
that compares rows against each other. Real XPM readers enforce the same thing
row by row; this was verified against ImageMagick before the rule was adopted.

---

## 6. Relationship to XPM

**A `.crpix` is not an XPM, and an XPM is not a `.crpix`.** There is no backward
compatibility in either direction and none is intended: feeding a real `.xpm` to
the parser fails on its first line. What is shared is the _palette entry_, which
was taken deliberately so a palette block pasted out of an `.xpm` resolves
without being rewritten.

Taken:

| XPM feature                                                | Here                                                    |
| ---------------------------------------------------------- | ------------------------------------------------------- |
| A header declaring width, height, colours, chars-per-pixel | `crpix:`, plus a frame count                            |
| `c None` for transparency                                  | Identical                                               |
| Multiple characters per pixel                              | Identical, via the header's fifth field                 |
| `<key> c <colour>` entries with colour contexts            | Identical                                               |
| Colour names                                               | The 234 XPM-compliant ones, plus `#rrrrggggbbbb` (§4.2) |

Not taken, and therefore where a real `.xpm` will fail:

- **C source syntax** — `/* XPM */`, `static char *name[] = { … };`, quoted and
  comma-separated rows. It buys compilability into a C program, which nothing
  here wants, at the cost of a quote and a comma on every line of art.
- **`/* … */` comments.** Comments are `#`.
- **The header's position and shape.** XPM's is a quoted string inside the array
  with four fields; this one is keyword-led, unquoted, and has five.
- **The optional hotspot and `XPMEXT`** in the header line.
- **`%hhhhssssvvvv`** HSV colours.
- **`#` as a palette key**, which is impossible here because it opens a comment.
- **`m`, `g`, `s` contexts** are parsed and _skipped_ rather than honoured. They
  serve monochrome and greyscale displays this engine will never meet; skipping
  rather than refusing is what keeps the paste-a-palette property.

**Why full compatibility is not pursued.** The migration already exists: the
engine reads PNG, so `magick bird.xpm bird.png` puts an XPM into the engine
today, and `crcbl crpix bird.png` (§7.1) puts it into _this_ format. Supporting
XPM's container would cost a C-wrapper tolerance, a second comment syntax, a
second header form and a mode-dependent lexer — and the moment a second frame, a
clip or a nine-slice is wanted, XPM has nowhere to put them.

## 7. Baking

`crcbl_sprite::bake::bake(art, image_name, tick_hz)` produces:

**The PNG.** RGBA8, frames laid **left to right in one horizontal strip** —
Aseprite's own default layout, and the one that makes frame `i`'s rect
`(i * width, 0, width, height)`. A packed atlas would save texture memory that a
sheet of eight 16×16 frames does not have a problem with, at the cost of a
bin-packer between the art and the screen. The encode names its compression
level so a crate version bump cannot silently change the bytes.

**The JSON sidecar**, _only if_ the art has more than one frame, or any clip, or
a nine-slice. A single still frame is fully described by its image, and a
sidecar saying "one frame, the whole picture" is a file to fetch, parse and
learn nothing from.

The mapping, field by field:

| `.crpix`                           | Aseprite JSON                                                                                |
| ---------------------------------- | -------------------------------------------------------------------------------------------- |
| frame name                         | `frames[i].filename`                                                                         |
| frame position in the strip        | `frames[i].frame`                                                                            |
| —                                  | `rotated: false`, `trimmed: false`, `spriteSourceSize` = the frame, `sourceSize` = the frame |
| `hold` ticks                       | `frames[i].duration`, milliseconds: `ceil(hold * 1000 / tick_hz)`, floored at 1              |
| clip name                          | `meta.frameTags[i].name`                                                                     |
| clip's frame run                   | `from` / `to`                                                                                |
| `reverse` / `pingpong`             | `direction`                                                                                  |
| `loop`                             | `repeat` **absent** (Aseprite's "forever")                                                   |
| no `loop`                          | `repeat: "1"`                                                                                |
| `nine: l r t b` on a `w × h` frame | `meta.slices[0].keys[0].center` = `{x: l, y: t, w: w-l-r, h: h-t-b}`                         |
| sheet size                         | `meta.size`                                                                                  |

Durations are floored at 1 ms because a zero-duration frame reads as "skip me"
to some consumers, and one tick at 240 Hz rounds to zero without it.

`meta.app` names this repository, **not** aseprite.org. The schema is
Aseprite's; the file was not written by Aseprite, and claiming otherwise in a
field whose purpose is provenance would be a lie a future reader might act on.

### 7.1 The other direction: images to `.crpix`

`crcbl_sprite::trace::trace(images, options)` — and `crcbl crpix` over it —
turns one or more PNGs into the text of a `.crpix`, for getting existing art
into the format.

- **Several images make one sheet.** They are given in playback order, one per
  frame, and each becomes a `frame` section. **Every image must be the same
  size**; a mismatch is an error rather than a pad or a crop, because silently
  padding one would move the art on that frame alone and point at nothing.
- **The palette is first-seen order**, scanning each image left to right and top
  to bottom, images in the order given. Stable, so the same input always
  produces the same file, and roughly the order a person would have written.
- **Fully transparent pixels are one colour** whatever RGB they carry, and
  always take the key `.`. A paint program leaves whatever was underneath, and
  each variant would otherwise take an entry no reader can tell apart.
- **More colours than the alphabet widens the keys** to two characters, and the
  header says so — XPM's own escape hatch.
- **It is a conversion, not a quantiser.** Colours are reproduced exactly, and
  art with more than 512 distinct ones is refused as a photograph rather than
  pixel art. Reducing an image's colours is the drawing tool's job.

The property that matters, and the one the tests hold it to: **what comes out
parses, and parses back to the pixels that went in.**

---

## 8. Worked example

```text
# apps/flappy/art/bird.crpix
crpix: 8 6 3 5 1                 # w h frames colours chars-per-pixel

palette:
  . c None
  k c #241c1c
  y c #f2c14e
  o c #e08a3c
  w c #ffffff

frame up:
  ..kkkk..
  .kyyyyk.
  kyywwyyk
  kyyyyyyk
  .kkoookk
  ..kkkk..

frame level:
  ..kkkk..
  .kyyyyk.
  kyywwyyk
  kyyyyyyk
  .kkoookk
  ..kkkk..

frame down:
  ..kkkk..
  .kyyyyk.
  kyywwyyk
  kyyyyyyk
  .kkoookk
  ..kkkk..

clip flap: up level down @ 6 loop
sample: pixel
```

bakes to a 24×6 PNG and a sidecar whose `frameTags` holds one `flap` tag from 0
to 2, forward, repeating forever, with each frame at 100 ms.

---

## 9. Not specified here

- **Loading.** How a PNG and a sidecar become a texture and a
  `crcbl_sprite::Sheet` at runtime, including the Aseprite JSON _reader_.
- **Playback.** Advancing a clip over ticks, and what `pingpong` does at the
  ends, beyond the sequence given in §4.4.
- **Rendering.** The sprite pass, the two sample modes' shader behaviour, layers
  and parallax, and how nine-slice insets become geometry.
- **Layers.** Aseprite's `meta.layers` is not read and `.crpix` has no
  equivalent. Parallax layers are currently a property of a _drawn sprite_, not
  of the sheet it came from.
- **A packed atlas.** Frames are a strip; nothing here forbids a packer later,
  and the sidecar's per-frame rects already express one.
- **Reading Aseprite JSON.** §7 specifies what is _written_; the reader that
  turns a sidecar back into a `Sheet` is owed by the loading slice.

## 10. Version history

- **1** — this document. Header, palette with XPM contexts, frames, clips with
  direction and loop, nine-slice, sample mode; bakes to PNG + Aseprite JSON.
  Colour names and `#rrrrggggbbbb` were added before anything depended on the
  format, along with the image-to-`.crpix` converter (§7.1); the earlier
  argument for refusing names — that a misspelling would be silently wrong — was
  simply false, since an unknown name is refused.
