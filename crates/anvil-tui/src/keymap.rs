//! Keymap resolution for normal-mode bindings.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::WordMotion;
use crate::input::KeyPress;
use crate::{InputMode, Model};
use anvil_editor::MacroStep;
use anvil_session::command::{self, CommandContext, CommandId, is_registered};
use anvil_session::keymap::KeymapContext;

mod config;
pub(crate) use config::{ContextAction, KeyChord, KeymapConfig, macro_key, macro_step};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NormalCommand {
    CollapseSelections,
    Pending(char),
    Hover,
    MacroRecord,
    MacroReplay,
    SplitSelection,
    SelectWord,
    ExtendLine(bool),
    InsertHere,
    CommandLine,
    Search,
    Append,
    AppendLineEnd,
    OpenBelow,
    OpenAbove,
    SearchNext(bool),
    RegisterWait,
    DeleteForward,
    Put,
    Undo,
    Redo,
    RepeatChange,
    ToggleComment,
    MatchBracket,
    JumpBack,
    JumpForward,
    Count(char),
    SelectBuffer(usize),
    MoveHome,
    MoveEnd,
    MoveWords(WordMotion),
    MoveChars(isize),
    MoveLine(isize),
    MoveFileEnd,
    LocationNext,
    LocationPrevious,
    OpenLocation,
}

impl NormalCommand {
    pub(crate) fn id(self) -> Option<CommandId> {
        use NormalCommand::*;
        match self {
            CollapseSelections => Some(command::COLLAPSE_SELECTIONS),
            Hover => Some(command::HOVER),
            MacroRecord => Some(command::MACRO_RECORD),
            MacroReplay => Some(command::MACRO_REPLAY),
            SplitSelection => Some(command::SPLIT_LINES),
            SelectWord => Some(command::WORD_OCCURRENCES),
            ExtendLine(true) => Some(command::EXTEND_UP),
            ExtendLine(false) => Some(command::EXTEND_DOWN),
            InsertHere => Some(command::INSERT_HERE),
            CommandLine => Some(command::COMMAND_LINE),
            Search => Some(command::SEARCH_OPEN),
            Append => Some(command::APPEND),
            AppendLineEnd => Some(command::APPEND_LINE_END),
            OpenBelow => Some(command::OPEN_BELOW),
            OpenAbove => Some(command::OPEN_ABOVE),
            SearchNext(true) => Some(command::SEARCH_NEXT),
            SearchNext(false) => Some(command::SEARCH_PREVIOUS),
            DeleteForward => Some(command::DELETE_FORWARD),
            Put => Some(command::PUT),
            Undo => Some(command::UNDO),
            Redo => Some(command::REDO),
            RepeatChange => Some(command::REPEAT_CHANGE),
            ToggleComment => Some(command::TOGGLE_COMMENT),
            MatchBracket => Some(command::MATCH_BRACKET),
            JumpBack => Some(command::JUMP_BACK),
            JumpForward => Some(command::JUMP_FORWARD),
            MoveHome => Some(command::LINE_START),
            MoveEnd => Some(command::LINE_END),
            MoveWords(WordMotion::NextStart) => Some(command::WORD_NEXT),
            MoveWords(WordMotion::PreviousStart) => Some(command::WORD_PREVIOUS),
            MoveWords(WordMotion::End) => Some(command::WORD_END),
            MoveChars(-1) => Some(command::CURSOR_LEFT),
            MoveChars(1) => Some(command::CURSOR_RIGHT),
            MoveChars(_) => None,
            MoveLine(1) => Some(command::CURSOR_DOWN),
            MoveLine(-1) => Some(command::CURSOR_UP),
            MoveLine(_) => None,
            MoveFileEnd => Some(command::CURSOR_FILE_END),
            Pending(_) | RegisterWait | Count(_) | SelectBuffer(_) | LocationNext
            | LocationPrevious | OpenLocation => None,
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "editor.collapse_selections" => Self::CollapseSelections,
            "lsp.hover" => Self::Hover,
            "macro.record" => Self::MacroRecord,
            "macro.replay" => Self::MacroReplay,
            "selection.split_lines" => Self::SplitSelection,
            "selection.word_occurrences" => Self::SelectWord,
            "selection.extend_up" => Self::ExtendLine(true),
            "selection.extend_down" => Self::ExtendLine(false),
            "editor.insert_here" => Self::InsertHere,
            "command_line.open" => Self::CommandLine,
            "search.open" => Self::Search,
            "editor.append" => Self::Append,
            "editor.append_line_end" => Self::AppendLineEnd,
            "editor.open_below" => Self::OpenBelow,
            "editor.open_above" => Self::OpenAbove,
            "search.next" => Self::SearchNext(true),
            "search.previous" => Self::SearchNext(false),
            "editor.delete_forward" => Self::DeleteForward,
            "editor.put" => Self::Put,
            "editor.undo" => Self::Undo,
            "editor.redo" => Self::Redo,
            "editor.repeat_change" => Self::RepeatChange,
            "editor.toggle_comment" => Self::ToggleComment,
            "navigation.match_bracket" => Self::MatchBracket,
            "navigation.jump_back" => Self::JumpBack,
            "navigation.jump_forward" => Self::JumpForward,
            "cursor.line_start" => Self::MoveHome,
            "cursor.line_end" => Self::MoveEnd,
            "cursor.word_next" => Self::MoveWords(WordMotion::NextStart),
            "cursor.word_previous" => Self::MoveWords(WordMotion::PreviousStart),
            "cursor.word_end" => Self::MoveWords(WordMotion::End),
            "cursor.left" => Self::MoveChars(-1),
            "cursor.right" => Self::MoveChars(1),
            "cursor.down" => Self::MoveLine(1),
            "cursor.up" => Self::MoveLine(-1),
            "cursor.file_end" => Self::MoveFileEnd,
            _ => return None,
        })
    }
}

