//! The editing model: every edit against every selection state, word
//! boundaries, multi-byte and combining text, and the keys that make edits.

use super::*;

/// A line written with its caret and anchor in it: `|` is the caret and `[`
/// the anchor, which is left out when it is at the caret.
fn line(marked: &str) -> LineEdit {
    let mut edit = LineEdit::new();
    let (mut caret, mut anchor) = (None, None);
    let mut count = 0;
    for c in marked.chars() {
        match c {
            '|' => caret = Some(count),
            '[' => anchor = Some(count),
            _ => {
                edit.text.push(c);
                count += 1;
            }
        }
    }
    edit.caret = caret.expect("a marked line has a caret");
    edit.anchor = anchor.unwrap_or(edit.caret);
    edit
}

/// `edit` written back the way [`line`] reads it.
fn marked(edit: &LineEdit) -> String {
    let mut out = String::new();
    for (index, c) in edit.text.chars().chain(['\0']).enumerate() {
        if index == edit.anchor && edit.anchor != edit.caret {
            out.push('[');
        }
        if index == edit.caret {
            out.push('|');
        }
        if c != '\0' {
            out.push(c);
        }
    }
    out
}

const fn mv(motion: Motion) -> Edit {
    Edit::Move {
        motion,
        select: false,
    }
}

const fn sel(motion: Motion) -> Edit {
    Edit::Move {
        motion,
        select: true,
    }
}

