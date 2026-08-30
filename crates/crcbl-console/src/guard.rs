//! Holding a crate's console table to its own source.
//!
//! Plan decision 2's second half. A crate declares its variables and commands
//! beside the code that owns them and lists them once in a `console_table()`;
//! nothing but a test makes the list keep up. That test is the same text-guard
//! shape `crcbl_shaders::volumetric`'s
//! `both_shaders_spell_the_same_atlas_walk` already uses: read the crate's own
//! `src/`, take every name it declared, and assert each is in the table. A
//! forgotten entry is then a red test rather than a command that quietly does
//! not exist.
//!
//! The helper is public so every crate writes the same three lines:
//!
//! ```no_run
//! # fn console_table() -> crcbl_console::Table { crcbl_console::Table::EMPTY }
//! let declared = crcbl_console::guard::declared_names("src").expect("this crate's src");
//! let table = console_table();
//! for name in &declared {
//!     assert!(
//!         table.commands().iter().any(|command| command.name() == name)
//!             || table.vars().iter().any(|var| var.name() == name),
//!         "`{name}` is declared in this crate's source and missing from its table"
//!     );
//! }
//! ```
//!
//! **It reads text, not syntax.** Two limits follow, and both are deliberate —
//! a parser would be a dependency this crate does not have:
//!
//! - Line comments are dropped before the scan, so a declaration in a doc
//!   example is not mistaken for a real one. Block comments are not.
//! - The name is the ident of the `static` or `fn` inside the invocation, which
//!   is what the macros use as the console name.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Every console name declared under `src_dir`, sorted, without duplicates.
///
/// Walks every `.rs` file below the directory.
///
/// # Errors
///
/// Whatever reading the directory or a file in it failed with.
pub fn declared_names(src_dir: impl AsRef<Path>) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    let mut pending = vec![src_dir.as_ref().to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let path: PathBuf = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                names.extend(names_in(&fs::read_to_string(&path)?));
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// Every console name one Rust source declares.
///
/// The seam [`declared_names`] is built on, public so a caller can scan text it
/// already has — an `include_str!`, or a fixture in a test that would otherwise
/// need a temporary directory.
#[must_use]
pub fn names_in(source: &str) -> Vec<String> {
    // Spelled in halves so this scanner does not find itself: the needle it
    // looks for would otherwise be sitting in the file it is applied to.
    let convar = concat!("convar", "!");
    let concommand = concat!("concommand", "!");

    let text = without_line_comments(source);
    let mut names = Vec::new();
    for (needle, keyword) in [(convar, "static"), (concommand, "fn")] {
        let mut rest = text.as_str();
        while let Some(at) = rest.find(needle) {
            rest = &rest[at + needle.len()..];
            if let Some(name) = ident_after(rest, keyword) {
                names.push(name);
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// `source` with everything from each `//` to the end of its line removed.
fn without_line_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        out.push_str(line.split("//").next().unwrap_or(""));
        out.push('\n');
    }
    out
}

/// The identifier following the first whole-word `keyword` in `text`.
fn ident_after(text: &str, keyword: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = text[from..].find(keyword) {
        let start = from + at;
        let end = start + keyword.len();
        let before_is_word = text[..start].chars().next_back().is_some_and(is_ident_char);
        let after_is_word = text[end..].chars().next().is_some_and(is_ident_char);
        from = end;
        if before_is_word || after_is_word {
            continue;
        }
        let ident: String = text[end..]
            .trim_start()
            .chars()
            .take_while(|c| is_ident_char(*c))
            .collect();
        return (!ident.is_empty()).then_some(ident);
    }
    None
}

/// Whether `c` may appear in a Rust identifier.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source with one of each declaration, written as a fixture rather than
    /// as a real one: a real declaration in this file would be a console entry
    /// the crate's own table then has to carry.
    ///
    /// The needles are joined at runtime for the same reason
    /// [`names_in`] spells them in halves.
    fn fixture() -> String {
        let convar = concat!("convar", "!");
        let concommand = concat!("concommand", "!");
        format!(
            "\
//! A doc example that must not count:
//! crcbl_console::{convar} {{ pub static not_a_real_one: bool = false; }}
crcbl_console::{convar} {{
    /// help
    pub static r_ao_view: bool = false;
}}
{convar} {{
    /// help
    #[flags(ARCHIVE)]
    pub static anisotropic_filtering: i64 in 1..=16 = 1;
}}
crate::{concommand} {{
    /// help
    pub fn quit(cx, _args) {{ Ok(()) }}
}}
fn something_else() {{}}
"
        )
    }

    #[test]
    fn every_declaration_in_a_source_is_found_by_its_ident() {
        assert_eq!(
            names_in(&fixture()),
            ["anisotropic_filtering", "quit", "r_ao_view"]
        );
    }

    #[test]
    fn a_declaration_inside_a_comment_is_not_a_declaration() {
        assert!(
            !names_in(&fixture())
                .iter()
                .any(|name| name == "not_a_real_one")
        );
    }

    #[test]
    fn a_source_that_declares_nothing_yields_nothing() {
        assert!(names_in("fn main() { let statics = 1; }").is_empty());
    }

    #[test]
    fn a_word_that_merely_contains_the_keyword_is_not_the_keyword() {
        // `statically` and `fnord` hold `static` and `fn`, and neither declares
        // anything; the ident has to come after the keyword itself.
        let convar = concat!("convar", "!");
        let source = format!("{convar} {{ statically fnord static real_one: bool }}");
        assert_eq!(names_in(&source), ["real_one"]);
    }

    #[test]
    fn the_scan_walks_this_crate_s_own_source_tree() {
        // The directory walk itself, rather than only the text scan: a scan that
        // matched no file would report an empty list and prove nothing.
        let names = declared_names("src").expect("this crate's src directory");
        assert!(
            names.contains(&"echo".to_owned()),
            "the walk did not reach `builtin.rs`: {names:?}"
        );
    }
}
