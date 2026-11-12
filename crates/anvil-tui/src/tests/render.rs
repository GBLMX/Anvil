use super::*;
#[test]
fn markdown_fenced_code_renders_a_full_width_background() {
    let mut model = Model::new(EditorState::from_text("```rust\nfn main() {}\n```\n"), None);
    model.path = Some(PathBuf::from("x.md"));
    for _ in 0..100 {
        model.poll_syntax();
        if model
            .syntax
            .borrow()
            .highlights()
            .iter()
            .any(|highlight| highlight.class == syntax::SyntaxClass::CodeBlock)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let width = buffer.area.width as usize;
    let cells = buffer.content();
    let keyword = cells
        .iter()
        .position(|cell| cell.symbol() == "f")
        .expect("code text");
    assert_eq!(cells[keyword].fg, theme::syntax_keyword());
    assert_eq!(cells[keyword].bg, theme::code_block_bg());
    let row = keyword / width;
    assert!(
        cells[row * width + width - 2].bg == theme::code_block_bg(),
        "code background does not reach the right edge"
    );
}

#[test]
fn fenced_code_background_reaches_right_edge_when_hscrolled() {
    let mut model = Model::new(EditorState::from_text("```rust\nfn main() {}\n```\n"), None);
    model.path = Some(PathBuf::from("x.md"));
    model.hscroll = 1;
    for _ in 0..100 {
        model.poll_syntax();
        if model
            .syntax
            .borrow()
            .highlights()
            .iter()
            .any(|highlight| highlight.class == syntax::SyntaxClass::CodeBlock)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let mut lines = model.visible_lines(8, 80);
    crate::render::apply_code_block_background(&model, &mut lines, 20);
    assert_eq!(lines[1].width(), 21);
}

#[test]
fn cursorline_overlays_fenced_code_background() {
    let mut model = Model::new(EditorState::from_text("```rust\nfn main() {}\n```\n"), None);
    model.path = Some(PathBuf::from("x.md"));
    model.cursor = "```rust\n".len();
    for _ in 0..100 {
        model.poll_syntax();
        if model
            .syntax
            .borrow()
            .highlights()
            .iter()
            .any(|highlight| highlight.class == syntax::SyntaxClass::CodeBlock)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let keyword = buffer
        .content()
        .iter()
        .position(|cell| cell.symbol() == "f")
        .expect("code text");
    assert_eq!(buffer.content()[keyword].bg, theme::surface_alt());
}

#[test]
fn inline_code_gets_a_background_span() {
    let mut model = Model::new(EditorState::from_text("before `code` after"), None);
    model.path = Some(PathBuf::from("x.md"));
    for _ in 0..100 {
        model.poll_syntax();
        if !model.syntax.borrow().highlights().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let code = buffer
        .content()
        .iter()
        .position(|cell| cell.symbol() == "c")
        .expect("inline code");
    assert_eq!(buffer.content()[code].bg, theme::code_block_bg());
}

#[test]
fn search_background_overlays_fenced_code_background() {
    let mut model = Model::new(EditorState::from_text("```rust\nfn main() {}\n```\n"), None);
    model.path = Some(PathBuf::from("x.md"));
    model.cursor = 0;
    model.commit_search("fn".to_owned());
    model.cursor = 10;
    for _ in 0..100 {
        model.poll_syntax();
        if model
            .syntax
            .borrow()
            .highlights()
            .iter()
            .any(|highlight| highlight.class == syntax::SyntaxClass::CodeBlock)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let buffer = terminal.backend().buffer();
    let keyword = buffer
        .content()
        .iter()
        .position(|cell| cell.symbol() == "f")
        .expect("code text");
    assert_eq!(buffer.content()[keyword].bg, theme::selection());
}

#[test]
fn switching_theme_repaints_backgrounds() {
    let mut model = Model::new(EditorState::from_text("alpha"), None);
    model.execute_command("theme solarized-light");
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.bg == ratatui::style::Color::Rgb(253, 246, 227))
    );
    model.execute_command("theme anvil-dark");
}

#[test]
fn mouse_hit_test_boundaries_and_cjk_snap() {
    let mut model = Model::new(EditorState::from_text("你好"), None);
    model.viewport_width = 20;
    model.viewport_height = 8;
    let _ = mouse_hit_boundary(&model, 0, 0, 20, 8);
    let _ = mouse_hit_boundary(&model, 19, 7, 20, 8);
    let _ = mouse_hit_boundary(&model, 20, 8, 20, 8);

    let press = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 7,
        row: 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(press));
    assert_eq!(model.cursor, 3);
}

#[test]
fn dragging_outside_then_keyboard_resets_pointer() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.viewport_width = 20;
    model.viewport_height = 8;
    let down = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 5,
        row: 1,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let drag = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column: 100,
        row: 100,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(down));
    handle_event(&mut model, Event::Mouse(drag));
    assert!(matches!(
        model.pointer,
        crate::mouse::PointerState::Dragging(_)
    ));
    handle_event(&mut model, key(KeyCode::Char('j')));
    assert_eq!(model.pointer, crate::mouse::PointerState::Idle);
}

