use anvil_core::{EditCommand, History, TextBuffer};
use anvil_text_bench::{GapBuffer, RopeBuffer};

fn exercise<B: TextBuffer>() {
    let mut buffer = B::from_text("ab");
    let mut history = History::default();

    let revision = history
        .apply(
            &mut buffer,
            0,
            EditCommand::Insert {
                at: 1,
                text: "X".to_owned(),
            },
        )
        .unwrap();
    assert_eq!(revision, 1);
    assert_eq!(buffer.slice(0..buffer.len_bytes()), "aXb");

    assert_eq!(history.undo(&mut buffer, 1).unwrap(), 2);
    assert_eq!(buffer.slice(0..buffer.len_bytes()), "ab");

    assert_eq!(history.redo(&mut buffer, 2).unwrap(), 3);
    assert_eq!(buffer.slice(0..buffer.len_bytes()), "aXb");

    assert_eq!(
        history
            .apply(&mut buffer, 3, EditCommand::Delete { range: 1..2 })
            .unwrap(),
        4
    );
    assert_eq!(buffer.slice(0..buffer.len_bytes()), "ab");

    assert_eq!(history.undo(&mut buffer, 4).unwrap(), 5);
    assert_eq!(buffer.slice(0..buffer.len_bytes()), "aXb");
}

fn typing_group<B: TextBuffer>() {
    let mut buffer = B::from_text("");
    let mut history = History::default();
    history.apply_typing(&mut buffer, 0, 0, "a").unwrap();
    history.apply_typing(&mut buffer, 1, 1, "b").unwrap();
    history.apply_typing(&mut buffer, 2, 2, "c").unwrap();
    assert_eq!(buffer.slice(0..buffer.len_bytes()), "abc");

    assert_eq!(history.undo(&mut buffer, 3).unwrap(), 4);
    assert_eq!(buffer.slice(0..buffer.len_bytes()), "");

    assert_eq!(history.redo(&mut buffer, 4).unwrap(), 5);
    assert_eq!(buffer.slice(0..buffer.len_bytes()), "abc");
}

#[test]
fn history_works_for_comparison_backends() {
    exercise::<RopeBuffer>();
    exercise::<GapBuffer>();
}

#[test]
fn contiguous_typing_is_one_undo_group_for_comparison_backends() {
    typing_group::<RopeBuffer>();
    typing_group::<GapBuffer>();
}
