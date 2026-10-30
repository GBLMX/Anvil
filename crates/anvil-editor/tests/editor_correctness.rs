use std::time::Instant;

use anvil_core::TextBuffer;
use anvil_editor::EditorState;

fn lines(count: usize) -> String {
    "line\n".repeat(count)
}

#[test]
fn b1_cross_chunk_delete_undo_restores_text_and_selection() {
    let text = lines(3000);
    let mut state = EditorState::from_text(&text);
    let end = 10_000;
    assert!(state.set_selection_bytes(0, end));
    let before = state.snapshot();
    state.delete(0..end);
    assert!(state.undo());
    let after = state.snapshot();
    assert_eq!(after.text().slice(0..text.len()), text);
    assert_eq!(
        after.view().selections[0].head.byte_offset,
        before.view().selections[0].head.byte_offset
    );
    assert_eq!(after.revision(), before.revision() + 2);
}

#[test]
fn b4_selection_mapping_across_chunk_boundary() {
    let mut state = EditorState::from_text(&lines(3000));
    let anchor = 1_000;
    let head = 3_000;
    assert!(state.set_selection_bytes(anchor, head));
    state.insert(500, "x\n");
    assert_eq!(state.selection_bytes(), vec![(anchor + 2, head + 2)]);
    state.delete(400..600);
    assert_eq!(state.selection_bytes(), vec![(anchor - 198, head - 198)]);
}

#[test]
fn b2_ten_thousand_edits_and_undos_restore_text() {
    let mut state = EditorState::from_text("a");
    let mut first = None;
    let mut last = None;
    for index in 0..10_000 {
        let at = if index % 2 == 0 {
            0
        } else {
            state.text().len_bytes()
        };
        state.insert(at, "x");
    }
    assert_eq!(state.undo_depth(), 10_000);
    for index in 0..10_000 {
        let start = Instant::now();
        assert!(state.undo());
        let elapsed = start.elapsed();
        first.get_or_insert(elapsed);
        last = Some(elapsed);
        if index == 0 || index == 9_999 {
            assert!(elapsed.as_micros() < 10_000);
        }
    }
    assert_eq!(state.text().slice(0..1), "a");
    let _ = (first, last);
}

#[test]
fn b3_undo_memory_is_bounded_by_retained_text() {
    let mut state = EditorState::from_text("");
    for _ in 0..10_000 {
        let at = state.text().len_bytes();
        state.insert(at, "x");
    }
    assert_eq!(state.undo_depth(), 1);
    let retained_text = 10_000;
    assert!(state.undo_bytes() <= retained_text * 8);
}
