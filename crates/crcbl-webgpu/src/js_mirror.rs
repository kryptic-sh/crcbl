//! The guards that hold `web/engine`'s hand-written halves of this format
//! against [`crate::tag`], the file that declares it.
//!
//! Neither half can see the other and no compiler anywhere reads both, so a
//! number changed on one side and not the other is a browser decoding the
//! **wrong command** — not merely one it cannot name. `web/tools`'s fixture
//! round trips catch that for the records the committed fixtures happen to
//! carry; these tests catch it for every code the two files spell, without a
//! browser.
//!
//! # Three shapes, three parsers
//!
//! The page's half spells a wire code three ways, and each is read here:
//!
//! * **A frozen table** — `export const FORMAT = Object.freeze({ … })`, in
//!   `gpu-reply.js` and `gpu-probe.js`. [`freeze_tables`] reads it and
//!   [`check_frozen`] holds one file's tables against a mirror; [`crate::probe`]
//!   calls both for its own state codes.
//! * **A flat declaration** — `const CREATE_BUFFER_TAG = 0x00;`, in
//!   `gpu-stream.js` and `gpu-reply.js`. The JS name is the Rust name, so the
//!   mirror is a list of constants and nothing else.
//! * **An ordered table** — `const LOAD_OP = ['Load', 'Clear', 'DontCare'];`,
//!   in `gpu-stream.js`. **A name's position is its wire code**, so a
//!   reordering is a fold between two codes that decodes to a valid command
//!   meaning something else — the failure that file's own comments spend the
//!   most words on.
//!
//! # The bitflag tables are a fourth shape, and they mirror `crcbl-hal`
//!
//! Six of `gpu-stream.js`'s ordered tables — and the one mask beside them —
//! restate a `crcbl_hal` `bitflags` type rather than anything [`crate::tag`]
//! declares. A name's position in one of those is its **bit** rather than its
//! code, and the page's `readFlags` claims `2 ** table.length - 1` of them, so
//! a name in the wrong row is a usage, a stage mask or a channel mask the
//! browser applies to the wrong thing. That failure is the quiet one: an
//! unknown tag stops the stream, while a wrong usage bit creates a resource
//! with a capability nobody asked for and renders on. [`check_flags`] holds
//! each table against its type, and the type's flag list is read back out of
//! `crcbl-hal`'s own text for [`pub_const_names`]'s reason.
//!
//! # What holds the parse honest
//!
//! Two properties, because a mirror guard's failure mode is matching less than
//! it claims and passing:
//!
//! * **A line the parse cannot read is a panic** naming the file, the line and
//!   the text — never a skip. [`js_decls`] classifies *every* top-level `const`
//!   in a file, and an expression it deliberately does not read has to be named
//!   in that file's [`FileMirror::opaque`] list, which is held from both
//!   directions.
//! * **The counts are pinned** — against the file's own independent count where
//!   there is one, and against a literal in each test otherwise — so a table
//!   whose shape changes is caught rather than dropped.

use std::collections::BTreeSet;

/// `web/engine/gpu-stream.js` — the page's half of the command stream, pulled in
/// whole so the mirror can be checked without a browser.
///
/// `include_str!` resolves against this file, so the crate is coupled to the
/// repository's layout on purpose. Move the page's half and this stops
/// compiling, which is a failure nobody can read as a pass.
const GPU_STREAM_JS: &str = include_str!("../../../web/engine/gpu-stream.js");

/// The same source with CRLF folded to LF, which is what every parse and every
/// pin below reads.
///
/// **Not tidiness — the guards are wrong on Windows without it.**
/// `.gitattributes` says `* text=auto`, so a Windows checkout hands these files
/// CRLF, and a pin like [`FLAGS_READ_AS_U32`] is a multi-line `&str` written
/// with `\n`. `contains` then answers `false` on a file nobody edited, which is
/// the same defect `.gitattributes` already records for the shader manifest —
/// and it failed exactly that way on `build + test (windows-latest)` for
/// commit `f610557`, while every Linux gate passed.
///
/// Folding here rather than pinning the files to LF in `.gitattributes`: that
/// would fix the checkout this repository controls and leave a contributor with
/// a different `core.autocrlf` reading a guard that cannot match.
/// Leaked, so the folded copy is `'static` like the `include_str!` it replaces
/// and every parser below can keep returning slices of it. A handful of
/// kilobytes for the life of a test binary, once per file.
pub(crate) fn lf(source: &'static str) -> &'static str {
    if source.contains('\r') {
        String::leak(source.replace("\r\n", "\n"))
    } else {
        source
    }
}

/// `web/engine/gpu-reply.js` — the page's half of the reply stream, for
/// [`GPU_STREAM_JS`]'s reason.
const GPU_REPLY_JS: &str = include_str!("../../../web/engine/gpu-reply.js");

/// [`crate::tag`]'s own text, for the names the mirror is checked to be complete
/// over. Rust has no reflection, so the list of what a module declares can only
/// be read back out of the module.
const TAG_RS: &str = include_str!("tag.rs");

/// `crcbl-hal`'s `resource` module, for [`bitflags_names`] — [`TAG_RS`]'s reason
/// applied to a `bitflags` type, which cannot say what it declares either.
const HAL_RESOURCE_RS: &str = include_str!("../../crcbl-hal/src/resource.rs");

/// `crcbl-hal`'s `shader` module, for [`HAL_RESOURCE_RS`]'s reason.
const HAL_SHADER_RS: &str = include_str!("../../crcbl-hal/src/shader.rs");

/// `crcbl-hal`'s `pipeline` module, for [`HAL_RESOURCE_RS`]'s reason.
const HAL_PIPELINE_RS: &str = include_str!("../../crcbl-hal/src/pipeline.rs");

// ── Reading the page's half ──────────────────────────────────────────────────

/// One `Object.freeze` table parsed out of a `web/engine` module: its name, and
/// its entries in file order.
pub(crate) type JsTable = (&'static str, Vec<(&'static str, u32)>);

