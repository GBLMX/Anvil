//! Keyboard dispatch for normal, insert, command, and overlay modes.

use crate::input::{KeyPress, is_action_key};
use crate::keymap::{
    ContextAction, NormalCommand, PendingCommand, macro_key, macro_step, resolve_normal,
    resolve_pending,
};
use anvil_session::command::{CommandContext, is_registered};
use anvil_session::ui::CreateKind;

use super::*;

use crossterm::event::{Event, KeyCode};

pub(crate) fn handle_event(model: &mut Model, event: Event) {
    let edit_before = match &event {
        Event::Key(key) if is_action_key(key) => {
            Some((model.path.clone(), model.cursor, model.editor.revision()))
        }
        Event::Paste(_) => Some((model.path.clone(), model.cursor, model.editor.revision())),
        _ => None,
    };
    match event {
        Event::Key(key) if is_action_key(&key) => {
            if matches!(model.view.pointer, mouse::PointerState::Dragging(_)) {
                model.view.pointer = mouse::PointerState::Idle;
            }
            model.syntax.borrow_mut().note_activity();
            handle_key(model, KeyPress::from_key(key));
        }
        Event::Mouse(mouse) => {
            mouse::handle_mouse(
                model,
                mouse,
                model.view.viewport_width,
                model.view.viewport_height,
            );
        }
        Event::Paste(text) => {
            #[cfg(unix)]
            if model.runtime.terminal_panel {
                model.send_terminal_text(&text);
                return;
            }
            model.syntax.borrow_mut().note_activity();
            model.input.pending = None;
            let (text, truncated) = normalize_paste(&text);
            if truncated {
                model.status = "E: paste truncated".to_owned();
                model.dirty = true;
            }
            if let InputMode::Insert = model.ui.input_mode {
                model.insert_text(&text);
            } else if let InputMode::CommandLine { buffer } = &mut model.ui.input_mode {
                buffer.insert_str(&text.replace('\n', " "));
                model.dirty = true;
            } else if let InputMode::Search { query } = &mut model.ui.input_mode {
                query.insert_str(&text.replace('\n', " "));
                model.dirty = true;
            } else if let InputMode::Rename { name } = &mut model.ui.input_mode {
                name.push_str(&text.replace('\n', " "));
                model.dirty = true;
            }
        }
        Event::FocusLost => {
            model.auto_save_before_focus_change();
            model.dirty = true;
        }
        Event::Resize(_, _) => model.dirty = true,
        _ => {}
    }
    model.drain_effects();
    if let Some((path, cursor, revision)) = edit_before
        && model.editor.revision() != revision
        && model.path == path
    {
        model.record_change(path, cursor, model.path.clone(), model.cursor);
    }
}

const MAX_PASTE_BYTES: usize = 8 * 1024 * 1024;

fn normalize_paste(text: &str) -> (String, bool) {
    let mut output = String::new();
    let mut chars = text.chars().peekable();
    let mut truncated = false;
    while let Some(character) = chars.next() {
        let character = if character == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            '\n'
        } else {
            character
        };
        if character.is_control() && !matches!(character, '\n' | '\t') {
            continue;
        }
        if output.len() + character.len_utf8() > MAX_PASTE_BYTES {
            truncated = true;
            break;
        }
        output.push(character);
    }
    (output, truncated)
}

