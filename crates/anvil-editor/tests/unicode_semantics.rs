use anvil_core::TextBuffer;
use anvil_editor::{ApplyEditError, EditorState};
use proptest::prelude::*;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone)]
enum Action {
    Insert { at: usize, text: String },
    Delete { start: usize, end: usize },
    Transaction { at: usize, text: String },
    Undo,
    Redo,
}

fn fragment() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "a",
            "Z",
            " ",
            "\n",
            "\t",
            "é",
            "e\u{301}",
            "中",
            "👨‍👩‍👧‍👦",
            "🦀",
            "\r\n",
        ]),
        0..5,
    )
    .prop_map(|parts| parts.concat())
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(fragment(), 0..20).prop_map(|parts| parts.concat())
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        (0usize..128, fragment()).prop_map(|(at, text)| Action::Insert { at, text }),
        (0usize..128, 0usize..128).prop_map(|(start, end)| Action::Delete {
            start: start.min(end),
            end: start.max(end),
        }),
        (0usize..128, fragment()).prop_map(|(at, text)| Action::Transaction { at, text }),
        Just(Action::Undo),
        Just(Action::Redo),
    ]
}

fn boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn assert_boundaries(state: &EditorState) {
    let buffer = state.text();
    let text = buffer.slice(0..buffer.len_bytes());
    for (anchor, head) in state.selection_bytes() {
        assert!(text.is_char_boundary(anchor), "anchor {anchor} in {text:?}");
        assert!(text.is_char_boundary(head), "head {head} in {text:?}");
    }
    let index = state.index();
    for line in 0..index.len_lines() {
        let start = index.line_to_offset(line).expect("line start");
        let line_range = index.offset_to_line(start).expect("line range");
        let slice = &text[start..line_range.end];
        for (grapheme, (byte, _)) in slice.grapheme_indices(true).enumerate() {
            let offset = start + byte;
            let position = index.position(buffer, offset).expect("position");
            assert_eq!(position.line, line);
            assert_eq!(position.byte_offset, offset);
            assert_eq!(position.char_column, slice[..byte].chars().count());
            assert_eq!(
                index.offset_at_char(buffer, line, position.char_column),
                Some(offset)
            );
            assert_eq!(
                index.offset_at_grapheme(buffer, line, grapheme),
                Some(offset)
            );
            assert_eq!(
                index.utf16_column(buffer, offset),
                Some(slice[..byte].chars().map(char::len_utf16).sum())
            );
            for (display, candidate) in [
                index.offset_at_display_column(buffer, line, position.display_column),
                index.offset_at_display_column_rounded(buffer, line, position.display_column),
            ]
            .into_iter()
            .enumerate()
            {
                let candidate = candidate.unwrap_or_else(|| {
                    panic!("display conversion {display} failed at {line}:{offset}")
                });
                assert!(
                    text.is_char_boundary(candidate),
                    "display conversion split UTF-8 at {candidate}"
                );
            }
        }
        for (byte, character) in slice.char_indices() {
            let offset = start + byte;
            let utf16 = slice[..byte].chars().map(char::len_utf16).sum();
            assert_eq!(
                index.offset_at_utf16(buffer, line, utf16),
                Some(offset),
                "UTF-16 round trip for {character:?}"
            );
        }
        let end_utf16 = slice.chars().map(char::len_utf16).sum();
        assert_eq!(
            index.offset_at_utf16(buffer, line, end_utf16),
            Some(line_range.end)
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn unicode_action_sequences_preserve_coordinates(
        text in text(),
        actions in prop::collection::vec(action(), 0..40),
    ) {
        let mut state = EditorState::from_text(&text);
        for action in actions {
            let current = state
                .text()
                .slice(0..state.text().len_bytes());
            match action {
                Action::Insert { at, text: insert } => {
                    let at = boundary(&current, at);
                    prop_assert!(state.insert(at, &insert));
                }
                Action::Delete { start, end } => {
                    let start = boundary(&current, start);
                    let end = boundary(&current, end).max(start);
                    prop_assert!(state.delete(start..end));
                }
                Action::Transaction { at, text: insert } => {
                    let at = boundary(&current, at);
                    let revision = state.revision();
                    let empty = insert.is_empty();
                    let result =
                        state.apply_transaction_at(revision, vec![(at..at, insert)]);
                    if !empty {
                        prop_assert!(result.is_ok());
                    }
                }
                Action::Undo => {
                    state.undo();
                }
                Action::Redo => {
                    state.redo();
                }
            }
            assert_boundaries(&state);
        }
    }
}

#[test]
fn invalid_edits_return_errors_without_panicking() {
    let mut state = EditorState::from_text("aé🦀b");
    let original = state.text().slice(0..state.text().len_bytes());
    let revision = state.revision();

    assert!(!state.insert(2, "x"));
    assert!(!state.delete(1..2));
    assert!(!state.set_selection_bytes(2, 2));
    assert!(matches!(
        state.apply_transaction_at(revision, vec![(2..2, "x".to_owned())]),
        Err(ApplyEditError::InvalidEdit)
    ));
    assert!(matches!(
        state.apply_transaction_at(
            revision,
            vec![(0..3, "x".to_owned()), (1..3, "y".to_owned())]
        ),
        Err(ApplyEditError::InvalidEdit)
    ));
    assert_eq!(state.text().slice(0..state.text().len_bytes()), original);
    assert_eq!(state.revision(), revision);

    assert!(state.insert(1, "x"));
    assert!(state.delete(1..2));
    assert_eq!(state.text().slice(0..state.text().len_bytes()), "aé🦀b");
}
