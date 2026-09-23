//! The drift gate: this crate's declarations against a downloaded SDK.
//!
//! Run by hand, with the SDK unzipped and `CRCBL_STEAM_SDK` naming it:
//!
//! ```text
//! CRCBL_STEAM_SDK=/path/to/sdk cargo test -p crcbl-steam -- --ignored drift
//! ```
//!
//! It reads every `*.h` under `$CRCBL_STEAM_SDK/public/steam/` as text — no
//! JSON, no C parser — and asserts:
//!
//! 1. **every `manifest` declaration appears in a header**, compared after
//!    [`normalize`]: `steam_api_flat.h` holds one `S_API …;` prototype per
//!    line for every interface method, and the lifecycle functions are
//!    single-line `S_API … S_CALLTYPE …;` declarations. An exact match catches
//!    a changed type, an added parameter and a renamed function alike;
//! 2. **every `versions` row matches**: the accessor appears in a header, the
//!    `#define` gives exactly the version string, and `steam_api.h` names the
//!    `#define` if and only if the row says Valve's `InitEx` passes it;
//! 3. **every bound struct matches its header block**: the ordered field
//!    declarations, the `#pragma pack` in force at the struct, and for a
//!    callback or call result, the `k_iCallback = base + n` expression
//!    against its row in `callbacks` or `call` — whether the header writes
//!    the struct out or declares it with `STEAM_CALLBACK_BEGIN`;
//! 4. **every callback base has its header's value**: `enum {
//!    k_iSteamFriendsCallbacks = 300 };` and the rest, which the symbolic
//!    comparison in 3 takes on trust;
//! 5. **every SDK constant a limit here is derived from has the header's
//!    value** — `k_cchMaxRichPresenceKeyLength` and the rest ([`LIMITS`]),
//!    written as an `enum`, a `#define` or a `const`,
//!    so a limit this crate checks before a call cannot drift from the one
//!    Steam enforces.
//!
//! Without `CRCBL_STEAM_SDK` it **fails**: it only ever runs on purpose, and
//! "skipped" must not read as "passed".
//!
//! **It has run against the Steamworks.NET mirror of the 1.65 headers**
//! (`docs/plan/42-steam.md`, "Conventions"), laid out as
//! `$CRCBL_STEAM_SDK/public/steam/*.h`, and never against an SDK zip from
//! Valve, which no machine this crate was written on had. Its first run there
//! found a real transcription error (`GetAppID` declared as returning
//! `AppId_t`; the header says `uint32`). The scanner itself is also proven by
//! this module's other tests, which build a synthetic SDK out of the crate's
//! own tables, check that it passes, and check that one changed parameter
//! type, one renamed field, one changed pragma, one wrong version string and
//! one wrong callback offset each fail it.

use std::{collections::HashSet, path::Path};

use super::{
    manifest::{BINDINGS, BoundFn, INTERFACES},
    structs::{DECLS, Pack, StructDecl},
    versions::Interface,
};
use crate::{
    MAX_CLOUD_FILE_BYTES, MAX_CLOUD_PATH_BYTES, MAX_LEADERBOARD_DETAILS,
    MAX_LEADERBOARD_NAME_LENGTH, MAX_LOBBY_KEY_LENGTH, MAX_PHASE_ID_LENGTH,
    MAX_RICH_PRESENCE_KEY_LENGTH, MAX_RICH_PRESENCE_KEYS, MAX_RICH_PRESENCE_VALUE_LENGTH,
    MAX_STAT_NAME_LENGTH, MAX_TIMELINE_PRIORITY,
    call::{CALL_ROWS, CallRow},
    callbacks::{Base, ROWS, Row},
    input::MAX_ORIGINS,
};