fn execute_pending_command(
    model: &mut Model,
    command: PendingCommand,
    pending: char,
    key: KeyPress,
) {
    debug_assert!(
        command
            .id()
            .is_none_or(|id| is_registered(id, CommandContext::Pending))
    );
    match command {
        PendingCommand::ObjectPending => {
            let operator = pending.to_ascii_lowercase();
            model.begin_pending(if key.code == KeyCode::Char('a') {
                operator.to_ascii_uppercase()
            } else {
                operator
            });
            model.input.object_pending = true;
        }
        PendingCommand::OperatorWord { operator, around } => {
            let count = model.take_count();
            model.apply_word_operator(operator, around, count);
        }
        PendingCommand::OperatorQuote {
            operator,
            quote,
            around,
        } => {
            let _ = model.take_count();
            model.apply_quote_operator(operator, quote, around);
        }
        PendingCommand::ChangeInnerWord => model.change_inner_word(),
        PendingCommand::OperatorBracket {
            operator,
            open,
            around,
        } => {
            let _ = model.take_count();
            model.apply_bracket_operator(operator, open, around);
        }
        PendingCommand::OperatorParagraph { operator, around } => {
            let count = model.take_count();
            model.apply_paragraph_operator(operator, around, count);
        }
        PendingCommand::OperatorTag { operator, around } => {
            let _ = model.take_count();
            model.apply_tag_operator(operator, around);
        }
        PendingCommand::ChangeInnerQuote(quote) => model.change_inner_quote(quote),
        PendingCommand::Definition => model.queue_effect(Effect::LspDefinition),
        PendingCommand::References => model.queue_effect(Effect::LspReferences),
        PendingCommand::DocumentHighlight => {
            model.queue_effect(Effect::LspDocumentHighlights);
        }
        PendingCommand::IncomingCalls => model.queue_effect(Effect::LspIncomingCalls),
        PendingCommand::OutgoingCalls => model.queue_effect(Effect::LspOutgoingCalls),
        PendingCommand::ChangeBack => model.change_back(),
        PendingCommand::ChangeForward => model.change_forward(),
        PendingCommand::ToggleFold => model.toggle_fold_at_cursor(),
        PendingCommand::CloseAllFolds => model.close_all_folds(),
        PendingCommand::OpenAllFolds => model.open_all_folds(),
        PendingCommand::Diagnostic(forward) => model.jump_diagnostic(forward),
        PendingCommand::Count { pending, digit } => {
            model.push_count(digit);
            model.begin_pending(pending);
        }
        PendingCommand::SplitVertical => {
            let _ = model.take_count();
            model.split_vertical();
        }
        PendingCommand::FocusSplit(right) => {
            let _ = model.take_count();
            model.focus_split(right);
        }
        PendingCommand::FocusEditor => {
            let _ = model.take_count();
            model.focus_editor();
        }
        PendingCommand::CloseSplit => {
            let _ = model.take_count();
            model.close_split();
        }
        PendingCommand::OpenTree => {
            let _ = model.take_count();
            model.open_tree();
        }
        PendingCommand::StartFuzzy => {
            let _ = model.take_count();
            model.start_fuzzy();
        }
        PendingCommand::CommandPalette => {
            let _ = model.take_count();
            model.open_command_palette();
        }
        PendingCommand::StartRename => {
            let _ = model.take_count();
            model.start_rename();
        }
        PendingCommand::CodeActions => {
            let _ = model.take_count();
            model.queue_effect(Effect::LspCodeActions);
        }
        PendingCommand::DocumentSymbols => {
            let _ = model.take_count();
            model.queue_effect(Effect::LspDocumentSymbols);
        }
        PendingCommand::WorkspaceSymbols => {
            let _ = model.take_count();
            model.queue_effect(Effect::LspWorkspaceSymbols);
        }
        PendingCommand::Formatting => {
            let _ = model.take_count();
            model.queue_effect(Effect::LspFormat);
        }
        PendingCommand::ChangeWord(motion) => {
            let _ = model.take_count();
            model.change_word(motion);
        }
        PendingCommand::DeleteLines => {
            let count = model.take_count();
            model.delete_lines(count);
        }
        PendingCommand::YankLines => {
            let count = model.take_count();
            model.yank_lines(count);
        }
        PendingCommand::DeleteWords(motion) => {
            let count = model.take_count();
            model.delete_words(motion, count);
        }
        PendingCommand::YankWords(motion) => {
            let count = model.take_count();
            model.yank_words(motion, count);
        }
    }
}

fn execute_normal_command(model: &mut Model, command: NormalCommand) {
    match command {
        NormalCommand::CollapseSelections => model.collapse_selections(),
        NormalCommand::Pending(pending) => model.begin_pending(pending),
        NormalCommand::Hover => model.queue_effect(Effect::LspHover),
        NormalCommand::MacroRecord => {
            model.input.macro_wait = Some('q');
            model.dirty = true;
        }
        NormalCommand::MacroReplay => {
            model.input.macro_wait = Some('@');
            model.dirty = true;
        }
        NormalCommand::SplitSelection => model.split_selection_lines(),
        NormalCommand::SelectWord => model.select_word_occurrences(),
        NormalCommand::ExtendLine(up) => model.extend_line_cursor(up),
        NormalCommand::InsertHere => model.insert_here(),
        NormalCommand::CommandLine => {
            model.ui.input_mode = InputMode::CommandLine {
                buffer: InputBuffer::default(),
            };
            model.overlays.command_completion = None;
            model.dirty = true;
        }
        NormalCommand::Search => model.begin_search(),
        NormalCommand::Append => model.append(),
        NormalCommand::AppendLineEnd => model.append_line_end(),
        NormalCommand::OpenBelow => model.open_below(),
        NormalCommand::OpenAbove => model.open_above(),
        NormalCommand::SearchNext(forward) => model.search_next(forward),
        NormalCommand::RegisterWait => {
            model.input.register_wait = true;
            model.dirty = true;
        }
        NormalCommand::DeleteForward => {
            let count = model.take_count();
            for _ in 0..count {
                model.delete_forward();
            }
        }
        NormalCommand::Put => model.put_register(),
        NormalCommand::Undo => model.undo(),
        NormalCommand::Redo => model.redo(),
        NormalCommand::RepeatChange => {
            let count = model.input.count;
            let _ = model.take_count();
            model.repeat_last_change(count);
        }
        NormalCommand::ToggleComment => model.toggle_comment(),
        NormalCommand::MatchBracket => model.match_bracket(),
        NormalCommand::JumpBack => model.jump_back(),
        NormalCommand::JumpForward => model.jump_forward(),
        NormalCommand::Count(character) => model.push_count(character),
        NormalCommand::MoveHome => model.move_home(),
        NormalCommand::MoveEnd => model.move_end(),
        NormalCommand::MoveWords(motion) => model.move_words(motion),
        NormalCommand::MoveChars(delta) => model.move_chars(delta),
        NormalCommand::MoveLine(delta) => model.move_lines(delta),
        NormalCommand::MoveFileEnd => model.move_file_end(),
        NormalCommand::LocationNext => model.move_stdin_location(1),
        NormalCommand::LocationPrevious => model.move_stdin_location(-1),
        NormalCommand::OpenLocation => model.open_stdin_location(),
    }
}