pub(crate) fn normal_action(key: KeyPress, count: usize) -> Option<NormalCommand> {
    let text_input = key.text_input();
    match key.code {
        KeyCode::Esc => Some(NormalCommand::CollapseSelections),
        KeyCode::Char(' ') if text_input => Some(NormalCommand::Pending(' ')),
        KeyCode::Char('g') if text_input => Some(NormalCommand::Pending('g')),
        KeyCode::Char('z') if text_input => Some(NormalCommand::Pending('z')),
        KeyCode::Char(']') if text_input => Some(NormalCommand::Pending(']')),
        KeyCode::Char('[') if text_input => Some(NormalCommand::Pending('[')),
        KeyCode::Char('K') if text_input => Some(NormalCommand::Hover),
        KeyCode::Char('q') if text_input => Some(NormalCommand::MacroRecord),
        KeyCode::Char('@') if text_input => Some(NormalCommand::MacroReplay),
        KeyCode::Char(_) if key.alt() && key.command_char() == Some('s') => {
            Some(NormalCommand::SplitSelection)
        }
        KeyCode::Char('s') if text_input => Some(NormalCommand::SelectWord),
        KeyCode::Char(_) if key.alt() && key.command_char() == Some('c') => {
            Some(NormalCommand::ExtendLine(true))
        }
        KeyCode::Char('C') if text_input => Some(NormalCommand::ExtendLine(false)),
        KeyCode::Char('G') if text_input => Some(NormalCommand::MoveFileEnd),
        KeyCode::Char('i') if text_input => Some(NormalCommand::InsertHere),
        KeyCode::Char(':') if text_input => Some(NormalCommand::CommandLine),
        KeyCode::Char('/') | KeyCode::Char('_') if key.ctrl() => Some(NormalCommand::ToggleComment),
        KeyCode::Char(_) if key.alt() && key.command_char() == Some('o') => {
            Some(NormalCommand::JumpBack)
        }
        KeyCode::Char(_) if key.alt() && key.command_char() == Some('i') => {
            Some(NormalCommand::JumpForward)
        }
        KeyCode::Char('o') if key.ctrl() => Some(NormalCommand::JumpBack),
        KeyCode::Char('i') if key.ctrl() => Some(NormalCommand::JumpForward),
        KeyCode::Char('%') if text_input => Some(NormalCommand::MatchBracket),
        KeyCode::Char('/') if text_input => Some(NormalCommand::Search),
        KeyCode::Char('a') if text_input => Some(NormalCommand::Append),
        KeyCode::Char('A') if text_input => Some(NormalCommand::AppendLineEnd),
        KeyCode::Char('o') if text_input => Some(NormalCommand::OpenBelow),
        KeyCode::Char('O') if text_input => Some(NormalCommand::OpenAbove),
        KeyCode::Char('n') if text_input => Some(NormalCommand::SearchNext(true)),
        KeyCode::Char('N') if text_input => Some(NormalCommand::SearchNext(false)),
        KeyCode::Char('d') if text_input => Some(NormalCommand::Pending('d')),
        KeyCode::Char('y') if text_input => Some(NormalCommand::Pending('y')),
        KeyCode::Char('c') if text_input => Some(NormalCommand::Pending('c')),
        KeyCode::Char('"') if text_input => Some(NormalCommand::RegisterWait),
        KeyCode::Char('x') if text_input => Some(NormalCommand::DeleteForward),
        KeyCode::Char('p') if text_input => Some(NormalCommand::Put),
        KeyCode::Char('u') if text_input => Some(NormalCommand::Undo),
        KeyCode::Char('r') if key.ctrl() => Some(NormalCommand::Redo),
        KeyCode::Char('.') if text_input => Some(NormalCommand::RepeatChange),
        KeyCode::Char(character) if key.alt() && character.is_ascii_digit() => {
            let digit = character as u8 - b'0';
            (digit > 0).then_some(NormalCommand::SelectBuffer((digit - 1) as usize))
        }
        KeyCode::Char('0') if text_input && count == 0 => Some(NormalCommand::MoveHome),
        KeyCode::Char(character) if text_input && character.is_ascii_digit() => {
            Some(NormalCommand::Count(character))
        }
        KeyCode::Char('w') if key.ctrl() => Some(NormalCommand::Pending('w')),
        KeyCode::Char('w') if text_input => Some(NormalCommand::MoveWords(WordMotion::NextStart)),
        KeyCode::Char('b') if text_input => {
            Some(NormalCommand::MoveWords(WordMotion::PreviousStart))
        }
        KeyCode::Char('e') if text_input => Some(NormalCommand::MoveWords(WordMotion::End)),
        KeyCode::Char('h') | KeyCode::Left => Some(NormalCommand::MoveChars(-1)),
        KeyCode::Char('l') | KeyCode::Right => Some(NormalCommand::MoveChars(1)),
        KeyCode::Char('j') | KeyCode::Down if !key.alt() => Some(NormalCommand::MoveLine(1)),
        KeyCode::Char('k') | KeyCode::Up if !key.alt() => Some(NormalCommand::MoveLine(-1)),
        KeyCode::Home => Some(NormalCommand::MoveHome),
        KeyCode::End => Some(NormalCommand::MoveEnd),
        KeyCode::Enter => Some(NormalCommand::OpenLocation),
        KeyCode::Char(_) if key.alt() && key.command_char() == Some('j') => {
            Some(NormalCommand::LocationNext)
        }
        KeyCode::Char(_) if key.alt() && key.command_char() == Some('k') => {
            Some(NormalCommand::LocationPrevious)
        }
        _ => None,
    }
}

