use super::*;

pub(super) enum PickerAction {
    Next,
    Previous,
    Accept,
    Cancel,
    PopQuery,
    AppendQuery(char),
}

pub(super) fn picker_action(key: KeyPress, text_input: bool) -> Option<PickerAction> {
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

pub(super) fn execute_buffer_picker_action(model: &mut Model, action: PickerAction) {
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
pub(super) fn execute_fuzzy_action(model: &mut Model, action: PickerAction) {
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
pub(super) enum FileTreeAction {
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

pub(super) fn file_tree_action(key: KeyPress, text_input: bool) -> Option<FileTreeAction> {
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

pub(super) fn execute_file_tree_action(model: &mut Model, action: FileTreeAction) {
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
            model.refresh_wrap_width();
            model.file.dirty = true;
        }
    }
}

pub(super) fn command_palette_action(key: KeyPress, text_input: bool) -> Option<PickerAction> {
    match key.code {
        KeyCode::Char('p') if key.ctrl() => Some(PickerAction::Previous),
        KeyCode::Char('n') if key.ctrl() => Some(PickerAction::Next),
        _ => picker_action(key, text_input),
    }
}

pub(super) fn execute_command_palette_action(
    model: &mut Model,
    action: PickerAction,
    key: KeyPress,
) {
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
pub(super) enum LspListKind {
    CallHierarchy,
    References,
    DocumentSymbols,
    CodeActions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LspListAction {
    Previous,
    Next,
    Accept,
    Cancel,
    CancelAndPassThrough,
}

pub(super) fn run_lsp_list_action(model: &mut Model, kind: LspListKind, key: KeyPress) -> bool {
    let action = lsp_list_action(key, kind != LspListKind::CodeActions);
    execute_lsp_list_action(model, kind, action)
}

pub(super) fn active_lsp_list(model: &Model) -> Option<LspListKind> {
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

pub(super) fn lsp_list_action(key: KeyPress, vi_keys: bool) -> LspListAction {
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

pub(super) fn execute_lsp_list_action(
    model: &mut Model,
    kind: LspListKind,
    action: LspListAction,
) -> bool {
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
pub(super) enum HoverAction {
    Dismiss,
    Cancel,
}

pub(super) fn hover_action(key: KeyPress) -> HoverAction {
    if key.code == KeyCode::Esc {
        HoverAction::Cancel
    } else {
        HoverAction::Dismiss
    }
}

pub(super) fn execute_hover_action(model: &mut Model, action: HoverAction) -> bool {
    model.close_hover();
    matches!(action, HoverAction::Cancel)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DiagnosticPeekAction {
    Previous,
    Next,
    PagePrevious,
    PageNext,
    CodeActions,
    Close,
    DismissAndPassThrough,
}

pub(super) fn diagnostic_peek_action(key: KeyPress) -> DiagnosticPeekAction {
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

pub(super) fn execute_diagnostic_peek_action(
    model: &mut Model,
    action: DiagnosticPeekAction,
) -> bool {
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
pub(super) enum ThemeBrowserAction {
    Previous,
    Next,
    Apply,
    Cancel,
}

pub(super) fn theme_browser_action(key: KeyPress) -> Option<ThemeBrowserAction> {
    match key.code {
        KeyCode::Esc => Some(ThemeBrowserAction::Cancel),
        KeyCode::Enter => Some(ThemeBrowserAction::Apply),
        KeyCode::Up | KeyCode::Char('k') => Some(ThemeBrowserAction::Previous),
        KeyCode::Down | KeyCode::Char('j') => Some(ThemeBrowserAction::Next),
        _ => None,
    }
}

pub(super) fn execute_theme_browser_action(model: &mut Model, action: ThemeBrowserAction) {
    match action {
        ThemeBrowserAction::Previous => model.move_theme_browser(-1),
        ThemeBrowserAction::Next => model.move_theme_browser(1),
        ThemeBrowserAction::Apply => model.apply_theme_browser(),
        ThemeBrowserAction::Cancel => model.cancel_theme_browser(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DebugPanelAction {
    Previous,
    Next,
    Cycle,
    Refresh,
    Cancel,
}

pub(super) fn debug_panel_action(key: KeyPress) -> Option<DebugPanelAction> {
    match key.code {
        KeyCode::Esc => Some(DebugPanelAction::Cancel),
        KeyCode::Tab => Some(DebugPanelAction::Cycle),
        KeyCode::Up | KeyCode::Char('k') => Some(DebugPanelAction::Previous),
        KeyCode::Down | KeyCode::Char('j') => Some(DebugPanelAction::Next),
        KeyCode::Char('r') => Some(DebugPanelAction::Refresh),
        _ => None,
    }
}

pub(super) fn execute_debug_panel_action(model: &mut Model, action: DebugPanelAction) {
    match action {
        DebugPanelAction::Previous => model.move_debug_selection(-1),
        DebugPanelAction::Next => model.move_debug_selection(1),
        DebugPanelAction::Cycle => model.cycle_debug_panel(),
        DebugPanelAction::Refresh => model.request_debug_refresh(),
        DebugPanelAction::Cancel => model.close_debug_panel(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TestExplorerAction {
    Previous,
    Next,
    Jump,
    Output,
    Cancel,
}

pub(super) fn test_explorer_action(key: KeyPress) -> Option<TestExplorerAction> {
    match key.code {
        KeyCode::Esc => Some(TestExplorerAction::Cancel),
        KeyCode::Up | KeyCode::Char('k') => Some(TestExplorerAction::Previous),
        KeyCode::Down | KeyCode::Char('j') => Some(TestExplorerAction::Next),
        KeyCode::Enter => Some(TestExplorerAction::Jump),
        KeyCode::Char('o') => Some(TestExplorerAction::Output),
        _ => None,
    }
}

pub(super) fn execute_test_explorer_action(model: &mut Model, action: TestExplorerAction) {
    match action {
        TestExplorerAction::Previous => model.move_test_explorer(-1),
        TestExplorerAction::Next => model.move_test_explorer(1),
        TestExplorerAction::Jump => model.jump_test_failure(),
        TestExplorerAction::Output => model.show_task_output(),
        TestExplorerAction::Cancel => model.close_test_explorer(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProblemsAction {
    Previous,
    Next,
    Jump,
    Cancel,
}

pub(super) fn problems_action(key: KeyPress) -> Option<ProblemsAction> {
    match key.code {
        KeyCode::Esc => Some(ProblemsAction::Cancel),
        KeyCode::Up => Some(ProblemsAction::Previous),
        KeyCode::Down => Some(ProblemsAction::Next),
        KeyCode::Enter => Some(ProblemsAction::Jump),
        _ => None,
    }
}

pub(super) fn execute_problems_action(model: &mut Model, action: ProblemsAction) {
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
pub(super) enum SettingsAction {
    Previous,
    Next,
    Decrease,
    Increase,
    CycleTab(bool),
    Reset,
    Save,
    Cancel,
}

pub(super) fn settings_action(key: KeyPress) -> Option<SettingsAction> {
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

pub(super) fn execute_settings_action(model: &mut Model, action: SettingsAction) {
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
pub(super) enum WorkspaceSymbolAction {
    Previous,
    Next,
    Accept,
    Cancel,
    PopQuery,
    AppendQuery(char),
    CancelAndPassThrough,
}

pub(super) fn workspace_symbol_action(key: KeyPress, text_input: bool) -> WorkspaceSymbolAction {
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

pub(super) fn execute_workspace_symbol_action(
    model: &mut Model,
    action: WorkspaceSymbolAction,
) -> bool {
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
pub(super) enum CompletionAction {
    Previous,
    Next,
    Accept,
    Cancel,
    Backspace,
    Insert(char),
    CancelAndPassThrough,
}

pub(super) fn completion_action(key: KeyPress, text_input: bool) -> CompletionAction {
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

pub(super) fn execute_completion_action(model: &mut Model, action: CompletionAction) -> bool {
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
