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
        PendingCommand::FileStart => {
            let _ = model.take_count();
            model.move_file_start();
        }
        PendingCommand::CenterCursor => {
            let _ = model.take_count();
            model.center_cursor();
        }
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
        PendingCommand::BufferPicker => {
            let _ = model.take_count();
            model.open_buffer_picker();
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
        PendingCommand::ChangeLines => {
            let count = model.take_count();
            model.change_lines(count);
        }
        PendingCommand::SectionPrevious => {
            let count = model.take_count();
            model.move_section(false, count);
        }
        PendingCommand::SectionNext => {
            let count = model.take_count();
            model.move_section(true, count);
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
            model.file.dirty = true;
        }
        NormalCommand::MacroReplay => {
            model.input.macro_wait = Some('@');
            model.file.dirty = true;
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
            model.file.dirty = true;
        }
        NormalCommand::Search => model.begin_search(),
        NormalCommand::Append => model.append(),
        NormalCommand::AppendLineEnd => model.append_line_end(),
        NormalCommand::OpenBelow => model.open_below(),
        NormalCommand::OpenAbove => model.open_above(),
        NormalCommand::SearchNext(forward) => model.search_next(forward),
        NormalCommand::RegisterWait => {
            model.input.register_wait = true;
            model.file.dirty = true;
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
        NormalCommand::SelectBuffer(index) => {
            let _ = model.take_count();
            model.select_buffer(index);
        }
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
    if model.edit.visual_anchor.is_some() {
        model.sync_visual_selection();
    }
}

fn dispatch_context_binding(model: &mut Model, context: KeymapContext, key: KeyPress) -> bool {
    if let Some((_, action)) = resolve_context_binding(&[context], key, &model.config.keymap) {
        execute_context_action(model, action, key);
        true
    } else {
        false
    }
}

fn route_context_key(
    contexts: &[KeymapContext],
    key: KeyPress,
    keymap: &crate::keymap::KeymapConfig,
    mut handler: impl FnMut(KeymapContext, ContextAction) -> bool,
) -> bool {
    let Some((context, action)) = resolve_context_binding(contexts, key, keymap) else {
        return false;
    };
    handler(context, action)
}

fn dispatch_specific_context_binding(model: &mut Model, key: KeyPress) -> bool {
    let contexts = model
        .active_keymap_contexts()
        .into_iter()
        .filter(|context| !matches!(context, KeymapContext::Editor | KeymapContext::Global))
        .collect::<Vec<_>>();
    if contexts.is_empty() {
        return false;
    }
    let mut routed = None;
    if !route_context_key(&contexts, key, &model.config.keymap, |_, action| {
        routed = Some(action);
        true
    }) {
        return false;
    }
    let Some(action) = routed else {
        return false;
    };
    model.input.pending = None;
    model.input.leader_deadline = None;
    execute_context_action(model, action, key);
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerAction {
    Next,
    Previous,
    Accept,
    Cancel,
    PopQuery,
    AppendQuery(char),
}

fn picker_action(key: KeyPress, text_input: bool) -> Option<PickerAction> {
    match key.code {
        KeyCode::Esc => Some(PickerAction::Cancel),
        KeyCode::Enter => Some(PickerAction::Accept),
        KeyCode::Up => Some(PickerAction::Previous),
        KeyCode::Down => Some(PickerAction::Next),
        KeyCode::Backspace => Some(PickerAction::PopQuery),
        KeyCode::Char(character) if text_input => Some(PickerAction::AppendQuery(character)),
        _ => None,
    }
}

fn execute_buffer_picker_action(model: &mut Model, action: PickerAction) {
    match action {
        PickerAction::Next => model.move_buffer_picker(1),
        PickerAction::Previous => model.move_buffer_picker(-1),
        PickerAction::Accept => model.apply_buffer_picker(),
        PickerAction::Cancel => model.close_buffer_picker(),
        PickerAction::PopQuery => {
            if let Some(picker) = model.overlays.buffer_picker.as_mut() {
                picker.query.pop();
            }
            model.refresh_buffer_picker();
        }
        PickerAction::AppendQuery(character) => {
            if let Some(picker) = model.overlays.buffer_picker.as_mut() {
                picker.query.push(character);
            }
            model.refresh_buffer_picker();
        }
    }
}
fn execute_fuzzy_action(model: &mut Model, action: PickerAction) {
    match action {
        PickerAction::Next => model.fuzzy_move(1),
        PickerAction::Previous => model.fuzzy_move(-1),
        PickerAction::Accept => model.fuzzy_open(),
        PickerAction::Cancel => model.close_fuzzy(),
        PickerAction::PopQuery => {
            if let Some(fuzzy) = model.overlays.fuzzy.as_mut() {
                fuzzy.query.pop();
            }
            model.fuzzy_refresh();
            model.file.dirty = true;
        }
        PickerAction::AppendQuery(character) => {
            if let Some(fuzzy) = model.overlays.fuzzy.as_mut() {
                fuzzy.query.push(character);
            }
            model.fuzzy_refresh();
            model.file.dirty = true;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileTreeAction {
    Next,
    Previous,
    Collapse,
    Toggle,
    ToggleHidden,
    CommandLine,
    Create(CreateKind),
    Rename,
    Transfer(bool),
    Delete,
    PendingLeader,
    Cancel,
}

fn file_tree_action(key: KeyPress, text_input: bool) -> Option<FileTreeAction> {
    match key.code {
        KeyCode::Esc => Some(FileTreeAction::Cancel),
        KeyCode::Char('j') | KeyCode::Down => Some(FileTreeAction::Next),
        KeyCode::Char('k') | KeyCode::Up => Some(FileTreeAction::Previous),
        KeyCode::Char('h') | KeyCode::Left => Some(FileTreeAction::Collapse),
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => Some(FileTreeAction::Toggle),
        KeyCode::Char('H') if text_input => Some(FileTreeAction::ToggleHidden),
        KeyCode::Char(':') if text_input => Some(FileTreeAction::CommandLine),
        KeyCode::Char('a') if text_input => Some(FileTreeAction::Create(CreateKind::File)),
        KeyCode::Char('A') if text_input => Some(FileTreeAction::Create(CreateKind::Directory)),
        KeyCode::Char('r') if text_input => Some(FileTreeAction::Rename),
        KeyCode::Char('m') if text_input => Some(FileTreeAction::Transfer(false)),
        KeyCode::Char('c') if text_input => Some(FileTreeAction::Transfer(true)),
        KeyCode::Char('d') | KeyCode::Delete => Some(FileTreeAction::Delete),
        KeyCode::Char(' ') if text_input => Some(FileTreeAction::PendingLeader),
        _ => None,
    }
}

fn execute_file_tree_action(model: &mut Model, action: FileTreeAction) {
    match action {
        FileTreeAction::Next => model.tree_move(1),
        FileTreeAction::Previous => model.tree_move(-1),
        FileTreeAction::Collapse => model.tree_collapse(),
        FileTreeAction::Toggle => model.tree_toggle(),
        FileTreeAction::ToggleHidden => model.toggle_hidden(),
        FileTreeAction::CommandLine => {
            model.ui.input_mode = InputMode::CommandLine {
                buffer: InputBuffer::default(),
            };
            model.overlays.command_completion = None;
            model.file.dirty = true;
        }
        FileTreeAction::Create(kind) => model.begin_tree_create(kind),
        FileTreeAction::Rename => model.begin_tree_rename(),
        FileTreeAction::Transfer(copy) => model.begin_tree_transfer(copy),
        FileTreeAction::Delete => model.begin_tree_delete(),
        FileTreeAction::PendingLeader => model.begin_pending(' '),
        FileTreeAction::Cancel => {
            model.ui.focus.tree = false;
            model.tree.visible = false;
            model.file.dirty = true;
        }
    }
}

fn command_palette_action(key: KeyPress, text_input: bool) -> Option<PickerAction> {
    match key.code {
        KeyCode::Char('p') if key.ctrl() => Some(PickerAction::Previous),
        KeyCode::Char('n') if key.ctrl() => Some(PickerAction::Next),
        _ => picker_action(key, text_input),
    }
}

fn execute_command_palette_action(model: &mut Model, action: PickerAction, key: KeyPress) {
    match action {
        PickerAction::Next => model.move_command_palette(1),
        PickerAction::Previous => model.move_command_palette(-1),
        PickerAction::Accept => {
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
        PickerAction::Cancel => model.close_command_palette(),
        PickerAction::PopQuery => {
            if let Some(palette) = model.overlays.command_palette.as_mut() {
                palette.table.query_mut().pop();
            }
            model.refresh_command_palette();
        }
        PickerAction::AppendQuery(character) => {
            if let Some(palette) = model.overlays.command_palette.as_mut() {
                palette.table.query_mut().push(character);
            }
            model.refresh_command_palette();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LspListKind {
    CallHierarchy,
    References,
    DocumentSymbols,
    CodeActions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LspListAction {
    Previous,
    Next,
    Accept,
    Cancel,
    CancelAndPassThrough,
}

fn run_lsp_list_action(model: &mut Model, kind: LspListKind, key: KeyPress) -> bool {
    let action = lsp_list_action(key, kind != LspListKind::CodeActions);
    execute_lsp_list_action(model, kind, action)
}

fn active_lsp_list(model: &Model) -> Option<LspListKind> {
    if model.overlays.lsp_results.call_hierarchy.is_some() {
        Some(LspListKind::CallHierarchy)
    } else if model.overlays.lsp_results.references.is_some() {
        Some(LspListKind::References)
    } else if model.overlays.lsp_results.document_symbols.is_some() {
        Some(LspListKind::DocumentSymbols)
    } else if model.overlays.lsp_results.code_actions.is_some() {
        Some(LspListKind::CodeActions)
    } else {
        None
    }
}

fn lsp_list_action(key: KeyPress, vi_keys: bool) -> LspListAction {
    match key.code {
        KeyCode::Esc => LspListAction::Cancel,
        KeyCode::Up => LspListAction::Previous,
        KeyCode::Down => LspListAction::Next,
        KeyCode::Enter => LspListAction::Accept,
        KeyCode::Char('k') if vi_keys => LspListAction::Previous,
        KeyCode::Char('j') if vi_keys => LspListAction::Next,
        _ => LspListAction::CancelAndPassThrough,
    }
}

fn execute_lsp_list_action(model: &mut Model, kind: LspListKind, action: LspListAction) -> bool {
    match (kind, action) {
        (LspListKind::CallHierarchy, LspListAction::Previous) => model.move_call_hierarchy(-1),
        (LspListKind::CallHierarchy, LspListAction::Next) => model.move_call_hierarchy(1),
        (LspListKind::CallHierarchy, LspListAction::Accept) => model.apply_call_hierarchy(),
        (
            LspListKind::CallHierarchy,
            LspListAction::Cancel | LspListAction::CancelAndPassThrough,
        ) => {
            model.cancel_call_hierarchy();
        }
        (LspListKind::References, LspListAction::Previous) => model.move_reference(-1),
        (LspListKind::References, LspListAction::Next) => model.move_reference(1),
        (LspListKind::References, LspListAction::Accept) => model.apply_reference(),
        (LspListKind::References, LspListAction::Cancel | LspListAction::CancelAndPassThrough) => {
            model.cancel_references();
        }
        (LspListKind::DocumentSymbols, LspListAction::Previous) => model.move_document_symbol(-1),
        (LspListKind::DocumentSymbols, LspListAction::Next) => model.move_document_symbol(1),
        (LspListKind::DocumentSymbols, LspListAction::Accept) => model.apply_document_symbol(),
        (
            LspListKind::DocumentSymbols,
            LspListAction::Cancel | LspListAction::CancelAndPassThrough,
        ) => model.cancel_document_symbols(),
        (LspListKind::CodeActions, LspListAction::Previous) => model.move_code_action(-1),
        (LspListKind::CodeActions, LspListAction::Next) => model.move_code_action(1),
        (LspListKind::CodeActions, LspListAction::Accept) => model.apply_code_action(),
        (LspListKind::CodeActions, LspListAction::Cancel | LspListAction::CancelAndPassThrough) => {
            model.cancel_code_actions();
        }
    }
    !matches!(action, LspListAction::CancelAndPassThrough)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoverAction {
    Dismiss,
    Cancel,
}

fn hover_action(key: KeyPress) -> HoverAction {
    if key.code == KeyCode::Esc {
        HoverAction::Cancel
    } else {
        HoverAction::Dismiss
    }
}

fn execute_hover_action(model: &mut Model, action: HoverAction) -> bool {
    model.close_hover();
    matches!(action, HoverAction::Cancel)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiagnosticPeekAction {
    Previous,
    Next,
    PagePrevious,
    PageNext,
    CodeActions,
    Close,
    DismissAndPassThrough,
}

fn diagnostic_peek_action(key: KeyPress) -> DiagnosticPeekAction {
    match key.code {
        KeyCode::Esc => DiagnosticPeekAction::Close,
        KeyCode::Up => DiagnosticPeekAction::Previous,
        KeyCode::Down => DiagnosticPeekAction::Next,
        KeyCode::PageUp => DiagnosticPeekAction::PagePrevious,
        KeyCode::PageDown => DiagnosticPeekAction::PageNext,
        KeyCode::Enter => DiagnosticPeekAction::CodeActions,
        _ => DiagnosticPeekAction::DismissAndPassThrough,
    }
}

fn execute_diagnostic_peek_action(model: &mut Model, action: DiagnosticPeekAction) -> bool {
    match action {
        DiagnosticPeekAction::Previous => model.scroll_diagnostic_peek(-1),
        DiagnosticPeekAction::Next => model.scroll_diagnostic_peek(1),
        DiagnosticPeekAction::PagePrevious => model.scroll_diagnostic_peek(-5),
        DiagnosticPeekAction::PageNext => model.scroll_diagnostic_peek(5),
        DiagnosticPeekAction::CodeActions => {
            model.close_diagnostic_peek();
            model.request_code_actions();
        }
        DiagnosticPeekAction::Close => model.close_diagnostic_peek(),
        DiagnosticPeekAction::DismissAndPassThrough => {
            model.close_diagnostic_peek();
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThemeBrowserAction {
    Previous,
    Next,
    Apply,
    Cancel,
}

fn theme_browser_action(key: KeyPress) -> Option<ThemeBrowserAction> {
    match key.code {
        KeyCode::Esc => Some(ThemeBrowserAction::Cancel),
        KeyCode::Enter => Some(ThemeBrowserAction::Apply),
        KeyCode::Up | KeyCode::Char('k') => Some(ThemeBrowserAction::Previous),
        KeyCode::Down | KeyCode::Char('j') => Some(ThemeBrowserAction::Next),
        _ => None,
    }
}

fn execute_theme_browser_action(model: &mut Model, action: ThemeBrowserAction) {
    match action {
        ThemeBrowserAction::Previous => model.move_theme_browser(-1),
        ThemeBrowserAction::Next => model.move_theme_browser(1),
        ThemeBrowserAction::Apply => model.apply_theme_browser(),
        ThemeBrowserAction::Cancel => model.cancel_theme_browser(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebugPanelAction {
    Previous,
    Next,
    Cycle,
    Refresh,
    Cancel,
}

fn debug_panel_action(key: KeyPress) -> Option<DebugPanelAction> {
    match key.code {
        KeyCode::Esc => Some(DebugPanelAction::Cancel),
        KeyCode::Tab => Some(DebugPanelAction::Cycle),
        KeyCode::Up | KeyCode::Char('k') => Some(DebugPanelAction::Previous),
        KeyCode::Down | KeyCode::Char('j') => Some(DebugPanelAction::Next),
        KeyCode::Char('r') => Some(DebugPanelAction::Refresh),
        _ => None,
    }
}

fn execute_debug_panel_action(model: &mut Model, action: DebugPanelAction) {
    match action {
        DebugPanelAction::Previous => model.move_debug_selection(-1),
        DebugPanelAction::Next => model.move_debug_selection(1),
        DebugPanelAction::Cycle => model.cycle_debug_panel(),
        DebugPanelAction::Refresh => model.request_debug_refresh(),
        DebugPanelAction::Cancel => model.close_debug_panel(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestExplorerAction {
    Previous,
    Next,
    Jump,
    Output,
    Cancel,
}

fn test_explorer_action(key: KeyPress) -> Option<TestExplorerAction> {
    match key.code {
        KeyCode::Esc => Some(TestExplorerAction::Cancel),
        KeyCode::Up | KeyCode::Char('k') => Some(TestExplorerAction::Previous),
        KeyCode::Down | KeyCode::Char('j') => Some(TestExplorerAction::Next),
        KeyCode::Enter => Some(TestExplorerAction::Jump),
        KeyCode::Char('o') => Some(TestExplorerAction::Output),
        _ => None,
    }
}

fn execute_test_explorer_action(model: &mut Model, action: TestExplorerAction) {
    match action {
        TestExplorerAction::Previous => model.move_test_explorer(-1),
        TestExplorerAction::Next => model.move_test_explorer(1),
        TestExplorerAction::Jump => model.jump_test_failure(),
        TestExplorerAction::Output => model.show_task_output(),
        TestExplorerAction::Cancel => model.close_test_explorer(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProblemsAction {
    Previous,
    Next,
    Jump,
    Cancel,
}

fn problems_action(key: KeyPress) -> Option<ProblemsAction> {
    match key.code {
        KeyCode::Esc => Some(ProblemsAction::Cancel),
        KeyCode::Up => Some(ProblemsAction::Previous),
        KeyCode::Down => Some(ProblemsAction::Next),
        KeyCode::Enter => Some(ProblemsAction::Jump),
        _ => None,
    }
}

fn execute_problems_action(model: &mut Model, action: ProblemsAction) {
    match action {
        ProblemsAction::Previous => {
            model.ui.problems.move_selection(-1);
            model.file.dirty = true;
        }
        ProblemsAction::Next => {
            model.ui.problems.move_selection(1);
            model.file.dirty = true;
        }
        ProblemsAction::Jump => model.jump_problem(),
        ProblemsAction::Cancel => model.close_problems(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsAction {
    Previous,
    Next,
    Decrease,
    Increase,
    CycleTab(bool),
    Reset,
    Save,
    Cancel,
}

fn settings_action(key: KeyPress) -> Option<SettingsAction> {
    match key.code {
        KeyCode::Esc => Some(SettingsAction::Cancel),
        KeyCode::Up | KeyCode::Char('k') => Some(SettingsAction::Previous),
        KeyCode::Down | KeyCode::Char('j') => Some(SettingsAction::Next),
        KeyCode::Left | KeyCode::Char('h') => Some(SettingsAction::Decrease),
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => Some(SettingsAction::Increase),
        KeyCode::Tab => Some(SettingsAction::CycleTab(
            key.modifiers
                .contains(crossterm::event::KeyModifiers::SHIFT),
        )),
        KeyCode::Char('r') => Some(SettingsAction::Reset),
        KeyCode::Char('s') => Some(SettingsAction::Save),
        _ => None,
    }
}

fn execute_settings_action(model: &mut Model, action: SettingsAction) {
    match action {
        SettingsAction::Previous => model.move_settings(-1),
        SettingsAction::Next => model.move_settings(1),
        SettingsAction::Decrease => model.adjust_settings(-1),
        SettingsAction::Increase => model.adjust_settings(1),
        SettingsAction::CycleTab(backwards) => model.cycle_settings_tab(backwards),
        SettingsAction::Reset => model.reset_settings_row(),
        SettingsAction::Save => model.save_settings(),
        SettingsAction::Cancel => model.cancel_settings(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceSymbolAction {
    Previous,
    Next,
    Accept,
    Cancel,
    PopQuery,
    AppendQuery(char),
    CancelAndPassThrough,
}

fn workspace_symbol_action(key: KeyPress, text_input: bool) -> WorkspaceSymbolAction {
    match key.code {
        KeyCode::Esc => WorkspaceSymbolAction::Cancel,
        KeyCode::Up => WorkspaceSymbolAction::Previous,
        KeyCode::Down => WorkspaceSymbolAction::Next,
        KeyCode::Enter => WorkspaceSymbolAction::Accept,
        KeyCode::Backspace => WorkspaceSymbolAction::PopQuery,
        KeyCode::Char(character) if text_input => WorkspaceSymbolAction::AppendQuery(character),
        _ => WorkspaceSymbolAction::CancelAndPassThrough,
    }
}

fn execute_workspace_symbol_action(model: &mut Model, action: WorkspaceSymbolAction) -> bool {
    match action {
        WorkspaceSymbolAction::Previous => model.move_workspace_symbol(-1),
        WorkspaceSymbolAction::Next => model.move_workspace_symbol(1),
        WorkspaceSymbolAction::Accept => model.apply_workspace_symbol(),
        WorkspaceSymbolAction::Cancel => model.cancel_workspace_symbols(),
        WorkspaceSymbolAction::PopQuery => {
            if let Some(symbols) = model.overlays.lsp_results.workspace_symbols.as_mut() {
                symbols.query.pop();
            }
            model.filter_workspace_symbols();
        }
        WorkspaceSymbolAction::AppendQuery(character) => {
            if let Some(symbols) = model.overlays.lsp_results.workspace_symbols.as_mut() {
                symbols.query.push(character);
            }
            model.filter_workspace_symbols();
        }
        WorkspaceSymbolAction::CancelAndPassThrough => {
            model.cancel_workspace_symbols();
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletionAction {
    Previous,
    Next,
    Accept,
    Cancel,
    Backspace,
    Insert(char),
    CancelAndPassThrough,
}

fn completion_action(key: KeyPress, text_input: bool) -> CompletionAction {
    match key.code {
        KeyCode::Esc => CompletionAction::Cancel,
        KeyCode::Up => CompletionAction::Previous,
        KeyCode::Down => CompletionAction::Next,
        KeyCode::Enter | KeyCode::Tab => CompletionAction::Accept,
        KeyCode::Backspace => CompletionAction::Backspace,
        KeyCode::Char(character) if text_input => CompletionAction::Insert(character),
        _ => CompletionAction::CancelAndPassThrough,
    }
}

fn execute_completion_action(model: &mut Model, action: CompletionAction) -> bool {
    match action {
        CompletionAction::Previous => model.move_completion(-1),
        CompletionAction::Next => model.move_completion(1),
        CompletionAction::Accept => model.apply_completion(),
        CompletionAction::Cancel => model.cancel_completion(),
        CompletionAction::Backspace => {
            if let Some(completion) = model.overlays.lsp_results.completion.as_mut() {
                completion.query.pop();
            }
            model.backspace();
            model.filter_completion();
        }
        CompletionAction::Insert(character) => {
            if let Some(completion) = model.overlays.lsp_results.completion.as_mut() {
                completion.query.push(character);
            }
            model.insert_text(&character.to_string());
            model.filter_completion();
        }
        CompletionAction::CancelAndPassThrough => {
            model.cancel_completion();
            return false;
        }
    }
    true
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
