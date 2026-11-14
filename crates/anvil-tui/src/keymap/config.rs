//! Keymap parsing, key chords, and configuration loading.

use super::*;
use serde::Deserialize;

pub(crate) const ANVIL_ERGO_PRESET: &str = include_str!("../../presets/anvil-ergo.toml");
pub(crate) const ANVIL_VIM_PRESET: &str = include_str!("../../presets/anvil-vim.toml");
pub(crate) const ANVIL_HELIX_PRESET: &str = include_str!("../../presets/anvil-helix.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct KeyChord {
    pub(crate) code: KeyCode,
    ctrl: bool,
    alt: bool,
    super_key: bool,
}

/// Converts a terminal key event into a frontend-neutral macro step.
pub(crate) fn macro_step(key: KeyEvent) -> MacroStep {
    MacroStep {
        key: KeyChord::from_key(KeyPress::from_key(key)).label(),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    }
}

/// Rebuilds a terminal key event from a macro step.
pub(crate) fn macro_key(step: &MacroStep) -> Option<KeyEvent> {
    let chord = KeyChord::parse(&step.key).ok()?;
    let mut modifiers = KeyModifiers::NONE;
    if chord.ctrl {
        modifiers |= KeyModifiers::CONTROL;
    }
    if chord.alt {
        modifiers |= KeyModifiers::ALT;
    }
    if chord.super_key {
        modifiers |= KeyModifiers::SUPER;
    }
    if step.shift {
        modifiers |= KeyModifiers::SHIFT;
    }
    Some(KeyEvent::new(chord.code, modifiers))
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
            KeyCode::BackTab => "backtab".to_owned(),
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

pub(crate) fn parse_key_name(name: &str) -> Result<KeyCode, String> {
    for number in 1..=12 {
        if name.eq_ignore_ascii_case(&format!("f{number}")) {
            return Ok(KeyCode::F(number));
        }
    }
    let code = match name {
        "space" => KeyCode::Char(' '),
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
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
    pub(crate) normal: HashMap<KeyChord, NormalCommand>,
    pub(crate) pending: HashMap<(char, KeyChord), PendingCommand>,
    pub(crate) contexts: HashMap<KeymapContext, HashMap<KeyChord, ContextAction>>,
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

    pub(crate) fn load_default() -> Result<Self, String> {
        if let Some(path) = KeyChord::default_path()
            && path.exists()
        {
            return Self::from_path(&path);
        }
        if let Some(preset) = std::env::var_os("ANVIL_KEYMAP_PRESET") {
            return Self::from_preset(&preset.to_string_lossy());
        }
        Ok(Self::default())
    }

    pub(crate) fn from_preset(name: &str) -> Result<Self, String> {
        let normalized = name.trim().to_ascii_lowercase();
        let text = match normalized.as_str() {
            "default" | "ergo" | "anvil-default" | "anvil-ergo" => ANVIL_ERGO_PRESET,
            "vim" | "anvil-vim" => ANVIL_VIM_PRESET,
            "helix" | "anvil-helix" => ANVIL_HELIX_PRESET,
            _ => {
                return Err(format!(
                    "unknown keymap preset {name:?}; available presets: ergo, vim, helix"
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

    pub(crate) fn resolve(&self, key: KeyPress) -> Option<NormalCommand> {
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

    #[cfg(test)]
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
