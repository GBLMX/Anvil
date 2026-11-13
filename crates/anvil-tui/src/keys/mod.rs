//! Keyboard dispatch for normal, insert, command, and overlay modes.

use crate::input::{KeyPress, is_action_key};
use crate::keymap::{
    ContextAction, NormalCommand, PendingCommand, macro_key, macro_step, resolve_context_binding,
    resolve_normal, resolve_pending,
};
use anvil_session::command::{CommandContext, is_registered};
use anvil_session::ui::CreateKind;

use super::*;

use crossterm::event::{Event, KeyCode};

pub(crate) fn handle_event(model: &mut Model, event: Event) {
    let edit_before = match &event {
        Event::Key(key) if is_action_key(key) => Some((
            model.session.path.clone(),
            model.session.cursor,
            model.session.editor.revision(),
        )),
        Event::Paste(_) => Some((
            model.session.path.clone(),
            model.session.cursor,
            model.session.editor.revision(),
        )),
        _ => None,
    };
    match event {
        Event::Key(key) if is_action_key(&key) => {
            if matches!(model.view.pointer, mouse::PointerState::Dragging(_)) {
                model.view.pointer = mouse::PointerState::Idle;
            }
            model.syntax_cache.highlights.borrow_mut().note_activity();
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
            model.syntax_cache.highlights.borrow_mut().note_activity();
            model.input.pending = None;
            let (text, truncated) = normalize_paste(&text);
            if truncated {
                model.status.text = "E: paste truncated".to_owned();
                model.file.dirty = true;
            }
            if let InputMode::Insert = model.ui.input_mode {
                model.insert_text(&text);
            } else if let InputMode::CommandLine { buffer } = &mut model.ui.input_mode {
                buffer.insert_str(&text.replace('\n', " "));
                model.file.dirty = true;
            } else if let InputMode::Search { query } = &mut model.ui.input_mode {
                query.insert_str(&text.replace('\n', " "));
                model.file.dirty = true;
            } else if let InputMode::Rename { name } = &mut model.ui.input_mode {
                name.push_str(&text.replace('\n', " "));
                model.file.dirty = true;
            }
        }
        Event::FocusLost => {
            model.auto_save_before_focus_change();
            model.file.dirty = true;
        }
        Event::Resize(_, _) => model.file.dirty = true,
        _ => {}
    }
    model.drain_effects();
    if let Some((path, cursor, revision)) = edit_before
        && model.session.editor.revision() != revision
        && model.session.path == path
    {
        model.record_change(
            path,
            cursor,
            model.session.path.clone(),
            model.session.cursor,
        );
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

mod actions;
mod normal;
use actions::*;
use normal::*;
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

    if model.start_page.welcome {
        if key.code == KeyCode::Esc {
            model.start_page.welcome = false;
            model.file.dirty = true;
            return;
        }
        if key.code == KeyCode::Char('?') {
            model.show_help();
            return;
        }
        if key.code == KeyCode::Char('r') {
            model.start_page.welcome = false;
            model.open_recent_files();
            return;
        }
        model.start_page.welcome = false;
        model.file.dirty = true;
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
                    && let Some(steps) = model
                        .session
                        .editor
                        .macro_steps(register)
                        .map(<[_]>::to_vec)
                {
                    model.input.last_macro = Some(register);
                    model.input.replaying = true;
                    for step in steps {
                        let Some(key) = macro_key(&step) else {
                            model.status.text = format!("E: macro step unavailable: {}", step.key);
                            model.file.dirty = true;
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
            model.session.editor.set_macro(register, steps);
            model.file.dirty = true;
            return;
        }
        model.input.recording_keys.push(key.event());
    }

    if model.input.register_wait {
        if text_input && let KeyCode::Char(character) = key.code {
            model.input.register = Some(character);
            model.input.register_wait = false;
            model.file.dirty = true;
        }
        return;
    }
    if let Some(kind) = model.top_overlay() {
        if dispatch_specific_context_binding(model, key) {
            return;
        }
        match kind {
            OverlayKind::CommandPalette => {
                if let Some(action) = command_palette_action(key, text_input) {
                    execute_command_palette_action(model, action, key);
                }
                return;
            }
            OverlayKind::Settings => {
                if let Some(action) = settings_action(key) {
                    execute_settings_action(model, action);
                }
                return;
            }
            OverlayKind::BufferPicker => {
                if let Some(action) = picker_action(key, text_input) {
                    execute_buffer_picker_action(model, action);
                }
                return;
            }
            OverlayKind::Fuzzy => {
                if let Some(action) = picker_action(key, text_input) {
                    execute_fuzzy_action(model, action);
                }
                return;
            }
            OverlayKind::Problems => {
                if let Some(action) = problems_action(key) {
                    execute_problems_action(model, action);
                }
                return;
            }
            OverlayKind::WorkspaceSymbols => {
                let action = workspace_symbol_action(key, text_input);
                if execute_workspace_symbol_action(model, action) {
                    return;
                }
            }
            OverlayKind::DocumentSymbols => {
                if run_lsp_list_action(model, LspListKind::DocumentSymbols, key) {
                    return;
                }
            }
            OverlayKind::References => {
                if run_lsp_list_action(model, LspListKind::References, key) {
                    return;
                }
            }
            OverlayKind::CallHierarchy => {
                if run_lsp_list_action(model, LspListKind::CallHierarchy, key) {
                    return;
                }
            }
            OverlayKind::CodeActions => {
                if run_lsp_list_action(model, LspListKind::CodeActions, key) {
                    return;
                }
            }
            OverlayKind::DiagnosticPeek => {
                let action = diagnostic_peek_action(key);
                if execute_diagnostic_peek_action(model, action) {
                    return;
                }
            }
            OverlayKind::Hover => {
                let action = hover_action(key);
                if execute_hover_action(model, action) {
                    return;
                }
            }
            OverlayKind::ThemeBrowser => {
                if let Some(action) = theme_browser_action(key) {
                    execute_theme_browser_action(model, action);
                    return;
                }
            }
            OverlayKind::DebugPanel => {
                if let Some(action) = debug_panel_action(key) {
                    execute_debug_panel_action(model, action);
                }
                return;
            }
            OverlayKind::TestExplorer => {
                if let Some(action) = test_explorer_action(key) {
                    execute_test_explorer_action(model, action);
                }
                return;
            }
            OverlayKind::Completion => {
                let action = completion_action(key, text_input);
                if execute_completion_action(model, action) {
                    return;
                }
            }
        }
    }

    if dispatch_specific_context_binding(model, key) {
        return;
    }

    if let Some(pending) = model.input.pending.take() {
        model.input.leader_deadline = None;
        model.file.dirty = true;
        if key.ctrl() || key.alt() || key.super_key() {
            model.status.text = "E: modifier not valid in pending command".to_owned();
            return;
        }
        let object_pending = model.input.object_pending;
        model.input.object_pending = false;
        let command = resolve_pending(pending, key, object_pending, &model.config.keymap);
        if command.is_none() && !object_pending && key.code == KeyCode::Char(pending) && text_input
        {
            model.begin_pending(pending);
            return;
        }
        if command.is_none() && !object_pending && !matches!(pending, 'g' | ']' | '[') {
            let _ = model.take_count();
        }
        if let Some(command) = command {
            execute_pending_command(model, command, pending, key);
        }
        return;
    }
    if model.overlays.buffer_picker.is_some() {
        if let Some(action) = picker_action(key, text_input) {
            execute_buffer_picker_action(model, action);
        }
        return;
    }
    if model.overlays.fuzzy.is_some() {
        if let Some(action) = picker_action(key, text_input) {
            execute_fuzzy_action(model, action);
        }
        return;
    }

    if model.overlays.lsp_results.workspace_symbols.is_some() {
        let action = workspace_symbol_action(key, text_input);
        if execute_workspace_symbol_action(model, action) {
            return;
        }
    }
    if let Some(kind) = active_lsp_list(model) {
        let action = lsp_list_action(key, kind != LspListKind::CodeActions);
        if execute_lsp_list_action(model, kind, action) {
            return;
        }
    }
    if model.overlays.lsp_results.document_highlights.is_some() && key.code == KeyCode::Esc {
        model.cancel_document_highlights();
        return;
    }

    if model.overlays.lsp_results.completion.is_some() {
        let action = completion_action(key, text_input);
        if execute_completion_action(model, action) {
            return;
        }
    }
    if model.overlays.lsp_results.hover.is_some() {
        let action = hover_action(key);
        if execute_hover_action(model, action) {
            return;
        }
    }
    if model.ui.diagnostic_peek.is_some() {
        let action = diagnostic_peek_action(key);
        if execute_diagnostic_peek_action(model, action) {
            return;
        }
    }
    if model.overlays.theme_browser.is_some() {
        if let Some(action) = theme_browser_action(key) {
            execute_theme_browser_action(model, action);
            return;
        }
    }
    if model.overlays.debug_panel.is_some() {
        if let Some(action) = debug_panel_action(key) {
            execute_debug_panel_action(model, action);
        }
        return;
    }
    if model.overlays.test_explorer.is_some() {
        if let Some(action) = test_explorer_action(key) {
            execute_test_explorer_action(model, action);
        }
        return;
    }
    if model.ui.problems.visible {
        if let Some(action) = problems_action(key) {
            execute_problems_action(model, action);
            return;
        }
    }
    #[cfg(unix)]
    if model.send_terminal_key(key.event()) {
        return;
    }
    if model.keymap_context() == KeymapContext::FileTree {
        if let Some(action) = file_tree_action(key, text_input) {
            execute_file_tree_action(model, action);
            return;
        }
        dispatch_context_binding(model, KeymapContext::Global, key);
        return;
    }
    if model.ui.input_mode == InputMode::Normal && key.code == KeyCode::Char('?') {
        model.show_help();
        return;
    }

    match &mut model.ui.input_mode {
        InputMode::Rename { name } => match key.code {
            KeyCode::Esc => {
                model.ui.input_mode = InputMode::Normal;
                model.status.text = "rename cancelled".to_owned();
                model.file.dirty = true;
            }
            KeyCode::Enter => {
                let new_name = std::mem::take(name);
                model.ui.input_mode = InputMode::Normal;
                model.queue_effect(Effect::LspRename(new_name));
            }
            KeyCode::Backspace => {
                name.pop();
                model.file.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                name.push(character);
                model.file.dirty = true;
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
                model.file.dirty = true;
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
                model.file.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                name.push(character);
                model.file.dirty = true;
            }
            _ => {}
        },
        InputMode::RenameEntry { path, name } => match key.code {
            KeyCode::Esc => {
                model.ui.input_mode = InputMode::Normal;
                model.ui.focus.tree = model.tree.visible;
                model.file.dirty = true;
            }
            KeyCode::Enter => {
                let path = path.clone();
                let name = std::mem::take(name);
                model.ui.input_mode = InputMode::Normal;
                model.queue_effect(Effect::TreeRename { path, name });
            }
            KeyCode::Backspace => {
                name.pop();
                model.file.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                name.push(character);
                model.file.dirty = true;
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
                model.status.text = "transfer cancelled".to_owned();
                model.file.dirty = true;
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
                model.file.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                directory.push(character);
                model.file.dirty = true;
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
                model.status.text = "close cancelled".to_owned();
                model.file.dirty = true;
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
                model.status.text = "kept local buffer".to_owned();
                model.file.dirty = true;
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
                model.status.text = "delete cancelled".to_owned();
                model.file.dirty = true;
            }
            _ => {}
        },
        InputMode::CommandLine { buffer } => match key.code {
            KeyCode::Esc => {
                if !model.close_command_completion() {
                    model.ui.input_mode = InputMode::Normal;
                }
                model.file.dirty = true;
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
                model.file.dirty = true;
            }
            KeyCode::Right => {
                buffer.move_right();
                model.overlays.command_completion = None;
                model.file.dirty = true;
            }
            KeyCode::Home => {
                buffer.move_home();
                model.overlays.command_completion = None;
                model.file.dirty = true;
            }
            KeyCode::End => {
                buffer.move_end();
                model.overlays.command_completion = None;
                model.file.dirty = true;
            }
            KeyCode::Backspace => {
                model.overlays.command_completion = None;
                buffer.backspace();
                model.file.dirty = true;
            }
            KeyCode::Delete => {
                model.overlays.command_completion = None;
                buffer.delete();
                model.file.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                model.overlays.command_completion = None;
                buffer.insert(character);
                model.file.dirty = true;
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
                    model.file.dirty = true;
                }
                KeyCode::Right => {
                    query.move_right();
                    model.file.dirty = true;
                }
                KeyCode::Home => {
                    query.move_home();
                    model.file.dirty = true;
                }
                KeyCode::End => {
                    query.move_end();
                    model.file.dirty = true;
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
                model.file.dirty = true;
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
            if model.edit.visual_anchor.is_some() {
                if model.edit.modal_layer == Some(ModalLayer::Surround) {
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
                model.status.text = "E: block selection is not supported".to_owned();
                model.file.dirty = true;
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
            if model.edit.visual_anchor.is_none()
                && dispatch_context_binding(model, KeymapContext::Editor, key)
            {
                return;
            }
            let command = resolve_normal(key, model.input.count, &model.config.keymap)
                .map(ContextAction::Normal);
            if let Some(command) = command {
                let command = match command {
                    ContextAction::Normal(command) => command,
                    ContextAction::Pending(command) => {
                        execute_pending_command(model, command, ' ', key);
                        if model.edit.visual_anchor.is_some() {
                            model.sync_visual_selection();
                        }
                        return;
                    }
                };
                execute_normal_command(model, command);
            } else if dispatch_context_binding(model, KeymapContext::Global, key) {
                return;
            }
            if model.edit.visual_anchor.is_some() {
                model.sync_visual_selection();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::KeyModifiers;
    use crate::keymap::KeymapConfig;

    fn press(code: KeyCode) -> KeyPress {
        KeyPress::from_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn route_context_key_uses_the_most_specific_context() {
        let keymap = KeymapConfig::from_toml(
            r#"
[keymap.contexts.popup]
x = "view.focus_editor"

[keymap.contexts.editor]
x = "view.focus_right"
"#,
        )
        .expect("keymap");
        let mut routed = None;
        let handled = route_context_key(
            &[KeymapContext::Editor, KeymapContext::Popup],
            press(KeyCode::Char('x')),
            &keymap,
            |context, action| {
                routed = Some((context, action));
                true
            },
        );
        assert!(handled);
        assert_eq!(
            routed,
            Some((
                KeymapContext::Popup,
                ContextAction::Pending(PendingCommand::FocusEditor)
            ))
        );
    }

    #[test]
    fn route_context_key_skips_handlers_when_no_binding_matches() {
        let keymap = KeymapConfig::from_toml(
            r#"
[keymap.contexts.popup]
x = "view.focus_editor"
"#,
        )
        .expect("keymap");
        let mut routed = false;
        let handled = route_context_key(
            &[KeymapContext::Popup],
            press(KeyCode::Char('z')),
            &keymap,
            |_, _| {
                routed = true;
                true
            },
        );
        assert!(!handled);
        assert!(!routed);
    }
}