/// One table and the Rust constants that must mirror it: the table's name in the
/// JS, the shared prefix of the constants, and each constant's own name and
/// value.
pub(crate) type MirrorTable = (&'static str, &'static str, Vec<(&'static str, u32)>);

/// One top-level `const` declaration in a `web/engine` module, classified by the
/// shape of what it binds.
#[derive(Debug)]
enum JsDecl {
    /// `export const NAME = Object.freeze({ … })`, whose body [`freeze_tables`]
    /// reads. Carried here only so the declaration is accounted for.
    Frozen,
    /// `const NAME = ['Variant', …]` — a variant's position is its wire code.
    Ordered(Vec<&'static str>),
    /// `const NAME = 0x00;` — one code, one declaration.
    Scalar(u32),
    /// `const NAME = new Uint8Array([…]);` — a buffer magic.
    Bytes(Vec<u8>),
    /// `const NAME = '…';` — a string both halves spell out.
    Text(&'static str),
    /// `const NAME = (1n << 27n) - 1n;` — a `BigInt` mask, sixty-four bits wide
    /// because the field it guards is.
    Mask(u64),
    /// An expression this parse deliberately does not read. Every one has to be
    /// named in its file's [`FileMirror::opaque`] list, so a new one fails by
    /// name rather than leaving a hole.
    Opaque,
}

/// Read a JS integer literal: hexadecimal, decimal, or a left shift of the two.
///
/// The shift is part of the grammar rather than a convenience — it is how both
/// halves of the seam spell [`crate::tag::MAX_FIELD_BYTES`].
fn js_number(text: &str) -> Option<u32> {
    if let Some((left, right)) = text.split_once(" << ") {
        return js_number(left)?.checked_shl(js_number(right)?);
    }
    match text.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Read a JS `BigInt` mask: a `27n` literal, a left shift, a subtraction, or a
/// parenthesised group of the three — the grammar `gpu-stream.js` spells
/// `crcbl_hal::Features`'s claimed mask in.
///
/// Sixty-four bits rather than [`js_number`]'s thirty-two, because that is the
/// width of the one field spelled this way: a mask that no longer fits is
/// `None`, which [`js_decls`] turns into the panic every declaration it cannot
/// read gets.
fn js_bigint(text: &str) -> Option<u64> {
    let text = text.trim();
    if let Some(inner) = text
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return js_bigint(inner);
    }
    if let Some((left, right)) = text.split_once(" - ") {
        return js_bigint(left)?.checked_sub(js_bigint(right)?);
    }
    if let Some((left, right)) = text.split_once(" << ") {
        return js_bigint(left)?.checked_shl(js_bigint(right)?.try_into().ok()?);
    }
    let digits = text.strip_suffix('n')?;
    match digits.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => digits.parse().ok(),
    }
}

/// The `'Variant'` entries in one line of an array literal, in file order.
///
/// An entry this cannot read is a panic naming the line, because a dropped entry
/// shifts every code after it and the table would still agree with itself.
fn array_entries(file: &str, number: usize, body: &'static str) -> Vec<&'static str> {
    body.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
                .unwrap_or_else(|| {
                    panic!(
                        "{file}:{number}: an array entry this test cannot read, so the table \
                         would be checked with a hole in it: {entry}"
                    )
                })
        })
        .collect()
}

/// Every top-level `const` a `web/engine` module declares, classified.
///
/// Total by construction, and that is the whole of its discipline: a
/// declaration whose shape this does not recognise is a panic naming the file,
/// the line and the text, and so is a line swallowed by a multi-line body whose
/// terminator this parse ran past — the entry parse meets it before the
/// declaration it belongs to is lost. How many declarations a file holds is
/// pinned by its own test rather than here, because a mirror and a file can
/// narrow together and still agree.
fn js_decls(file: &str, src: &'static str) -> Vec<(&'static str, JsDecl)> {
    let lines: Vec<&'static str> = src.lines().collect();
    let mut decls: Vec<(&'static str, JsDecl)> = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let number = at + 1;
        let line = lines[at];
        at += 1;
        let Some(rest) = line
            .strip_prefix("export const ")
            .or_else(|| line.strip_prefix("const "))
        else {
            continue;
        };
        let Some((name, value)) = rest.split_once(" = ") else {
            panic!("{file}:{number}: a top-level declaration this test cannot read: {line}");
        };

        let decl = if value == "Object.freeze({" {
            JsDecl::Frozen
        } else if value == "[];" {
            JsDecl::Ordered(Vec::new())
        } else if let Some(inline) = value
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix("];"))
        {
            JsDecl::Ordered(array_entries(file, number, inline))
        } else if value == "[" {
            let mut entries = Vec::new();
            while at < lines.len() && lines[at] != "];" {
                entries.extend(array_entries(file, at + 1, lines[at]));
                at += 1;
            }
            assert!(
                at < lines.len(),
                "{file}:{number}: `{name}` opens an array this file never closes"
            );
            at += 1;
            JsDecl::Ordered(entries)
        } else if value == "new Uint8Array([" {
            let mut bytes = Vec::new();
            while at < lines.len() && !lines[at].starts_with("])") {
                for text in lines[at]
                    .split(',')
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                {
                    let byte = js_number(text).and_then(|value| u8::try_from(value).ok());
                    let Some(byte) = byte else {
                        panic!(
                            "{file}:{}: `{name}` holds a byte this test cannot read: {text}",
                            at + 1
                        );
                    };
                    bytes.push(byte);
                }
                at += 1;
            }
            assert!(
                at < lines.len(),
                "{file}:{number}: `{name}` opens a byte array this file never closes"
            );
            at += 1;
            JsDecl::Bytes(bytes)
        } else if let Some(text) = value
            .strip_prefix('\'')
            .and_then(|rest| rest.strip_suffix("';"))
        {
            JsDecl::Text(text)
        } else if let Some(number) = value.strip_suffix(';').and_then(js_number) {
            JsDecl::Scalar(number)
        } else if let Some(mask) = value.strip_suffix(';').and_then(js_bigint) {
            JsDecl::Mask(mask)
        } else {
            JsDecl::Opaque
        };
        decls.push((name, decl));
    }

    decls
}

/// Every `Object.freeze` table in a `web/engine` module, parsed.
///
/// A line inside a table body this cannot read is a panic rather than a skip,
/// and the table count is held against the `Object.freeze` count in the whole
/// file: a parse that quietly matched less than it claims would leave the mirror
/// checked with a hole in it and the test still green.
pub(crate) fn freeze_tables(file: &str, src: &'static str) -> Vec<JsTable> {
    let mut tables: Vec<JsTable> = Vec::new();
    let mut inside = false;
    for (index, line) in src.lines().enumerate() {
        let number = index + 1;
        if !inside {
            if let Some(name) = line
                .strip_prefix("export const ")
                .and_then(|rest| rest.strip_suffix(" = Object.freeze({"))
            {
                tables.push((name, Vec::new()));
                inside = true;
            }
            continue;
        }
        if line == "});" {
            inside = false;
            continue;
        }
        let (name, entries) = tables.last_mut().expect("a table is open");
        let entry = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(','))
            .and_then(|rest| rest.split_once(": "))
            .and_then(|(key, code)| js_number(code).map(|code| (key, code)));
        let Some(entry) = entry else {
            panic!(
                "{file}:{number}: `{name}` holds a line this test cannot read, so the table \
                 would be checked with a hole in it: {line}"
            );
        };
        entries.push(entry);
    }
    assert!(
        !inside,
        "{file} ends inside an Object.freeze table, so its last table is short"
    );
    assert_eq!(
        tables.len(),
        src.matches("Object.freeze").count(),
        "{file} has Object.freeze tables this parse never reached, so they are mirrored by \
         nothing"
    );
    tables
}

/// Hold one file's `Object.freeze` tables against `mirror`, and answer how many
/// tables and how many codes were checked so the caller can pin its own totals.
///
/// A value that disagrees, a table the page's half has lost, and a name the
/// page's half believes in that no constant mirrors all fail by name.
pub(crate) fn check_frozen(
    file: &str,
    src: &'static str,
    mirror: &[MirrorTable],
) -> (usize, usize) {
    let tables = freeze_tables(file, src);
    let codes_seen: usize = tables.iter().map(|(_, entries)| entries.len()).sum();

    let mut unclaimed: BTreeSet<(&str, &str)> = tables
        .iter()
        .flat_map(|(table, entries)| entries.iter().map(move |(key, _)| (*table, *key)))
        .collect();

    for (table, prefix, codes) in mirror {
        let entries = &tables
            .iter()
            .find(|(name, _)| name == table)
            .unwrap_or_else(|| {
                panic!(
                    "{file} declares no `{table}` table, so the {prefix}_* codes are named \
                     nowhere in the page's half"
                )
            })
            .1;
        for (name, code) in codes {
            let key = name
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix('_'))
                .unwrap_or_else(|| {
                    panic!(
                        "{name} is mirrored by {file}'s `{table}` table but is not named \
                         `{prefix}_…`, so this test cannot say which entry is its own"
                    )
                });
            let js = entries
                .iter()
                .find(|(entry, _)| *entry == key)
                .unwrap_or_else(|| {
                    panic!(
                        "{file}'s `{table}` table has no `{key}`, so the browser has no name \
                         for {name} = {code}"
                    )
                })
                .1;
            assert_eq!(
                *code, js,
                "{file} has {table}.{key} = {js}, but {name} = {code}"
            );
            unclaimed.remove(&(*table, key));
        }
    }

    let strays: Vec<String> = unclaimed
        .iter()
        .map(|(table, key)| format!("{table}.{key}"))
        .collect();
    assert!(
        strays.is_empty(),
        "{file} names codes this crate publishes no constant for, so the browser believes in \
         values wasm never sends: {}",
        strays.join(", ")
    );

    // Last, so that a table or an entry that actually moved reports itself by
    // name above rather than as a number that no longer matches.
    assert_eq!(
        tables.len(),
        mirror.len(),
        "{file} has {} tables and this test mirrors {}",
        tables.len(),
        mirror.len()
    );
    assert_eq!(
        codes_seen,
        mirror
            .iter()
            .map(|(_, _, codes)| codes.len())
            .sum::<usize>(),
        "{file} and this test's mirror hold different numbers of codes"
    );
    (tables.len(), codes_seen)
}