#[test]
fn clicking_outside_peek_restores_focus() {
    let mut model = Model::new(
        EditorState::from_text("one"),
        Some(PathBuf::from("diag.rs")),
    );
    model.viewport_width = 40;
    model.viewport_height = 12;
    model
        .diagnostics
        .insert(PathBuf::from("diag.rs"), vec![diagnostic(0, 0, "problem")]);
    model.ui.focus.split = true;
    model.jump_diagnostic(true);
    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(click));
    assert!(model.ui.diagnostic_peek.is_none());
    assert!(model.ui.focus.split);
}

#[test]
fn dragging_split_handle_updates_ratio() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.viewport_width = 40;
    model.viewport_height = 12;
    model.split_vertical();
    let down = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 20,
        row: 3,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let drag = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column: 30,
        row: 3,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(down));
    handle_event(&mut model, Event::Mouse(drag));
    assert!(model.split_ratio > 50);
}

fn mouse_hit_boundary(
    model: &Model,
    col: u16,
    row: u16,
    width: u16,
    height: u16,
) -> crate::mouse::Target {
    crate::mouse::hit_test(model, col, row, width, height)
}

#[test]
fn location_panel_rows_are_mouse_targets() {
    let mut model = Model::new(EditorState::from_text("log"), None);
    model.welcome = false;
    model.set_pipe_locations("src/a.rs:1:1: first\nsrc/b.rs:2:2: second");
    let content = ratatui::layout::Rect {
        x: 1,
        y: 1,
        width: 78,
        height: 18,
    };
    let panel = crate::pipe_locations::panel_rect(&model, content).expect("panel");
    assert_eq!(
        mouse_hit_boundary(&model, panel.x + 1, panel.y + 2, 80, 20),
        crate::mouse::Target::Location(1)
    );
}

#[test]
fn tree_visible_drag_keeps_editor_origin() {
    let mut model = Model::new(EditorState::from_text("abc\ndef"), None);
    model.viewport_width = 80;
    model.viewport_height = 12;
    model.tree_visible = true;
    let press = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 20,
        row: 2,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let drag = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column: 20,
        row: 2,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(press));
    let cursor = model.cursor;
    handle_event(&mut model, Event::Mouse(drag));
    assert_eq!(model.cursor, cursor);
}

#[test]
fn tree_visible_split_resize_uses_editor_width() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.viewport_width = 80;
    model.viewport_height = 12;
    model.tree_visible = true;
    model.split_vertical();
    let press = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 49,
        row: 3,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    let drag = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column: 54,
        row: 3,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(press));
    handle_event(&mut model, Event::Mouse(drag));
    assert_eq!(model.split_ratio, 58);
}

#[test]
fn clicking_outside_command_palette_closes_it() {
    let mut model = Model::new(EditorState::from_text("one"), None);
    model.open_command_palette();
    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(click));
    assert!(model.command_palette.is_none());
}

#[test]
fn clicking_outside_fuzzy_closes_it() {
    let mut model = Model::new(EditorState::from_text("one"), None);
    model.fuzzy = Some(FuzzyState {
        generation: 0,
        query: String::new(),
        entries: vec!["src/main.rs".to_owned()],
        filtered: vec![0],
        selected: 0,
    });
    let click = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    handle_event(&mut model, Event::Mouse(click));
    assert!(model.fuzzy.is_none());
}

