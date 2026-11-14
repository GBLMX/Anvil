use super::*;

pub(super) fn execute_pending_command(
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
        PendingCommand::Declaration => model.queue_effect(Effect::LspDeclaration),
        PendingCommand::TypeDefinition => model.queue_effect(Effect::LspTypeDefinition),
        PendingCommand::Implementation => model.queue_effect(Effect::LspImplementation),
        PendingCommand::References => model.queue_effect(Effect::LspReferences),
        PendingCommand::Hover => model.queue_effect(Effect::LspHover),
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
        PendingCommand::ToggleComment => model.toggle_comment(),
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

pub(super) fn execute_normal_command(model: &mut Model, command: NormalCommand) {
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

pub(super) fn execute_context_action(model: &mut Model, action: ContextAction, key: KeyPress) {
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

pub(super) fn dispatch_context_binding(
    model: &mut Model,
    context: KeymapContext,
    key: KeyPress,
) -> bool {
    if let Some((_, action)) = resolve_context_binding(&[context], key, &model.config.keymap) {
        execute_context_action(model, action, key);
        true
    } else {
        false
    }
}

pub(super) fn route_context_key(
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

pub(super) fn dispatch_specific_context_binding(model: &mut Model, key: KeyPress) -> bool {
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

pub(super) fn execute_command_id(model: &mut Model, id: &str, key: KeyPress) -> bool {
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