/// Every `pub const <NAME>: <ty> = …;` a module's own text declares at its top
/// level, in file order.
pub(crate) fn pub_const_names(src: &'static str, ty: &str) -> Vec<&'static str> {
    let opening = format!(": {ty} = ");
    src.lines()
        .filter_map(|line| line.strip_prefix("pub const "))
        .filter_map(|rest| rest.split_once(opening.as_str()).map(|(name, _)| name))
        .collect()
}

/// Every `pub const <NAME>: … = …;` a module's own text declares at its top
/// level, whatever the type, in file order.
///
/// `pub const fn` is the only other thing the prefix reaches, and is dropped —
/// which is why the partition below is over values and not over items.
fn pub_const_value_names(src: &'static str) -> Vec<&'static str> {
    src.lines()
        .filter_map(|line| line.strip_prefix("pub const "))
        .filter(|rest| !rest.starts_with("fn "))
        .filter_map(|rest| rest.split_once(':').map(|(name, _)| name))
        .collect()
}

/// Every flag one `bitflags!` type declares, read back out of `crcbl-hal`'s own
/// text, in declaration order.
///
/// [`pub_const_value_names`]'s reason applied to a block rather than a module: a
/// `bitflags` type cannot enumerate itself either, so a mirror that named only
/// the flags somebody remembered would narrow silently as the type grew. Total
/// over the block's lines for [`js_decls`]'s reason — a doc comment, an
/// attribute, a blank line and the continuation of a multi-line union are each
/// recognised, and anything else is a panic naming the module and the line, so a
/// flag written in a shape this cannot read fails rather than going unmirrored.
fn bitflags_names(module: &str, src: &'static str, ty: &str) -> Vec<&'static str> {
    let opening = format!("    pub struct {ty}: ");
    let mut names: Vec<&'static str> = Vec::new();
    let mut inside = false;
    let mut closed = false;
    let mut continuing = false;
    for (index, line) in src.lines().enumerate() {
        let number = index + 1;
        if !inside {
            inside = line.starts_with(opening.as_str()) && line.ends_with('{');
            continue;
        }
        if line == "    }" {
            closed = true;
            break;
        }
        let text = line.trim();
        if continuing {
            continuing = !text.ends_with(';');
            continue;
        }
        if text.is_empty() || text.starts_with("//") || text.starts_with("#[") {
            continue;
        }
        let name = text
            .strip_prefix("const ")
            .and_then(|rest| rest.split_once(" = ").map(|(name, _)| name));
        let Some(name) = name else {
            panic!(
                "{module}:{number}: a line inside `{ty}` this test cannot read, so the type \
                 would be mirrored with a hole in it: {line}"
            );
        };
        names.push(name);
        continuing = !text.ends_with(';');
    }
    assert!(
        inside,
        "{module} declares no `{ty}` bitflags type, so the table that mirrors it mirrors nothing"
    );
    assert!(
        closed,
        "{module}'s `{ty}` block never closes, so its flag list is short"
    );
    names
}

/// A HAL variant name as [`crate::tag`] spells it: `D2Array` is `D2_ARRAY`.
///
/// One underscore before an upper-case letter that follows a lower-case letter
/// or a digit, which splits `OneMinusSrcAlpha` into words while leaving `Ccw`,
/// `D1` and `Uint16` whole.
fn screaming_snake(variant: &str) -> String {
    let mut out = String::with_capacity(variant.len() + 4);
    let mut previous: Option<char> = None;
    for character in variant.chars() {
        if character.is_ascii_uppercase()
            && previous
                .is_some_and(|earlier| earlier.is_ascii_lowercase() || earlier.is_ascii_digit())
        {
            out.push('_');
        }
        out.push(character.to_ascii_uppercase());
        previous = Some(character);
    }
    out
}

// ── The mirror ───────────────────────────────────────────────────────────────

/// One constant widened for comparison against the page's half.
///
/// The codes are `u8`, the versions `u16` and the caps `usize`; JS spells all
/// three as plain numbers, so the comparison happens in one type.
fn widen<T: TryInto<u32>>(name: &str, value: T) -> u32 {
    value
        .try_into()
        .unwrap_or_else(|_| panic!("{name} does not fit in a u32, so the JS cannot hold it"))
}

/// Each constant paired with its own name, written once: the compiler binds the
/// identifier, so renaming or deleting one stops this compiling rather than
/// quietly narrowing what is checked.
macro_rules! codes {
    ($($konst:ident),+ $(,)?) => {
        vec![$((stringify!($konst), widen(stringify!($konst), $konst))),+]
    };
}

/// Each flag paired with its own name, written once, for [`codes!`]'s reason:
/// the compiler binds the path, so a renamed or deleted flag stops this
/// compiling rather than quietly narrowing what is checked.
///
/// The width comes from the same place. [`FlagMirror::bits`] is a `u32`, which
/// is what the field crosses the seam as and what `gpu-stream.js` reads it back
/// with, so a `crcbl-hal` type widened past that fails to compile here.
macro_rules! flag_bits {
    ($ty:ty $(, $flag:ident)+ $(,)?) => {
        vec![$((stringify!($flag), <$ty>::$flag.bits())),+]
    };
}

/// The flags of a type that are unions of its bits rather than bits of their
/// own, each with why `gpu-stream.js`'s table has no row for it.
///
/// Same discipline as [`flag_bits!`], plus the reason, because this is the list
/// that says a flag is missing from the page's half *on purpose* — and it is
/// checked to really be a union, so a bit cannot hide in it.
macro_rules! flag_unions {
    ($ty:ty $(, $flag:ident => $why:literal)* $(,)?) => {
        vec![$((stringify!($flag), <$ty>::$flag.bits(), $why)),*]
    };
}

/// One `crcbl_hal` `bitflags` type and the `gpu-stream.js` table that restates
/// it.
struct FlagMirror {
    /// The table's name in the page's half.
    table: &'static str,
    /// The `crcbl_hal` type it mirrors.
    rust: &'static str,
    /// That type's module in `crcbl-hal`, for the failure messages.
    module: &'static str,
    /// That module's text, for [`bitflags_names`].
    src: &'static str,
    /// Each single-bit flag paired with its own name, in ascending bit order —
    /// which is the order the table has to spell them in, because a name's
    /// position there is its bit.
    bits: Vec<(&'static str, u32)>,
    /// The type's composite flags, with the reason each is absent from the
    /// table. Held from both directions.
    composites: Vec<(&'static str, u32, &'static str)>,
}

/// The `crcbl_hal` bitflags types `web/engine/gpu-stream.js` restates, and the
/// tables that restate them.
///
/// These are the fields where a disagreement does not stop the stream: the
/// browser decodes a valid command carrying the wrong usage, the wrong
/// visibility or the wrong write mask, and the frame is subtly wrong instead.
fn stream_flag_mirrors() -> Vec<FlagMirror> {
    use crcbl_hal::{
        BindingFlags, BufferUsage, ColorWrites, ImageAspect, ImageUsage, ShaderStages,
    };

    vec![
        FlagMirror {
            table: "BUFFER_USAGE",
            rust: "BufferUsage",
            module: "crcbl-hal/src/resource.rs",
            src: lf(HAL_RESOURCE_RS),
            bits: flag_bits![
                BufferUsage,
                TRANSFER_SRC,
                TRANSFER_DST,
                UNIFORM,
                STORAGE,
                INDEX,
                INDIRECT,
                DEVICE_ADDRESS,
                QUERY_RESOLVE,
            ],
            composites: flag_unions![BufferUsage],
        },
        FlagMirror {
            table: "SHADER_STAGES",
            rust: "ShaderStages",
            module: "crcbl-hal/src/shader.rs",
            src: lf(HAL_SHADER_RS),
            bits: flag_bits![ShaderStages, VERTEX, FRAGMENT, COMPUTE, MESH, TASK],
            composites: flag_unions![
                ShaderStages,
                GRAPHICS => "the guaranteed graphics stages, which the page decodes as the \
                             two bits it is",
                ALL => "every guaranteed stage, likewise — and deliberately without the two \
                        mesh bits, which is the whole of that type's own note",
            ],
        },
        FlagMirror {
            table: "BINDING_FLAGS",
            rust: "BindingFlags",
            module: "crcbl-hal/src/pipeline.rs",
            src: lf(HAL_PIPELINE_RS),
            bits: flag_bits![
                BindingFlags,
                PARTIALLY_BOUND,
                UPDATE_AFTER_BIND,
                VARIABLE_COUNT
            ],
            composites: flag_unions![BindingFlags],
        },
        FlagMirror {
            table: "IMAGE_USAGE",
            rust: "ImageUsage",
            module: "crcbl-hal/src/resource.rs",
            src: lf(HAL_RESOURCE_RS),
            bits: flag_bits![
                ImageUsage,
                TRANSFER_SRC,
                TRANSFER_DST,
                SAMPLED,
                STORAGE,
                COLOR_ATTACHMENT,
                DEPTH_STENCIL_ATTACHMENT,
                PRESENT,
            ],
            composites: flag_unions![ImageUsage],
        },
        FlagMirror {
            table: "IMAGE_ASPECT",
            rust: "ImageAspect",
            module: "crcbl-hal/src/resource.rs",
            src: lf(HAL_RESOURCE_RS),
            bits: flag_bits![ImageAspect, COLOR, DEPTH, STENCIL],
            composites: flag_unions![ImageAspect],
        },
        FlagMirror {
            table: "COLOR_WRITES",
            rust: "ColorWrites",
            module: "crcbl-hal/src/pipeline.rs",
            src: lf(HAL_PIPELINE_RS),
            bits: flag_bits![ColorWrites, R, G, B, A],
            composites: flag_unions![
                ColorWrites,
                ALL => "all four channels, which the page decodes as the four bits it is",
            ],
        },
    ]
}

/// What one `web/engine` module's declarations must mirror.
struct FileMirror {
    /// The file's name, for the failure messages.
    file: &'static str,
    /// Its text.
    src: &'static str,
    /// Constants the page's half restates under the *same* name, one flat
    /// declaration each.
    scalars: Vec<(&'static str, u32)>,
    /// Ordered tables: the JS name, the shared prefix of the constants, and the
    /// constants in wire-code order.
    ordered: Vec<MirrorTable>,
    /// Byte arrays: the JS name and the Rust constant's own name and bytes.
    bytes: Vec<(&'static str, &'static str, &'static [u8])>,
    /// Strings: the JS name and the Rust constant's own name and text.
    text: Vec<(&'static str, &'static str, &'static str)>,
    /// Ordered tables whose positions are the bits of a `crcbl_hal` `bitflags`
    /// type rather than [`crate::tag`] codes, held by [`check_flags`].
    flags: Vec<FlagMirror>,
    /// `BigInt` masks: the JS name and the Rust expression's own text and value.
    masks: Vec<(&'static str, &'static str, u64)>,
    /// Ordered tables that mirror something *other* than [`crate::tag`], with
    /// the reason. Held from both directions, so one that stops existing fails.
    foreign: &'static [(&'static str, &'static str)],
    /// Expressions [`js_decls`] does not read, with the reason. Held from both
    /// directions too.
    opaque: &'static [(&'static str, &'static str)],
}

/// `web/engine/gpu-stream.js` — the command tags and the enum code tables.
fn stream_mirror() -> FileMirror {
    use crate::tag::*;

    FileMirror {
        file: "gpu-stream.js",
        src: lf(GPU_STREAM_JS),
        scalars: codes![
            STREAM_VERSION,
            MAX_FIELD_BYTES,
            MAX_ELEMENT_COUNT,
            CREATE_BUFFER_TAG,
            CREATE_SURFACE_TAG,
            CREATE_IMAGE_TAG,
            CREATE_IMAGE_VIEW_TAG,
            CREATE_SAMPLER_TAG,
            CREATE_BIND_GROUP_LAYOUT_TAG,
            CREATE_BIND_GROUP_TAG,
            CREATE_SHADER_MODULE_TAG,
            CREATE_PIPELINE_LAYOUT_TAG,
            CREATE_COMPUTE_PIPELINE_TAG,
            CREATE_GRAPHICS_PIPELINE_TAG,
            REQUEST_READBACK_TAG,
            CREATE_OFFSCREEN_SURFACE_TAG,
            CREATE_QUERY_SET_TAG,
            DESTROY_BUFFER_TAG,
            DESTROY_SURFACE_TAG,
            DESTROY_IMAGE_TAG,
            DESTROY_IMAGE_VIEW_TAG,
            DESTROY_SAMPLER_TAG,
            DESTROY_BIND_GROUP_LAYOUT_TAG,
            DESTROY_BIND_GROUP_TAG,
            DESTROY_SHADER_MODULE_TAG,
            DESTROY_PIPELINE_LAYOUT_TAG,
            DESTROY_COMPUTE_PIPELINE_TAG,
            DESTROY_GRAPHICS_PIPELINE_TAG,
            DESTROY_COMMAND_BUFFER_TAG,
            DESTROY_READBACK_TAG,
            DESTROY_QUERY_SET_TAG,
            BEGIN_DEBUG_LABEL_TAG,
            BEGIN_RENDER_PASS_TAG,
            BIND_GRAPHICS_PIPELINE_TAG,
            BIND_GROUP_TAG,
            PUSH_CONSTANTS_TAG,
            CREATE_COMMAND_ENCODER_TAG,
            END_RENDER_PASS_TAG,
            FINISH_TAG,
            BEGIN_COMPUTE_PASS_TAG,
            END_COMPUTE_PASS_TAG,
            BIND_COMPUTE_PIPELINE_TAG,
            PIPELINE_BARRIER_TAG,
            SET_VIEWPORT_TAG,
            SET_SCISSOR_TAG,
            BIND_INDEX_BUFFER_TAG,
            END_DEBUG_LABEL_TAG,
            INSERT_DEBUG_MARKER_TAG,
            SET_STENCIL_REFERENCE_TAG,
            DRAW_TAG,
            DRAW_INDEXED_TAG,
            DRAW_INDIRECT_TAG,
            DRAW_INDEXED_INDIRECT_TAG,
            DISPATCH_TAG,
            DISPATCH_INDIRECT_TAG,
            COPY_IMAGE_TO_BUFFER_TAG,
            COPY_BUFFER_TO_BUFFER_TAG,
            COPY_BUFFER_TO_IMAGE_TAG,
            COPY_IMAGE_TO_IMAGE_TAG,
            CLEAR_BUFFER_TAG,
            WRITE_BUFFER_TAG,
            RESET_QUERY_SET_TAG,
            RESOLVE_QUERY_SET_TAG,
            QUERY_RESULTS_TAG,
            CREATE_SWAPCHAIN_TAG,
            ACQUIRE_NEXT_FRAME_TAG,
            PRESENT_TAG,
            DESTROY_SWAPCHAIN_TAG,
            RECONFIGURE_SWAPCHAIN_TAG,
            ENUMERATE_ADAPTERS_TAG,
            REQUEST_DEVICE_TAG,
            SURFACE_CAPS_TAG,
            SUBMIT_TAG,
            POLL_READBACK_TAG,
            TAKE_ERROR_TAG,
            ABSENT,
            PRESENT,
        ],
        ordered: vec![
            (
                "LOAD_OP",
                "LOAD_OP",
                codes![LOAD_OP_LOAD, LOAD_OP_CLEAR, LOAD_OP_DONT_CARE],
            ),
            (
                "RESOURCE_STATE",
                "RESOURCE_STATE",
                codes![
                    RESOURCE_STATE_UNDEFINED,
                    RESOURCE_STATE_SHADER_READ,
                    RESOURCE_STATE_SHADER_WRITE,
                    RESOURCE_STATE_SHADER_READ_WRITE,
                    RESOURCE_STATE_COLOR_ATTACHMENT,
                    RESOURCE_STATE_DEPTH_STENCIL_WRITE,
                    RESOURCE_STATE_DEPTH_STENCIL_READ,
                    RESOURCE_STATE_TRANSFER_SRC,
                    RESOURCE_STATE_TRANSFER_DST,
                    RESOURCE_STATE_INDIRECT_ARGUMENT,
                    RESOURCE_STATE_INDEX_BUFFER,
                    RESOURCE_STATE_HOST_READ,
                    RESOURCE_STATE_PRESENT,
                ],
            ),
            (
                "STORE_OP",
                "STORE_OP",
                codes![STORE_OP_STORE, STORE_OP_DISCARD],
            ),
            (
                "INDEX_FORMAT",
                "INDEX_FORMAT",
                codes![INDEX_FORMAT_UINT16, INDEX_FORMAT_UINT32],
            ),
            (
                "MEMORY_LOCATION",
                "MEMORY",
                codes![
                    MEMORY_DEVICE_LOCAL,
                    MEMORY_HOST_UPLOAD,
                    MEMORY_HOST_READBACK
                ],
            ),
            (
                "QUERY_KIND",
                "QUERY_KIND",
                codes![
                    QUERY_KIND_TIMESTAMP,
                    QUERY_KIND_OCCLUSION,
                    QUERY_KIND_PIPELINE_STATISTICS,
                ],
            ),
            (
                "IMAGE_TYPE",
                "IMAGE_TYPE",
                codes![IMAGE_TYPE_D1, IMAGE_TYPE_D2, IMAGE_TYPE_D3],
            ),
            (
                "IMAGE_VIEW_TYPE",
                "IMAGE_VIEW_TYPE",
                codes![
                    IMAGE_VIEW_TYPE_D1,
                    IMAGE_VIEW_TYPE_D2,
                    IMAGE_VIEW_TYPE_D2_ARRAY,
                    IMAGE_VIEW_TYPE_CUBE,
                    IMAGE_VIEW_TYPE_CUBE_ARRAY,
                    IMAGE_VIEW_TYPE_D3,
                ],
            ),
            (
                "FILTER_MODE",
                "FILTER_MODE",
                codes![FILTER_MODE_NEAREST, FILTER_MODE_LINEAR],
            ),
            (
                "SAMPLER_ADDRESS_MODE",
                "SAMPLER_ADDRESS",
                codes![
                    SAMPLER_ADDRESS_REPEAT,
                    SAMPLER_ADDRESS_MIRROR_REPEAT,
                    SAMPLER_ADDRESS_CLAMP_TO_EDGE,
                    SAMPLER_ADDRESS_CLAMP_TO_BORDER,
                ],
            ),
            (
                "COMPARE_OP",
                "COMPARE_OP",
                codes![
                    COMPARE_OP_NEVER,
                    COMPARE_OP_LESS,
                    COMPARE_OP_EQUAL,
                    COMPARE_OP_LESS_OR_EQUAL,
                    COMPARE_OP_GREATER,
                    COMPARE_OP_NOT_EQUAL,
                    COMPARE_OP_GREATER_OR_EQUAL,
                    COMPARE_OP_ALWAYS,
                ],
            ),
            (
                "PRIMITIVE_TOPOLOGY",
                "PRIMITIVE_TOPOLOGY",
                codes![
                    PRIMITIVE_TOPOLOGY_POINT_LIST,
                    PRIMITIVE_TOPOLOGY_LINE_LIST,
                    PRIMITIVE_TOPOLOGY_LINE_STRIP,
                    PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                    PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP,
                ],
            ),
            (
                "FRONT_FACE",
                "FRONT_FACE",
                codes![FRONT_FACE_CCW, FRONT_FACE_CW],
            ),
            (
                "CULL_MODE",
                "CULL_MODE",
                codes![CULL_MODE_NONE, CULL_MODE_FRONT, CULL_MODE_BACK],
            ),
            (
                "POLYGON_MODE",
                "POLYGON_MODE",
                codes![POLYGON_MODE_FILL, POLYGON_MODE_LINE],
            ),
            (
                "STENCIL_OP",
                "STENCIL_OP",
                codes![
                    STENCIL_OP_KEEP,
                    STENCIL_OP_ZERO,
                    STENCIL_OP_REPLACE,
                    STENCIL_OP_INVERT,
                    STENCIL_OP_INCREMENT_CLAMP,
                    STENCIL_OP_DECREMENT_CLAMP,
                    STENCIL_OP_INCREMENT_WRAP,
                    STENCIL_OP_DECREMENT_WRAP,
                ],
            ),
            (
                "BLEND_FACTOR",
                "BLEND_FACTOR",
                codes![
                    BLEND_FACTOR_ZERO,
                    BLEND_FACTOR_ONE,
                    BLEND_FACTOR_SRC,
                    BLEND_FACTOR_ONE_MINUS_SRC,
                    BLEND_FACTOR_SRC_ALPHA,
                    BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
                    BLEND_FACTOR_DST,
                    BLEND_FACTOR_ONE_MINUS_DST,
                    BLEND_FACTOR_DST_ALPHA,
                    BLEND_FACTOR_ONE_MINUS_DST_ALPHA,
                ],
            ),
            (
                "BLEND_OP",
                "BLEND_OP",
                codes![
                    BLEND_OP_ADD,
                    BLEND_OP_SUBTRACT,
                    BLEND_OP_REVERSE_SUBTRACT,
                    BLEND_OP_MIN,
                    BLEND_OP_MAX,
                ],
            ),
            (
                "SAMPLE_TYPE",
                "SAMPLE_TYPE",
                codes![SAMPLE_TYPE_FLOAT, SAMPLE_TYPE_DEPTH],
            ),
            (
                "BINDING_KIND",
                "BINDING_KIND",
                codes![
                    BINDING_KIND_UNIFORM_BUFFER,
                    BINDING_KIND_STORAGE_BUFFER,
                    BINDING_KIND_SAMPLED_IMAGE,
                    BINDING_KIND_STORAGE_IMAGE,
                    BINDING_KIND_SAMPLER,
                ],
            ),
            (
                "BINDING_RESOURCE",
                "BINDING_RESOURCE",
                codes![
                    BINDING_RESOURCE_BUFFER,
                    BINDING_RESOURCE_IMAGE_VIEW,
                    BINDING_RESOURCE_SAMPLER,
                ],
            ),
        ],
        bytes: vec![("STREAM_MAGIC", "STREAM_MAGIC", STREAM_MAGIC)],
        text: Vec::new(),
        flags: stream_flag_mirrors(),
        masks: vec![(
            "FEATURES_CLAIMED",
            "crcbl_hal::Features::all().bits()",
            crcbl_hal::Features::all().bits(),
        )],
        foreign: &[
            (
                "IMAGE_FORMAT",
                "gpu-reply.js's FORMAT table, inverted at module load",
            ),
            (
                "SWAPCHAIN_PRESENT_MODE",
                "gpu-reply.js's PRESENT_MODE table, inverted at module load",
            ),
            (
                "SWAPCHAIN_COMPOSITE_ALPHA",
                "gpu-reply.js's COMPOSITE_ALPHA table, inverted at module load",
            ),
        ],
        opaque: &[],
    }
}

/// `web/engine/gpu-reply.js` — the reply tags, the header and the caps. Its
/// `Object.freeze` tables are held by [`check_frozen`] against
/// [`reply_frozen_mirror`] instead.
fn reply_mirror() -> FileMirror {
    use crate::tag::*;

    FileMirror {
        file: "gpu-reply.js",
        src: lf(GPU_REPLY_JS),
        scalars: codes![
            REPLY_VERSION,
            MAX_FIELD_BYTES,
            MAX_ELEMENT_COUNT,
            MAX_DEVICE_ERROR_BYTES,
            MAX_DEVICE_ERRORS,
            ADAPTER_REPLY_TAG,
            NO_ADAPTER_REPLY_TAG,
            DEVICE_REPLY_TAG,
            DEVICE_FAILED_REPLY_TAG,
            SURFACE_CAPS_REPLY_TAG,
            SURFACE_CAPS_FAILED_REPLY_TAG,
            READBACK_PENDING_REPLY_TAG,
            READBACK_READY_REPLY_TAG,
            READBACK_FAILED_REPLY_TAG,
            QUERY_RESULTS_REPLY_TAG,
            DEVICE_ERRORS_REPLY_TAG,
            ABSENT,
            PRESENT,
        ],
        ordered: Vec::new(),
        bytes: vec![("REPLY_MAGIC", "REPLY_MAGIC", REPLY_MAGIC)],
        text: vec![(
            "TRUNCATION_MARKER",
            "crate::reply::TRUNCATION_MARKER",
            crate::reply::TRUNCATION_MARKER,
        )],
        flags: Vec::new(),
        masks: Vec::new(),
        foreign: &[],
        opaque: &[],
    }
}

/// The `Object.freeze` tables `web/engine/gpu-reply.js` exports, against the
/// wire codes that must mirror them.
///
/// Prefix and table name agree throughout, unlike `gpu-probe.js`'s
/// `CAPS_FAILURE`; it is still written beside each table because
/// [`check_frozen`] is shared with that file.
fn reply_frozen_mirror() -> Vec<MirrorTable> {
    use crate::tag::*;

    vec![
        (
            "DEVICE_TYPE",
            "DEVICE_TYPE",
            codes![
                DEVICE_TYPE_CPU,
                DEVICE_TYPE_INTEGRATED,
                DEVICE_TYPE_DISCRETE,
                DEVICE_TYPE_VIRTUAL,
                DEVICE_TYPE_OTHER,
            ],
        ),
        (
            "FORMAT",
            "FORMAT",
            codes![
                FORMAT_R8_UNORM,
                FORMAT_RG8_UNORM,
                FORMAT_RGBA8_UNORM,
                FORMAT_RGBA8_UNORM_SRGB,
                FORMAT_BGRA8_UNORM,
                FORMAT_BGRA8_UNORM_SRGB,
                FORMAT_RGB10A2_UNORM,
                FORMAT_R11G11B10_FLOAT,
                FORMAT_R16_FLOAT,
                FORMAT_RG16_FLOAT,
                FORMAT_RGBA16_FLOAT,
                FORMAT_R32_FLOAT,
                FORMAT_RG32_FLOAT,
                FORMAT_RGBA32_FLOAT,
                FORMAT_R32_UINT,
                FORMAT_RG32_UINT,
                FORMAT_D32_FLOAT,
                FORMAT_D32_FLOAT_S8_UINT,
                FORMAT_D24_UNORM_S8_UINT,
                FORMAT_D16_UNORM,
                FORMAT_BC1_RGBA_UNORM,
                FORMAT_BC1_RGBA_UNORM_SRGB,
                FORMAT_BC3_RGBA_UNORM,
                FORMAT_BC3_RGBA_UNORM_SRGB,
                FORMAT_BC4_R_UNORM,
                FORMAT_BC5_RG_UNORM,
                FORMAT_BC6H_RGB_UFLOAT,
                FORMAT_BC7_RGBA_UNORM,
                FORMAT_BC7_RGBA_UNORM_SRGB,
            ],
        ),
        (
            "PRESENT_MODE",
            "PRESENT_MODE",
            codes![
                PRESENT_MODE_FIFO,
                PRESENT_MODE_FIFO_RELAXED,
                PRESENT_MODE_MAILBOX,
                PRESENT_MODE_IMMEDIATE,
            ],
        ),
        (
            "COMPOSITE_ALPHA",
            "COMPOSITE_ALPHA",
            codes![
                COMPOSITE_ALPHA_OPAQUE,
                COMPOSITE_ALPHA_PRE_MULTIPLIED,
                COMPOSITE_ALPHA_POST_MULTIPLIED,
                COMPOSITE_ALPHA_INHERIT,
            ],
        ),
        (
            "SURFACE_CAPS_FAILURE",
            "SURFACE_CAPS_FAILURE",
            codes![SURFACE_CAPS_FAILURE_BACKEND],
        ),
    ]
}

/// The [`crate::tag`] constants **no** `web/engine` module names, and why.
///
/// Written out so the partition is exhaustive: a value constant that file
/// declares is either mirrored by one of the two files or listed here, and a new
/// one is neither until somebody says which it is.
///
/// Every entry is one of three classes, and all three are Rust-side dispatch
/// rather than anything on the wire:
///
/// * **Family bounds.** `FAMILY_*`, `FAMILIES`, `FAMILIES_END` and their reply
///   twins are the half-open ranges [`crate::reader`] sorts a tag into before it
///   decodes it. The page's half decodes by exact tag — a `switch` over the
///   flat declarations above — so it has no use for a range, and a bound is not
///   a code any buffer ever carries.
/// * **Table cardinalities.** [`crate::tag::BINDING_KIND_CODES`] and
///   [`crate::tag::BINDING_RESOURCE_CODES`] are how many codes those tables
///   hold, which the JS expresses as the length of its own array.
/// * **Offsets and caps neither file reads.** A header's field widths, the
///   reply cap and the waiting-reply cap are wasm-side bookkeeping: the page
///   writes a reply buffer whose size it was handed, and never computes one.
const NOT_MIRRORED: &[&str] = &[
    "FAMILY_CREATE",
    "FAMILY_CREATE_END",
    "FAMILY_DESTROY",
    "FAMILY_DESTROY_END",
    "FAMILY_ENCODER",
    "FAMILY_ENCODER_END",
    "FAMILY_DRAW",
    "FAMILY_DRAW_END",
    "FAMILY_DISPATCH",
    "FAMILY_DISPATCH_END",
    "FAMILY_COPY",
    "FAMILY_COPY_END",
    "FAMILY_QUERY",
    "FAMILY_QUERY_END",
    "FAMILY_PRESENT",
    "FAMILY_PRESENT_END",
    "FAMILY_INSTANCE",
    "FAMILY_INSTANCE_END",
    "FAMILY_DEVICE",
    "FAMILY_DEVICE_END",
    "FAMILIES",
    "FAMILIES_END",
    "REPLY_FAMILY_INSTANCE",
    "REPLY_FAMILY_INSTANCE_END",
    "REPLY_FAMILY_READBACK",
    "REPLY_FAMILY_READBACK_END",
    "REPLY_FAMILY_QUERY",
    "REPLY_FAMILY_QUERY_END",
    "REPLY_FAMILY_ERROR",
    "REPLY_FAMILY_ERROR_END",
    "REPLY_FAMILIES",
    "REPLY_FAMILIES_END",
    "BINDING_KIND_CODES",
    "BINDING_RESOURCE_CODES",
    "HEADER_BYTES",
    "REPLY_HEADER_BYTES",
    "REPLY_SEQUENCE_BYTES",
    "MAX_REPLY_BYTES",
    "MAX_WAITING_REPLIES",
];

// ── The guards ───────────────────────────────────────────────────────────────

/// The declaration `wanted` binds, or a panic saying the page's half has lost
/// it — which is what a wire code with no browser case for it looks like.
fn find_decl<'a>(file: &str, decls: &'a [(&'static str, JsDecl)], wanted: &str) -> &'a JsDecl {
    &decls
        .iter()
        .find(|(name, _)| *name == wanted)
        .unwrap_or_else(|| {
            panic!(
                "{file} declares no `{wanted}`, so the browser has no name for a value tag.rs \
                 publishes"
            )
        })
        .1
}

/// Hold one file's bitflag tables against the `crcbl_hal` types they restate,
/// and account for every table one restates.
///
/// Four directions, because this is the one shape on the seam whose failure is
/// quiet — a wrong bit is a valid command carrying the wrong usage, not a
/// command the browser cannot name:
///
/// * a row whose position is not the bit of the flag it names;
/// * a flag the type declares that the table has no row for, which the page
///   would refuse outright — `readFlags` claims `2 ** table.length - 1` bits and
///   errors on the rest;
/// * a row the type declares no flag for, which is a bit the page would accept
///   and wasm can never send;
/// * a composite left out of the table on purpose that turns out to hold a bit
///   of its own, which is the first three wearing a reason.
fn check_flags(file: &str, decls: &[(&'static str, JsDecl)], mirrors: &[FlagMirror]) {
    for FlagMirror {
        table,
        rust,
        module,
        src,
        bits,
        composites,
    } in mirrors
    {
        let JsDecl::Ordered(entries) = find_decl(file, decls, table) else {
            panic!(
                "{file}'s `{table}` is not an array, so its positions are not the bits of \
                 crcbl_hal::{rust}"
            );
        };

        for (index, (name, bit)) in bits.iter().enumerate() {
            let position = widen("a bit index", index);
            let expected = 1u32.checked_shl(position).unwrap_or_else(|| {
                panic!(
                    "`{table}` would put crcbl_hal::{rust}::{name} at bit {position}, which is \
                     past the width the page reads it at"
                )
            });
            assert_eq!(
                *bit, expected,
                "crcbl_hal::{rust}::{name} is {bit:#x}, but this mirror puts it in row \
                 {position} of `{table}`, where the page's half reads it as {expected:#x}"
            );
            let entry = entries.get(index).unwrap_or_else(|| {
                panic!(
                    "{file}'s `{table}` has no row {index}, so the browser has no name for \
                     crcbl_hal::{rust}::{name} = {bit:#x} and refuses every value that sets it"
                )
            });
            assert_eq!(
                entry, name,
                "{file} has {table}[{index}] = '{entry}', but the flag at {bit:#x} is \
                 crcbl_hal::{rust}::{name}"
            );
        }

        let surplus: Vec<&str> = entries.iter().skip(bits.len()).copied().collect();
        assert!(
            surplus.is_empty(),
            "{file}'s `{table}` names flags crcbl_hal::{rust} does not declare, so the browser \
             accepts bits wasm can never send: {}",
            surplus.join(", ")
        );

        let claimed = bits.iter().fold(0, |mask, (_, bit)| mask | bit);
        let declared = bitflags_names(module, src, rust);
        for (name, value, why) in composites {
            assert!(
                declared.contains(name),
                "this mirror leaves crcbl_hal::{rust}::{name} out of `{table}` as {why}, but \
                 {module} no longer declares it"
            );
            assert!(
                value.count_ones() > 1,
                "crcbl_hal::{rust}::{name} is {value:#x}, a bit of its own rather than a union \
                 of the rows above, so `{table}` needs a row for it — it is left out as {why}"
            );
            assert_eq!(
                value & !claimed,
                0,
                "crcbl_hal::{rust}::{name} sets {:#x}, which no row of `{table}` claims, so it \
                 is not a union of the rows above — it is left out as {why}",
                value & !claimed
            );
        }
        for name in &declared {
            let mirrored = bits.iter().any(|(flag, _)| flag == name)
                || composites.iter().any(|(flag, _, _)| flag == name);
            assert!(
                mirrored,
                "crcbl_hal::{rust} declares {name}, which {file}'s `{table}` has no row for, so \
                 the browser refuses every value that sets it: give it a row and a line in this \
                 file's mirror, or — if it is a union of other flags rather than a bit of its \
                 own — add it to `composites`"
            );
        }
        // Last, so that a flag that actually moved reports itself by name above
        // rather than as a number that no longer matches. It is the guard on the
        // parse itself: a block read as empty agrees with every direction above.
        assert_eq!(
            declared.len(),
            bits.len() + composites.len(),
            "{module} declares {} flags in `{rust}` and this mirror holds {} of them, so \
             `{table}` is checked against a list that read less than the type holds",
            declared.len(),
            bits.len() + composites.len()
        );
    }
}

/// Hold one file's declarations against `mirror`, and answer the Rust constant
/// names it claimed.
///
/// Every declaration [`js_decls`] found has to be claimed by exactly one arm of
/// the mirror; one that is claimed by none fails by name, which is the direction
/// that catches a code the page believes in and wasm never sends.
fn check_file(mirror: &FileMirror) -> BTreeSet<&'static str> {
    let file = mirror.file;
    let decls = js_decls(file, mirror.src);
    let mut unclaimed: BTreeSet<&'static str> = decls.iter().map(|(name, _)| *name).collect();
    let mut claimed: BTreeSet<&'static str> = BTreeSet::new();

    for (name, value) in &mirror.scalars {
        let JsDecl::Scalar(js) = find_decl(file, &decls, name) else {
            panic!("{file}'s `{name}` is not a plain number, so it cannot mirror tag::{name}");
        };
        assert_eq!(
            js, value,
            "{file} has {name} = {js}, but tag::{name} = {value}"
        );
        unclaimed.remove(name);
        claimed.insert(name);
    }

    for (table, prefix, codes) in &mirror.ordered {
        let JsDecl::Ordered(variants) = find_decl(file, &decls, table) else {
            panic!("{file}'s `{table}` is not an array, so its positions are not wire codes");
        };
        for (index, (name, code)) in codes.iter().enumerate() {
            let variant = variants.get(index).unwrap_or_else(|| {
                panic!(
                    "{file}'s `{table}` has no entry at {index}, so the browser has no name \
                     for {name} = {code}"
                )
            });
            let spelled = format!("{prefix}_{}", screaming_snake(variant));
            assert_eq!(
                &spelled, name,
                "{file} has {table}[{index}] = '{variant}', which spells {spelled}, but \
                 tag.rs has {name} there"
            );
            let index = widen("an array index", index);
            assert_eq!(
                index, *code,
                "{file} has '{variant}' at {table}[{index}], but tag::{name} = {code}"
            );
        }
        // A table shorter than the mirror already failed by name above; this is
        // the other end of it, and names the surplus for the same reason.
        let surplus: Vec<&str> = variants.iter().skip(codes.len()).copied().collect();
        assert!(
            surplus.is_empty(),
            "{file}'s `{table}` names entries no tag.rs code claims, so the browser believes \
             in values wasm never sends: {}",
            surplus.join(", ")
        );
        unclaimed.remove(table);
        claimed.extend(codes.iter().map(|(name, _)| *name));
    }

    for (js_name, rust_name, value) in &mirror.bytes {
        let JsDecl::Bytes(bytes) = find_decl(file, &decls, js_name) else {
            panic!("{file}'s `{js_name}` is not a byte array, so it cannot mirror {rust_name}");
        };
        assert_eq!(
            bytes.as_slice(),
            *value,
            "{file}'s `{js_name}` is {:?}, but {rust_name} is {:?}",
            String::from_utf8_lossy(bytes),
            String::from_utf8_lossy(value)
        );
        unclaimed.remove(js_name);
        claimed.insert(rust_name);
    }

    for (js_name, rust_name, value) in &mirror.text {
        let JsDecl::Text(text) = find_decl(file, &decls, js_name) else {
            panic!("{file}'s `{js_name}` is not a string, so it cannot mirror {rust_name}");
        };
        assert_eq!(
            text, value,
            "{file}'s `{js_name}` is {text:?}, but {rust_name} is {value:?}"
        );
        unclaimed.remove(js_name);
        claimed.insert(rust_name);
    }

    for (js_name, rust_name, value) in &mirror.masks {
        let JsDecl::Mask(mask) = find_decl(file, &decls, js_name) else {
            panic!(
                "{file}'s `{js_name}` is not a mask expression, so it cannot mirror {rust_name}"
            );
        };
        assert_eq!(
            mask, value,
            "{file}'s `{js_name}` is {mask:#x}, but {rust_name} is {value:#x}"
        );
        unclaimed.remove(js_name);
        claimed.insert(rust_name);
    }

    check_flags(file, &decls, &mirror.flags);
    for FlagMirror { table, .. } in &mirror.flags {
        unclaimed.remove(table);
    }

    // The frozen tables are held by `check_frozen` against their own mirror, so
    // here they are only accounted for.
    for (name, decl) in &decls {
        if matches!(decl, JsDecl::Frozen) {
            unclaimed.remove(name);
        }
    }

    for (name, mirrors) in mirror.foreign {
        let JsDecl::Ordered(_) = find_decl(file, &decls, name) else {
            panic!("{file}'s `{name}` is no longer an array, so it no longer mirrors {mirrors}");
        };
        unclaimed.remove(name);
    }

    for (name, reason) in mirror.opaque {
        let JsDecl::Opaque = find_decl(file, &decls, name) else {
            panic!(
                "{file}'s `{name}` is now an expression this test can read, so it no longer \
                 belongs in `opaque` — it was listed as {reason}"
            );
        };
        unclaimed.remove(name);
    }

    let strays: Vec<&str> = unclaimed.iter().copied().collect();
    assert!(
        strays.is_empty(),
        "{file} declares wire values this file's mirror checks nothing against, so the browser \
         believes in codes wasm never sends: {}",
        strays.join(", ")
    );
    claimed
}

/// `web/engine/gpu-stream.js`'s command tags and enum tables are the browser's
/// only copy of [`crate::tag`]'s command half, and nothing else compares them.
///
/// A value that disagrees, a tag the page's half has lost, a name it believes in
/// that no constant publishes, and a table this parse never reached all fail by
/// name. The totals are pinned because a parse that narrowed would still agree
/// with itself, so an added or retired tag has to move them deliberately.
#[test]
fn gpu_stream_js_mirrors_every_command_tag_and_enum_code_tag_rs_declares() {
    let mirror = stream_mirror();
    let ordered_codes: usize = mirror.ordered.iter().map(|(_, _, codes)| codes.len()).sum();
    let claimed = check_file(&mirror);

    assert_eq!(
        js_decls("gpu-stream.js", lf(GPU_STREAM_JS)).len(),
        109,
        "gpu-stream.js's top-level declaration count moved; the mirror above has to move with it"
    );
    assert_eq!(
        mirror.scalars.len(),
        77,
        "gpu-stream.js's flat declaration count moved; the mirror above has to move with it"
    );
    assert_eq!(
        mirror.ordered.len(),
        21,
        "gpu-stream.js's enum table count moved; the mirror above has to move with it"
    );
    assert_eq!(
        ordered_codes, 94,
        "gpu-stream.js's enum code count moved; the mirror above has to move with it"
    );
    assert_eq!(
        claimed.len(),
        173,
        "the tag.rs constants gpu-stream.js mirrors moved; the mirror above has to move with it"
    );
}

/// The `gpu-stream.js` helper every bitflag table is read through, and the width
/// it is read at. `crcbl-hal` stores each of those types in a `u32` and
/// [`crate::writer`] puts every one of their fields on the wire with `put_u32`,
/// so this line is the premise the tables below are checked under.
const FLAGS_READ_AS_U32: &str = "  readFlags(field, table) {\n    const bits = this.readU32();";

/// The same for `crcbl_hal::Features`, which `crcbl-hal` stores in a `u64` and
/// [`crate::writer`] puts on the wire with `put_u64` — which is why the page's
/// half spells its mask as a `BigInt` and not as a number.
const FEATURES_READ_AS_U64: &str = "  readFeatures(field) {\n    const bits = this.readU64();";

/// The `gpu-stream.js` tables that restate a `crcbl_hal` `bitflags` type, held
/// against the type — the class the guards above do not reach, because these
/// mirror `crcbl-hal` rather than [`crate::tag`], and the class whose
/// disagreement is silent: the browser decodes a valid command carrying the
/// wrong usage, the wrong visibility or the wrong write mask.
///
/// [`check_flags`] runs inside [`check_file`], so a bit that disagrees fails the
/// two tests above as well. What this one adds is the width the tables are read
/// at and the totals, and the totals are what stop a table being quietly
/// demoted back to [`FileMirror::foreign`], where nothing would compare its bits
/// again.
#[test]
fn gpu_stream_js_mirrors_every_bit_of_the_crcbl_hal_flag_sets_it_restates() {
    let mirror = stream_mirror();
    check_file(&mirror);

    assert!(
        lf(GPU_STREAM_JS).contains(FLAGS_READ_AS_U32),
        "gpu-stream.js no longer reads a bitflags word as a u32, so its bitflag tables are \
         checked against a width the stream does not use"
    );
    assert!(
        lf(GPU_STREAM_JS).contains(FEATURES_READ_AS_U64),
        "gpu-stream.js no longer reads a crcbl_hal::Features word as a u64, so FEATURES_CLAIMED \
         is checked against a width the stream does not use"
    );

    assert_eq!(
        mirror
            .flags
            .iter()
            .map(|flags| flags.table)
            .collect::<Vec<_>>(),
        [
            "BUFFER_USAGE",
            "SHADER_STAGES",
            "BINDING_FLAGS",
            "IMAGE_USAGE",
            "IMAGE_ASPECT",
            "COLOR_WRITES",
        ],
        "the gpu-stream.js tables held against a crcbl_hal type moved; a table that left this \
         list is one nothing compares the bits of any more"
    );
    assert_eq!(
        mirror
            .flags
            .iter()
            .map(|flags| flags.bits.len())
            .sum::<usize>(),
        30,
        "the crcbl-hal bits gpu-stream.js restates moved; the mirror above has to move with it"
    );
    assert_eq!(
        mirror
            .flags
            .iter()
            .map(|flags| flags.composites.len())
            .sum::<usize>(),
        3,
        "the crcbl-hal composites gpu-stream.js leaves out moved; the mirror above has to move \
         with it"
    );
    assert_eq!(
        mirror.masks.len(),
        1,
        "gpu-stream.js's mask count moved; the mirror above has to move with it"
    );
}

/// The same for `web/engine/gpu-reply.js`, whose tags travel the other way: the
/// browser encodes them and wasm decodes them.
#[test]
fn gpu_reply_js_mirrors_every_reply_tag_and_capability_code_tag_rs_declares() {
    let mirror = reply_mirror();
    let claimed = check_file(&mirror);
    let (tables, codes) = check_frozen("gpu-reply.js", lf(GPU_REPLY_JS), &reply_frozen_mirror());

    assert_eq!(
        js_decls("gpu-reply.js", lf(GPU_REPLY_JS)).len(),
        25,
        "gpu-reply.js's top-level declaration count moved; the mirror above has to move with it"
    );
    assert_eq!(
        mirror.scalars.len(),
        18,
        "gpu-reply.js's flat declaration count moved; the mirror above has to move with it"
    );
    assert_eq!(
        tables, 5,
        "gpu-reply.js's frozen table count moved; the mirror above has to move with it"
    );
    assert_eq!(
        codes, 43,
        "gpu-reply.js's frozen code count moved; the mirror above has to move with it"
    );
    assert_eq!(
        claimed.len(),
        20,
        "the tag.rs constants gpu-reply.js mirrors outside its frozen tables moved; the \
         mirror above has to move with it"
    );
}

/// The other direction: a wire code added to [`crate::tag`] and never named in
/// `web/engine`, which is the browser meeting a command it has no case for.
///
/// Exhaustive rather than a spot check, because the partition is: every
/// `pub const` value `tag.rs` declares is either mirrored by one of the two
/// files or listed in [`NOT_MIRRORED`], and one that is neither fails by name.
/// [`NOT_MIRRORED`] is held the other way too — every name in it must still be
/// declared — which is also what proves the parse read anything at all.
#[test]
fn every_wire_code_tag_rs_declares_is_named_in_web_engine_js() {
    let mut mirrored = check_file(&stream_mirror());
    mirrored.extend(check_file(&reply_mirror()));
    mirrored.extend(
        reply_frozen_mirror()
            .iter()
            .flat_map(|(_, _, codes)| codes.iter().map(|(name, _)| *name)),
    );
    let declared = pub_const_value_names(lf(TAG_RS));

    for &name in NOT_MIRRORED {
        assert!(
            declared.contains(&name),
            "`NOT_MIRRORED` names {name}, which tag.rs no longer declares"
        );
    }
    for &name in &declared {
        assert!(
            mirrored.contains(&name) || NOT_MIRRORED.contains(&name),
            "tag.rs declares {name}, which neither gpu-stream.js nor gpu-reply.js names: give \
             it a declaration there and a line in this file's mirror, or — if it is a family \
             bound or a wasm-side size rather than a wire code — add it to `NOT_MIRRORED`"
        );
    }
    assert_eq!(
        declared.len(),
        269,
        "tag.rs's value-constant count moved; the mirror above has to move with it"
    );
    assert_eq!(
        declared
            .iter()
            .filter(|name| mirrored.contains(*name))
            .count(),
        230,
        "tag.rs's mirrored-constant count moved; the mirror above has to move with it"
    );
}

/// [`screaming_snake`] is a transcription, and the ordered tables lean on it for
/// every name they check, so the shapes it has to get right are pinned here.
#[test]
fn screaming_snake_spells_a_variant_the_way_tag_rs_does() {
    assert_eq!(screaming_snake("Load"), "LOAD");
    assert_eq!(screaming_snake("DontCare"), "DONT_CARE");
    assert_eq!(screaming_snake("ShaderReadWrite"), "SHADER_READ_WRITE");
    assert_eq!(screaming_snake("Uint16"), "UINT16");
    assert_eq!(screaming_snake("D2Array"), "D2_ARRAY");
    assert_eq!(screaming_snake("Ccw"), "CCW");
    assert_eq!(screaming_snake("OneMinusSrcAlpha"), "ONE_MINUS_SRC_ALPHA");
}
