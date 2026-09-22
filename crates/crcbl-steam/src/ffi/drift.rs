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
//!    callback, the `k_iCallback = base + n` expression against the row in
//!    `callbacks`.
//!
//! Without `CRCBL_STEAM_SDK` it **fails**: it only ever runs on purpose, and
//! "skipped" must not read as "passed".
//!
//! **It has never run against a real SDK** — no machine this crate was
//! written on had one. The scanner itself is proven by this module's other
//! tests, which build a synthetic SDK out of the crate's own tables, check
//! that it passes, and check that one changed parameter type, one renamed
//! field, one changed pragma, one wrong version string and one wrong callback
//! offset each fail it.

use std::{collections::HashSet, path::Path};

use super::{
    manifest::{BINDINGS, BoundFn, INTERFACES},
    structs::{DECLS, Pack, StructDecl},
    versions::Interface,
};
use crate::callbacks::{ROWS, Row};

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
}

const REAL: Tables<'static> = Tables {
    bindings: BINDINGS,
    interfaces: INTERFACES,
    decls: DECLS,
    rows: ROWS,
};

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
        let after = hay[at + needle.len()..].chars().next().is_none_or(|c| !word(c));
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
                Selection::Outside => stack.push(n.parse().map_or(Scanned::Unreadable, Scanned::Fixed)),
            }
        } else if flat == "#pragma pack(pop)" {
            stack.pop();
        } else if is_struct_head(&flat, name) {
            let pack = stack.last().copied().unwrap_or(Scanned::Natural);
            blocks.push(read_body(&flat, &mut lines, pack));
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
        if depth == 1 && opens == 0 && closes == 0 && flat.ends_with(';') {
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

/// Every failure, as a line a human can act on.
fn check(headers: &[Header], tables: Tables<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    let lines: HashSet<String> = headers
        .iter()
        .flat_map(|header| strip_comments(&header.text).lines().map(normalize).collect::<Vec<_>>())
        .collect();

    for bound in tables.bindings {
        if !lines.contains(&normalize(bound.declaration)) {
            failures.push(format!("{}: no header declares `{}`", bound.symbol, bound.declaration));
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
            failures.push(format!("{}: no header declares the accessor", iface.accessor));
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
                if iface.in_init_ex { "does not name" } else { "names" }
            ));
        }
    }

    for decl in tables.decls {
        let blocks: Vec<Block> = headers
            .iter()
            .flat_map(|header| struct_blocks(&header.text, decl.name))
            .collect();
        let [block] = blocks.as_slice() else {
            failures.push(format!("{}: {} definitions found, expected one", decl.name, blocks.len()));
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
        let expected = tables
            .rows
            .iter()
            .find(|row| row.name == decl.name)
            .map(|row| normalize(&format!("{} + {}", row.base.valve_name(), row.offset)));
        if block.callback != expected {
            failures.push(format!(
                "{}: the header's k_iCallback is {:?}, the callback table says {expected:?}",
                decl.name, block.callback
            ));
        }
    }
    for row in tables.rows {
        if !tables.decls.iter().any(|decl| decl.name == row.name) {
            failures.push(format!("{}: a callback row with no struct declaration", row.name));
        }
    }
    failures
}

/// Every `*.h` directly under `dir`.
fn read_headers(dir: &Path) -> Vec<Header> {
    let mut headers: Vec<Header> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()))
        .map(|entry| entry.unwrap_or_else(|err| panic!("{}: {err}", dir.display())).path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "h"))
        .map(|path| Header {
            name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
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
    assert!(failures.is_empty(), "drift from the SDK:\n{}", failures.join("\n"));
}

#[cfg(test)]
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
            defines.push_str(&format!("#define {} \"{}\" /* rev */\n", iface.define, iface.version));
            if iface.in_init_ex {
                init_ex.push_str(&format!("\t\t{}  \"\\0\"\n", iface.define));
            }
        }
        init_ex.push_str("}\n");
        let mut structs = String::from(
            "#if defined( VALVE_CALLBACK_PACK_SMALL )\n#pragma pack( push, 4 )\n#elif defined( VALVE_CALLBACK_PACK_LARGE )\n#pragma pack( push, 8 )\n#else\n#error pack\n#endif\n",
        );
        for decl in tables.decls {
            structs.push_str(&format!("struct {};\n", decl.name));
            structs.push_str(&format!("typedef struct {}\n{{\n", decl.name));
            if let Some(row) = tables.rows.iter().find(|row| row.name == decl.name) {
                structs.push_str(&format!(
                    "\tenum {{ k_iCallback = {} + {} }};\n",
                    row.base.valve_name(),
                    row.offset
                ));
            }
            for field in decl.fields {
                structs.push_str(&format!("\t{field};\t// what it is\n"));
            }
            structs.push_str(&format!("}} {};\n\n", decl.name));
        }
        structs.push_str("#pragma pack( pop )\n");
        vec![
            Header { name: "steam_api_flat.h".into(), text: flat },
            Header { name: "isteamthings.h".into(), text: defines },
            Header { name: "steam_api.h".into(), text: init_ex },
            Header { name: "callbacks.h".into(), text: structs },
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
        assert_eq!(normalize("S_API  bool  f( ISteamUser* self );"), "S_API bool f(ISteamUser*self);");
        assert_eq!(normalize("S_API bool f(ISteamUser *self);"), normalize("S_API bool f( ISteamUser* self );"));
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
        edit(&mut headers, "SteamAPI_ManualDispatch_RunFrame( HSteamPipe", "SteamAPI_ManualDispatch_RunFrame( int64");
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(failures[0].starts_with("SteamAPI_ManualDispatch_RunFrame:"), "{failures:#?}");
    }

    #[test]
    fn a_wrong_version_string_or_a_missing_accessor_fails() {
        let mut headers = synthetic(REAL);
        edit(&mut headers, "\"SteamUser023\"", "\"SteamUser024\"");
        assert_eq!(check(&headers, REAL).len(), 1);
        let mut headers = synthetic(REAL);
        edit(&mut headers, "SteamAPI_SteamUtils_v011(", "SteamAPI_SteamUtils_v012(");
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
        assert!(failures[0].starts_with("GameOverlayActivated_t:"), "{failures:#?}");
    }

    #[test]
    fn a_changed_pragma_fails() {
        let mut headers = synthetic(REAL);
        edit(&mut headers, "#if defined( VALVE_CALLBACK_PACK_SMALL )\n#pragma pack( push, 4 )", "#if 0\n#endif\n#pragma pack( push, 1 )\n#if defined( VALVE_CALLBACK_PACK_SMALL )");
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), DECLS.len(), "{failures:#?}");
        assert!(failures.iter().all(|f| f.contains("packs it Fixed(1)")), "{failures:#?}");
    }

    #[test]
    fn a_wrong_callback_offset_fails() {
        let mut headers = synthetic(REAL);
        edit(&mut headers, "k_iSteamFriendsCallbacks + 31", "k_iSteamFriendsCallbacks + 32");
        let failures = check(&headers, REAL);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert!(failures[0].contains("k_iCallback"), "{failures:#?}");
    }

    #[test]
    fn a_struct_outside_any_pragma_reads_as_natural() {
        let blocks = struct_blocks("struct A\n{\n\tint x;\n};\n", "A");
        assert_eq!(
            blocks,
            [Block { pack: Scanned::Natural, callback: None, fields: vec!["int x".into()] }]
        );
    }

    #[test]
    fn a_missing_or_duplicated_struct_fails() {
        let mut headers = synthetic(REAL);
        edit(&mut headers, "typedef struct CallbackMsg_t\n", "typedef struct CallbackMsgRenamed_t\n");
        assert_eq!(check(&headers, REAL).len(), 1);
        let mut headers = synthetic(REAL);
        let copy = headers[3].clone();
        headers.push(Header { name: "again.h".into(), ..copy });
        assert_eq!(check(&headers, REAL).len(), DECLS.len());
    }
}