#[test]
#[ignore = "manual long-line render boundary"]
fn long_line_render_stays_bounded() {
    let megabytes = std::env::var("ANVIL_LONG_LINE_MB")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    let text = "x".repeat(megabytes * 1024 * 1024);
    let model = Model::new(EditorState::from_text(&text), None);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
    let start = std::time::Instant::now();
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let elapsed = start.elapsed();
    println!("long-line-{megabytes}MB-render-ms={}", elapsed.as_millis());
    assert!(
        elapsed < std::time::Duration::from_millis(10),
        "long-line render took {elapsed:?}"
    );
}

#[test]
fn hit_test_extreme_dimensions_do_not_overflow() {
    let mut model = Model::new(EditorState::from_text("abc"), None);
    model.tree_visible = true;
    model.split_vertical();
    for (width, height) in [(0, 0), (1, 1), (2, 2), (u16::MAX, u16::MAX)] {
        let _ = mouse_hit_boundary(&model, 0, 0, width, height);
        let _ = mouse_hit_boundary(
            &model,
            width.saturating_sub(1),
            height.saturating_sub(1),
            width,
            height,
        );
    }
}

#[test]
fn dense_substitution_is_rejected() {
    let text = "a".repeat(1024 * 1024);
    let mut model = Model::new(EditorState::from_text(&text), None);
    model.execute_command("%s/a/b/g");
    assert!(model.status.contains("too many matches"));
    assert_eq!(model.editor.text().len_bytes(), text.len());
}

#[test]
fn line_substitution_ignores_dense_prefix() {
    let text = "a\n".repeat(10_050);
    let mut model = Model::new(EditorState::from_text(&text), None);
    model.cursor = text.len() - 2;
    model.execute_command("s/a/b/");
    assert!(model.status.contains("1 replacements"));
    assert_eq!(model.editor.text().slice(text.len() - 2..text.len()), "b\n");
}

#[test]
#[ignore = "manual 100MB cursor-at-end boundary"]
fn long_line_cursor_at_end_render_stays_bounded() {
    let megabytes = std::env::var("ANVIL_LONG_LINE_MB")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100);
    let text = "x".repeat(megabytes * 1024 * 1024);
    let mut model = Model::new(EditorState::from_text(&text), None);
    model.cursor = text.len();
    model.hscroll = text.len().saturating_sub(100);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
    let start = std::time::Instant::now();
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let elapsed = start.elapsed();
    println!(
        "long-line-{megabytes}MB-cursor-end-render-ms={}",
        elapsed.as_millis()
    );
    assert!(
        elapsed < std::time::Duration::from_millis(10),
        "cursor-at-end render took {elapsed:?}"
    );
}

#[test]
fn diagnostics_follow_horizontal_window() {
    let mut model = Model::new(
        EditorState::from_text("abcdefghij"),
        Some(PathBuf::from("diag.rs")),
    );
    model.hscroll = 5;
    model.diagnostics.insert(
        PathBuf::from("diag.rs"),
        vec![lsp::Diagnostic {
            range: lsp::Range {
                start: lsp::Position {
                    line: 0,
                    character: 6,
                },
                end: lsp::Position {
                    line: 0,
                    character: 8,
                },
            },
            severity: Some(1),
            message: "error".to_owned(),
            source: Some("test".to_owned()),
            extra: serde_json::Map::new(),
        }],
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let cells = terminal.backend().buffer().content();
    let underlined = |symbol: &str| {
        cells.iter().any(|cell| {
            cell.symbol() == symbol && cell.modifier.contains(ratatui::style::Modifier::UNDERLINED)
        })
    };
    assert!(!underlined("f"));
    assert!(underlined("g"));
}

#[test]
fn inlay_hints_follow_horizontal_window() {
    let mut model = Model::new(EditorState::from_text("abcdefghij"), None);
    model.hscroll = 5;
    model.inlay_hints = vec![
        InlayHint {
            line: 0,
            character: 7,
            label: ":visible".to_owned(),
        },
        InlayHint {
            line: 0,
            character: 1,
            label: ":hidden".to_owned(),
        },
    ];
    let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("terminal");
    terminal.draw(|frame| view(&model, frame)).expect("draw");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains(":visible"));
    assert!(!rendered.contains(":hidden"));
}

#[test]
fn stale_syntax_boundaries_are_clamped_to_char_boundaries() {
    let mut spans = Vec::new();
    crate::render::push_syntax_spans(
        &mut spans,
        "👩",
        0,
        &[crate::syntax::Highlight {
            start: 2,
            end: 4,
            class: crate::syntax::SyntaxClass::Keyword,
        }],
    );
    let rendered = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(rendered, "👩");
}
