//! Stable command identifiers and their input contexts.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CommandId(&'static str);

impl CommandId {
    pub(crate) const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub(crate) const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandContext {
    Normal,
    Pending,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CommandSpec {
    pub(crate) id: CommandId,
    pub(crate) contexts: &'static [CommandContext],
}

const NORMAL: &[CommandContext] = &[CommandContext::Normal];
const PENDING: &[CommandContext] = &[CommandContext::Pending];

pub(crate) const COLLAPSE_SELECTIONS: CommandId = CommandId::new("editor.collapse_selections");
pub(crate) const HOVER: CommandId = CommandId::new("lsp.hover");
pub(crate) const MACRO_RECORD: CommandId = CommandId::new("macro.record");
pub(crate) const MACRO_REPLAY: CommandId = CommandId::new("macro.replay");
pub(crate) const SPLIT_LINES: CommandId = CommandId::new("selection.split_lines");
pub(crate) const WORD_OCCURRENCES: CommandId = CommandId::new("selection.word_occurrences");
pub(crate) const EXTEND_UP: CommandId = CommandId::new("selection.extend_up");
pub(crate) const EXTEND_DOWN: CommandId = CommandId::new("selection.extend_down");
pub(crate) const INSERT_HERE: CommandId = CommandId::new("editor.insert_here");
pub(crate) const COMMAND_LINE: CommandId = CommandId::new("command_line.open");
pub(crate) const SEARCH_OPEN: CommandId = CommandId::new("search.open");
pub(crate) const APPEND: CommandId = CommandId::new("editor.append");
pub(crate) const APPEND_LINE_END: CommandId = CommandId::new("editor.append_line_end");
pub(crate) const OPEN_BELOW: CommandId = CommandId::new("editor.open_below");
pub(crate) const OPEN_ABOVE: CommandId = CommandId::new("editor.open_above");
pub(crate) const SEARCH_NEXT: CommandId = CommandId::new("search.next");
pub(crate) const SEARCH_PREVIOUS: CommandId = CommandId::new("search.previous");
pub(crate) const DELETE_FORWARD: CommandId = CommandId::new("editor.delete_forward");
pub(crate) const PUT: CommandId = CommandId::new("editor.put");
pub(crate) const UNDO: CommandId = CommandId::new("editor.undo");
pub(crate) const REDO: CommandId = CommandId::new("editor.redo");
pub(crate) const LINE_START: CommandId = CommandId::new("cursor.line_start");
pub(crate) const LINE_END: CommandId = CommandId::new("cursor.line_end");
pub(crate) const WORD_NEXT: CommandId = CommandId::new("cursor.word_next");
pub(crate) const WORD_PREVIOUS: CommandId = CommandId::new("cursor.word_previous");
pub(crate) const WORD_END: CommandId = CommandId::new("cursor.word_end");
pub(crate) const CURSOR_LEFT: CommandId = CommandId::new("cursor.left");
pub(crate) const CURSOR_RIGHT: CommandId = CommandId::new("cursor.right");
pub(crate) const CURSOR_DOWN: CommandId = CommandId::new("cursor.down");
pub(crate) const CURSOR_UP: CommandId = CommandId::new("cursor.up");

pub(crate) const CHANGE_INNER_WORD: CommandId = CommandId::new("editor.change_inner_word");
pub(crate) const CHANGE_INNER_QUOTE: CommandId = CommandId::new("editor.change_inner_quote");
pub(crate) const DEFINITION: CommandId = CommandId::new("lsp.definition");
pub(crate) const DIAGNOSTICS_NEXT: CommandId = CommandId::new("diagnostics.next");
pub(crate) const DIAGNOSTICS_PREVIOUS: CommandId = CommandId::new("diagnostics.previous");
pub(crate) const SPLIT_VERTICAL: CommandId = CommandId::new("view.split_vertical");
pub(crate) const FOCUS_LEFT: CommandId = CommandId::new("view.focus_left");
pub(crate) const FOCUS_RIGHT: CommandId = CommandId::new("view.focus_right");
pub(crate) const CLOSE_SPLIT: CommandId = CommandId::new("view.close_split");
pub(crate) const TREE_OPEN: CommandId = CommandId::new("tree.open");
pub(crate) const FUZZY: CommandId = CommandId::new("picker.fuzzy");
pub(crate) const RENAME_SYMBOL: CommandId = CommandId::new("editor.rename_symbol");
pub(crate) const CODE_ACTION: CommandId = CommandId::new("lsp.code_action");
pub(crate) const DOCUMENT_SYMBOLS: CommandId = CommandId::new("lsp.document_symbols");
pub(crate) const WORKSPACE_SYMBOLS: CommandId = CommandId::new("lsp.workspace_symbols");
pub(crate) const FORMAT: CommandId = CommandId::new("lsp.format");
pub(crate) const CHANGE_WORD_NEXT: CommandId = CommandId::new("editor.change_word_next");
pub(crate) const CHANGE_WORD_END: CommandId = CommandId::new("editor.change_word_end");
pub(crate) const CHANGE_WORD_PREVIOUS: CommandId = CommandId::new("editor.change_word_previous");
pub(crate) const DELETE_LINES: CommandId = CommandId::new("editor.delete_lines");
pub(crate) const YANK_LINES: CommandId = CommandId::new("editor.yank_lines");
pub(crate) const DELETE_WORDS_NEXT: CommandId = CommandId::new("editor.delete_words_next");
pub(crate) const DELETE_WORDS_END: CommandId = CommandId::new("editor.delete_words_end");
pub(crate) const DELETE_WORDS_PREVIOUS: CommandId = CommandId::new("editor.delete_words_previous");
pub(crate) const YANK_WORDS_NEXT: CommandId = CommandId::new("editor.yank_words_next");
pub(crate) const YANK_WORDS_END: CommandId = CommandId::new("editor.yank_words_end");
pub(crate) const YANK_WORDS_PREVIOUS: CommandId = CommandId::new("editor.yank_words_previous");

pub(crate) const COMMANDS: &[CommandSpec] = &[
    spec(COLLAPSE_SELECTIONS, NORMAL),
    spec(HOVER, NORMAL),
    spec(MACRO_RECORD, NORMAL),
    spec(MACRO_REPLAY, NORMAL),
    spec(SPLIT_LINES, NORMAL),
    spec(WORD_OCCURRENCES, NORMAL),
    spec(EXTEND_UP, NORMAL),
    spec(EXTEND_DOWN, NORMAL),
    spec(INSERT_HERE, NORMAL),
    spec(COMMAND_LINE, NORMAL),
    spec(SEARCH_OPEN, NORMAL),
    spec(APPEND, NORMAL),
    spec(APPEND_LINE_END, NORMAL),
    spec(OPEN_BELOW, NORMAL),
    spec(OPEN_ABOVE, NORMAL),
    spec(SEARCH_NEXT, NORMAL),
    spec(SEARCH_PREVIOUS, NORMAL),
    spec(DELETE_FORWARD, NORMAL),
    spec(PUT, NORMAL),
    spec(UNDO, NORMAL),
    spec(REDO, NORMAL),
    spec(LINE_START, NORMAL),
    spec(LINE_END, NORMAL),
    spec(WORD_NEXT, NORMAL),
    spec(WORD_PREVIOUS, NORMAL),
    spec(WORD_END, NORMAL),
    spec(CURSOR_LEFT, NORMAL),
    spec(CURSOR_RIGHT, NORMAL),
    spec(CURSOR_DOWN, NORMAL),
    spec(CURSOR_UP, NORMAL),
    spec(CHANGE_INNER_WORD, PENDING),
    spec(CHANGE_INNER_QUOTE, PENDING),
    spec(DEFINITION, PENDING),
    spec(DIAGNOSTICS_NEXT, PENDING),
    spec(DIAGNOSTICS_PREVIOUS, PENDING),
    spec(SPLIT_VERTICAL, PENDING),
    spec(FOCUS_LEFT, PENDING),
    spec(FOCUS_RIGHT, PENDING),
    spec(CLOSE_SPLIT, PENDING),
    spec(TREE_OPEN, PENDING),
    spec(FUZZY, PENDING),
    spec(RENAME_SYMBOL, PENDING),
    spec(CODE_ACTION, PENDING),
    spec(DOCUMENT_SYMBOLS, PENDING),
    spec(WORKSPACE_SYMBOLS, PENDING),
    spec(FORMAT, PENDING),
    spec(CHANGE_WORD_NEXT, PENDING),
    spec(CHANGE_WORD_END, PENDING),
    spec(CHANGE_WORD_PREVIOUS, PENDING),
    spec(DELETE_LINES, PENDING),
    spec(YANK_LINES, PENDING),
    spec(DELETE_WORDS_NEXT, PENDING),
    spec(DELETE_WORDS_END, PENDING),
    spec(DELETE_WORDS_PREVIOUS, PENDING),
    spec(YANK_WORDS_NEXT, PENDING),
    spec(YANK_WORDS_END, PENDING),
    spec(YANK_WORDS_PREVIOUS, PENDING),
];

const fn spec(id: CommandId, contexts: &'static [CommandContext]) -> CommandSpec {
    CommandSpec { id, contexts }
}

pub(crate) fn is_registered(id: CommandId, context: CommandContext) -> bool {
    debug_assert!(!id.as_str().is_empty());
    COMMANDS
        .iter()
        .any(|spec| spec.id == id && spec.contexts.contains(&context))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for spec in COMMANDS {
            assert!(
                ids.insert(spec.id),
                "duplicate command id: {}",
                spec.id.as_str()
            );
        }
    }

    #[test]
    fn command_ids_are_namespaced() {
        for spec in COMMANDS {
            assert!(
                spec.id.as_str().contains('.'),
                "command id is not namespaced: {}",
                spec.id.as_str()
            );
        }
    }
}