/// Each SDK constant a limit in this crate comes from, and the value the
/// crate's limit implies for it. The `cch` lengths count the NUL; the
/// crate's limits do not.
const LIMITS: &[(&str, usize)] = &[
    ("k_cchMaxRichPresenceKeys", MAX_RICH_PRESENCE_KEYS),
    (
        "k_cchMaxRichPresenceKeyLength",
        MAX_RICH_PRESENCE_KEY_LENGTH + 1,
    ),
    (
        "k_cchMaxRichPresenceValueLength",
        MAX_RICH_PRESENCE_VALUE_LENGTH + 1,
    ),
    ("k_nMaxLobbyKeyLength", MAX_LOBBY_KEY_LENGTH),
    ("k_cchFilenameMax", MAX_CLOUD_PATH_BYTES + 1),
    ("k_unMaxCloudFileChunkSize", MAX_CLOUD_FILE_BYTES),
    ("k_cchStatNameMax", MAX_STAT_NAME_LENGTH + 1),
    ("k_cchLeaderboardNameMax", MAX_LEADERBOARD_NAME_LENGTH + 1),
    ("k_cLeaderboardDetailsMax", MAX_LEADERBOARD_DETAILS),
    ("STEAM_INPUT_MAX_ORIGINS", MAX_ORIGINS),
    ("k_unMaxTimelinePriority", MAX_TIMELINE_PRIORITY as usize),
    ("k_cchMaxPhaseIDLength", MAX_PHASE_ID_LENGTH + 1),
];

/// One header's name and text.
#[derive(Debug, Clone)]
struct Header {
    name: String,
    text: String,
}

/// The tables the gate checks, so the tests can check a doctored copy.
#[derive(Debug, Clone, Copy)]
struct Tables<'a> {
    bindings: &'a [BoundFn],
    interfaces: &'a [Interface],
    decls: &'a [StructDecl],
    rows: &'a [Row],
    call_rows: &'a [CallRow],
    bases: &'a [Base],
    limits: &'a [(&'static str, usize)],
}

const REAL: Tables<'static> = Tables {
    bindings: BINDINGS,
    interfaces: INTERFACES,
    decls: DECLS,
    rows: ROWS,
    call_rows: CALL_ROWS,
    bases: Base::ALL,
    limits: LIMITS,
};

impl Tables<'_> {
    /// The `base + offset` a callback or call-result row gives `name`, as
    /// Valve writes it.
    fn callback_expression(&self, name: &str) -> Option<String> {
        let row = self
            .rows
            .iter()
            .find(|row| row.name == name)
            .map(|row| (row.base, row.offset));
        let call = self
            .call_rows
            .iter()
            .find(|row| row.name == name)
            .map(|row| (row.base, row.offset));
        row.or(call)
            .map(|(base, offset)| normalize(&format!("{} + {offset}", base.valve_name())))
    }
}

/// Collapses whitespace to single spaces, then drops every space next to a
/// character that cannot be part of an identifier — so `ISteamUser* self` and
/// `ISteamUser *self` compare equal (the headers are not consistent about
/// where the `*` goes) while `int a` and `inta` still differ.
fn normalize(text: &str) -> String {
    let collapsed: Vec<&str> = text.split_whitespace().collect();
    let collapsed = collapsed.join(" ");
    let chars: Vec<char> = collapsed.chars().collect();
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' {
            let before = chars.get(i.wrapping_sub(1)).copied().is_some_and(word);
            let after = chars.get(i + 1).copied().is_some_and(word);
            if !(before && after) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Removes `/* … */` (across lines, keeping the newlines) and `// …` comments.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let line = rest.find("//");
        let block = rest.find("/*").filter(|&b| line.is_none_or(|l| b < l));
        match (block, line) {
            (Some(b), _) => {
                out.push_str(&rest[..b]);
                let after = &rest[b + 2..];
                let end = after.find("*/").map_or(after.len(), |e| e + 2);
                out.extend(after[..end].chars().filter(|&c| c == '\n'));
                rest = &after[end..];
            }
            (None, Some(l)) => {
                out.push_str(&rest[..l]);
                let after = &rest[l..];
                let end = after.find('\n').unwrap_or(after.len());
                rest = &after[end..];
            }
            (None, None) => {
                out.push_str(rest);
                rest = "";
            }
        }
    }
    out
}

/// Whether `needle` occurs in `hay` as a whole identifier.
fn contains_word(hay: &str, needle: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    hay.match_indices(needle).any(|(at, _)| {
        let before = hay[..at].chars().next_back().is_none_or(|c| !word(c));
        let after = hay[at + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| !word(c));
        before && after
    })
}

