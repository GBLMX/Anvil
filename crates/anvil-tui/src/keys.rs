//! Keyboard dispatch for normal, insert, command, and overlay modes.

use crate::input::{KeyPress, is_action_key};
use crate::keymap::{NormalCommand, PendingCommand, resolve_normal, resolve_pending};
use anvil_session::command::{CommandContext, is_registered};
use anvil_session::ui::CreateKind;

use super::*;

use crossterm::event::{Event, KeyCode};

pub(crate) fn handle_event(model: &mut Model, event: Event) {
    match event {
        Event::Key(key) if is_action_key(&key) => {
            model.syntax.borrow_mut().note_activity();
            handle_key(model, KeyPress::from_key(key));
        }
        Event::Paste(text) => {
            model.syntax.borrow_mut().note_activity();
            model.pending = None;
            if let InputMode::Insert = model.ui.input_mode {
                model.insert_text(&text);
            } else if let InputMode::CommandLine { buffer } = &mut model.ui.input_mode {
                buffer.push_str(&text);
                model.dirty = true;
            } else if let InputMode::Search { query } = &mut model.ui.input_mode {
                query.push_str(&text);
                model.dirty = true;
            } else if let InputMode::Rename { name } = &mut model.ui.input_mode {
                name.push_str(&text);
                model.dirty = true;
            }
        }
        Event::Resize(_, _) => model.dirty = true,
        _ => {}
    }
}