fn execute_context_action(model: &mut Model, action: ContextAction, key: KeyPress) {
    match action {
        ContextAction::Normal(command) => execute_normal_command(model, command),
        ContextAction::Pending(command) => {
            execute_pending_command(model, command, ' ', key);
        }
    }
    if model.visual_anchor.is_some() {
        model.sync_visual_selection();
    }
}

fn execute_command_id(model: &mut Model, id: &str, key: KeyPress) -> bool {
    if let Some(command) = NormalCommand::from_id(id) {
        execute_normal_command(model, command);
        return true;
    }
    if let Some(command) = PendingCommand::from_id(id)
        && command.is_direct()
    {
        execute_pending_command(model, command, ' ', key);
        return true;
    }
    false
}

fn handle_key(model: &mut Model, key: KeyPress) {
    let control = key.ctrl();
    let text_input = key.text_input();
    if model.plugins.pkg_install.is_some() {
        match key.code {
            KeyCode::Esc => model.cancel_pkg_install(),
            KeyCode::Enter => model.confirm_pkg_install(),
            KeyCode::Char('t') => model.trust_pkg_key(),
            _ => {}
        }
        return;
    }
    #[cfg(target_os = "linux")]
    if model.plugins.manager.is_some() {
        let confirming = model
            .plugins
            .manager
            .as_ref()
            .is_some_and(|manager| manager.confirm_uninstall);
        if confirming {
            match key.code {
                KeyCode::Char('y') => model.uninstall_selected_plugin(),
                KeyCode::Char('n') | KeyCode::Esc => model.cancel_uninstall(),
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Esc => model.close_plugin_manager(),
                KeyCode::Up => model.move_plugin_manager(-1),
                KeyCode::Down => model.move_plugin_manager(1),
                KeyCode::Char(' ') => model.toggle_selected_plugin(),
                KeyCode::Char('x') => model.request_uninstall(),
                KeyCode::Char('r') => model.rollback_selected_plugin(),
                _ => {}
            }
        }
        return;
    }

    if model.welcome {
        if key.code == KeyCode::Esc {
            model.welcome = false;
            model.dirty = true;
            return;
        }
        if key.code == KeyCode::Char('?') {
            model.show_help();
            return;
        }
        if key.code == KeyCode::Char('r') {
            model.welcome = false;
            model.open_recent_files();
            return;
        }
        model.welcome = false;
        model.dirty = true;
    }
    if let Some(waiting) = model.input.macro_wait.take() {
        match waiting {
            'q' => {
                model.input.recording = Some(match key.code {
                    KeyCode::Char(register) => register,
                    _ => 'q',
                });
                model.input.recording_keys.clear();
            }
            '@' => {
                let register = match key.code {
                    KeyCode::Char('@') => model.input.last_macro,
                    KeyCode::Char(register) => Some(register),
                    _ => None,
                };
                if let Some(register) = register
                    && let Some(steps) = model.editor.macro_steps(register).map(<[_]>::to_vec)
                {
                    model.input.last_macro = Some(register);
                    model.input.replaying = true;
                    for step in steps {
                        let Some(key) = macro_key(&step) else {
                            model.status = format!("E: macro step unavailable: {}", step.key);
                            model.dirty = true;
                            break;
                        };
                        handle_key(model, KeyPress::from_key(key));
                    }
                    model.input.replaying = false;
                }
            }
            _ => {}
        }
        return;
    }

    if let Some(register) = model.input.recording {
        if key.code == KeyCode::Char('q') {
            model.input.recording = None;
            let steps = std::mem::take(&mut model.input.recording_keys)
                .into_iter()
                .map(macro_step)
                .collect();
            model.editor.set_macro(register, steps);
            model.dirty = true;
            return;
        }
        model.input.recording_keys.push(key.event());
    }

    if model.input.register_wait {
        if text_input && let KeyCode::Char(character) = key.code {
            model.input.register = Some(character);
            model.input.register_wait = false;
            model.dirty = true;
        }
        return;
    }
    if model.overlays.command_palette.is_some() {
        match key.code {
            KeyCode::Esc => model.close_command_palette(),
            KeyCode::Up => model.move_command_palette(-1),
            KeyCode::Down => model.move_command_palette(1),
            KeyCode::Char('p') if key.ctrl() => model.move_command_palette(-1),
            KeyCode::Char('n') if key.ctrl() => model.move_command_palette(1),
            KeyCode::Enter => {
                if let Some(name) = model.selected_theme() {
                    model.commit_theme(name);
                    return;
                }
                let selected = model.selected_command();
                model.close_command_palette();
                if let Some((id, plugin_id)) = selected {
                    model.remember_command(&id);
                    if let Some(plugin_id) = plugin_id {
                        model.run_plugin_command(&plugin_id, &id);
                    } else {
                        execute_command_id(model, &id, key);
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(palette) = model.overlays.command_palette.as_mut() {
                    palette.table.query_mut().pop();
                }
                model.refresh_command_palette();
            }
            KeyCode::Char(character) if text_input => {
                if let Some(palette) = model.overlays.command_palette.as_mut() {
                    palette.table.query_mut().push(character);
                }
                model.refresh_command_palette();
            }
            _ => {}
        }
        return;
    }

    if model.keymap_context() == KeymapContext::Popup {
        model.input.pending = None;
        model.input.leader_deadline = None;
        if let Some(action) = model
            .keymap
            .resolve_context_exact(KeymapContext::Popup, key)
        {
            execute_context_action(model, action, key);
            return;
        }
    }

    if let Some(pending) = model.input.pending.take() {
        model.input.leader_deadline = None;
        model.dirty = true;
        if key.ctrl() || key.alt() || key.super_key() {
            model.status = "E: modifier not valid in pending command".to_owned();
            return;
        }
        let object_pending = model.input.object_pending;
        model.input.object_pending = false;
        let command = resolve_pending(pending, key, object_pending, &model.keymap);
        if command.is_none() && !object_pending && !matches!(pending, 'g' | ']' | '[') {
            let _ = model.take_count();
        }
        if let Some(command) = command {
            execute_pending_command(model, command, pending, key);
        }
        return;
    }
    if model.overlays.fuzzy.is_some() {
        match key.code {
            KeyCode::Esc => {
                model.overlays.fuzzy = None;
                model.clear_preview();
                model.dirty = true;
            }
            KeyCode::Enter => model.fuzzy_open(),
            KeyCode::Up => model.fuzzy_move(-1),
            KeyCode::Down => model.fuzzy_move(1),
            KeyCode::Backspace => {
                if let Some(fuzzy) = model.overlays.fuzzy.as_mut() {
                    fuzzy.query.pop();
                }
                model.fuzzy_refresh();
                model.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                if let Some(fuzzy) = model.overlays.fuzzy.as_mut() {
                    fuzzy.query.push(character);
                }
                model.fuzzy_refresh();
                model.dirty = true;
            }
            _ => {}
        }
        return;
    }

    if model.overlays.lsp_results.workspace_symbols.is_some() {
        let consumed = match key.code {
            KeyCode::Esc => {
                model.cancel_workspace_symbols();
                true
            }
            KeyCode::Up => {
                model.move_workspace_symbol(-1);
                true
            }
            KeyCode::Down => {
                model.move_workspace_symbol(1);
                true
            }
            KeyCode::Enter => {
                model.apply_workspace_symbol();
                true
            }
            KeyCode::Backspace => {
                if let Some(symbols) = model.overlays.lsp_results.workspace_symbols.as_mut() {
                    symbols.query.pop();
                }
                model.filter_workspace_symbols();
                true
            }
            KeyCode::Char(character) if text_input => {
                if let Some(symbols) = model.overlays.lsp_results.workspace_symbols.as_mut() {
                    symbols.query.push(character);
                }
                model.filter_workspace_symbols();
                true
            }
            _ => {
                model.cancel_workspace_symbols();
                false
            }
        };
        if consumed {
            return;
        }
    }
    if model.overlays.settings_overlay.is_some() {
        match key.code {
            KeyCode::Esc => model.cancel_settings(),
            KeyCode::Up | KeyCode::Char('k') => model.move_settings(-1),
            KeyCode::Down | KeyCode::Char('j') => model.move_settings(1),
            KeyCode::Left | KeyCode::Char('h') => model.adjust_settings(-1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => model.adjust_settings(1),
            KeyCode::Tab => model.cycle_settings_tab(
                key.modifiers
                    .contains(crossterm::event::KeyModifiers::SHIFT),
            ),
            KeyCode::Char('r') => model.reset_settings_row(),
            KeyCode::Char('s') => model.save_settings(),
            _ => {}
        }
        return;
    }
    if model.overlays.lsp_results.call_hierarchy.is_some() {
        let consumed = match key.code {
            KeyCode::Esc => {
                model.cancel_call_hierarchy();
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                model.move_call_hierarchy(-1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                model.move_call_hierarchy(1);
                true
            }
            KeyCode::Enter => {
                model.apply_call_hierarchy();
                true
            }
            _ => {
                model.cancel_call_hierarchy();
                false
            }
        };
        if consumed {
            return;
        }
    }
    if model.overlays.lsp_results.references.is_some() {
        let consumed = match key.code {
            KeyCode::Esc => {
                model.cancel_references();
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                model.move_reference(-1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                model.move_reference(1);
                true
            }
            KeyCode::Enter => {
                model.apply_reference();
                true
            }
            _ => {
                model.cancel_references();
                false
            }
        };
        if consumed {
            return;
        }
    }

    if model.overlays.lsp_results.document_symbols.is_some() {
        let consumed = match key.code {
            KeyCode::Esc => {
                model.cancel_document_symbols();
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                model.move_document_symbol(-1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                model.move_document_symbol(1);
                true
            }
            KeyCode::Enter => {
                model.apply_document_symbol();
                true
            }
            _ => {
                model.cancel_document_symbols();
                false
            }
        };
        if consumed {
            return;
        }
    }
    if model.overlays.lsp_results.document_highlights.is_some() && key.code == KeyCode::Esc {
        model.cancel_document_highlights();
        return;
    }

    if model.overlays.lsp_results.code_actions.is_some() {
        let consumed = match key.code {
            KeyCode::Esc => {
                model.cancel_code_actions();
                true
            }
            KeyCode::Up => {
                model.move_code_action(-1);
                true
            }
            KeyCode::Down => {
                model.move_code_action(1);
                true
            }
            KeyCode::Enter => {
                model.apply_code_action();
                true
            }
            _ => {
                model.cancel_code_actions();
                false
            }
        };
        if consumed {
            return;
        }
    }

    if model.overlays.lsp_results.completion.is_some() {
        let consumed = match key.code {
            KeyCode::Esc => {
                model.cancel_completion();
                true
            }
            KeyCode::Up => {
                model.move_completion(-1);
                true
            }
            KeyCode::Down => {
                model.move_completion(1);
                true
            }
            KeyCode::Enter | KeyCode::Tab => {
                model.apply_completion();
                true
            }
            KeyCode::Backspace => {
                if let Some(completion) = model.overlays.lsp_results.completion.as_mut() {
                    completion.query.pop();
                }
                model.backspace();
                model.filter_completion();
                true
            }
            KeyCode::Char(character) if text_input => {
                if let Some(completion) = model.overlays.lsp_results.completion.as_mut() {
                    completion.query.push(character);
                }
                model.insert_text(&character.to_string());
                model.filter_completion();
                true
            }
            _ => {
                model.cancel_completion();
                false
            }
        };
        if consumed {
            return;
        }
    }

    if model.overlays.lsp_results.hover.is_some() {
        model.overlays.lsp_results.hover = None;
        model.dirty = true;
        if key.code == KeyCode::Esc {
            return;
        }
    }

    if model.ui.diagnostic_peek.is_some() {
        match key.code {
            KeyCode::Esc => {
                model.close_diagnostic_peek();
                return;
            }
            KeyCode::Up => {
                model.scroll_diagnostic_peek(-1);
                return;
            }
            KeyCode::Down => {
                model.scroll_diagnostic_peek(1);
                return;
            }
            KeyCode::PageUp => {
                model.scroll_diagnostic_peek(-5);
                return;
            }
            KeyCode::PageDown => {
                model.scroll_diagnostic_peek(5);
                return;
            }
            KeyCode::Enter => {
                model.close_diagnostic_peek();
                model.request_code_actions();
                return;
            }
            _ => model.close_diagnostic_peek(),
        }
    }
    if model.overlays.theme_browser.is_some() {
        match key.code {
            KeyCode::Esc => {
                model.cancel_theme_browser();
                return;
            }
            KeyCode::Enter => {
                model.apply_theme_browser();
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                model.move_theme_browser(-1);
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                model.move_theme_browser(1);
                return;
            }
            _ => {}
        }
    }
    if model.overlays.debug_panel.is_some() {
        match key.code {
            KeyCode::Esc => {
                model.close_debug_panel();
                return;
            }
            KeyCode::Tab => {
                model.cycle_debug_panel();
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                model.move_debug_selection(-1);
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                model.move_debug_selection(1);
                return;
            }
            KeyCode::Char('r') => {
                model.request_debug_refresh();
                return;
            }
            _ => {}
        }
    }
    if model.overlays.test_explorer.is_some() {
        match key.code {
            KeyCode::Esc => {
                model.close_test_explorer();
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                model.move_test_explorer(-1);
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                model.move_test_explorer(1);
                return;
            }
            KeyCode::Enter => {
                model.jump_test_failure();
                return;
            }
            KeyCode::Char('o') => {
                model.show_task_output();
                return;
            }
            _ => {}
        }
    }
    if model.ui.problems.visible {
        match key.code {
            KeyCode::Esc => {
                model.ui.problems.visible = false;
                model.dirty = true;
                return;
            }
            KeyCode::Up => {
                model.ui.problems.move_selection(-1);
                model.dirty = true;
                return;
            }
            KeyCode::Down => {
                model.ui.problems.move_selection(1);
                model.dirty = true;
                return;
            }
            KeyCode::Enter => {
                model.jump_problem();
                return;
            }
            _ => {}
        }
    }
    #[cfg(unix)]
    if model.send_terminal_key(key.event()) {
        return;
    }
    if model.keymap_context() == KeymapContext::FileTree {
        if let Some(action) = model
            .keymap
            .resolve_context_exact(KeymapContext::FileTree, key)
        {
            execute_context_action(model, action, key);
            return;
        }
        if key.code == KeyCode::Char(' ') && text_input {
            model.begin_pending(' ');
            return;
        }
        match key.code {
            KeyCode::Esc => {
                model.ui.focus.tree = false;
                model.tree.visible = false;
                model.dirty = true;
            }
            KeyCode::Char('j') | KeyCode::Down => model.tree_move(1),
            KeyCode::Char('k') | KeyCode::Up => model.tree_move(-1),
            KeyCode::Char('h') | KeyCode::Left => model.tree_collapse(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => model.tree_toggle(),
            KeyCode::Char('H') => model.toggle_hidden(),
            KeyCode::Char('a') => model.begin_tree_create(CreateKind::File),
            KeyCode::Char('A') => model.begin_tree_create(CreateKind::Directory),
            KeyCode::Char('r') => model.begin_tree_rename(),
            KeyCode::Char('m') => model.begin_tree_transfer(false),
            KeyCode::Char('c') => model.begin_tree_transfer(true),
            KeyCode::Char('d') | KeyCode::Delete => model.begin_tree_delete(),
            _ => {
                if let Some(action) = model.keymap.resolve_context(KeymapContext::Global, key) {
                    execute_context_action(model, action, key);
                }
            }
        }
        return;
    }

    let text_context = match &model.ui.input_mode {
        InputMode::CommandLine { .. } => Some(KeymapContext::CommandLine),
        InputMode::Search { .. } => Some(KeymapContext::Search),
        InputMode::Rename { .. } => Some(KeymapContext::Rename),
        _ => None,
    };
    if let Some(context) = text_context
        && let Some(action) = model.keymap.resolve_context_exact(context, key)
    {
        execute_context_action(model, action, key);
        return;
    }

    match &mut model.ui.input_mode {
        InputMode::Rename { name } => match key.code {
            KeyCode::Esc => {
                model.ui.input_mode = InputMode::Normal;
                model.status = "rename cancelled".to_owned();
                model.dirty = true;
            }
            KeyCode::Enter => {
                let new_name = std::mem::take(name);
                model.ui.input_mode = InputMode::Normal;
                model.queue_effect(Effect::LspRename(new_name));
            }
            KeyCode::Backspace => {
                name.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                name.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::Create {
            directory,
            name,
            kind,
        } => match key.code {
            KeyCode::Esc => {
                model.ui.input_mode = InputMode::Normal;
                model.ui.focus.tree = model.tree.visible;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let directory = directory.clone();
                let name = std::mem::take(name);
                let kind = *kind;
                model.ui.input_mode = InputMode::Normal;
                model.queue_effect(Effect::TreeCreate {
                    directory,
                    name,
                    kind,
                });
            }
            KeyCode::Backspace => {
                name.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                name.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::RenameEntry { path, name } => match key.code {
            KeyCode::Esc => {
                model.ui.input_mode = InputMode::Normal;
                model.ui.focus.tree = model.tree.visible;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let path = path.clone();
                let name = std::mem::take(name);
                model.ui.input_mode = InputMode::Normal;
                model.queue_effect(Effect::TreeRename { path, name });
            }
            KeyCode::Backspace => {
                name.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                name.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::TransferEntry {
            path,
            copy,
            directory,
        } => match key.code {
            KeyCode::Esc => {
                model.ui.input_mode = InputMode::Normal;
                model.ui.focus.tree = model.tree.visible;
                model.status = "transfer cancelled".to_owned();
                model.dirty = true;
            }
            KeyCode::Enter => {
                let path = path.clone();
                let copy = *copy;
                let directory = std::mem::take(directory);
                model.ui.input_mode = InputMode::Normal;
                model.queue_effect(Effect::TreeTransfer {
                    path,
                    copy,
                    directory,
                });
            }
            KeyCode::Backspace => {
                directory.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                directory.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::ConfirmClose { index } => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let index = *index;
                model.queue_effect(Effect::SaveBuffer { index, close: true });
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                let index = *index;
                model.ui.input_mode = InputMode::Normal;
                model.close_buffer(index, true);
            }
            KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => {
                model.ui.input_mode = InputMode::Normal;
                model.status = "close cancelled".to_owned();
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::ConfirmReload { path } => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let path = path.clone();
                model.ui.input_mode = InputMode::Normal;
                model.queue_effect(Effect::Reload(path));
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                model.ui.input_mode = InputMode::Normal;
                model.status = "kept local buffer".to_owned();
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::ConfirmDelete { path } => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let path = path.clone();
                model.ui.input_mode = InputMode::Normal;
                model.queue_effect(Effect::TreeDelete(path));
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                model.ui.input_mode = InputMode::Normal;
                model.ui.focus.tree = model.tree.visible;
                model.status = "delete cancelled".to_owned();
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::CommandLine { buffer } => match key.code {
            KeyCode::Esc => {
                if !model.close_command_completion() {
                    model.ui.input_mode = InputMode::Normal;
                }
                model.dirty = true;
            }
            KeyCode::Tab => model.complete_command_line(),
            KeyCode::Up => model.move_command_completion(-1),
            KeyCode::Down => model.move_command_completion(1),
            KeyCode::Enter => {
                if model.overlays.command_completion.is_some() {
                    model.complete_command_line();
                } else {
                    let command = buffer.take();
                    model.ui.input_mode = InputMode::Normal;
                    model.execute_command(command.trim());
                }
            }
            KeyCode::Left => {
                buffer.move_left();
                model.overlays.command_completion = None;
                model.dirty = true;
            }
            KeyCode::Right => {
                buffer.move_right();
                model.overlays.command_completion = None;
                model.dirty = true;
            }
            KeyCode::Home => {
                buffer.move_home();
                model.overlays.command_completion = None;
                model.dirty = true;
            }
            KeyCode::End => {
                buffer.move_end();
                model.overlays.command_completion = None;
                model.dirty = true;
            }
            KeyCode::Backspace => {
                model.overlays.command_completion = None;
                buffer.backspace();
                model.dirty = true;
            }
            KeyCode::Delete => {
                model.overlays.command_completion = None;
                buffer.delete();
                model.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                model.overlays.command_completion = None;
                buffer.insert(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::Search { query } => {
            if key.ctrl() {
                match key.code {
                    KeyCode::Char('r') => model.toggle_search_regex(),
                    KeyCode::Char('w') => model.toggle_search_whole_word(),
                    KeyCode::Char('c') => model.toggle_search_case(),
                    _ => {}
                }
                return;
            }
            match key.code {
                KeyCode::Esc => model.cancel_search(),
                KeyCode::Enter => {
                    let query = query.take();
                    model.commit_search(query);
                }
                KeyCode::Left => {
                    query.move_left();
                    model.dirty = true;
                }
                KeyCode::Right => {
                    query.move_right();
                    model.dirty = true;
                }
                KeyCode::Home => {
                    query.move_home();
                    model.dirty = true;
                }
                KeyCode::End => {
                    query.move_end();
                    model.dirty = true;
                }
                KeyCode::Backspace => {
                    query.backspace();
                    model.schedule_search();
                }
                KeyCode::Delete => {
                    query.delete();
                    model.schedule_search();
                }
                KeyCode::Char(character) if text_input => {
                    query.insert(character);
                    model.schedule_search();
                }
                _ => {}
            }
        }
        InputMode::Insert => match key.code {
            KeyCode::Esc => {
                model.cancel_completion();
                model.cancel_signature_help();
                model.cancel_snippet();
                model.finish_insert_session();
                model.ui.input_mode = InputMode::Normal;
                model.dirty = true;
            }
            KeyCode::Enter => model.insert_text("\n"),
            KeyCode::Tab => {
                if !model.next_snippet_stop() {
                    model.insert_text("    ");
                }
            }
            KeyCode::BackTab => {
                model.previous_snippet_stop();
            }
            KeyCode::Backspace => model.backspace(),
            KeyCode::Delete => model.delete_forward(),
            KeyCode::Left => model.move_char(-1),
            KeyCode::Right => model.move_char(1),
            KeyCode::Up => model.move_line(-1),
            KeyCode::Down => model.move_line(1),
            KeyCode::Home => model.move_home(),
            KeyCode::End => model.move_end(),
            KeyCode::Char(' ') if control => model.queue_effect(Effect::LspCompletion),
            KeyCode::Char(character) if text_input => {
                model.insert_auto_pair(character);
                if matches!(character, '.' | ':') {
                    model.auto_completion();
                }
            }
            _ => {}
        },
        InputMode::Normal => {
            if model.visual_anchor.is_some() {
                if model.modal_layer == Some(ModalLayer::Surround) {
                    match key.code {
                        KeyCode::Esc => model.cancel_modal_layer(),
                        KeyCode::Char('(') | KeyCode::Char(')') => {
                            model.surround_selection('(', ')');
                        }
                        KeyCode::Char('[') | KeyCode::Char(']') => {
                            model.surround_selection('[', ']');
                        }
                        KeyCode::Char('{') | KeyCode::Char('}') => {
                            model.surround_selection('{', '}');
                        }
                        KeyCode::Char('<') | KeyCode::Char('>') => {
                            model.surround_selection('<', '>');
                        }
                        KeyCode::Char('"') => model.surround_selection('"', '"'),
                        KeyCode::Char('\'') => model.surround_selection('\'', '\''),
                        KeyCode::Char('`') => model.surround_selection('`', '`'),
                        _ => model.cancel_modal_layer(),
                    }
                    return;
                }
                if let Some(action) = model
                    .keymap
                    .resolve_context_exact(KeymapContext::Modal, key)
                {
                    execute_context_action(model, action, key);
                    return;
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('v') => {
                        model.exit_visual();
                        return;
                    }
                    KeyCode::Char('V') => {
                        model.begin_visual(true);
                        return;
                    }
                    KeyCode::Char('s') => {
                        model.begin_surround();
                        return;
                    }
                    KeyCode::Char('d') => {
                        model.delete_visual();
                        return;
                    }
                    KeyCode::Char('y') => {
                        model.yank_visual();
                        return;
                    }
                    KeyCode::Char('c') => {
                        if model.reject_read_only() {
                            model.exit_visual();
                            return;
                        }
                        model.delete_visual();
                        model.ui.input_mode = InputMode::Insert;
                        model.begin_insert(InsertOrigin::InsertHere);
                        return;
                    }
                    _ => {}
                }
            }
            if key.ctrl() && key.code == KeyCode::Char('v') {
                model.status = "E: block selection is not supported".to_owned();
                model.dirty = true;
                return;
            }
            match key.code {
                KeyCode::Char('v') => {
                    model.begin_visual(false);
                    return;
                }
                KeyCode::Char('V') => {
                    model.begin_visual(true);
                    return;
                }
                _ => {}
            }
            let context = model.keymap_context();
            let command = if context == KeymapContext::Editor {
                model
                    .keymap
                    .resolve_context_exact(context, key)
                    .or_else(|| {
                        resolve_normal(key, model.input.count, &model.keymap)
                            .map(ContextAction::Normal)
                    })
                    .or_else(|| {
                        model
                            .keymap
                            .resolve_context_exact(KeymapContext::Global, key)
                    })
            } else {
                resolve_normal(key, model.input.count, &model.keymap)
                    .map(ContextAction::Normal)
                    .or_else(|| {
                        model
                            .keymap
                            .resolve_context_exact(KeymapContext::Global, key)
                    })
            };
            if let Some(command) = command {
                let command = match command {
                    ContextAction::Normal(command) => command,
                    ContextAction::Pending(command) => {
                        execute_pending_command(model, command, ' ', key);
                        if model.visual_anchor.is_some() {
                            model.sync_visual_selection();
                        }
                        return;
                    }
                };
                execute_normal_command(model, command);
            }
            if model.visual_anchor.is_some() {
                model.sync_visual_selection();
            }
        }
    }
}
