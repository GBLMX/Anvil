//! Keymap resolution for normal-mode bindings.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::WordMotion;
use crate::input::KeyPress;
use anvil_session::command::{self, CommandContext, CommandId, is_registered};
use anvil_session::keymap::KeymapContext;

pub(crate) const ANVIL_ERGO_PRESET: &str = include_str!("../presets/anvil-ergo.toml");
pub(crate) const ANVIL_VIM_PRESET: &str = include_str!("../presets/anvil-vim.toml");

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
    Count(char),
    MoveHome,
    MoveEnd,
    MoveWords(WordMotion),
    MoveChars(isize),
    MoveLine(isize),
    MoveFileEnd,
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
            Pending(_) | RegisterWait | Count(_) => None,
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
        KeyCode::Char('j') | KeyCode::Down => Some(NormalCommand::MoveLine(1)),
        KeyCode::Char('k') | KeyCode::Up => Some(NormalCommand::MoveLine(-1)),
        KeyCode::Home => Some(NormalCommand::MoveHome),
        KeyCode::End => Some(NormalCommand::MoveEnd),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct KeyChord {
    code: KeyCode,
    ctrl: bool,
    alt: bool,
    super_key: bool,
}

impl KeyChord {
    pub(crate) fn from_key(key: KeyPress) -> Self {
        let code = match key.code {
            KeyCode::Char(character) if key.alt() => KeyCode::Char(character.to_ascii_lowercase()),
            code => code,
        };
        Self {
            code,
            ctrl: key.ctrl(),
            alt: key.alt(),
            super_key: key.super_key(),
        }
    }

    pub(crate) fn parse(spec: &str) -> Result<Self, String> {
        let parts: Vec<&str> = spec.split('-').collect();
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(format!("empty key in {spec:?}"));
        }
        let (base, modifiers) = parts.split_last().expect("non-empty key parts");
        let (mut ctrl, mut alt, mut super_key, mut shift) = (false, false, false, false);
        for modifier in modifiers {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => {
                    if ctrl {
                        return Err(format!("duplicate ctrl modifier in {spec:?}"));
                    }
                    ctrl = true;
                }
                "alt" => {
                    if alt {
                        return Err(format!("duplicate alt modifier in {spec:?}"));
                    }
                    alt = true;
                }
                "super" => {
                    if super_key {
                        return Err(format!("duplicate super modifier in {spec:?}"));
                    }
                    super_key = true;
                }
                "shift" => {
                    if shift {
                        return Err(format!("duplicate shift modifier in {spec:?}"));
                    }
                    shift = true;
                }
                other => return Err(format!("unknown modifier {other:?} in {spec:?}")),
            }
        }
        let mut code = parse_key_name(base)?;
        if shift {
            code = match code {
                KeyCode::Char(character) if character.is_ascii_alphabetic() => {
                    KeyCode::Char(character.to_ascii_uppercase())
                }
                _ => return Err(format!("shift requires a letter in {spec:?}")),
            };
        }
        if alt && let KeyCode::Char(character) = code {
            code = KeyCode::Char(character.to_ascii_lowercase());
        }
        Ok(Self {
            code,
            ctrl,
            alt,
            super_key,
        })
    }

    pub(crate) fn label(self) -> String {
        let mut label = String::new();
        if self.ctrl {
            label.push_str("ctrl-");
        }
        if self.alt {
            label.push_str("alt-");
        }
        if self.super_key {
            label.push_str("super-");
        }
        label.push_str(&match self.code {
            KeyCode::Char(' ') => "space".to_owned(),
            KeyCode::Char(character) => character.to_string(),
            KeyCode::Esc => "esc".to_owned(),
            KeyCode::Tab => "tab".to_owned(),
            KeyCode::Enter => "enter".to_owned(),
            KeyCode::Backspace => "backspace".to_owned(),
            KeyCode::Delete => "delete".to_owned(),
            KeyCode::Insert => "insert".to_owned(),
            KeyCode::Left => "left".to_owned(),
            KeyCode::Right => "right".to_owned(),
            KeyCode::Up => "up".to_owned(),
            KeyCode::Down => "down".to_owned(),
            KeyCode::Home => "home".to_owned(),
            KeyCode::End => "end".to_owned(),
            KeyCode::PageUp => "pageup".to_owned(),
            KeyCode::PageDown => "pagedown".to_owned(),
            KeyCode::F(number) => format!("f{number}"),
            _ => format!("{:?}", self.code).to_ascii_lowercase(),
        });
        label
    }

    pub(crate) fn default_path() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os("ANVIL_KEYMAP") {
            return Some(PathBuf::from(path));
        }
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(config).join("anvil/keymap.toml"));
        }
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/anvil/keymap.toml"))
    }
}

