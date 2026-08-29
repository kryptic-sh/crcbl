//! The settings file as it stands, in the debug panel.
//!
//! `docs/plan/sample/20-options.md`'s scope asks for "a view of the settings
//! file as it stands", and `SettingsStack::dump` already produces one: the
//! layers merged into TOML, every key reading as `get` would. The screen's rows
//! show the keys they own, and the file may hold keys none of them do — a
//! hand-edited `[engine.window]`, a key from a build that had a row this one
//! does not — and this is the one place those show. It lives under `F3`
//! because that is where the frame's other facts already are, and where a
//! player who typed a key into the file goes looking for it.
//!
//! [`FileView`] borrows the stack for the one call the panel makes; the dump is
//! serialised on every frame the panel is **visible** — `DebugPanel::add` asks
//! nothing of a module while it is hidden — and a settings file is a few dozen
//! lines, so that is a string a frame can afford.

use crcbl::store::settings::SettingsStack;
use crcbl::ui::{DebugModule, DebugSection};

/// What the panel's heading says.
pub const TITLE: &str = "settings file";

/// The one row an empty file gets, so an empty section is a statement rather
/// than a heading over nothing.
pub const EMPTY_ROW: (&str, &str) = ("file", "empty");

/// The stack, as a section of the debug panel.
#[derive(Debug)]
pub struct FileView<'a>(pub &'a SettingsStack);

impl DebugModule for FileView<'_> {
    fn debug_section(&self, section: &mut DebugSection) {
        section.set_title(TITLE);
        let dump = self.0.dump();
        let mut any = false;
        for (label, value) in rows(&dump) {
            section.row_str(label, value);
            any = true;
        }
        if !any {
            section.row_str(EMPTY_ROW.0, EMPTY_ROW.1);
        }
    }
}

/// The `label: value` rows of a TOML dump, in the dump's order.
///
/// A `[table]` header is a row with the header as its label and nothing
/// beside it, so the keys under it read as the file groups them; a
/// `key = value` line is one row, split at the first ` = `, which is the one
/// the serialiser wrote — a value that contains the same three characters is
/// a string, and the split lands before it. Anything else — a blank line, or
/// a continuation line of a value the serialiser broke over several — is
/// skipped rather than shown as a key it is not.
pub fn rows(dump: &str) -> impl Iterator<Item = (&str, &str)> {
    dump.lines().filter_map(|line| {
        let line = line.trim_end();
        if line.starts_with('[') && line.ends_with(']') {
            Some((line, ""))
        } else {
            line.split_once(" = ")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A header is a row of its own, a key is a row, and nothing else is.**
    /// The blank line the serialiser puts between tables would otherwise be a
    /// key with no name, and a header split at ` = ` would be no row at all.
    #[test]
    fn a_dump_reads_as_headers_and_keys_in_order() {
        let dump = "top = 1\n\n[engine.audio]\nmusic_volume = 0.25\n\n[engine.video]\nframe_limit = 60\nlabel = \"a = b\"\n";
        let rows: Vec<_> = rows(dump).collect();
        assert_eq!(
            rows,
            [
                ("top", "1"),
                ("[engine.audio]", ""),
                ("music_volume", "0.25"),
                ("[engine.video]", ""),
                ("frame_limit", "60"),
                ("label", "\"a = b\""),
            ],
        );
    }

    /// A continuation line — an array the serialiser broke over several lines
    /// — is not a key, and an empty dump has no rows at all.
    #[test]
    fn a_continuation_line_is_not_a_key_and_an_empty_dump_has_no_rows() {
        let dump = "[engine]\nlist = [\n    1,\n    2,\n]\n";
        let read: Vec<_> = rows(dump).collect();
        assert_eq!(read, [("[engine]", ""), ("list", "[")]);
        assert_eq!(rows("").count(), 0);
        assert_eq!(rows("\n\n").count(), 0);
    }
}