pub(crate) fn executable_command(id: &str) -> bool {
    NormalCommand::from_id(id).is_some()
        || PendingCommand::from_id(id).is_some_and(PendingCommand::is_direct)
}

pub(crate) fn resolve_pending(
    prefix: char,
    key: KeyPress,
    object_pending: bool,
    config: &KeymapConfig,
) -> Option<PendingCommand> {
    if !object_pending
        && let Some(command) = config
            .pending
            .get(&(prefix, KeyChord::from_key(key)))
            .copied()
    {
        return Some(command);
    }
    pending_action(prefix, key, object_pending)
}
pub(crate) fn resolve_normal(
    key: KeyPress,
    count: usize,
    config: &KeymapConfig,
) -> Option<NormalCommand> {
    config.resolve(key).or_else(|| {
        normal_action(key, count).filter(|command| {
            command
                .id()
                .is_none_or(|id| is_registered(id, CommandContext::Normal))
        })
    })
}

pub(crate) fn resolve_context_binding(
    contexts: &[KeymapContext],
    key: KeyPress,
    config: &KeymapConfig,
) -> Option<(KeymapContext, ContextAction)> {
    contexts.iter().rev().find_map(|context| {
        config
            .resolve_context_exact(*context, key)
            .map(|action| (*context, action))
    })
}