/// The `#pragma pack` in force at a line, as the scanner reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scanned {
    /// Inside the `VALVE_CALLBACK_PACK_SMALL`/`_LARGE` selection.
    Callback,
    /// A literal `pack( push, n )`.
    Fixed(u32),
    /// No pragma pushed.
    Natural,
    /// A `pack( push, … )` whose argument is not a number.
    Unreadable,
}

impl From<Pack> for Scanned {
    fn from(pack: Pack) -> Self {
        match pack {
            Pack::Callback => Self::Callback,
            Pack::One => Self::Fixed(1),
            Pack::Natural => Self::Natural,
        }
    }
}

/// Where the scanner is inside Valve's packing selection block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    Outside,
    Small,
    Other,
}

/// One struct as the header declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    pack: Scanned,
    /// The `k_iCallback` expression, normalized, if it has one.
    callback: Option<String>,
    /// Each field declaration, normalized, without its `;`.
    fields: Vec<String>,
}

/// Every definition of `struct name` in one header, with the pragma in force
/// at each.
fn struct_blocks(text: &str, name: &str) -> Vec<Block> {
    let text = strip_comments(text);
    let mut blocks = Vec::new();
    let mut stack: Vec<Scanned> = Vec::new();
    let mut selection = Selection::Outside;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let flat = normalize(line);
        if flat.starts_with("#if") && flat.contains("VALVE_CALLBACK_PACK_SMALL") {
            selection = Selection::Small;
        } else if selection != Selection::Outside
            && (flat.starts_with("#elif") || flat.starts_with("#else"))
        {
            selection = Selection::Other;
        } else if selection != Selection::Outside && flat.starts_with("#endif") {
            selection = Selection::Outside;
        } else if let Some(n) = flat
            .strip_prefix("#pragma pack(push,")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            match selection {
                Selection::Small => stack.push(Scanned::Callback),
                Selection::Other => {}
                Selection::Outside => {
                    stack.push(n.parse().map_or(Scanned::Unreadable, Scanned::Fixed))
                }
            }
        } else if flat == "#pragma pack(pop)" {
            stack.pop();
        } else if is_struct_head(&flat, name) {
            let pack = stack.last().copied().unwrap_or(Scanned::Natural);
            blocks.push(read_body(&flat, &mut lines, pack));
        } else if let Some(callback) = macro_head(&flat, name) {
            let pack = stack.last().copied().unwrap_or(Scanned::Natural);
            blocks.push(read_macro_body(callback, &mut lines, pack));
        }
    }
    blocks
}

/// Whether a normalized line opens a definition (not a forward declaration)
/// of `struct name`.
fn is_struct_head(flat: &str, name: &str) -> bool {
    let tokens: Vec<&str> = flat
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
        .collect();
    let named = tokens.windows(2).any(|w| w[0] == "struct" && w[1] == name);
    named && !flat.ends_with(';')
}

/// The `k_iCallback` expression of a normalized
/// `STEAM_CALLBACK_BEGIN( name, expression )` line for `name`.
fn macro_head(flat: &str, name: &str) -> Option<String> {
    let (callback, expression) = flat
        .strip_prefix("STEAM_CALLBACK_BEGIN(")?
        .strip_suffix(')')?
        .split_once(',')?;
    (callback == name).then(|| expression.to_owned())
}

/// Reads a `STEAM_CALLBACK_BEGIN` body up to `STEAM_CALLBACK_END`:
/// `STEAM_CALLBACK_MEMBER( n, type, name )` is the field `type name`, and
/// `STEAM_CALLBACK_MEMBER_ARRAY( n, type, name, count )` is `type
/// name[count]`.
fn read_macro_body<'a>(
    callback: String,
    lines: &mut impl Iterator<Item = &'a str>,
    pack: Scanned,
) -> Block {
    let mut block = Block {
        pack,
        callback: Some(callback),
        fields: Vec::new(),
    };
    for line in lines {
        let flat = normalize(line);
        if flat.starts_with("STEAM_CALLBACK_END(") {
            break;
        }
        let member = flat
            .strip_prefix("STEAM_CALLBACK_MEMBER_ARRAY(")
            .or_else(|| flat.strip_prefix("STEAM_CALLBACK_MEMBER("))
            .and_then(|rest| rest.strip_suffix(')'));
        let Some(member) = member else {
            continue;
        };
        let parts: Vec<&str> = member.split(',').collect();
        let field = match parts.as_slice() {
            [_, kind, name] => format!("{kind} {name}"),
            [_, kind, name, count] => format!("{kind} {name}[{count}]"),
            // Not a shape the macros take: kept whole, so it cannot match a
            // declared field by accident.
            _ => flat.clone(),
        };
        block.fields.push(normalize(&field));
    }
    block
}

