use super::*;
use crate::input::KeyPress;
use crate::keymap::{KeyChord, KeymapConfig, NormalCommand, PendingCommand};
use anvil_lsp_client::{Diagnostic, Position, Range};
use anvil_session::ui::CreateKind;
use anvil_workspace::PreviewData;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

mod buffers;
mod editing;
mod gateway_multi_root;
mod interaction;
mod lsp_tests;
mod render;
mod ui;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn shift(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::SHIFT))
}

fn ctrl(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn alt(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

fn repeat(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ))
}

fn release(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ))
}

fn type_text(model: &mut Model, text: &str) {
    for character in text.chars() {
        handle_event(model, key(KeyCode::Char(character)));
    }
}

fn command(model: &mut Model, text: &str) {
    handle_event(model, key(KeyCode::Char(':')));
    type_text(model, text);
    handle_event(model, key(KeyCode::Enter));
}

fn diagnostic(line: u32, character: u32, message: &str) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: Position { line, character },
            end: Position {
                line,
                character: character + 1,
            },
        },
        severity: Some(1),
        message: message.to_owned(),
        source: Some("test".to_owned()),
        extra: serde_json::Map::new(),
    }
}

#[test]
fn renders_two_split_panes() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
    model.split_vertical();
    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let rendered = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.matches("alpha").count() >= 2, "{rendered}");
}

#[test]
fn split_pane_wraps_long_lines_when_soft_wrap_enabled() {
    let line = format!("{}TAIL{}", "a".repeat(60), "b".repeat(20));
    let mut model = Model::new(EditorState::from_text(&line), None);
    model.config.settings.editor.soft_wrap = true;
    model.view.viewport_width = 80;
    model.split_vertical();
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        rendered.matches("TAIL").count() >= 2,
        "split pane did not wrap the long line: {rendered}"
    );
}

#[test]
fn split_panes_hold_different_buffers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let left = dir.path().join("left.txt");
    let right = dir.path().join("right.txt");
    std::fs::write(&left, "left text").expect("write left");
    std::fs::write(&right, "right text").expect("write right");
    let mut model = Model::new(EditorState::from_text("scratch"), None);
    model.open_path(&left).expect("open left");
    model.split_vertical();
    model.focus_split(true);
    model.open_path(&right).expect("open right");
    assert_eq!(
        model
            .session
            .editor
            .text()
            .slice(0..model.session.editor.text().len_bytes()),
        "right text"
    );
    model.focus_split(false);
    assert_eq!(
        model
            .session
            .editor
            .text()
            .slice(0..model.session.editor.text().len_bytes()),
        "left text"
    );
    model.focus_split(true);
    assert_eq!(
        model
            .session
            .editor
            .text()
            .slice(0..model.session.editor.text().len_bytes()),
        "right text"
    );
}

#[test]
fn split_focus_restores_buffer_state_and_active_tab() {
    let dir = tempfile::tempdir().expect("tempdir");
    let left = dir.path().join("left.txt");
    let right = dir.path().join("right.txt");
    fs::write(&left, "left").expect("left");
    fs::write(&right, "right").expect("right");
    let mut model = Model::open(&left).expect("open left");
    model.split_vertical();
    model.focus_split(true);
    model.open_path(&right).expect("open right");
    model.mark_missing(&right);
    handle_event(&mut model, key(KeyCode::Char('v')));
    handle_event(&mut model, key(KeyCode::Right));
    assert_eq!(model.buffers.active, 1);

    model.focus_split(false);
    assert_eq!(model.buffers.active, 0);
    assert!(model.edit.visual_anchor.is_none());
    assert!(!model.file.missing);

    model.focus_split(true);
    assert_eq!(model.buffers.active, 1);
    assert_eq!(model.edit.visual_anchor, Some(0));
    assert!(model.file.missing);
}

#[test]
fn split_focus_resets_stale_search_matches() {
    let mut model = Model::new(EditorState::from_text("foo"), None);
    model.commit_search("foo".to_owned());
    assert_eq!(model.search.state.matches.len(), 1);
    model.split_vertical();
    model.focus_split(true);
    assert!(model.search.state.matches.is_empty());
}
#[test]
fn closing_focused_split_restores_left_pane() {
    let mut model = Model::new(EditorState::from_text("left\nright"), None);
    model.split_vertical();
    model.focus_split(true);
    model.session.cursor = 5;
    model.close_split();
    assert!(model.buffers.split.is_none());
    assert!(!model.ui.focus.split);
    assert_eq!(model.session.cursor, 0);
    assert_eq!(model.session.editor.text().slice(0..4), "left");
}