fn parse_key_name(name: &str) -> Result<KeyCode, String> {
    for number in 1..=12 {
        if name.eq_ignore_ascii_case(&format!("f{number}")) {
            return Ok(KeyCode::F(number));
        }
    }
    let code = match name {
        "space" => KeyCode::Char(' '),
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "enter" | "return" => KeyCode::Enter,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "page_up" => KeyCode::PageUp,
        "pagedown" | "page_down" => KeyCode::PageDown,
        _ => {
            let mut characters = name.chars();
            let Some(character) = characters.next() else {
                return Err(format!("empty key name {name:?}"));
            };
            if characters.next().is_some() {
                return Err(format!("unknown key name {name:?}"));
            }
            KeyCode::Char(character)
        }
    };
    Ok(code)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapFile {
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    keymap: Option<KeymapSections>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapSections {
    #[serde(default)]
    normal: HashMap<String, String>,
    #[serde(default)]
    pending: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    contexts: HashMap<String, HashMap<String, String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContextAction {
    Normal(NormalCommand),
    Pending(PendingCommand),
}

impl ContextAction {
    fn from_id(id: &str) -> Option<Self> {
        NormalCommand::from_id(id).map(Self::Normal).or_else(|| {
            PendingCommand::from_id(id)
                .filter(|command| command.is_direct())
                .map(Self::Pending)
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct KeymapConfig {
    normal: HashMap<KeyChord, NormalCommand>,
    pending: HashMap<(char, KeyChord), PendingCommand>,
    contexts: HashMap<KeymapContext, HashMap<KeyChord, ContextAction>>,
}

impl KeymapConfig {
    pub(crate) const MAX_BYTES: u64 = 64 * 1024;

    pub(crate) fn from_path(path: &Path) -> Result<Self, String> {
        let metadata = fs::metadata(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if metadata.len() > Self::MAX_BYTES {
            return Err(format!(
                "{} exceeds {} bytes",
                path.display(),
                Self::MAX_BYTES
            ));
        }
        let text = fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        Self::from_toml(&text)
    }

    pub(crate) fn from_toml(text: &str) -> Result<Self, String> {
        let file: KeymapFile = toml_edit::de::from_str(text)
            .map_err(|error| format!("invalid keymap TOML: {error}"))?;
        let mut config = match file.preset.as_deref() {
            Some(name) => Self::from_preset(name)?,
            None => Self::default(),
        };
        let Some(sections) = file.keymap else {
            if file.preset.is_some() {
                return Ok(config);
            }
            return Err("missing [keymap] table".to_owned());
        };
        config.apply_sections(sections)?;
        Ok(config)
    }

    pub(crate) fn from_preset(name: &str) -> Result<Self, String> {
        let normalized = name.trim().to_ascii_lowercase();
        let text = match normalized.as_str() {
            "default" | "ergo" | "anvil-default" | "anvil-ergo" => ANVIL_ERGO_PRESET,
            "vim" | "anvil-vim" => ANVIL_VIM_PRESET,
            _ => {
                return Err(format!(
                    "unknown keymap preset {name:?}; available presets: ergo, vim"
                ));
            }
        };
        let file: KeymapFile = toml_edit::de::from_str(text)
            .map_err(|error| format!("invalid bundled preset {name:?}: {error}"))?;
        if file.preset.is_some() {
            return Err(format!(
                "bundled preset {name:?} must not select another preset"
            ));
        }
        let sections = file
            .keymap
            .ok_or_else(|| format!("bundled preset {name:?} has no [keymap] table"))?;
        let mut config = Self::default();
        config.apply_sections(sections)?;
        Ok(config)
    }

    fn apply_sections(&mut self, sections: KeymapSections) -> Result<(), String> {
        let mut seen_normal = HashSet::new();
        for (spec, value) in sections.normal {
            let chord =
                KeyChord::parse(&spec).map_err(|error| format!("keymap.normal.{spec}: {error}"))?;
            let command = NormalCommand::from_id(&value)
                .ok_or_else(|| format!("unknown normal command {value:?}"))?;
            if command.id().is_none() {
                return Err(format!(
                    "normal binding {spec:?} cannot use a state command"
                ));
            }
            if !seen_normal.insert(chord) {
                return Err(format!("duplicate normal binding {spec:?}"));
            }
            self.normal.insert(chord, command);
        }
        for (prefix_name, bindings) in sections.pending {
            let mut characters = prefix_name.chars();
            let Some(prefix) = characters.next() else {
                return Err("empty pending prefix".to_owned());
            };
            if characters.next().is_some() {
                return Err(format!(
                    "pending prefix must be one character: {prefix_name:?}"
                ));
            }
            let mut seen_pending = HashSet::new();
            for (spec, value) in bindings {
                let chord = KeyChord::parse(&spec)
                    .map_err(|error| format!("keymap.pending.{prefix_name}.{spec}: {error}"))?;
                let command = PendingCommand::from_id(&value)
                    .ok_or_else(|| format!("unknown pending command {value:?}"))?;
                if !seen_pending.insert(chord) {
                    return Err(format!(
                        "duplicate pending binding {prefix_name:?}.{spec:?}"
                    ));
                }
                self.pending.insert((prefix, chord), command);
            }
        }
        for (context_name, bindings) in sections.contexts {
            let context = KeymapContext::from_name(&context_name)
                .ok_or_else(|| format!("unknown keymap context {context_name:?}"))?;
            let mut seen_context = HashSet::new();
            for (spec, value) in bindings {
                let chord = KeyChord::parse(&spec)
                    .map_err(|error| format!("keymap.contexts.{context_name}.{spec}: {error}"))?;
                let action = ContextAction::from_id(&value).ok_or_else(|| {
                    format!("unknown or non-direct context command {value:?} for {context_name:?}")
                })?;
                if !seen_context.insert(chord) {
                    return Err(format!(
                        "duplicate context binding {context_name:?}.{spec:?}"
                    ));
                }
                self.contexts
                    .entry(context)
                    .or_default()
                    .insert(chord, action);
            }
        }
        Ok(())
    }

    #[allow(dead_code)] // Config files will call this before the loader lands.
    pub(crate) fn bind(&mut self, chord: KeyChord, command: NormalCommand) -> bool {
        let Some(id) = command.id() else {
            return false;
        };
        if !is_registered(id, CommandContext::Normal) || self.normal.contains_key(&chord) {
            return false;
        }
        self.normal.insert(chord, command);
        true
    }

    #[allow(dead_code)] // Config files will call this before the loader lands.
    pub(crate) fn bind_pending(
        &mut self,
        prefix: char,
        chord: KeyChord,
        command: PendingCommand,
    ) -> bool {
        let Some(id) = command.id() else {
            return false;
        };
        if !is_registered(id, CommandContext::Pending)
            || self.pending.contains_key(&(prefix, chord))
        {
            return false;
        }
        self.pending.insert((prefix, chord), command);
        true
    }

    pub(crate) fn pending_actions(
        &self,
        prefix: char,
        object_pending: bool,
    ) -> Vec<(String, String)> {
        let mut entries = HashMap::new();
        for byte in 32u8..=126 {
            let character = char::from(byte);
            let key =
                KeyPress::from_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
            if let Some(command) = pending_action(prefix, key, object_pending)
                && !matches!(command, PendingCommand::Count { .. })
            {
                entries.insert(character.to_string(), command.label());
            }
        }
        if !object_pending {
            for ((candidate_prefix, chord), command) in &self.pending {
                if *candidate_prefix == prefix {
                    entries.insert(chord.label(), command.label());
                }
            }
        }
        let mut actions = entries.into_iter().collect::<Vec<_>>();
        actions.sort_by(|left, right| left.0.cmp(&right.0));
        actions
    }
    fn resolve(&self, key: KeyPress) -> Option<NormalCommand> {
        self.normal.get(&KeyChord::from_key(key)).copied()
    }

    fn context_binding(&self, context: KeymapContext, key: KeyPress) -> Option<ContextAction> {
        let chord = KeyChord::from_key(key);
        self.contexts
            .get(&context)
            .and_then(|bindings| bindings.get(&chord))
            .copied()
    }

    pub(crate) fn resolve_context_exact(
        &self,
        context: KeymapContext,
        key: KeyPress,
    ) -> Option<ContextAction> {
        self.context_binding(context, key)
    }

    pub(crate) fn resolve_context(
        &self,
        context: KeymapContext,
        key: KeyPress,
    ) -> Option<ContextAction> {
        self.context_binding(context, key).or_else(|| {
            if context == KeymapContext::Global {
                None
            } else {
                self.context_binding(KeymapContext::Global, key)
            }
        })
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
            | Count { .. } => None,
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
            _ => None,
        };
    }
    if pending == 'z' {
        return match key.code {
            KeyCode::Char('a') => Some(PendingCommand::ToggleFold),
            KeyCode::Char('M') => Some(PendingCommand::CloseAllFolds),
            KeyCode::Char('R') => Some(PendingCommand::OpenAllFolds),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyPress {
        KeyPress::from_key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn alt_bindings_fold_case() {
        assert_eq!(
            normal_action(key(KeyCode::Char('C'), KeyModifiers::ALT), 0),
            Some(NormalCommand::ExtendLine(true))
        );
        assert_eq!(
            normal_action(key(KeyCode::Char('s'), KeyModifiers::ALT), 0),
            Some(NormalCommand::SplitSelection)
        );
    }

    #[test]
    fn zero_moves_home_only_without_a_count() {
        assert_eq!(
            normal_action(key(KeyCode::Char('0'), KeyModifiers::NONE), 0),
            Some(NormalCommand::MoveHome)
        );
        assert_eq!(
            normal_action(key(KeyCode::Char('0'), KeyModifiers::NONE), 2),
            Some(NormalCommand::Count('0'))
        );
    }

    #[test]
    fn shift_g_resolves_to_file_end() {
        assert_eq!(
            normal_action(key(KeyCode::Char('G'), KeyModifiers::SHIFT), 0),
            Some(NormalCommand::MoveFileEnd)
        );
    }

    #[test]
    fn modified_letters_do_not_trigger_plain_bindings() {
        assert_eq!(
            normal_action(key(KeyCode::Char('d'), KeyModifiers::ALT), 0),
            None
        );
    }

    #[test]
    fn pending_operators_resolve_chords() {
        assert_eq!(
            pending_action('d', key(KeyCode::Char('d'), KeyModifiers::NONE), false),
            Some(PendingCommand::DeleteLines)
        );
        assert_eq!(
            pending_action('c', key(KeyCode::Char('i'), KeyModifiers::NONE), false),
            Some(PendingCommand::ObjectPending)
        );
        assert_eq!(
            pending_action('c', key(KeyCode::Char('w'), KeyModifiers::NONE), true),
            Some(PendingCommand::OperatorWord {
                operator: 'c',
                around: false,
            })
        );
    }

    #[test]
    fn pending_g_resolves_navigation_commands() {
        assert_eq!(
            pending_action('g', key(KeyCode::Char('d'), KeyModifiers::NONE), false),
            Some(PendingCommand::Definition)
        );
        assert_eq!(
            pending_action('g', key(KeyCode::Char('r'), KeyModifiers::NONE), false),
            Some(PendingCommand::References)
        );
        assert_eq!(
            pending_action('g', key(KeyCode::Char('h'), KeyModifiers::NONE), false),
            Some(PendingCommand::DocumentHighlight)
        );
        assert_eq!(
            pending_action('g', key(KeyCode::Char('i'), KeyModifiers::NONE), false),
            Some(PendingCommand::IncomingCalls)
        );
        assert_eq!(
            pending_action('g', key(KeyCode::Char('o'), KeyModifiers::NONE), false),
            Some(PendingCommand::OutgoingCalls)
        );
        assert_eq!(
            pending_action('z', key(KeyCode::Char('a'), KeyModifiers::NONE), false),
            Some(PendingCommand::ToggleFold)
        );
        assert_eq!(
            pending_action('z', key(KeyCode::Char('M'), KeyModifiers::NONE), false),
            Some(PendingCommand::CloseAllFolds)
        );
        assert_eq!(
            pending_action('z', key(KeyCode::Char('R'), KeyModifiers::NONE), false),
            Some(PendingCommand::OpenAllFolds)
        );
    }

    #[test]
    fn pending_digits_preserve_the_prefix() {
        assert_eq!(
            pending_action('d', key(KeyCode::Char('2'), KeyModifiers::NONE), false),
            Some(PendingCommand::Count {
                pending: 'd',
                digit: '2',
            })
        );
    }

    #[test]
    fn config_overrides_normal_bindings() {
        let chord = KeyChord::from_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
        let mut config = KeymapConfig::default();
        assert!(config.bind(chord, NormalCommand::Undo));
        assert_eq!(
            resolve_normal(key(KeyCode::Char('x'), KeyModifiers::NONE), 0, &config),
            Some(NormalCommand::Undo)
        );
        assert_eq!(
            normal_action(key(KeyCode::Char('x'), KeyModifiers::NONE), 0),
            Some(NormalCommand::DeleteForward)
        );
    }

    #[test]
    fn config_rejects_state_only_commands() {
        let chord = KeyChord::from_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
        let mut config = KeymapConfig::default();
        assert!(!config.bind(chord, NormalCommand::Pending('d')));
        assert!(!config.bind(chord, NormalCommand::Count('1')));
    }

    #[test]
    fn config_overrides_pending_bindings() {
        let chord = KeyChord::from_key(key(KeyCode::Char('d'), KeyModifiers::NONE));
        let mut config = KeymapConfig::default();
        assert!(config.bind_pending('d', chord, PendingCommand::YankLines));
        assert_eq!(
            resolve_pending(
                'd',
                key(KeyCode::Char('d'), KeyModifiers::NONE),
                false,
                &config
            ),
            Some(PendingCommand::YankLines)
        );
        assert_eq!(
            pending_action('d', key(KeyCode::Char('d'), KeyModifiers::NONE), false),
            Some(PendingCommand::DeleteLines)
        );
    }

    #[test]
    fn config_rejects_pending_state_commands() {
        let chord = KeyChord::from_key(key(KeyCode::Char('i'), KeyModifiers::NONE));
        let mut config = KeymapConfig::default();
        assert!(!config.bind_pending('c', chord, PendingCommand::ObjectPending));
    }

    #[test]
    fn command_ids_are_registered() {
        assert_eq!(NormalCommand::Append.id(), Some(command::APPEND));
        assert!(is_registered(command::APPEND, CommandContext::Normal));
        assert_eq!(
            PendingCommand::DeleteLines.id(),
            Some(command::DELETE_LINES)
        );
        assert!(is_registered(
            command::DELETE_LINES,
            CommandContext::Pending
        ));
    }

    #[test]
    fn toml_loads_normal_and_pending_overrides() {
        let config = KeymapConfig::from_toml(
            r#"
[keymap.normal]
x = "editor.undo"
alt-c = "selection.extend_up"

[keymap.pending.d]
d = "editor.yank_lines"
"#,
        )
        .expect("valid keymap");
        assert_eq!(
            resolve_normal(key(KeyCode::Char('x'), KeyModifiers::NONE), 0, &config),
            Some(NormalCommand::Undo)
        );
        assert_eq!(
            resolve_normal(key(KeyCode::Char('C'), KeyModifiers::ALT), 0, &config),
            Some(NormalCommand::ExtendLine(true))
        );
        assert_eq!(
            resolve_pending(
                'd',
                key(KeyCode::Char('d'), KeyModifiers::NONE),
                false,
                &config
            ),
            Some(PendingCommand::YankLines)
        );
    }

    #[test]
    fn toml_rejects_unknown_commands_and_bad_keys() {
        assert!(
            KeymapConfig::from_toml(
                r#"
[keymap.normal]
x = "editor.not_a_command"
"#
            )
            .is_err()
        );
        assert!(
            KeymapConfig::from_toml(
                r#"
[keymap.normal]
"ctrl-" = "editor.undo"
"#
            )
            .is_err()
        );
    }

    #[test]
    fn toml_rejects_duplicate_chords() {
        let error = KeymapConfig::from_toml(
            r#"
[keymap.normal]
"ctrl-w" = "editor.undo"
"control-w" = "editor.redo"
"#,
        )
        .expect_err("duplicate chord");
        assert!(error.contains("duplicate"), "{error}");
    }

    #[test]
    fn toml_accepts_function_keys() {
        let config = KeymapConfig::from_toml(
            r#"
[keymap.normal]
F1 = "editor.undo"
f12 = "editor.redo"
"#,
        )
        .expect("function keys are configurable");
        assert_eq!(
            resolve_normal(key(KeyCode::F(1), KeyModifiers::NONE), 0, &config),
            Some(NormalCommand::Undo)
        );
        assert_eq!(
            resolve_normal(key(KeyCode::F(12), KeyModifiers::NONE), 0, &config),
            Some(NormalCommand::Redo)
        );
    }

    #[test]
    fn context_bindings_override_normal_bindings_and_global_falls_back() {
        let config = KeymapConfig::from_toml(
            r#"
[keymap.contexts.global]
"alt-u" = "editor.redo"

[keymap.contexts.editor]
"ctrl-w" = "view.split_vertical"
"alt-u" = "editor.undo"
"#,
        )
        .expect("valid contextual keymap");
        assert_eq!(
            resolve_normal_in_context(
                KeymapContext::Editor,
                key(KeyCode::Char('w'), KeyModifiers::CONTROL),
                0,
                &config
            ),
            Some(ContextAction::Pending(PendingCommand::SplitVertical))
        );
        assert_eq!(
            resolve_normal_in_context(
                KeymapContext::Editor,
                key(KeyCode::Char('u'), KeyModifiers::ALT),
                0,
                &config
            ),
            Some(ContextAction::Normal(NormalCommand::Undo))
        );
        assert_eq!(
            resolve_normal_in_context(
                KeymapContext::FileTree,
                key(KeyCode::Char('u'), KeyModifiers::ALT),
                0,
                &config
            ),
            Some(ContextAction::Normal(NormalCommand::Redo))
        );
    }

    #[test]
    fn context_schema_rejects_unknown_contexts_and_state_commands() {
        let unknown_context = KeymapConfig::from_toml(
            r#"
[keymap.contexts.unknown]
x = "editor.undo"
"#,
        )
        .expect_err("unknown context");
        assert!(
            unknown_context.contains("unknown keymap context"),
            "{unknown_context}"
        );

        let state_command = KeymapConfig::from_toml(
            r#"
[keymap.contexts.editor]
x = "editor.change_inner_word"
"#,
        )
        .expect("direct pending commands are allowed");
        assert_eq!(
            resolve_normal_in_context(
                KeymapContext::Editor,
                key(KeyCode::Char('x'), KeyModifiers::NONE),
                0,
                &state_command
            ),
            Some(ContextAction::Pending(PendingCommand::ChangeInnerWord))
        );

        let invalid = KeymapConfig::from_toml(
            r#"
[keymap.contexts.editor]
x = "not.a.command"
"#,
        )
        .expect_err("unknown command");
        assert!(invalid.contains("unknown or non-direct"), "{invalid}");
    }

    #[test]
    fn bundled_presets_parse_without_function_keys() {
        for name in ["ergo", "vim"] {
            let config = KeymapConfig::from_preset(name).expect("bundled preset parses");
            assert!(
                config
                    .normal
                    .keys()
                    .all(|chord| !matches!(chord.code, KeyCode::F(_))),
                "{name} normal bindings contain an F-key"
            );
            assert!(
                config
                    .pending
                    .keys()
                    .all(|(_, chord)| !matches!(chord.code, KeyCode::F(_))),
                "{name} pending bindings contain an F-key"
            );
            assert!(
                config
                    .contexts
                    .values()
                    .flatten()
                    .all(|(chord, _)| !matches!(chord.code, KeyCode::F(_))),
                "{name} context bindings contain an F-key"
            );
        }
    }

    #[test]
    fn config_can_select_a_preset_as_its_base() {
        let config = KeymapConfig::from_toml(
            r#"
preset = "vim"

[keymap.normal]
x = "editor.undo"
"#,
        )
        .expect("preset base with override");
        assert_eq!(
            resolve_normal(key(KeyCode::Char('x'), KeyModifiers::NONE), 0, &config),
            Some(NormalCommand::Undo)
        );
        assert_eq!(
            resolve_normal(key(KeyCode::Char('j'), KeyModifiers::NONE), 0, &config),
            Some(NormalCommand::MoveLine(1))
        );
        assert!(
            KeymapConfig::from_toml(r#"preset = "missing""#).is_err(),
            "unknown presets must be rejected"
        );
    }

    #[test]
    fn pending_actions_include_text_object_extenders() {
        let config = KeymapConfig::default();
        let actions = config.pending_actions('d', false);
        assert!(
            actions
                .iter()
                .any(|(key, action)| key == "i" && action == "text object"),
            "{actions:?}"
        );
        assert!(
            actions
                .iter()
                .any(|(key, action)| key == "a" && action == "text object"),
            "{actions:?}"
        );
        let inner = config.pending_actions('d', true);
        assert!(
            inner
                .iter()
                .any(|(key, action)| key == "w" && action == "delete inner word"),
            "{inner:?}"
        );
        let around = config.pending_actions('D', true);
        assert!(
            around
                .iter()
                .any(|(key, action)| key == "w" && action == "delete around word"),
            "{around:?}"
        );
    }
}