/// Whether a normalized member line declares a member function rather than
/// a data member: it has a parameter list, and is not a function-pointer
/// member, which is written `(*name)(…)`. The networking structs declare
/// methods between their fields; the gate compares data members only.
fn is_method(flat: &str) -> bool {
    flat.contains('(') && !flat.contains("(*")
}

/// Reads a struct body from its opening `{` to the matching `}`.
fn read_body<'a>(head: &str, lines: &mut impl Iterator<Item = &'a str>, pack: Scanned) -> Block {
    let mut block = Block {
        pack,
        callback: None,
        fields: Vec::new(),
    };
    let mut depth = usize::from(head.contains('{'));
    for line in lines {
        let flat = normalize(line);
        if flat.is_empty() {
            continue;
        }
        if flat.contains("k_iCallback=") {
            let expr = flat
                .split_once("k_iCallback=")
                .map(|(_, rest)| rest.trim_end_matches(';').trim_end_matches('}'))
                .unwrap_or_default();
            block.callback = Some(expr.to_owned());
            continue;
        }
        let opens = flat.matches('{').count();
        let closes = flat.matches('}').count();
        if depth == 1 && opens == 0 && closes == 0 && flat.ends_with(';') && !is_method(&flat) {
            block.fields.push(flat.trim_end_matches(';').to_owned());
        }
        depth = (depth + opens).saturating_sub(closes);
        if depth == 0 && closes > 0 {
            break;
        }
    }
    block
}

/// The quoted value of `#define name "value"` in any header.
fn define_value(headers: &[Header], name: &str) -> Option<String> {
    headers.iter().find_map(|header| {
        strip_comments(&header.text).lines().find_map(|line| {
            let mut tokens = line.split_whitespace();
            (tokens.next() == Some("#define") && tokens.next() == Some(name))
                .then(|| tokens.next())
                .flatten()
                .map(|value| value.trim_matches('"').to_owned())
        })
    })
}

/// The value of `enum { name = value };` in any header.
fn enum_value(headers: &[Header], name: &str) -> Option<i64> {
    let prefix = format!("enum{{{name}=");
    headers.iter().find_map(|header| {
        strip_comments(&header.text).lines().find_map(|line| {
            normalize(line)
                .strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix("};"))
                .and_then(|value| value.parse().ok())
        })
    })
}

/// The value of `const type name = value;` in any header, where `value` is
/// an integer or a product of integers (`100 * 1024 * 1024`).
fn const_value(headers: &[Header], name: &str) -> Option<i64> {
    let suffix = format!(" {name}");
    headers.iter().find_map(|header| {
        strip_comments(&header.text).lines().find_map(|line| {
            let flat = normalize(line);
            let (declared, value) = flat
                .strip_prefix("const ")?
                .strip_suffix(';')?
                .split_once('=')?;
            if !declared.ends_with(&suffix) {
                return None;
            }
            value.split('*').try_fold(1_i64, |product, factor| {
                product.checked_mul(factor.parse().ok()?)
            })
        })
    })
}

/// The value of a constant as the headers give it: `enum { name = value };`,
/// `#define name value`, or `const type name = value;`.
fn constant_value(headers: &[Header], name: &str) -> Option<i64> {
    enum_value(headers, name)
        .or_else(|| define_value(headers, name)?.parse().ok())
        .or_else(|| const_value(headers, name))
}