#[test]
fn ctrl_w_v_opens_split_and_ctrl_w_h_l_switches_focus() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('v')));
    assert!(model.buffers.split.is_some());
    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('l')));
    assert!(model.ui.focus.split);
    handle_event(&mut model, ctrl(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Char('h')));
    assert!(!model.ui.focus.split);
}

#[test]
fn split_panes_keep_independent_cursors() {
    let mut model = Model::new(EditorState::from_text("one\ntwo\nthree"), None);
    model.session.cursor = 1;
    model.split_vertical();
    assert!(model.buffers.split.is_some());
    assert!(!model.ui.focus.split);
    model.focus_split(true);
    assert!(model.ui.focus.split);
    assert_eq!(model.session.cursor, 1);
    model.session.cursor = 5;
    model.view.scroll = 2;
    model.view.hscroll = 3;
    model.focus_split(false);
    assert!(!model.ui.focus.split);
    assert_eq!(model.session.cursor, 1);
    assert_eq!(model.view.scroll, 0);
    assert_eq!(model.view.hscroll, 0);
    model.focus_split(true);
    assert_eq!(model.session.cursor, 5);
    assert_eq!(model.view.scroll, 2);
    assert_eq!(model.view.hscroll, 3);
}

#[test]
fn select_word_occurrences_replaces_all_as_one_undo() {
    let original = "foo\n".repeat(100);
    let mut model = Model::new(
        EditorState::from_text(&original),
        Some(PathBuf::from("multi.txt")),
    );
    model.select_word_occurrences();
    assert_eq!(model.session.editor.selection_bytes().len(), 100);
    model.insert_text("bar");
    assert_eq!(
        model.session.editor.text().slice(0..400),
        "bar\n".repeat(100)
    );
    assert_eq!(model.session.editor.undo_depth(), 1);
    model.undo();
    assert_eq!(model.session.editor.text().slice(0..400), original);
    assert_eq!(model.session.editor.selection_bytes().len(), 100);
}

#[test]
fn split_selection_lines_creates_one_caret_per_line() {
    let mut model = Model::new(EditorState::from_text("one\ntwo\nthree"), None);
    assert!(model.session.editor.set_selection_bytes(0, 7));
    model.split_selection_lines();
    assert_eq!(model.session.editor.selection_bytes().len(), 2);
}

#[test]
fn add_cursor_after_motion_uses_the_moved_caret() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
    handle_event(&mut model, key(KeyCode::Char('j')));
    handle_event(&mut model, key(KeyCode::Char('C')));
    assert_eq!(
        model.session.editor.selection_bytes(),
        vec![(6, 6), (11, 11)]
    );

    handle_event(&mut model, key(KeyCode::Char('i')));
    type_text(&mut model, "X");
    let len = model.session.editor.text().len_bytes();
    assert_eq!(
        model.session.editor.text().slice(0..len),
        "alpha\nXbeta\nXgamma"
    );
}

#[test]
fn escape_merges_cursors_only_in_normal_mode() {
    let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
    handle_event(&mut model, key(KeyCode::Char('j')));
    handle_event(&mut model, key(KeyCode::Char('C')));
    assert_eq!(model.session.editor.selection_bytes().len(), 2);

    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.session.editor.selection_bytes(), vec![(6, 6)]);

    handle_event(&mut model, key(KeyCode::Char('C')));
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.session.editor.selection_bytes().len(), 2);
    assert!(matches!(model.ui.input_mode, InputMode::Normal));

    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.session.editor.selection_bytes(), vec![(6, 6)]);
    assert_eq!(model.session.cursor, 6);
    let len = model.session.editor.text().len_bytes();
    assert_eq!(
        model.session.editor.text().slice(0..len),
        "alpha\nbeta\ngamma"
    );
}

#[test]
fn alt_c_adds_a_cursor_above_for_both_keycodes() {
    for code in [KeyCode::Char('c'), KeyCode::Char('C')] {
        let mut model = Model::new(EditorState::from_text("alpha\nbeta\ngamma"), None);
        handle_event(&mut model, key(KeyCode::Char('j')));
        handle_event(&mut model, alt(code));
        assert_eq!(model.session.editor.selection_bytes(), vec![(0, 0), (6, 6)]);
    }
}