fn handle_key(model: &mut Model, key: KeyPress) {
    let control = key.ctrl();
    let text_input = key.text_input();

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
        model.welcome = false;
        model.dirty = true;
    }
    if let Some(waiting) = model.macro_wait.take() {
        match waiting {
            'q' => {
                model.recording = Some(match key.code {
                    KeyCode::Char(register) => register,
                    _ => 'q',
                });
                model.recording_keys.clear();
            }
            '@' => {
                let register = match key.code {
                    KeyCode::Char('@') => model.last_macro,
                    KeyCode::Char(register) => Some(register),
                    _ => None,
                };
                if let Some(register) = register
                    && let Some(keys) = model.macros.get(&register).cloned()
                {
                    model.last_macro = Some(register);
                    model.replaying = true;
                    for key in keys {
                        handle_key(model, KeyPress::from_key(key));
                    }
                    model.replaying = false;
                }
            }
            _ => {}
        }
        return;
    }

    if let Some(register) = model.recording {
        if key.code == KeyCode::Char('q') {
            model.recording = None;
            model
                .macros
                .insert(register, std::mem::take(&mut model.recording_keys));
            model.dirty = true;
            return;
        }
        model.recording_keys.push(key.event());
    }

    if model.register_wait {
        if text_input && let KeyCode::Char(character) = key.code {
            model.register = Some(character);
            model.register_wait = false;
            model.dirty = true;
        }
        return;
    }
    if let Some(pending) = model.pending.take() {
        model.dirty = true;
        if key.ctrl() || key.alt() || key.super_key() {
            model.status = "E: modifier not valid in pending command".to_owned();
            return;
        }
        let object_pending = model.object_pending;
        model.object_pending = false;
        let command = resolve_pending(pending, key, object_pending, &model.keymap);
        if command.is_none() && !object_pending && !matches!(pending, 'g' | ']' | '[') {
            let _ = model.take_count();
        }
        if let Some(command) = command {
            debug_assert!(
                command
                    .id()
                    .is_none_or(|id| is_registered(id, CommandContext::Pending))
            );
            match command {
                PendingCommand::ObjectPending => {
                    model.pending = Some('c');
                    model.object_pending = true;
                }
                PendingCommand::ChangeInnerWord => model.change_inner_word(),
                PendingCommand::ChangeInnerQuote(quote) => model.change_inner_quote(quote),
                PendingCommand::Definition => model.request_definition(),
                PendingCommand::Diagnostic(forward) => model.jump_diagnostic(forward),
                PendingCommand::Count { pending, digit } => {
                    model.push_count(digit);
                    model.pending = Some(pending);
                }
                PendingCommand::SplitVertical => {
                    let _ = model.take_count();
                    model.split_vertical();
                }
                PendingCommand::FocusSplit(right) => {
                    let _ = model.take_count();
                    model.focus_split(right);
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
                PendingCommand::StartRename => {
                    let _ = model.take_count();
                    model.start_rename();
                }
                PendingCommand::CodeActions => {
                    let _ = model.take_count();
                    model.request_code_actions();
                }
                PendingCommand::DocumentSymbols => {
                    let _ = model.take_count();
                    model.request_document_symbols();
                }
                PendingCommand::WorkspaceSymbols => {
                    let _ = model.take_count();
                    model.request_workspace_symbols();
                }
                PendingCommand::Formatting => {
                    let _ = model.take_count();
                    model.request_formatting();
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
        return;
    }
    if model.fuzzy.is_some() {
        match key.code {
            KeyCode::Esc => {
                model.fuzzy = None;
                model.dirty = true;
            }
            KeyCode::Enter => model.fuzzy_open(),
            KeyCode::Up => model.fuzzy_move(-1),
            KeyCode::Down => model.fuzzy_move(1),
            KeyCode::Backspace => {
                if let Some(fuzzy) = model.fuzzy.as_mut() {
                    fuzzy.query.pop();
                }
                model.fuzzy_refresh();
                model.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                if let Some(fuzzy) = model.fuzzy.as_mut() {
                    fuzzy.query.push(character);
                }
                model.fuzzy_refresh();
                model.dirty = true;
            }
            _ => {}
        }
        return;
    }

    if model.workspace_symbols.is_some() {
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
                if let Some(symbols) = model.workspace_symbols.as_mut() {
                    symbols.query.pop();
                }
                model.filter_workspace_symbols();
                true
            }
            KeyCode::Char(character) if text_input => {
                if let Some(symbols) = model.workspace_symbols.as_mut() {
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

    if model.document_symbols.is_some() {
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

    if model.code_actions.is_some() {
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

    if model.completion.is_some() {
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
                if let Some(completion) = model.completion.as_mut() {
                    completion.query.pop();
                }
                model.backspace();
                model.filter_completion();
                true
            }
            KeyCode::Char(character) if text_input => {
                if let Some(completion) = model.completion.as_mut() {
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

    if model.hover.is_some() {
        model.hover = None;
        model.dirty = true;
        if key.code == KeyCode::Esc {
            return;
        }
    }

    if model.diagnostic_popup.is_some() {
        model.diagnostic_popup = None;
        model.dirty = true;
        if key.code == KeyCode::Esc {
            return;
        }
    }

    if model.ui.focus.tree {
        match key.code {
            KeyCode::Esc => {
                model.ui.focus.tree = false;
                model.tree_visible = false;
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
            _ => {}
        }
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
                model.request_rename(&new_name);
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
                model.ui.focus.tree = model.tree_visible;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let directory = directory.clone();
                let name = std::mem::take(name);
                let kind = *kind;
                model.ui.input_mode = InputMode::Normal;
                model.finish_tree_create(directory, name, kind);
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
                model.ui.focus.tree = model.tree_visible;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let path = path.clone();
                let name = std::mem::take(name);
                model.ui.input_mode = InputMode::Normal;
                model.finish_tree_rename(path, name);
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
                model.ui.focus.tree = model.tree_visible;
                model.status = "transfer cancelled".to_owned();
                model.dirty = true;
            }
            KeyCode::Enter => {
                let path = path.clone();
                let copy = *copy;
                let directory = std::mem::take(directory);
                model.ui.input_mode = InputMode::Normal;
                model.finish_tree_transfer(path, copy, directory);
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
                if model.save_buffer(index) {
                    model.ui.input_mode = InputMode::Normal;
                    model.close_buffer(index, true);
                }
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
                if let Err(error) = model.reload_external(&path) {
                    model.status = format!("E: {error}");
                    model.dirty = true;
                }
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
                model.finish_tree_delete(path);
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                model.ui.input_mode = InputMode::Normal;
                model.ui.focus.tree = model.tree_visible;
                model.status = "delete cancelled".to_owned();
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::CommandLine { buffer } => match key.code {
            KeyCode::Esc => {
                model.ui.input_mode = InputMode::Normal;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let command = std::mem::take(buffer);
                model.ui.input_mode = InputMode::Normal;
                model.execute_command(command.trim());
            }
            KeyCode::Backspace => {
                buffer.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                buffer.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::Search { query } => match key.code {
            KeyCode::Esc => {
                model.ui.input_mode = InputMode::Normal;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let query = std::mem::take(query);
                model.ui.input_mode = InputMode::Normal;
                model.execute_search(query);
            }
            KeyCode::Backspace => {
                query.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if text_input => {
                query.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::Insert => match key.code {
            KeyCode::Esc => {
                model.cancel_completion();
                model.cancel_signature_help();
                model.ui.input_mode = InputMode::Normal;
                model.dirty = true;
            }
            KeyCode::Enter => model.insert_text("\n"),
            KeyCode::Tab => model.insert_text("    "),
            KeyCode::Backspace => model.backspace(),
            KeyCode::Delete => model.delete_forward(),
            KeyCode::Left => model.move_char(-1),
            KeyCode::Right => model.move_char(1),
            KeyCode::Up => model.move_line(-1),
            KeyCode::Down => model.move_line(1),
            KeyCode::Home => model.move_home(),
            KeyCode::End => model.move_end(),
            KeyCode::Char(' ') if control => model.request_completion(),
            KeyCode::Char(character) if text_input => {
                model.insert_text(&character.to_string());
                if matches!(character, '.' | ':') {
                    model.auto_completion();
                }
            }
            _ => {}
        },
        InputMode::Normal => {
            if let Some(command) = resolve_normal(key, model.count, &model.keymap) {
                match command {
                    NormalCommand::CollapseSelections => model.collapse_selections(),
                    NormalCommand::Pending(pending) => {
                        model.pending = Some(pending);
                        model.dirty = true;
                    }
                    NormalCommand::Hover => model.request_hover(),
                    NormalCommand::MacroRecord => {
                        model.macro_wait = Some('q');
                        model.dirty = true;
                    }
                    NormalCommand::MacroReplay => {
                        model.macro_wait = Some('@');
                        model.dirty = true;
                    }
                    NormalCommand::SplitSelection => model.split_selection_lines(),
                    NormalCommand::SelectWord => model.select_word_occurrences(),
                    NormalCommand::ExtendLine(up) => model.extend_line_cursor(up),
                    NormalCommand::InsertHere => model.insert_here(),
                    NormalCommand::CommandLine => {
                        model.ui.input_mode = InputMode::CommandLine {
                            buffer: String::new(),
                        };
                        model.dirty = true;
                    }
                    NormalCommand::Search => {
                        model.ui.input_mode = InputMode::Search {
                            query: String::new(),
                        };
                        model.dirty = true;
                    }
                    NormalCommand::Append => model.append(),
                    NormalCommand::AppendLineEnd => model.append_line_end(),
                    NormalCommand::OpenBelow => model.open_below(),
                    NormalCommand::OpenAbove => model.open_above(),
                    NormalCommand::SearchNext(forward) => model.search_next(forward),
                    NormalCommand::RegisterWait => {
                        model.register_wait = true;
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
                    NormalCommand::Count(character) => model.push_count(character),
                    NormalCommand::MoveHome => model.move_home(),
                    NormalCommand::MoveEnd => model.move_end(),
                    NormalCommand::MoveWords(motion) => model.move_words(motion),
                    NormalCommand::MoveChars(delta) => model.move_chars(delta),
                    NormalCommand::MoveLine(delta) => model.move_lines(delta),
                }
            }
        }
    }
}