/// Every failure, as a line a human can act on.
fn check(headers: &[Header], tables: Tables<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    let lines: HashSet<String> = headers
        .iter()
        .flat_map(|header| {
            strip_comments(&header.text)
                .lines()
                .map(normalize)
                .collect::<Vec<_>>()
        })
        .collect();

    for bound in tables.bindings {
        if !lines.contains(&normalize(bound.declaration)) {
            failures.push(format!(
                "{}: no header declares `{}`",
                bound.symbol, bound.declaration
            ));
        }
    }

    let steam_api = headers
        .iter()
        .find(|header| header.name == "steam_api.h")
        .map(|header| strip_comments(&header.text));
    if steam_api.is_none() {
        failures.push("steam_api.h: not found".to_owned());
    }
    for iface in tables.interfaces {
        let call = format!("{}(", iface.accessor);
        if !lines.iter().any(|line| line.contains(&call)) {
            failures.push(format!(
                "{}: no header declares the accessor",
                iface.accessor
            ));
        }
        match define_value(headers, iface.define) {
            Some(value) if value == iface.version => {}
            Some(value) => failures.push(format!(
                "{}: the header defines {value:?}, not {:?}",
                iface.define, iface.version
            )),
            None => failures.push(format!("{}: no header defines it", iface.define)),
        }
        if let Some(text) = &steam_api
            && contains_word(text, iface.define) != iface.in_init_ex
        {
            failures.push(format!(
                "{}: in_init_ex is {}, but steam_api.h {} it",
                iface.define,
                iface.in_init_ex,
                if iface.in_init_ex {
                    "does not name"
                } else {
                    "names"
                }
            ));
        }
    }

    for decl in tables.decls {
        let blocks: Vec<Block> = headers
            .iter()
            .flat_map(|header| struct_blocks(&header.text, decl.name))
            .collect();
        let [block] = blocks.as_slice() else {
            failures.push(format!(
                "{}: {} definitions found, expected one",
                decl.name,
                blocks.len()
            ));
            continue;
        };
        let fields: Vec<String> = decl.fields.iter().map(|field| normalize(field)).collect();
        if block.fields != fields {
            failures.push(format!(
                "{}: the header's fields are {:?}, not {fields:?}",
                decl.name, block.fields
            ));
        }
        if block.pack != Scanned::from(decl.pack) {
            failures.push(format!(
                "{}: the header packs it {:?}, not {:?}",
                decl.name, block.pack, decl.pack
            ));
        }
        let expected = tables.callback_expression(decl.name);
        if block.callback != expected {
            failures.push(format!(
                "{}: the header's k_iCallback is {:?}, the callback table says {expected:?}",
                decl.name, block.callback
            ));
        }
    }
    let names = tables.rows.iter().map(|row| row.name);
    for name in names.chain(tables.call_rows.iter().map(|row| row.name)) {
        if !tables.decls.iter().any(|decl| decl.name == name) {
            failures.push(format!("{name}: a callback row with no struct declaration"));
        }
    }
    for &base in tables.bases {
        match enum_value(headers, base.valve_name()) {
            Some(value) if value == i64::from(base as i32) => {}
            Some(value) => failures.push(format!(
                "{}: the header says {value}, the table says {}",
                base.valve_name(),
                base as i32
            )),
            None => failures.push(format!("{}: no header defines it", base.valve_name())),
        }
    }
    for &(name, implied) in tables.limits {
        match constant_value(headers, name) {
            Some(value) if usize::try_from(value).ok() == Some(implied) => {}
            Some(value) => failures.push(format!(
                "{name}: the header says {value}, this crate's limit implies {implied}"
            )),
            None => failures.push(format!("{name}: no header defines it")),
        }
    }
    failures
}

/// Every `*.h` directly under `dir`.
fn read_headers(dir: &Path) -> Vec<Header> {
    let mut headers: Vec<Header> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
                .path()
        })
        .filter(|path| path.extension().is_some_and(|ext| ext == "h"))
        .map(|path| Header {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            text: String::from_utf8_lossy(
                &std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display())),
            )
            .into_owned(),
        })
        .collect();
    headers.sort_by(|a, b| a.name.cmp(&b.name));
    headers
}

#[test]
#[ignore = "needs the Steamworks SDK: set CRCBL_STEAM_SDK and run with --ignored"]
fn drift() {
    let Some(sdk) = std::env::var_os(super::load::SDK_ENV) else {
        panic!("CRCBL_STEAM_SDK is not set; the drift gate needs the Steamworks SDK");
    };
    let headers = read_headers(&Path::new(&sdk).join("public").join("steam"));
    assert!(headers.len() > 10, "{} headers found", headers.len());
    let failures = check(&headers, REAL);
    assert!(
        failures.is_empty(),
        "drift from the SDK:\n{}",
        failures.join("\n")
    );
}