#[cfg(test)]
pub(crate) fn resolve_normal_in_context(
    context: KeymapContext,
    key: KeyPress,
    count: usize,
    config: &KeymapConfig,
) -> Option<ContextAction> {
    config
        .resolve_context(context, key)
        .or_else(|| resolve_normal(key, count, config).map(ContextAction::Normal))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingCommand {
    ObjectPending,
    OperatorWord {
        operator: char,
        around: bool,
    },
    OperatorQuote {
        operator: char,
        quote: char,
        around: bool,
    },
    OperatorBracket {
        operator: char,
        open: char,
        around: bool,
    },
    OperatorParagraph {
        operator: char,
        around: bool,
    },
    OperatorTag {
        operator: char,
        around: bool,
    },
    ChangeInnerWord,
    ChangeInnerQuote(char),
    Definition,
    References,
    DocumentHighlight,
    IncomingCalls,
    OutgoingCalls,
    ChangeBack,
    ChangeForward,
    ToggleFold,
    CloseAllFolds,
    OpenAllFolds,
    Diagnostic(bool),
    Count {
        pending: char,
        digit: char,
    },
    SplitVertical,
    FocusSplit(bool),
    FocusEditor,
    CloseSplit,
    OpenTree,
    StartFuzzy,
    BufferPicker,
    CommandPalette,
    StartRename,
    CodeActions,
    DocumentSymbols,
    WorkspaceSymbols,
    Formatting,
    ChangeWord(WordMotion),
    DeleteLines,
    YankLines,
    DeleteWords(WordMotion),
    YankWords(WordMotion),
    FileStart,
    CenterCursor,
}

impl PendingCommand {
    pub(crate) fn id(self) -> Option<CommandId> {
        use PendingCommand::*;
        match self {
            ChangeInnerWord => Some(command::CHANGE_INNER_WORD),
            ChangeInnerQuote(_) => Some(command::CHANGE_INNER_QUOTE),
            Definition => Some(command::DEFINITION),
            References => Some(command::REFERENCES),
            DocumentHighlight => Some(command::DOCUMENT_HIGHLIGHT),
            IncomingCalls => Some(command::INCOMING_CALLS),
            OutgoingCalls => Some(command::OUTGOING_CALLS),
            ChangeBack => Some(command::CHANGE_BACK),
            ChangeForward => Some(command::CHANGE_FORWARD),
            ToggleFold => Some(command::TOGGLE_FOLD),
            CloseAllFolds => Some(command::CLOSE_ALL_FOLDS),
            OpenAllFolds => Some(command::OPEN_ALL_FOLDS),
            Diagnostic(true) => Some(command::DIAGNOSTICS_NEXT),
            Diagnostic(false) => Some(command::DIAGNOSTICS_PREVIOUS),
            SplitVertical => Some(command::SPLIT_VERTICAL),
            FocusSplit(true) => Some(command::FOCUS_RIGHT),
            FocusEditor => Some(command::FOCUS_EDITOR),
            FocusSplit(false) => Some(command::FOCUS_LEFT),
            CloseSplit => Some(command::CLOSE_SPLIT),
            OpenTree => Some(command::TREE_OPEN),
            StartFuzzy => Some(command::FUZZY),
            CommandPalette => Some(command::COMMAND_PALETTE),
            StartRename => Some(command::RENAME_SYMBOL),
            CodeActions => Some(command::CODE_ACTION),
            DocumentSymbols => Some(command::DOCUMENT_SYMBOLS),
            WorkspaceSymbols => Some(command::WORKSPACE_SYMBOLS),
            Formatting => Some(command::FORMAT),
            ChangeWord(WordMotion::NextStart) => Some(command::CHANGE_WORD_NEXT),
            ChangeWord(WordMotion::End) => Some(command::CHANGE_WORD_END),
            ChangeWord(WordMotion::PreviousStart) => Some(command::CHANGE_WORD_PREVIOUS),
            DeleteLines => Some(command::DELETE_LINES),
            YankLines => Some(command::YANK_LINES),
            DeleteWords(WordMotion::NextStart) => Some(command::DELETE_WORDS_NEXT),
            DeleteWords(WordMotion::End) => Some(command::DELETE_WORDS_END),
            DeleteWords(WordMotion::PreviousStart) => Some(command::DELETE_WORDS_PREVIOUS),
            YankWords(WordMotion::NextStart) => Some(command::YANK_WORDS_NEXT),
            YankWords(WordMotion::End) => Some(command::YANK_WORDS_END),
            YankWords(WordMotion::PreviousStart) => Some(command::YANK_WORDS_PREVIOUS),
            ObjectPending
            | OperatorWord { .. }
            | OperatorQuote { .. }
            | OperatorBracket { .. }
            | OperatorParagraph { .. }
            | OperatorTag { .. }
            | Count { .. }
            | BufferPicker
            | FileStart
            | CenterCursor => None,
        }
    }

    pub(crate) fn label(self) -> String {
        if let Some(id) = self.id() {
            return id.as_str().to_owned();
        }
        let operator_name = |operator: char| match operator {
            'd' => "delete",
            'c' => "change",
            'y' => "yank",
            _ => "operate",
        };
        let around = |around: bool| if around { "around" } else { "inner" };
        match self {
            Self::ObjectPending => "text object".to_owned(),
            Self::OperatorWord {
                operator,
                around: is_around,
            } => {
                format!("{} {} word", operator_name(operator), around(is_around))
            }
            Self::OperatorQuote {
                operator,
                quote,
                around: is_around,
            } => {
                format!(
                    "{} {} {quote} quote",
                    operator_name(operator),
                    around(is_around)
                )
            }
            Self::OperatorBracket {
                operator,
                open,
                around: is_around,
            } => {
                format!(
                    "{} {} {open} block",
                    operator_name(operator),
                    around(is_around)
                )
            }
            Self::OperatorParagraph {
                operator,
                around: is_around,
            } => {
                format!(
                    "{} {} paragraph",
                    operator_name(operator),
                    around(is_around)
                )
            }
            Self::OperatorTag {
                operator,
                around: is_around,
            } => {
                format!("{} {} tag", operator_name(operator), around(is_around))
            }
            Self::Count { .. } => "count".to_owned(),
            Self::FileStart => "file start".to_owned(),
            Self::CenterCursor => "center cursor".to_owned(),
            _ => "command".to_owned(),
        }
    }

    pub(crate) fn is_direct(self) -> bool {
        matches!(
            self,
            Self::ChangeInnerWord
                | Self::ChangeInnerQuote(_)
                | Self::Definition
                | Self::Diagnostic(_)
                | Self::SplitVertical
                | Self::FocusSplit(_)
                | Self::FocusEditor
                | Self::CloseSplit
                | Self::OpenTree
                | Self::StartFuzzy
                | Self::StartRename
                | Self::References
                | Self::DocumentHighlight
                | Self::IncomingCalls
                | Self::OutgoingCalls
                | Self::ChangeBack
                | Self::ChangeForward
                | Self::ToggleFold
                | Self::CloseAllFolds
                | Self::OpenAllFolds
                | Self::CodeActions
                | Self::DocumentSymbols
                | Self::WorkspaceSymbols
                | Self::Formatting
                | Self::ChangeWord(_)
                | Self::DeleteLines
                | Self::YankLines
                | Self::DeleteWords(_)
                | Self::YankWords(_)
        )
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "editor.change_inner_word" => Self::ChangeInnerWord,
            "editor.change_inner_quote" => Self::ChangeInnerQuote('"'),
            "lsp.definition" => Self::Definition,
            "lsp.references" => Self::References,
            "lsp.document_highlight" => Self::DocumentHighlight,
            "lsp.incoming_calls" => Self::IncomingCalls,
            "lsp.outgoing_calls" => Self::OutgoingCalls,
            "navigation.change_back" => Self::ChangeBack,
            "navigation.change_forward" => Self::ChangeForward,
            "view.toggle_fold" => Self::ToggleFold,
            "view.close_all_folds" => Self::CloseAllFolds,
            "view.open_all_folds" => Self::OpenAllFolds,
            "diagnostics.next" => Self::Diagnostic(true),
            "diagnostics.previous" => Self::Diagnostic(false),
            "view.split_vertical" => Self::SplitVertical,
            "view.focus_left" => Self::FocusSplit(false),
            "view.focus_right" => Self::FocusSplit(true),
            "view.focus_editor" => Self::FocusEditor,
            "view.close_split" => Self::CloseSplit,
            "tree.open" => Self::OpenTree,
            "picker.fuzzy" => Self::StartFuzzy,
            "command_palette.open" => Self::CommandPalette,
            "editor.rename_symbol" => Self::StartRename,
            "lsp.code_action" => Self::CodeActions,
            "lsp.document_symbols" => Self::DocumentSymbols,
            "lsp.workspace_symbols" => Self::WorkspaceSymbols,
            "lsp.format" => Self::Formatting,
            "editor.change_word_next" => Self::ChangeWord(WordMotion::NextStart),
            "editor.change_word_end" => Self::ChangeWord(WordMotion::End),
            "editor.change_word_previous" => Self::ChangeWord(WordMotion::PreviousStart),
            "editor.delete_lines" => Self::DeleteLines,
            "editor.yank_lines" => Self::YankLines,
            "editor.delete_words_next" => Self::DeleteWords(WordMotion::NextStart),
            "editor.delete_words_end" => Self::DeleteWords(WordMotion::End),
            "editor.delete_words_previous" => Self::DeleteWords(WordMotion::PreviousStart),
            "editor.yank_words_next" => Self::YankWords(WordMotion::NextStart),
            "editor.yank_words_end" => Self::YankWords(WordMotion::End),
            "editor.yank_words_previous" => Self::YankWords(WordMotion::PreviousStart),
            _ => return None,
        })
    }
}

