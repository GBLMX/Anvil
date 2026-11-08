//! Stable command identifiers and their input contexts.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CommandId(&'static str);

impl CommandId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandContext {
    Normal,
    Pending,
}

#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    pub id: CommandId,
    pub contexts: &'static [CommandContext],
}

const NORMAL: &[CommandContext] = &[CommandContext::Normal];
const PENDING: &[CommandContext] = &[CommandContext::Pending];

pub const COLLAPSE_SELECTIONS: CommandId = CommandId::new("editor.collapse_selections");
pub const HOVER: CommandId = CommandId::new("lsp.hover");
pub const MACRO_RECORD: CommandId = CommandId::new("macro.record");
pub const MACRO_REPLAY: CommandId = CommandId::new("macro.replay");
pub const SPLIT_LINES: CommandId = CommandId::new("selection.split_lines");
pub const WORD_OCCURRENCES: CommandId = CommandId::new("selection.word_occurrences");
pub const EXTEND_UP: CommandId = CommandId::new("selection.extend_up");
pub const EXTEND_DOWN: CommandId = CommandId::new("selection.extend_down");
pub const INSERT_HERE: CommandId = CommandId::new("editor.insert_here");
pub const COMMAND_LINE: CommandId = CommandId::new("command_line.open");
pub const SEARCH_OPEN: CommandId = CommandId::new("search.open");
pub const APPEND: CommandId = CommandId::new("editor.append");
pub const APPEND_LINE_END: CommandId = CommandId::new("editor.append_line_end");
pub const OPEN_BELOW: CommandId = CommandId::new("editor.open_below");
pub const OPEN_ABOVE: CommandId = CommandId::new("editor.open_above");
pub const SEARCH_NEXT: CommandId = CommandId::new("search.next");
pub const SEARCH_PREVIOUS: CommandId = CommandId::new("search.previous");
pub const DELETE_FORWARD: CommandId = CommandId::new("editor.delete_forward");
pub const PUT: CommandId = CommandId::new("editor.put");
pub const UNDO: CommandId = CommandId::new("editor.undo");
pub const REDO: CommandId = CommandId::new("editor.redo");
pub const REPEAT_CHANGE: CommandId = CommandId::new("editor.repeat_change");
pub const LINE_START: CommandId = CommandId::new("cursor.line_start");
pub const LINE_END: CommandId = CommandId::new("cursor.line_end");
pub const WORD_NEXT: CommandId = CommandId::new("cursor.word_next");
pub const WORD_PREVIOUS: CommandId = CommandId::new("cursor.word_previous");
pub const WORD_END: CommandId = CommandId::new("cursor.word_end");
pub const CURSOR_LEFT: CommandId = CommandId::new("cursor.left");
pub const CURSOR_RIGHT: CommandId = CommandId::new("cursor.right");
pub const CURSOR_DOWN: CommandId = CommandId::new("cursor.down");
pub const CURSOR_UP: CommandId = CommandId::new("cursor.up");

pub const CHANGE_INNER_WORD: CommandId = CommandId::new("editor.change_inner_word");
pub const CHANGE_INNER_QUOTE: CommandId = CommandId::new("editor.change_inner_quote");
pub const DEFINITION: CommandId = CommandId::new("lsp.definition");
pub const REFERENCES: CommandId = CommandId::new("lsp.references");
pub const DIAGNOSTICS_NEXT: CommandId = CommandId::new("diagnostics.next");
pub const DIAGNOSTICS_PREVIOUS: CommandId = CommandId::new("diagnostics.previous");
pub const SPLIT_VERTICAL: CommandId = CommandId::new("view.split_vertical");
pub const FOCUS_LEFT: CommandId = CommandId::new("view.focus_left");
pub const FOCUS_EDITOR: CommandId = CommandId::new("view.focus_editor");
pub const FOCUS_RIGHT: CommandId = CommandId::new("view.focus_right");
pub const CLOSE_SPLIT: CommandId = CommandId::new("view.close_split");
pub const TREE_OPEN: CommandId = CommandId::new("tree.open");
pub const FUZZY: CommandId = CommandId::new("picker.fuzzy");
pub const COMMAND_PALETTE: CommandId = CommandId::new("command_palette.open");
pub const RENAME_SYMBOL: CommandId = CommandId::new("editor.rename_symbol");
pub const CODE_ACTION: CommandId = CommandId::new("lsp.code_action");
pub const DOCUMENT_SYMBOLS: CommandId = CommandId::new("lsp.document_symbols");
pub const WORKSPACE_SYMBOLS: CommandId = CommandId::new("lsp.workspace_symbols");
pub const FORMAT: CommandId = CommandId::new("lsp.format");
pub const CHANGE_WORD_NEXT: CommandId = CommandId::new("editor.change_word_next");
pub const CHANGE_WORD_END: CommandId = CommandId::new("editor.change_word_end");
pub const CHANGE_WORD_PREVIOUS: CommandId = CommandId::new("editor.change_word_previous");
pub const DELETE_LINES: CommandId = CommandId::new("editor.delete_lines");
pub const YANK_LINES: CommandId = CommandId::new("editor.yank_lines");
pub const DELETE_WORDS_NEXT: CommandId = CommandId::new("editor.delete_words_next");
pub const DELETE_WORDS_END: CommandId = CommandId::new("editor.delete_words_end");
pub const DELETE_WORDS_PREVIOUS: CommandId = CommandId::new("editor.delete_words_previous");
pub const YANK_WORDS_NEXT: CommandId = CommandId::new("editor.yank_words_next");
pub const YANK_WORDS_END: CommandId = CommandId::new("editor.yank_words_end");
pub const YANK_WORDS_PREVIOUS: CommandId = CommandId::new("editor.yank_words_previous");

pub const COMMANDS: &[CommandSpec] = &[
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
    spec(REPEAT_CHANGE, NORMAL),
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
    spec(REFERENCES, PENDING),
    spec(DIAGNOSTICS_NEXT, PENDING),
    spec(DIAGNOSTICS_PREVIOUS, PENDING),
    spec(SPLIT_VERTICAL, PENDING),
    spec(FOCUS_LEFT, PENDING),
    spec(FOCUS_RIGHT, PENDING),
    spec(FOCUS_EDITOR, PENDING),
    spec(CLOSE_SPLIT, PENDING),
    spec(TREE_OPEN, PENDING),
    spec(FUZZY, PENDING),
    spec(COMMAND_PALETTE, PENDING),
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

pub fn is_registered(id: CommandId, context: CommandContext) -> bool {
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