// Not under Miri: this is text scanning with no `unsafe` in it, and
// interpreting its synthetic SDKs takes longer than the Miri job's whole
// budget. The ordinary test run is what proves the scanner.
#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    /// A synthetic SDK built from the crate's own tables, laid out the way the
    /// real headers are: flat prototypes one per line, `#define`d version
    /// strings, an `InitEx` list, and structs under Valve's packing selection.
    fn synthetic(tables: Tables<'_>) -> Vec<Header> {
        let mut flat = String::from("// comment\n");
        for bound in tables.bindings {
            flat.push_str(bound.declaration);
            flat.push('\n');
        }
        let mut defines = String::new();
        let mut init_ex = String::from("inline bool SteamAPI_InitEx()\n{\n");
        for iface in tables.interfaces {
            flat.push_str(&format!("S_API void *{}();\n", iface.accessor));
            defines.push_str(&format!(
                "#define {} \"{}\" /* rev */\n",
                iface.define, iface.version
            ));
            if iface.in_init_ex {
                init_ex.push_str(&format!("\t\t{}  \"\\0\"\n", iface.define));
            }
        }
        init_ex.push_str("}\n");
        let mut bases = String::new();
        for (index, &(name, value)) in tables.limits.iter().enumerate() {
            match index % 3 {
                0 => bases.push_str(&format!("enum {{ {name} = {value} }};\n")),
                1 => bases.push_str(&format!("#define {name} {value}\n")),
                _ => bases.push_str(&format!("const uint32 {name} = {value};\n")),
            }
        }
        for &base in tables.bases {
            bases.push_str(&format!(
                "enum {{ {} = {} }};\n",
                base.valve_name(),
                base as i32
            ));
        }
        // Each struct under its own pragma, as the real headers mix them.
        let mut structs = String::new();
        for decl in tables.decls {
            match decl.pack {
                Pack::Callback => structs.push_str(
                    "#if defined( VALVE_CALLBACK_PACK_SMALL )\n#pragma pack( push, 4 )\n#elif defined( VALVE_CALLBACK_PACK_LARGE )\n#pragma pack( push, 8 )\n#else\n#error pack\n#endif\n",
                ),
                Pack::One => structs.push_str("#pragma pack(push,1)\n"),
                Pack::Natural => {}
            }
            let row = tables
                .rows
                .iter()
                .find(|row| row.name == decl.name)
                .map(|row| (row.base, row.offset));
            let call = tables
                .call_rows
                .iter()
                .find(|row| row.name == decl.name)
                .map(|row| (row.base, row.offset));
            match row.or(call) {
                // A callback with no members, as Valve declares its empty
                // ones: through the macros.
                Some((base, offset)) if decl.fields.is_empty() => structs.push_str(&format!(
                    "STEAM_CALLBACK_BEGIN( {}, {} + {offset} )\nSTEAM_CALLBACK_END(0)\n",
                    decl.name,
                    base.valve_name()
                )),
                callback => {
                    structs.push_str(&format!("struct {};\n", decl.name));
                    structs.push_str(&format!("typedef struct {}\n{{\n", decl.name));
                    if let Some((base, offset)) = callback {
                        structs.push_str(&format!(
                            "\tenum {{ k_iCallback = {} + {offset} }};\n",
                            base.valve_name()
                        ));
                    }
                    for field in decl.fields {
                        structs.push_str(&format!("\t{field};\t// what it is\n"));
                    }
                    structs.push_str(&format!("}} {};\n\n", decl.name));
                }
            }
            if decl.pack != Pack::Natural {
                structs.push_str("#pragma pack( pop )\n");
            }
        }
        vec![
            Header {
                name: "steam_api_flat.h".into(),
                text: flat,
            },
            Header {
                name: "isteamthings.h".into(),
                text: defines,
            },
            Header {
                name: "steam_api.h".into(),
                text: init_ex,
            },
            Header {
                name: "callbacks.h".into(),
                text: structs,
            },
            Header {
                name: "steam_api_internal.h".into(),
                text: bases,
            },
        ]
    }

    fn edit(headers: &mut [Header], from: &str, to: &str) {
        let mut hit = false;
        for header in headers {
            if header.text.contains(from) {
                header.text = header.text.replacen(from, to, 1);
                hit = true;
                break;
            }
        }
        assert!(hit, "{from:?} not in the synthetic SDK");
    }

    #[test]
    fn normalize_ignores_spacing_but_not_words() {
        assert_eq!(
            normalize("S_API  bool  f( ISteamUser* self );"),
            "S_API bool f(ISteamUser*self);"
        );
        assert_eq!(
            normalize("S_API bool f(ISteamUser *self);"),
            normalize("S_API bool f( ISteamUser* self );")
        );
        assert_ne!(normalize("unsigned int x"), normalize("unsignedint x"));
    }

    #[test]
    fn comments_are_stripped_across_lines() {
        assert_eq!(strip_comments("a /* b\nc */ d // e\nf"), "a \n d \nf");
    }

    #[test]
    fn the_crates_own_tables_pass_against_a_synthetic_sdk() {
        let failures = check(&synthetic(REAL), REAL);
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn a_changed_parameter_type_fails() {
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "SteamAPI_ManualDispatch_RunFrame( HSteamPipe",
            "SteamAPI_ManualDispatch_RunFrame( int64",
        );
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(
            failures[0].starts_with("SteamAPI_ManualDispatch_RunFrame:"),
            "{failures:#?}"
        );
    }

    #[test]
    fn a_wrong_version_string_or_a_missing_accessor_fails() {
        let mut headers = synthetic(REAL);
        edit(&mut headers, "\"SteamUser023\"", "\"SteamUser024\"");
        assert_eq!(check(&headers, REAL).len(), 1);
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "SteamAPI_SteamUtils_v011(",
            "SteamAPI_SteamUtils_v012(",
        );
        assert_eq!(check(&headers, REAL).len(), 1);
    }

    #[test]
    fn an_init_ex_flag_steam_api_h_disagrees_with_fails() {
        let mut headers = synthetic(REAL);
        edit(&mut headers, "STEAMUTILS_INTERFACE_VERSION  \"\\0\"", "");
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(failures[0].contains("in_init_ex"), "{failures:#?}");
    }

    #[test]
    fn a_renamed_field_fails() {
        let mut headers = synthetic(REAL);
        edit(&mut headers, "uint8 m_bActive", "uint8 m_bIsActive");
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(
            failures[0].starts_with("GameOverlayActivated_t:"),
            "{failures:#?}"
        );
    }

    #[test]
    fn a_changed_pragma_fails() {
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "#if defined( VALVE_CALLBACK_PACK_SMALL )\n#pragma pack( push, 4 )",
            "#if 0\n#endif\n#pragma pack( push, 1 )\n#if defined( VALVE_CALLBACK_PACK_SMALL )",
        );
        // The first struct is the sentinel, under the callback selection.
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(
            failures[0].starts_with("ValvePackingSentinel_t: the header packs it Fixed(1)"),
            "{failures:#?}"
        );
        // And the other way: a pack(1) struct found under another packing.
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "#pragma pack(push,1)\nstruct SteamNetworkingIPAddr;",
            "#pragma pack(push,4)\nstruct SteamNetworkingIPAddr;",
        );
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(
            failures[0].contains("packs it Fixed(4), not One"),
            "{failures:#?}"
        );
    }

    #[test]
    fn a_wrong_callback_offset_fails() {
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "k_iSteamFriendsCallbacks + 31",
            "k_iSteamFriendsCallbacks + 32",
        );
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(failures[0].contains("k_iCallback"), "{failures:#?}");
    }

    #[test]
    fn a_wrong_call_result_offset_fails() {
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "k_iSteamMatchmakingCallbacks + 13",
            "k_iSteamMatchmakingCallbacks + 14",
        );
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(failures[0].starts_with("LobbyCreated_t:"), "{failures:#?}");
    }

    #[test]
    fn a_wrong_or_missing_base_value_fails() {
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "k_iSteamMatchmakingCallbacks = 500",
            "k_iSteamMatchmakingCallbacks = 501",
        );
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(failures[0].contains("the header says 501"), "{failures:#?}");
        let mut headers = synthetic(REAL);
        edit(&mut headers, "enum { k_iSteamAppsCallbacks = 1000 };\n", "");
        assert_eq!(check(&headers, REAL).len(), 1);
    }

    #[test]
    fn a_limit_the_header_disagrees_with_fails() {
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "#define k_cchMaxRichPresenceKeyLength 64",
            "#define k_cchMaxRichPresenceKeyLength 128",
        );
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(failures[0].contains("implies 64"), "{failures:#?}");
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "#define k_cchMaxRichPresenceKeyLength",
            "#define k_cchSomethingElse",
        );
        let failures = check(&headers, REAL);
        assert!(
            failures
                .iter()
                .any(|f| f.contains("k_cchMaxRichPresenceKeyLength")),
            "{failures:#?}"
        );
    }

    #[test]
    fn methods_are_skipped_and_function_pointer_members_kept() {
        let blocks = struct_blocks(
            "struct A
{
	int x;
	void Clear();
	bool Ok() const { return true; }
	void (*m_pfn)( A *p );
	enum {
		k_n = 1,
	};
	union {
		int u;
	};
};
",
            "A",
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].fields, ["int x", "void(*m_pfn)(A*p)"]);
    }

    #[test]
    fn a_macro_declared_callback_reads_like_a_written_out_one() {
        let text = "#pragma pack( push, 1 )
STEAM_CALLBACK_BEGIN( A_t, k_iSteamVideoCallbacks + 11 )
\tSTEAM_CALLBACK_MEMBER( 0, EResult, m_eResult ) // result
\tSTEAM_CALLBACK_MEMBER( 1, char, m_rgchURL[256] )
\tSTEAM_CALLBACK_MEMBER_ARRAY( 2, uint8, m_rgubData, 16 )
STEAM_CALLBACK_END(3)
STEAM_CALLBACK_BEGIN( B_t, k_iSteamVideoCallbacks + 12 )
STEAM_CALLBACK_END(0)
#pragma pack( pop )
";
        assert_eq!(
            struct_blocks(text, "A_t"),
            [Block {
                pack: Scanned::Fixed(1),
                callback: Some("k_iSteamVideoCallbacks+11".into()),
                fields: vec![
                    "EResult m_eResult".into(),
                    "char m_rgchURL[256]".into(),
                    "uint8 m_rgubData[16]".into()
                ]
            }]
        );
        let empty = struct_blocks(text, "B_t");
        assert_eq!(empty.len(), 1);
        assert!(empty[0].fields.is_empty());
        assert_eq!(
            empty[0].callback.as_deref(),
            Some("k_iSteamVideoCallbacks+12")
        );
    }

    #[test]
    fn a_const_limit_is_read_and_a_product_multiplied() {
        let headers = [Header {
            name: "a.h".into(),
            text: "const uint32 k_cchFilenameMax = 260;\n\
                   const uint32 k_unChunk = 100 * 1024 * 1024; // 100MB\n"
                .into(),
        }];
        assert_eq!(constant_value(&headers, "k_cchFilenameMax"), Some(260));
        assert_eq!(constant_value(&headers, "k_unChunk"), Some(104_857_600));
        assert_eq!(
            constant_value(&headers, "k_cchFilename"),
            None,
            "whole names only"
        );
    }

    #[test]
    fn a_struct_outside_any_pragma_reads_as_natural() {
        let blocks = struct_blocks("struct A\n{\n\tint x;\n};\n", "A");
        assert_eq!(
            blocks,
            [Block {
                pack: Scanned::Natural,
                callback: None,
                fields: vec!["int x".into()]
            }]
        );
    }

    #[test]
    fn a_missing_or_duplicated_struct_fails() {
        let mut headers = synthetic(REAL);
        edit(
            &mut headers,
            "typedef struct CallbackMsg_t\n",
            "typedef struct CallbackMsgRenamed_t\n",
        );
        assert_eq!(check(&headers, REAL).len(), 1);
        let mut headers = synthetic(REAL);
        let copy = headers[3].clone();
        headers.push(Header {
            name: "again.h".into(),
            ..copy
        });
        assert_eq!(check(&headers, REAL).len(), DECLS.len());
    }
}