pub(crate) fn pending_action(
    pending: char,
    key: KeyPress,
    object_pending: bool,
) -> Option<PendingCommand> {
    if object_pending {
        let operator = pending.to_ascii_lowercase();
        if !matches!(operator, 'd' | 'y' | 'c') {
            return None;
        }
        return match key.code {
            KeyCode::Char('w') => Some(PendingCommand::OperatorWord {
                operator,
                around: pending.is_ascii_uppercase(),
            }),
            KeyCode::Char('"') => Some(PendingCommand::OperatorQuote {
                operator,
                quote: '"',
                around: pending.is_ascii_uppercase(),
            }),
            KeyCode::Char('\'') => Some(PendingCommand::OperatorQuote {
                operator,
                quote: '\'',
                around: pending.is_ascii_uppercase(),
            }),
            KeyCode::Char('(') | KeyCode::Char(')') => Some(PendingCommand::OperatorBracket {
                operator,
                open: '(',
                around: pending.is_ascii_uppercase(),
            }),
            KeyCode::Char('[') | KeyCode::Char(']') => Some(PendingCommand::OperatorBracket {
                operator,
                open: '[',
                around: pending.is_ascii_uppercase(),
            }),
            KeyCode::Char('{') | KeyCode::Char('}') => Some(PendingCommand::OperatorBracket {
                operator,
                open: '{',
                around: pending.is_ascii_uppercase(),
            }),
            KeyCode::Char('<') | KeyCode::Char('>') => Some(PendingCommand::OperatorBracket {
                operator,
                open: '<',
                around: pending.is_ascii_uppercase(),
            }),
            KeyCode::Char('p') => Some(PendingCommand::OperatorParagraph {
                operator,
                around: pending.is_ascii_uppercase(),
            }),
            KeyCode::Char('t') => Some(PendingCommand::OperatorTag {
                operator,
                around: pending.is_ascii_uppercase(),
            }),
            _ => None,
        };
    }
    if matches!(pending, 'd' | 'y' | 'c')
        && matches!(key.code, KeyCode::Char('i') | KeyCode::Char('a'))
    {
        return Some(PendingCommand::ObjectPending);
    }
    if pending == 'g' {
        return match key.code {
            KeyCode::Char('d') => Some(PendingCommand::Definition),
            KeyCode::Char('r') => Some(PendingCommand::References),
            KeyCode::Char('h') => Some(PendingCommand::DocumentHighlight),
            KeyCode::Char('i') => Some(PendingCommand::IncomingCalls),
            KeyCode::Char('o') => Some(PendingCommand::OutgoingCalls),
            KeyCode::Char(';') => Some(PendingCommand::ChangeBack),
            KeyCode::Char(',') => Some(PendingCommand::ChangeForward),
            KeyCode::Char('g') => Some(PendingCommand::FileStart),
            _ => None,
        };
    }
    if pending == 'z' {
        return match key.code {
            KeyCode::Char('a') => Some(PendingCommand::ToggleFold),
            KeyCode::Char('M') => Some(PendingCommand::CloseAllFolds),
            KeyCode::Char('R') => Some(PendingCommand::OpenAllFolds),
            KeyCode::Char('z') => Some(PendingCommand::CenterCursor),
            _ => None,
        };
    }
    if pending == ']' || pending == '[' {
        return (key.code == KeyCode::Char('d'))
            .then_some(PendingCommand::Diagnostic(pending == ']'));
    }
    if let KeyCode::Char(character) = key.code
        && character.is_ascii_digit()
        && !(pending == ' ' && matches!(character, '1' | '2' | '3'))
    {
        return Some(PendingCommand::Count {
            pending,
            digit: character,
        });
    }
    match (pending, key.code) {
        ('w', KeyCode::Char('v')) => Some(PendingCommand::SplitVertical),
        ('w', KeyCode::Char('h')) => Some(PendingCommand::FocusSplit(false)),
        (' ', KeyCode::Char('1')) => Some(PendingCommand::FocusEditor),
        (' ', KeyCode::Char('2')) => Some(PendingCommand::OpenTree),
        ('w', KeyCode::Char('l')) => Some(PendingCommand::FocusSplit(true)),
        ('w', KeyCode::Char('q')) => Some(PendingCommand::CloseSplit),
        (' ', KeyCode::Char('e')) => Some(PendingCommand::OpenTree),
        (' ', KeyCode::Char('f')) => Some(PendingCommand::StartFuzzy),
        (' ', KeyCode::Char('p')) => Some(PendingCommand::CommandPalette),
        (' ', KeyCode::Char('r')) => Some(PendingCommand::StartRename),
        (' ', KeyCode::Char('a')) => Some(PendingCommand::CodeActions),
        (' ', KeyCode::Char('s')) => Some(PendingCommand::DocumentSymbols),
        (' ', KeyCode::Char('b')) => Some(PendingCommand::BufferPicker),
        (' ', KeyCode::Char('S')) => Some(PendingCommand::WorkspaceSymbols),
        (' ', KeyCode::Char('F')) => Some(PendingCommand::Formatting),
        ('c', KeyCode::Char('w')) => Some(PendingCommand::ChangeWord(WordMotion::NextStart)),
        ('c', KeyCode::Char('e')) => Some(PendingCommand::ChangeWord(WordMotion::End)),
        ('c', KeyCode::Char('b')) => Some(PendingCommand::ChangeWord(WordMotion::PreviousStart)),
        ('d', KeyCode::Char('d')) => Some(PendingCommand::DeleteLines),
        ('y', KeyCode::Char('y')) => Some(PendingCommand::YankLines),
        ('d', KeyCode::Char('w')) => Some(PendingCommand::DeleteWords(WordMotion::NextStart)),
        ('d', KeyCode::Char('e')) => Some(PendingCommand::DeleteWords(WordMotion::End)),
        ('d', KeyCode::Char('b')) => Some(PendingCommand::DeleteWords(WordMotion::PreviousStart)),
        ('y', KeyCode::Char('w')) => Some(PendingCommand::YankWords(WordMotion::NextStart)),
        ('y', KeyCode::Char('e')) => Some(PendingCommand::YankWords(WordMotion::End)),
        ('y', KeyCode::Char('b')) => Some(PendingCommand::YankWords(WordMotion::PreviousStart)),
        _ => None,
    }
}

