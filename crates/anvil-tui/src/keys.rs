//! Keyboard dispatch for normal, insert, command, and overlay modes.

use super::*;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};

pub(crate) fn handle_event(model: &mut Model, event: Event) {
    model.syntax.borrow_mut().note_activity();
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => handle_key(model, key),
        Event::Paste(text) => {
            model.pending = None;
            if let InputMode::Insert = model.input_mode {
                model.insert_text(&text);
            } else if let InputMode::CommandLine { buffer } = &mut model.input_mode {
                buffer.push_str(&text);
                model.dirty = true;
            } else if let InputMode::Search { query } = &mut model.input_mode {
                query.push_str(&text);
                model.dirty = true;
            } else if let InputMode::Rename { name } = &mut model.input_mode {
                name.push_str(&text);
                model.dirty = true;
            }
        }
        Event::Resize(_, _) => model.dirty = true,
        _ => {}
    }
}

fn handle_key(model: &mut Model, key: KeyEvent) {
    let control = key
        .modifiers
        .contains(crossterm::event::KeyModifiers::CONTROL);

    if model.welcome {
        model.welcome = false;
        model.dirty = true;
        if key.code == KeyCode::Esc {
            return;
        }
        if key.code == KeyCode::Char('?') {
            model.show_help();
            return;
        }
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
                        handle_key(model, key);
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
        model.recording_keys.push(key);
    }

    if model.register_wait {
        if let KeyCode::Char(character) = key.code {
            model.register = Some(character);
            model.register_wait = false;
            model.dirty = true;
        }
        return;
    }
    if let Some(pending) = model.pending.take() {
        model.dirty = true;
        if model.object_pending {
            model.object_pending = false;
            if pending == 'c' {
                match key.code {
                    KeyCode::Char('w') => model.change_inner_word(),
                    KeyCode::Char('"') => model.change_inner_quote('"'),
                    KeyCode::Char('\'') => model.change_inner_quote('\''),
                    _ => {}
                }
            }
            return;
        }
        if pending == 'c' && key.code == KeyCode::Char('i') {
            model.pending = Some('c');
            model.object_pending = true;
            return;
        }
        if pending == 'g' {
            if key.code == KeyCode::Char('d') {
                model.request_definition();
            }
            return;
        }
        if pending == ']' || pending == '[' {
            if key.code == KeyCode::Char('d') {
                model.jump_diagnostic(pending == ']');
            }
            return;
        }
        if let KeyCode::Char(character) = key.code
            && character.is_ascii_digit()
        {
            model.count = model.count.saturating_mul(10) + usize::from(character as u8 - b'0');
            model.pending = Some(pending);
            return;
        }
        let count = model.take_count();
        if pending == 'w' {
            match key.code {
                KeyCode::Char('v') => model.split_vertical(),
                KeyCode::Char('h') => model.focus_split(false),
                KeyCode::Char('l') => model.focus_split(true),
                KeyCode::Char('q') => model.close_split(),
                _ => {}
            }
            return;
        }
        if pending == ' ' {
            match key.code {
                KeyCode::Char('e') => model.open_tree(),
                KeyCode::Char('f') => model.start_fuzzy(),
                KeyCode::Char('r') => model.start_rename(),
                KeyCode::Char('a') => model.request_code_actions(),
                KeyCode::Char('s') => model.request_document_symbols(),
                KeyCode::Char('S') => model.request_workspace_symbols(),
                KeyCode::Char('F') => model.request_formatting(),
                _ => {}
            }
            return;
        }
        match (pending, key.code) {
            ('c', KeyCode::Char('w')) => model.change_word(WordMotion::NextStart),
            ('c', KeyCode::Char('e')) => model.change_word(WordMotion::End),
            ('c', KeyCode::Char('b')) => model.change_word(WordMotion::PreviousStart),
            ('d', KeyCode::Char('d')) => model.delete_lines(count),
            ('y', KeyCode::Char('y')) => model.yank_lines(count),
            ('d', KeyCode::Char('w')) => model.delete_words(WordMotion::NextStart, count),
            ('d', KeyCode::Char('e')) => model.delete_words(WordMotion::End, count),
            ('d', KeyCode::Char('b')) => model.delete_words(WordMotion::PreviousStart, count),
            ('y', KeyCode::Char('w')) => model.yank_words(WordMotion::NextStart, count),
            ('y', KeyCode::Char('e')) => model.yank_words(WordMotion::End, count),
            ('y', KeyCode::Char('b')) => model.yank_words(WordMotion::PreviousStart, count),
            _ => {}
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
            KeyCode::Char(character) if !control => {
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
            KeyCode::Char(character) if !control => {
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
            KeyCode::Char(character) if !control => {
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

    if model.tree_focus {
        match key.code {
            KeyCode::Esc => {
                model.tree_focus = false;
                model.tree_visible = false;
                model.dirty = true;
            }
            KeyCode::Char('j') | KeyCode::Down => model.tree_move(1),
            KeyCode::Char('k') | KeyCode::Up => model.tree_move(-1),
            KeyCode::Char('h') | KeyCode::Left => model.tree_collapse(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => model.tree_toggle(),
            KeyCode::Char('H') => model.toggle_hidden(),
            _ => {}
        }
        return;
    }

    match &mut model.input_mode {
        InputMode::Rename { name } => match key.code {
            KeyCode::Esc => {
                model.input_mode = InputMode::Normal;
                model.status = "rename cancelled".to_owned();
                model.dirty = true;
            }
            KeyCode::Enter => {
                let new_name = std::mem::take(name);
                model.input_mode = InputMode::Normal;
                model.request_rename(&new_name);
            }
            KeyCode::Backspace => {
                name.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if !control => {
                name.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::CommandLine { buffer } => match key.code {
            KeyCode::Esc => {
                model.input_mode = InputMode::Normal;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let command = std::mem::take(buffer);
                model.input_mode = InputMode::Normal;
                model.execute_command(command.trim());
            }
            KeyCode::Backspace => {
                buffer.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if !control => {
                buffer.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::Search { query } => match key.code {
            KeyCode::Esc => {
                model.input_mode = InputMode::Normal;
                model.dirty = true;
            }
            KeyCode::Enter => {
                let query = std::mem::take(query);
                model.input_mode = InputMode::Normal;
                model.execute_search(query);
            }
            KeyCode::Backspace => {
                query.pop();
                model.dirty = true;
            }
            KeyCode::Char(character) if !control => {
                query.push(character);
                model.dirty = true;
            }
            _ => {}
        },
        InputMode::Insert => match key.code {
            KeyCode::Esc => {
                model.cancel_completion();
                model.cancel_signature_help();
                model.input_mode = InputMode::Normal;
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
            KeyCode::Char(character) if !control => {
                model.insert_text(&character.to_string());
                if matches!(character, '.' | ':') {
                    model.auto_completion();
                }
            }
            _ => {}
        },
        InputMode::Normal => match key.code {
            KeyCode::Char(' ') if !control => {
                model.pending = Some(' ');
                model.dirty = true;
            }
            KeyCode::Char('g') if !control => {
                model.pending = Some('g');
                model.dirty = true;
            }
            KeyCode::Char(']') if !control => {
                model.pending = Some(']');
                model.dirty = true;
            }
            KeyCode::Char('[') if !control => {
                model.pending = Some('[');
                model.dirty = true;
            }
            KeyCode::Char('K') if !control => model.request_hover(),
            KeyCode::Char('q') if !control => {
                model.macro_wait = Some('q');
                model.dirty = true;
            }
            KeyCode::Char('@') if !control => {
                model.macro_wait = Some('@');
                model.dirty = true;
            }
            KeyCode::Char('s') if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) => {
                model.split_selection_lines();
            }
            KeyCode::Char('s') if !control => model.select_word_occurrences(),
            KeyCode::Char('C') if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) => {
                model.extend_line_cursor(true);
            }
            KeyCode::Char('C') if !control => model.extend_line_cursor(false),
            KeyCode::Char('i') => model.insert_here(),
            KeyCode::Char(':') => {
                model.input_mode = InputMode::CommandLine {
                    buffer: String::new(),
                };
                model.dirty = true;
            }
            KeyCode::Char('/') => {
                model.input_mode = InputMode::Search {
                    query: String::new(),
                };
                model.dirty = true;
            }
            KeyCode::Char('a') if !control => model.append(),
            KeyCode::Char('A') if !control => model.append_line_end(),
            KeyCode::Char('o') if !control => model.open_below(),
            KeyCode::Char('O') if !control => model.open_above(),
            KeyCode::Char('n') if !control => model.search_next(true),
            KeyCode::Char('N') if !control => model.search_next(false),
            KeyCode::Char('d') if !control => model.pending = Some('d'),
            KeyCode::Char('y') if !control => model.pending = Some('y'),
            KeyCode::Char('c') if !control => model.pending = Some('c'),
            KeyCode::Char('"') if !control => {
                model.register_wait = true;
                model.dirty = true;
            }
            KeyCode::Char('x') if !control => {
                let count = model.take_count();
                for _ in 0..count {
                    model.delete_forward();
                }
            }
            KeyCode::Char('p') if !control => model.put_register(),
            KeyCode::Char('u') if !control => model.undo(),
            KeyCode::Char('r') if control => model.redo(),
            KeyCode::Char(character) if !control && character.is_ascii_digit() => {
                model.push_count(character);
            }
            KeyCode::Char('0') if !control && model.count == 0 => model.move_home(),
            KeyCode::Char('w') if control => {
                model.pending = Some('w');
                model.dirty = true;
            }
            KeyCode::Char('w') if !control => model.move_words(WordMotion::NextStart),
            KeyCode::Char('b') if !control => model.move_words(WordMotion::PreviousStart),
            KeyCode::Char('e') if !control => model.move_words(WordMotion::End),
            KeyCode::Char('h') | KeyCode::Left => model.move_chars(-1),
            KeyCode::Char('l') | KeyCode::Right => model.move_chars(1),
            KeyCode::Char('j') | KeyCode::Down => model.move_lines(1),
            KeyCode::Char('k') | KeyCode::Up => model.move_lines(-1),
            KeyCode::Home => model.move_home(),
            KeyCode::End => model.move_end(),
            _ => {}
        },
    }
}