/// One row of the edit table: the edit, what each state becomes, and what
/// each asks of the clipboard.
type Row = (Edit, [&'static str; 3], [Option<ClipboardOp>; 3]);

/// **Every edit against every selection state.** The three states are one
/// line, `one two`, with the caret after `t` and nothing selected; with
/// `ne t` selected forwards (the anchor before it, the caret after); and with
/// the same text selected backwards. Each row is the edit, then what each
/// state becomes, then what each asks of the clipboard.
#[test]
fn every_edit_against_every_selection_state() {
    const STATES: [&str; 3] = ["one t|wo", "o[ne t|wo", "o|ne t]wo"];
    let offer = |text: &str| Some(ClipboardOp::Offer(text.to_owned()));
    let table: Vec<Row> = vec![
        (
            Edit::Insert("X".to_owned()),
            ["one tX|wo", "oX|wo", "oX|wo"],
            [None, None, None],
        ),
        (
            mv(Motion::Left),
            ["one |two", "o|ne two", "o|ne two"],
            [None, None, None],
        ),
        (
            mv(Motion::Right),
            ["one tw|o", "one t|wo", "one t|wo"],
            [None, None, None],
        ),
        (
            mv(Motion::WordLeft),
            ["one |two", "|one two", "|one two"],
            [None, None, None],
        ),
        (
            mv(Motion::WordRight),
            ["one two|", "one two|", "one two|"],
            [None, None, None],
        ),
        (
            mv(Motion::Home),
            ["|one two", "|one two", "|one two"],
            [None, None, None],
        ),
        (
            mv(Motion::End),
            ["one two|", "one two|", "one two|"],
            [None, None, None],
        ),
        (
            sel(Motion::Left),
            ["one |t[wo", "o[ne |two", "|one t[wo"],
            [None, None, None],
        ),
        (
            sel(Motion::Right),
            ["one t[w|o", "o[ne tw|o", "on|e t[wo"],
            [None, None, None],
        ),
        (
            sel(Motion::WordLeft),
            ["one |t[wo", "o[ne |two", "|one t[wo"],
            [None, None, None],
        ),
        (
            sel(Motion::WordRight),
            ["one t[wo|", "o[ne two|", "one| t[wo"],
            [None, None, None],
        ),
        (
            sel(Motion::Home),
            ["|one t[wo", "|o[ne two", "|one t[wo"],
            [None, None, None],
        ),
        (
            sel(Motion::End),
            ["one t[wo|", "o[ne two|", "one t[wo|"],
            [None, None, None],
        ),
        (
            Edit::Backspace,
            ["one |wo", "o|wo", "o|wo"],
            [None, None, None],
        ),
        (
            Edit::Delete,
            ["one t|o", "o|wo", "o|wo"],
            [None, None, None],
        ),
        (
            Edit::SelectAll,
            ["[one two|", "[one two|", "[one two|"],
            [None, None, None],
        ),
        (Edit::Copy, STATES, [None, offer("ne t"), offer("ne t")]),
        (
            Edit::Cut,
            ["one t|wo", "o|wo", "o|wo"],
            [None, offer("ne t"), offer("ne t")],
        ),
        (
            Edit::Paste,
            STATES,
            [
                Some(ClipboardOp::Read),
                Some(ClipboardOp::Read),
                Some(ClipboardOp::Read),
            ],
        ),
    ];
    // The table's backward state is written with `]` for readability; the
    // reader takes `[` for the anchor wherever it lies.
    let read = |state: &str| line(&state.replace(']', "["));
    for (edit, after, clipboard) in table {
        for ((state, want), want_clipboard) in STATES.iter().zip(after).zip(clipboard) {
            let mut model = read(state);
            let before = model.clone();
            let applied = model.apply(&edit);
            assert_eq!(marked(&model), marked(&read(want)), "{edit:?} on `{state}`");
            assert_eq!(
                applied.clipboard, want_clipboard,
                "{edit:?} on `{state}` asked the clipboard for the wrong thing"
            );
            assert_eq!(
                applied.text,
                model.text != before.text,
                "{edit:?} on `{state}` misreported a text change"
            );
            assert_eq!(
                applied.caret,
                (model.caret, model.anchor) != (before.caret, before.anchor),
                "{edit:?} on `{state}` misreported a caret move"
            );
        }
    }
}

/// **The ends of the line stop every move**, and a deletion with nothing on
/// its side deletes nothing.
#[test]
fn moves_and_deletions_stop_at_the_ends_of_the_line() {
    let mut start = line("|ab");
    for edit in [
        mv(Motion::Left),
        mv(Motion::WordLeft),
        mv(Motion::Home),
        Edit::Backspace,
    ] {
        let applied = start.apply(&edit);
        assert_eq!(marked(&start), "|ab", "{edit:?} at the start");
        assert_eq!(applied, Applied::default(), "{edit:?} reported work");
    }
    let mut end = line("ab|");
    for edit in [
        mv(Motion::Right),
        mv(Motion::WordRight),
        mv(Motion::End),
        Edit::Delete,
    ] {
        let applied = end.apply(&edit);
        assert_eq!(marked(&end), "ab|", "{edit:?} at the end");
        assert_eq!(applied, Applied::default(), "{edit:?} reported work");
    }
}

/// **Word boundaries**: a word is a run of alphanumerics and `_` or a run of
/// other symbols, whitespace between them is passed over, and a move ends at
/// the far edge of the word it reached.
#[test]
fn word_moves_stop_at_the_edges_of_runs_of_one_class() {
    let text: Vec<char> = "  foo_1.bar  -- baz".chars().collect();
    // Every stop a walk right makes from the start, then a walk left from the
    // end: `foo_1` is one word, `.` is its own run, `--` is one run.
    let mut stops = Vec::new();
    let mut at = 0;
    loop {
        let next = word_right(&text, at);
        if next == at {
            break;
        }
        stops.push(next);
        at = next;
    }
    assert_eq!(stops, [7, 8, 11, 15, 19]);

    let mut stops = Vec::new();
    let mut at = text.len();
    loop {
        let next = word_left(&text, at);
        if next == at {
            break;
        }
        stops.push(next);
        at = next;
    }
    assert_eq!(stops, [16, 13, 8, 7, 2, 0]);

    // A double-click's run: the word, the punctuation, or the space it hit.
    assert_eq!(run_at(&text, 3), 2..7, "inside `foo_1`");
    assert_eq!(run_at(&text, 7), 7..8, "the `.`");
    assert_eq!(run_at(&text, 12), 11..13, "the spaces after `bar`");
    assert_eq!(run_at(&text, 99), 16..19, "past the end is the last word");
    assert_eq!(run_at(&[], 0), 0..0);
}

/// **Multi-byte text is edited by `char`, and combining text by `char` too.**
/// A precomposed `é` is one caret stop; `e` with U+0301 is two, and one
/// Backspace takes the accent only — the boundary rule the module docs
/// choose. CJK letters are alphanumeric, so they make a word; an emoji is
/// punctuation-class and its own run. No edit cuts a scalar: a byte-indexed
/// model panics on every one of these.
#[test]
fn multi_byte_and_combining_text_is_edited_by_char() {
    let mut precomposed = line("caf\u{e9}|");
    assert!(precomposed.apply(&Edit::Backspace).text);
    assert_eq!(precomposed.text(), "caf");

    let mut combining = line("cafe\u{301}|");
    assert_eq!(combining.len(), 5, "the accent is a char of its own");
    assert!(combining.apply(&mv(Motion::Left)).caret);
    assert_eq!(
        combining.caret(),
        4,
        "the caret stopped between e and its accent"
    );
    assert!(combining.apply(&Edit::Delete).text);
    assert_eq!(
        combining.text(),
        "cafe",
        "Delete took the accent and nothing else"
    );

    let mut cjk = line("|\u{65e5}\u{672c}\u{8a9e} \u{1f600}x");
    cjk.apply(&sel(Motion::WordRight));
    assert_eq!(cjk.selected_text(), "\u{65e5}\u{672c}\u{8a9e}");
    cjk.apply(&Edit::Insert("\u{fc}".to_owned()));
    assert_eq!(cjk.text(), "\u{fc} \u{1f600}x");
    cjk.apply(&mv(Motion::WordRight));
    assert_eq!(cjk.caret(), 3, "the emoji is a run of its own");
    cjk.apply(&sel(Motion::Left));
    assert_eq!(cjk.selected_text(), "\u{1f600}");
    let applied = cjk.apply(&Edit::Cut);
    assert_eq!(
        applied.clipboard,
        Some(ClipboardOp::Offer("\u{1f600}".to_owned()))
    );
    assert_eq!(marked(&cjk), "\u{fc} |x");
}

/// **A paste replaces the selection**, and control characters in it are
/// dropped one by one — a pasted line break joins the two lines.
#[test]
fn a_paste_replaces_the_selection_and_drops_line_breaks() {
    let mut edit = line("say [hello| world");
    assert!(edit.insert("good\r\nbye"));
    assert_eq!(marked(&edit), "say goodbye| world");

    // Nothing left to insert leaves the selection standing.
    let mut edit = line("[ab|");
    assert!(!edit.insert("\n\t"));
    assert_eq!(marked(&edit), "[ab|");
}

/// **A line changed from outside keeps its caret inside it**, and the same
/// line again changes nothing.
#[test]
fn sync_takes_an_outside_edit_and_holds_the_caret_inside_it() {
    let mut edit = line("abc[def|");
    assert!(!edit.sync("abcdef"));
    assert_eq!(marked(&edit), "abc[def|");
    assert!(edit.sync("ab"));
    assert_eq!(marked(&edit), "ab|");
    assert!(edit.sync("abcd"));
    assert_eq!(marked(&edit), "ab|cd", "the caret moved with a longer line");
}

/// **The keys make the edits the module docs name**, `Shift` selects, and
/// `AltGr` reported as `Ctrl`+`Alt` is not a shortcut.
#[test]
fn keys_become_edits() {
    let none = Modifiers::empty();
    let shift = Modifiers::SHIFT;
    assert_eq!(
        Edit::for_key(KeyCode::ArrowLeft, none),
        Some(mv(Motion::Left))
    );
    assert_eq!(
        Edit::for_key(KeyCode::ArrowRight, shift),
        Some(sel(Motion::Right))
    );
    for word in [Modifiers::CTRL, Modifiers::ALT] {
        assert_eq!(
            Edit::for_key(KeyCode::ArrowLeft, word | shift),
            Some(sel(Motion::WordLeft))
        );
        assert_eq!(
            Edit::for_key(KeyCode::ArrowRight, word),
            Some(mv(Motion::WordRight))
        );
    }
    assert_eq!(
        Edit::for_key(KeyCode::ArrowLeft, Modifiers::SUPER),
        Some(mv(Motion::Home))
    );
    assert_eq!(Edit::for_key(KeyCode::End, shift), Some(sel(Motion::End)));
    assert_eq!(Edit::for_key(KeyCode::Home, none), Some(mv(Motion::Home)));
    assert_eq!(
        Edit::for_key(KeyCode::Backspace, none),
        Some(Edit::Backspace)
    );
    assert_eq!(Edit::for_key(KeyCode::Delete, none), Some(Edit::Delete));
    for shortcut in [Modifiers::CTRL, Modifiers::SUPER] {
        assert_eq!(
            Edit::for_key(KeyCode::KeyA, shortcut),
            Some(Edit::SelectAll)
        );
        assert_eq!(Edit::for_key(KeyCode::KeyC, shortcut), Some(Edit::Copy));
        assert_eq!(Edit::for_key(KeyCode::KeyX, shortcut), Some(Edit::Cut));
        assert_eq!(Edit::for_key(KeyCode::KeyV, shortcut), Some(Edit::Paste));
    }
    assert_eq!(Edit::for_key(KeyCode::KeyV, none), None, "a bare V types");
    assert_eq!(
        Edit::for_key(KeyCode::KeyC, Modifiers::CTRL | Modifiers::ALT),
        None,
        "AltGr+C types a character on many layouts"
    );
    for key in [
        KeyCode::Enter,
        KeyCode::Escape,
        KeyCode::Tab,
        KeyCode::Space,
    ] {
        assert_eq!(Edit::for_key(key, none), None, "{key} is not an edit");
    }
}