impl Model {
    pub(crate) fn active_keymap_contexts(&self) -> Vec<KeymapContext> {
        let mut contexts = vec![KeymapContext::Global];
        contexts.push(if self.ui.focus.tree {
            KeymapContext::FileTree
        } else {
            KeymapContext::Editor
        });
        let input = match &self.ui.input_mode {
            InputMode::CommandLine { .. } => Some(KeymapContext::CommandLine),
            InputMode::Search { .. } => Some(KeymapContext::Search),
            InputMode::Rename { .. } => Some(KeymapContext::Rename),
            InputMode::Create { .. }
            | InputMode::RenameEntry { .. }
            | InputMode::ConfirmDelete { .. }
            | InputMode::ConfirmClose { .. }
            | InputMode::ConfirmReload { .. }
            | InputMode::TransferEntry { .. } => Some(KeymapContext::Popup),
            InputMode::Normal | InputMode::Insert => None,
        };
        if let Some(context) = input
            && contexts.last() != Some(&context)
        {
            contexts.push(context);
        }
        let overlay = self.overlays.command_palette.is_some()
            || self.overlays.fuzzy.is_some()
            || self.overlays.buffer_picker.is_some()
            || self.overlays.lsp_results.workspace_symbols.is_some()
            || self.overlays.lsp_results.document_symbols.is_some()
            || self.overlays.lsp_results.references.is_some()
            || self.overlays.lsp_results.call_hierarchy.is_some()
            || self.overlays.settings_overlay.is_some()
            || self.overlays.lsp_results.code_actions.is_some()
            || self.overlays.lsp_results.completion.is_some()
            || self.overlays.lsp_results.hover.is_some()
            || self.ui.diagnostic_peek.is_some()
            || self.ui.problems.visible
            || self.pager.back.is_some();
        if overlay && contexts.last() != Some(&KeymapContext::Popup) {
            contexts.push(KeymapContext::Popup);
        }
        if self.edit.visual_anchor.is_some() {
            contexts.push(KeymapContext::Modal);
        }
        contexts
    }

    pub(crate) fn keymap_context(&self) -> KeymapContext {
        self.active_keymap_contexts()
            .last()
            .copied()
            .unwrap_or(KeymapContext::Global)
    }

    pub(crate) fn pending_hints(&self) -> Vec<(String, String)> {
        self.input
            .pending
            .map(|prefix| {
                self.config
                    .keymap
                    .pending_actions(prefix, self.input.object_pending)
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