#[test]
fn alt_s_is_case_insensitive() {
    for code in [KeyCode::Char('s'), KeyCode::Char('S')] {
        let mut model = Model::new(EditorState::from_text("one\ntwo\nthree"), None);
        assert!(model.session.editor.set_selection_bytes(0, 7));
        handle_event(&mut model, alt(code));
        assert_eq!(model.session.editor.selection_bytes().len(), 2);
    }
}

#[test]
fn pending_commands_ignore_alt_modifiers() {
    let mut model = Model::new(EditorState::from_text("one two"), None);
    handle_event(&mut model, key(KeyCode::Char('d')));
    handle_event(&mut model, alt(KeyCode::Char('w')));
    assert_eq!(model.session.editor.text().slice(0..7), "one two");
    assert!(model.input.pending.is_none());
}

#[test]
fn insert_ignores_alt_modified_characters() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, alt(KeyCode::Char('j')));
    assert_eq!(model.session.editor.text().slice(0..3), "abc");
    assert!(matches!(model.ui.input_mode, InputMode::Insert));
}

#[test]
fn normal_mode_ignores_alt_commands() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    handle_event(&mut model, alt(KeyCode::Char('i')));
    handle_event(&mut model, alt(KeyCode::Char('d')));
    assert!(matches!(model.ui.input_mode, InputMode::Normal));
    assert!(model.input.pending.is_none());
}

#[test]
fn repeated_key_events_still_dispatch() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    handle_event(&mut model, repeat(KeyCode::Char('l')));
    assert_eq!(model.session.cursor, 1);
}

#[test]
fn released_key_events_are_ignored() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    handle_event(&mut model, release(KeyCode::Char('l')));
    assert_eq!(model.session.cursor, 0);
}

#[test]
fn normal_tab_does_not_insert_text() {
    let mut model = Model::new(EditorState::from_text("a"), None);
    handle_event(&mut model, key(KeyCode::Tab));
    handle_event(&mut model, key(KeyCode::BackTab));
    assert_eq!(model.session.editor.text().slice(0..1), "a");
    assert!(!model.file.modified);
}

#[test]
fn insert_tab_adds_four_spaces() {
    let mut model = Model::new(EditorState::from_text("a"), None);
    handle_event(&mut model, key(KeyCode::Char('a')));
    handle_event(&mut model, key(KeyCode::Tab));
    assert_eq!(model.session.editor.text().slice(0..5), "a    ");
}

#[test]
fn rendered_tabs_use_display_columns() {
    let model = Model::new(
        EditorState::from_text("a\tb"),
        Some(PathBuf::from("tabs.rs")),
    );
    let backend = TestBackend::new(20, 5);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(content.contains("a   b"), "{content:?}");
    assert!(!content.contains('\t'));
}

#[test]
fn insert_mode_modifies_piece_table() {
    let mut model = Model::new(EditorState::from_text("ac"), None);
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Char('b')));
    handle_event(&mut model, key(KeyCode::Esc));
    assert_eq!(model.session.editor.text().slice(0..3), "bac");
    assert!(model.file.modified);
    assert_eq!(model.ui.input_mode, InputMode::Normal);
}

#[test]
fn command_line_saves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("file.txt");
    fs::write(&path, "old").expect("write");
    let mut model = Model::open(&path).expect("open");
    handle_event(&mut model, key(KeyCode::Char('i')));
    handle_event(&mut model, key(KeyCode::Char('n')));
    handle_event(&mut model, key(KeyCode::Esc));
    handle_event(&mut model, key(KeyCode::Char(':')));
    handle_event(&mut model, key(KeyCode::Char('w')));
    handle_event(&mut model, key(KeyCode::Enter));
    assert!(!model.file.modified);
    assert_eq!(fs::read_to_string(path).expect("read"), "nold");
}

#[test]
fn cursor_and_viewport_are_independent() {
    let mut model = Model::new(EditorState::from_text("a\nb\nc\nd"), None);
    model.move_line(3);
    model.ensure_cursor_visible(2, 80);
    assert_eq!(model.view.scroll, 2);
    model.move_line(-1);
    model.ensure_cursor_visible(2, 80);
    assert_eq!(model.view.scroll, 2);
}

