//! What an input method's composition becomes on the seam, with no platform
//! in it.
//!
//! Compiled wherever a consumer exists — the AppKit and Win32 backends — and,
//! under `cfg(test)`, on every host, on the terms [`crate::keysym`] is. Both
//! platforms hand a composition over in UTF-16 code units, with its cursor as a
//! code-unit index — `ImmGetCompositionStringW` on Win32, an `NSString` and an
//! `NSRange` on AppKit — while
//! [`TextPreedit::cursor`](crate::ShellEvent::TextPreedit) is a byte offset
//! into UTF-8. And both end a composition more than one way at once, which the
//! seam reports once. Each was written for Win32 first; AppKit is the second
//! consumer, which is what moved them here.

use crate::WindowId;

/// An input method's composition string as UTF-8, with its cursor.
///
/// `units` is the string in UTF-16 code units and `cursor` a **code unit**
/// index into it: `ImmGetCompositionStringW` and its `GCS_CURSORPOS` on Win32,
/// `setMarkedText:selectedRange:replacementRange:`'s string and its selection's
/// location on AppKit. The seam's
/// [`TextPreedit::cursor`](crate::ShellEvent::TextPreedit) is a byte offset
/// into the UTF-8 text. The two disagree on everything outside ASCII — every
/// kana is one unit and three bytes — so the index is walked across rather than
/// copied. A cursor past the end, or inside a surrogate pair, lands on the
/// nearest `char` boundary at or before it. An unpaired surrogate becomes
/// U+FFFD, as it would anywhere else a UTF-16 string reaches UTF-8.
#[must_use]
pub fn preedit(units: &[u16], cursor: Option<usize>) -> (String, Option<usize>) {
    let mut text = String::with_capacity(units.len());
    let mut at = None;
    let mut consumed = 0;
    for character in char::decode_utf16(units.iter().copied()) {
        let character = character.unwrap_or(char::REPLACEMENT_CHARACTER);
        // The first character that reaches past the cursor is the one it sits
        // before — or inside, for a cursor between a surrogate pair's halves,
        // which this puts before the pair.
        if at.is_none() && cursor.is_some_and(|cursor| consumed + character.len_utf16() > cursor) {
            at = Some(text.len());
        }
        consumed += character.len_utf16();
        text.push(character);
    }
    if cursor.is_some() && at.is_none() {
        at = Some(text.len());
    }
    if text.is_empty() {
        return (text, None);
    }
    (text, at)
}

/// The windows showing a pre-edit, so each composition ends once.
///
/// An input method ends a composition more than one way at once — the string
/// deleted to nothing, then `WM_IME_ENDCOMPOSITION` on Win32 or `unmarkText`
/// on AppKit — and one "no pre-edit" is the whole answer. An end for a window that was never shown a pre-edit is
/// dropped too: there is nothing for the consumer to clear.
#[derive(Debug, Default)]
pub struct Composing {
    windows: Vec<WindowId>,
}

impl Composing {
    /// Records a pre-edit of `text` for `window`, answering whether the
    /// consumer is owed it: every non-empty one is, and an empty one only when
    /// it ends a pre-edit the consumer was shown.
    pub fn report(&mut self, window: WindowId, text: &str) -> bool {
        let showing = self.windows.contains(&window);
        if text.is_empty() {
            self.forget(window);
            return showing;
        }
        if !showing {
            self.windows.push(window);
        }
        true
    }

    /// Forgets `window`, which is being destroyed or has ended its composition.
    pub fn forget(&mut self, window: WindowId) {
        self.windows.retain(|&known| known != window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_composition_cursor_is_counted_in_code_units_and_reported_in_bytes() {
        // か (U+304B) is one code unit and three bytes.
        let ka: Vec<u16> = "かな".encode_utf16().collect();
        assert_eq!(preedit(&ka, Some(0)), ("かな".to_owned(), Some(0)));
        assert_eq!(preedit(&ka, Some(1)), ("かな".to_owned(), Some(3)));
        assert_eq!(preedit(&ka, Some(2)), ("かな".to_owned(), Some(6)));
        // Past the end clamps to the end rather than inventing an offset.
        assert_eq!(preedit(&ka, Some(9)), ("かな".to_owned(), Some(6)));
        assert_eq!(preedit(&ka, None), ("かな".to_owned(), None));

        // An astral character is two code units; a cursor between them lands
        // before it, never inside it.
        let astral: Vec<u16> = "a𠀋b".encode_utf16().collect();
        assert_eq!(preedit(&astral, Some(3)), ("a𠀋b".to_owned(), Some(5)));
        assert_eq!(preedit(&astral, Some(2)), ("a𠀋b".to_owned(), Some(1)));
        assert_eq!(preedit(&astral, Some(1)), ("a𠀋b".to_owned(), Some(1)));

        // Nothing composed is no pre-edit, and has no cursor.
        assert_eq!(preedit(&[], Some(0)), (String::new(), None));
        // A lone surrogate does not abort the string.
        assert_eq!(
            preedit(&[0xD800, 0x61], None),
            ("\u{FFFD}a".to_owned(), None)
        );
    }

    #[test]
    fn a_composition_ends_once_and_only_where_one_was_shown() {
        // Generation 1 in the high half, which a real handle never lacks.
        let window = |index: u64| WindowId::from_bits(1 << 32 | index).expect("a valid handle");
        let (a, b) = (window(0), window(1));
        let mut composing = Composing::default();
        // An end with nothing shown is nothing to clear.
        assert!(!composing.report(a, ""));
        assert!(composing.report(a, "か"));
        assert!(composing.report(a, "かな"), "each change is reported");
        assert!(composing.report(b, "な"));
        // The string deleted to nothing, then the end message: one end.
        assert!(composing.report(a, ""));
        assert!(!composing.report(a, ""), "the second end is dropped");
        // A destroyed window's composition does not outlive it.
        composing.forget(b);
        assert!(!composing.report(b, ""));
    }
}