#[test]
fn horizontal_scroll_follows_the_cursor() {
    let mut model = Model::new(EditorState::from_text("0123456789"), None);
    model.session.cursor = 8;
    model.ensure_cursor_visible(1, 4);
    assert_eq!(model.view.hscroll, 5);
    model.session.cursor = 0;
    model.ensure_cursor_visible(1, 4);
    assert_eq!(model.view.hscroll, 0);
}

#[test]
fn horizontal_scroll_uses_display_cells() {
    let mut model = Model::new(EditorState::from_text("中中中中中"), None);
    model.session.cursor = model.session.editor.text().len_bytes();
    model.ensure_cursor_visible(1, 3);
    assert_eq!(model.view.hscroll, 8);
}

#[test]
fn syntax_keyword_is_rendered() {
    let mut model = Model::new(
        EditorState::from_text("fn main() {}"),
        Some(PathBuf::from("sample.rs")),
    );
    for _ in 0..200 {
        model.poll_syntax();
        if !model
            .syntax_cache
            .highlights
            .borrow()
            .highlights()
            .is_empty()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        !model
            .syntax_cache
            .highlights
            .borrow()
            .highlights()
            .is_empty(),
        "no syntax highlights produced"
    );
    let backend = TestBackend::new(80, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let colored = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .any(|cell| cell.fg == theme::syntax_keyword());
    assert!(colored, "keyword span was not colored");
    model.file.dirty = false;
}

#[test]
fn search_preserves_syntax_and_adds_background() {
    let mut model = Model::new(
        EditorState::from_text("fn main() {} fn"),
        Some(PathBuf::from("sample.rs")),
    );
    for _ in 0..200 {
        model.poll_syntax();
        if !model
            .syntax_cache
            .highlights
            .borrow()
            .highlights()
            .is_empty()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        !model
            .syntax_cache
            .highlights
            .borrow()
            .highlights()
            .is_empty(),
        "no syntax highlights produced"
    );
    handle_event(&mut model, key(KeyCode::Char('/')));
    type_text(&mut model, "fn");
    handle_event(&mut model, key(KeyCode::Enter));
    let lines = model.visible_lines(8, 80);
    let keyword = lines[0]
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "fn")
        .expect("keyword span");
    assert_eq!(keyword.style.fg, Some(theme::syntax_keyword()));
    assert_eq!(keyword.style.bg, Some(theme::warning()));
    let backgrounds = lines[0]
        .spans
        .iter()
        .filter(|span| span.content.as_ref() == "fn")
        .filter_map(|span| span.style.bg)
        .collect::<Vec<_>>();
    assert!(backgrounds.contains(&theme::warning()));
    assert!(backgrounds.contains(&theme::selection()));
    let backend = TestBackend::new(80, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal.backend().buffer().content().iter().any(|cell| {
        cell.symbol() == "f" && cell.fg == theme::syntax_keyword() && cell.bg == theme::warning()
    });
    assert!(rendered, "search overlay replaced the syntax foreground");
}

#[test]
fn snippets_drop_placeholders() {
    assert_eq!(
        lsp_ui::expand_snippet("fn ${1:name}(${2:args}) $0"),
        "fn name(args) "
    );
}

#[test]
fn utf16_offset_maps_through_emoji() {
    let model = Model::new(EditorState::from_text("a😀b"), None);
    assert_eq!(model.offset_at_utf16(0, 1), 1);
    assert_eq!(model.offset_at_utf16(0, 3), 5);
}

#[test]
fn utf16_columns_map_through_emoji() {
    let text = "a😀b";
    assert_eq!(utf16_to_byte(text, 1), 1);
    assert_eq!(utf16_to_byte(text, 3), 5);
    assert_eq!(utf16_to_byte(text, 4), 6);
}

#[test]
fn navigation_preserves_modified_buffer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "first").expect("first");
    fs::write(&second, "second").expect("second");
    let mut model = Model::open(&first).expect("open");
    model.insert_text("X");
    assert!(model.file.modified);
    model.open_path(&second).expect("second open");
    assert_eq!(
        model.session.path.as_deref(),
        Some(second.canonicalize().expect("canonical").as_path())
    );
    assert_eq!(model.session.editor.text().slice(0..6), "second");
    assert_eq!(model.buffers.entries.len(), 2);
    assert!(model.buffers.entries[0].modified);
    assert_eq!(model.buffers.entries[0].editor.text().slice(0..6), "Xfirst");
    model.switch_buffer(-1);
    assert_eq!(model.session.editor.text().slice(0..6), "Xfirst");
    assert!(model.file.modified);
}
