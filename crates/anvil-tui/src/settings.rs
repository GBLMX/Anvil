//! Typed settings registry with explicit defaults, file, and environment layers.

use crate::theme::{self, Choice};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const PROJECT_SETTINGS: &str = ".anvil.toml";
pub(crate) const MAX_BYTES: u64 = 64 * 1024;
pub(crate) const DEFAULT_SCROLL_MARGIN: usize = 3;
pub(crate) const MAX_SCROLL_MARGIN: usize = 16;
pub(crate) const DEFAULT_MAX_BUFFERS: usize = 9;
pub(crate) const MAX_MAX_BUFFERS: usize = 99;
pub(crate) const DEFAULT_SIDEBAR_PERCENT: u16 = 20;
pub(crate) const MIN_SIDEBAR_PERCENT: u16 = 10;
pub(crate) const MAX_SIDEBAR_PERCENT: u16 = 40;

const ENV_SCROLL_MARGIN: &str = "ANVIL_EDITOR_SCROLL_MARGIN";
const ENV_AUTO_SAVE: &str = "ANVIL_EDITOR_AUTO_SAVE";
const ENV_SOFT_WRAP: &str = "ANVIL_EDITOR_SOFT_WRAP";
const ENV_FORMAT_ON_SAVE: &str = "ANVIL_EDITOR_FORMAT_ON_SAVE";
const ENV_FILE_WATCHING: &str = "ANVIL_EDITOR_FILE_WATCHING";
const ENV_MAX_BUFFERS: &str = "ANVIL_EDITOR_MAX_BUFFERS";
const ENV_SIDEBAR_PERCENT: &str = "ANVIL_EDITOR_SIDEBAR_PERCENT";
const ENV_SEARCH_CASE_SENSITIVE: &str = "ANVIL_SEARCH_CASE_SENSITIVE";
const ENV_INLAY_HINTS: &str = "ANVIL_LSP_INLAY_HINTS";
const ENV_REDUCED_MOTION: &str = "ANVIL_REDUCED_MOTION";
const ENV_HIGH_CONTRAST: &str = "ANVIL_HIGH_CONTRAST";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EditorSettings {
    pub(crate) scroll_margin: usize,
    pub(crate) max_buffers: usize,
    pub(crate) auto_save: bool,
    pub(crate) soft_wrap: bool,
    pub(crate) format_on_save: bool,
    pub(crate) file_watching: bool,
    pub(crate) sidebar_percent: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchSettings {
    pub(crate) case_sensitive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LspSettings {
    pub(crate) inlay_hints: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UiSettings {
    pub(crate) reduced_motion: bool,
    pub(crate) high_contrast: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) theme: Choice,
    pub(crate) editor: EditorSettings,
    pub(crate) search: SearchSettings,
    pub(crate) lsp: LspSettings,
    pub(crate) ui: UiSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Choice::Fixed("anvil-dark"),
            editor: EditorSettings {
                scroll_margin: DEFAULT_SCROLL_MARGIN,
                max_buffers: DEFAULT_MAX_BUFFERS,
                auto_save: false,
                soft_wrap: false,
                format_on_save: false,
                file_watching: true,
                sidebar_percent: DEFAULT_SIDEBAR_PERCENT,
            },
            search: SearchSettings {
                case_sensitive: false,
            },
            lsp: LspSettings { inlay_hints: true },
            ui: UiSettings {
                reduced_motion: false,
                high_contrast: false,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SettingsKey {
    Theme,
    ScrollMargin,
    AutoSave,
    SoftWrap,
    FormatOnSave,
    MaxBuffers,
    SidebarPercent,
    CaseSensitive,
    InlayHints,
    ReducedMotion,
    HighContrast,
    FileWatching,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingKind {
    Bool,
    Integer { min: i64, max: i64 },
    Theme,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SettingDef {
    pub(crate) key: SettingsKey,
    pub(crate) path: &'static str,
    pub(crate) env: Option<&'static str>,
    pub(crate) kind: SettingKind,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) group: &'static str,
    pub(crate) tab: &'static str,
}

pub(crate) const SETTINGS: &[SettingDef] = &[
    SettingDef {
        key: SettingsKey::ScrollMargin,
        path: "editor.scroll_margin",
        env: Some(ENV_SCROLL_MARGIN),
        kind: SettingKind::Integer { min: 0, max: 16 },
        label: "editor.scroll_margin",
        description: "Lines kept visible around the cursor before scrolling.",
        group: "Editor",
        tab: "Editor",
    },
    SettingDef {
        key: SettingsKey::AutoSave,
        path: "editor.auto_save",
        env: Some(ENV_AUTO_SAVE),
        kind: SettingKind::Bool,
        label: "editor.auto_save",
        description: "Save the active buffer when focus moves to another surface.",
        group: "Editor",
        tab: "Editor",
    },
    SettingDef {
        key: SettingsKey::SoftWrap,
        path: "editor.soft_wrap",
        env: Some(ENV_SOFT_WRAP),
        kind: SettingKind::Bool,
        label: "editor.soft_wrap",
        description: "Wrap long lines to the pane width and scroll by display row.",
        group: "Editor",
        tab: "Editor",
    },
    SettingDef {
        key: SettingsKey::FormatOnSave,
        path: "editor.format_on_save",
        env: Some(ENV_FORMAT_ON_SAVE),
        kind: SettingKind::Bool,
        label: "editor.format_on_save",
        description: "Run LSP formatting after a successful write.",
        group: "Editor",
        tab: "Editor",
    },
    SettingDef {
        key: SettingsKey::MaxBuffers,
        path: "editor.max_buffers",
        env: Some(ENV_MAX_BUFFERS),
        kind: SettingKind::Integer { min: 1, max: 99 },
        label: "editor.max_buffers",
        description: "Maximum foreground buffers; Alt-1..9 still select the first nine.",
        group: "Editor",
        tab: "Editor",
    },
    SettingDef {
        key: SettingsKey::SidebarPercent,
        path: "editor.sidebar_percent",
        env: Some(ENV_SIDEBAR_PERCENT),
        kind: SettingKind::Integer { min: 10, max: 40 },
        label: "editor.sidebar_percent",
        description: "Sidebar width as a percentage of the terminal.",
        group: "Editor",
        tab: "Editor",
    },
    SettingDef {
        key: SettingsKey::CaseSensitive,
        path: "search.case_sensitive",
        env: Some(ENV_SEARCH_CASE_SENSITIVE),
        kind: SettingKind::Bool,
        label: "search.case_sensitive",
        description: "Use case-sensitive matching for `/` and project search.",
        group: "Search",
        tab: "Language",
    },
    SettingDef {
        key: SettingsKey::InlayHints,
        path: "lsp.inlay_hints",
        env: Some(ENV_INLAY_HINTS),
        kind: SettingKind::Bool,
        label: "lsp.inlay_hints",
        description: "Request and render language-server inlay hints.",
        group: "LSP",
        tab: "Language",
    },
    SettingDef {
        key: SettingsKey::ReducedMotion,
        path: "ui.reduced_motion",
        env: Some(ENV_REDUCED_MOTION),
        kind: SettingKind::Bool,
        label: "ui.reduced_motion",
        description: "Freeze spinners and avoid continuous animation.",
        group: "Accessibility",
        tab: "Interface",
    },
    SettingDef {
        key: SettingsKey::HighContrast,
        path: "ui.high_contrast",
        env: Some(ENV_HIGH_CONTRAST),
        kind: SettingKind::Bool,
        label: "ui.high_contrast",
        description: "Increase text, muted, and border contrast for the active theme.",
        group: "Accessibility",
        tab: "Interface",
    },
    SettingDef {
        key: SettingsKey::FileWatching,
        path: "editor.file_watching",
        env: Some(ENV_FILE_WATCHING),
        kind: SettingKind::Bool,
        label: "editor.file_watching",
        description: "Reload buffers, tree, Git, and LSP state on external changes.",
        group: "Editor",
        tab: "Editor",
    },
    SettingDef {
        key: SettingsKey::Theme,
        path: "theme.name",
        env: None,
        kind: SettingKind::Theme,
        label: "theme.name",
        description: "Select the semantic palette used by the TUI.",
        group: "Appearance",
        tab: "Interface",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsOrigin {
    Default,
    User,
    Project,
    Environment,
    StartupOverride,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SettingsSources {
    origins: BTreeMap<SettingsKey, SettingsOrigin>,
    user: Option<PathBuf>,
    project: Option<PathBuf>,
    ignored: Vec<(PathBuf, String)>,
}

impl SettingsSources {
    pub(crate) fn origin(&self, key: SettingsKey) -> SettingsOrigin {
        self.origins
            .get(&key)
            .copied()
            .unwrap_or(SettingsOrigin::Default)
    }

    pub(crate) fn label(&self, key: SettingsKey) -> String {
        match self.origin(key) {
            SettingsOrigin::Default => "default".to_owned(),
            SettingsOrigin::Environment => "environment".to_owned(),
            SettingsOrigin::StartupOverride => "startup override".to_owned(),
            SettingsOrigin::User => self
                .user
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "user".to_owned()),
            SettingsOrigin::Project => self
                .project
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "project".to_owned()),
        }
    }

    pub(crate) fn layer_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Some(path) = &self.user {
            files.push(path.clone());
        }
        if let Some(path) = &self.project
            && !files.contains(path)
        {
            files.push(path.clone());
        }
        files
    }

    pub(crate) fn ignored(&self) -> &[(PathBuf, String)] {
        &self.ignored
    }

    pub(crate) fn mark_theme_saved(&mut self) {
        self.set(SettingsKey::Theme, SettingsOrigin::User);
    }

    fn set(&mut self, key: SettingsKey, origin: SettingsOrigin) {
        self.origins.insert(key, origin);
    }
}

#[derive(Debug)]
pub(crate) struct LoadedSettings {
    pub(crate) settings: Settings,
    pub(crate) sources: SettingsSources,
    pub(crate) user: SettingsPatch,
    pub(crate) inherited: Settings,
}

pub(crate) fn lsp_command_override(
    language_id: &str,
    default_args: &[String],
) -> Option<(String, Vec<String>)> {
    let path = path().ok()?;
    if !path.exists() {
        return None;
    }
    if fs::metadata(&path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let text = fs::read_to_string(&path).ok()?;
    let file: SettingsPatch = toml_edit::de::from_str(&text).ok()?;
    resolve_lsp_command(file.lsp.commands.get(language_id)?, default_args)
}

pub(crate) fn apply_lsp_root_profile(root: &Path, settings: &mut anvil_lsp_client::LspSettings) {
    let Ok(path) = path() else {
        return;
    };
    if fs::metadata(&path)
        .ok()
        .is_none_or(|metadata| metadata.len() > MAX_BYTES)
    {
        return;
    }
    let Some(file) = fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml_edit::de::from_str::<SettingsPatch>(&text).ok())
    else {
        return;
    };
    apply_lsp_root_profile_file(&file, root, settings);
}

fn apply_lsp_root_profile_file(
    file: &SettingsPatch,
    root: &Path,
    settings: &mut anvil_lsp_client::LspSettings,
) {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let Some(profile) = file
        .lsp
        .roots
        .get(canonical.to_string_lossy().as_ref())
        .and_then(|profiles| profiles.get(&settings.language_id))
    else {
        return;
    };
    if let Some(command) = &profile.command
        && let Some((tool, args)) = resolve_lsp_command(command, &settings.args)
    {
        settings.tool = tool;
        settings.args = args;
    }
    if let Some(args) = &profile.args {
        settings.args = args.clone();
    }
    if let Some(root_markers) = &profile.root_markers {
        settings.root_markers = root_markers.clone();
    }
    settings.env.extend(profile.env.clone());
    if let Some(value) = &profile.settings {
        settings.settings = value.clone();
    }
}

fn resolve_lsp_command(
    command: &LspCommandFile,
    default_args: &[String],
) -> Option<(String, Vec<String>)> {
    match command {
        LspCommandFile::Command(command) => {
            (!command.trim().is_empty()).then(|| (command.clone(), default_args.to_vec()))
        }
        LspCommandFile::Argv(argv) => {
            let (command, args) = argv.split_first()?;
            (!command.trim().is_empty()).then(|| (command.clone(), args.to_vec()))
        }
    }
}

/// Load settings for one workspace.
///
/// The project layer is read only when its workspace root is trusted. An
/// explicit `ANVIL_SETTINGS` file is a complete override, matching the
/// pre-project-layer behavior and keeping tests isolated.
pub(crate) fn load_for_workspace_with_sources(
    workspace: &Path,
    project_trusted: bool,
) -> Result<LoadedSettings, String> {
    let user = path()?;
    let candidate = workspace.join(PROJECT_SETTINGS);
    let mut ignored = Vec::new();
    let project = if explicit_settings_path().is_some() {
        if candidate.is_file() {
            ignored.push((
                candidate.clone(),
                "explicit ANVIL_SETTINGS replaces project layer".to_owned(),
            ));
        }
        None
    } else if project_trusted {
        Some(candidate)
    } else {
        if candidate.is_file() {
            ignored.push((candidate, "workspace root is untrusted".to_owned()));
        }
        None
    };
    load_from_layers_with_ignored(Some(&user), project.as_deref(), ignored)
}

#[cfg(test)]
pub(crate) fn load_for_workspace(
    workspace: &Path,
    project_trusted: bool,
) -> Result<Settings, String> {
    load_for_workspace_with_sources(workspace, project_trusted).map(|loaded| loaded.settings)
}

#[cfg(test)]
fn project_layer(workspace: &Path, trusted: bool) -> Option<PathBuf> {
    trusted.then(|| workspace.join(PROJECT_SETTINGS))
}

#[cfg(test)]
fn load_from_layers(user: Option<&Path>, project: Option<&Path>) -> Result<Settings, String> {
    load_from_layers_with_sources(user, project).map(|loaded| loaded.settings)
}

#[cfg(test)]
fn load_from_layers_with_sources(
    user: Option<&Path>,
    project: Option<&Path>,
) -> Result<LoadedSettings, String> {
    load_from_layers_with_ignored(user, project, Vec::new())
}

fn load_from_layers_with_ignored(
    user: Option<&Path>,
    project: Option<&Path>,
    ignored: Vec<(PathBuf, String)>,
) -> Result<LoadedSettings, String> {
    let mut sources = SettingsSources {
        ignored,
        ..SettingsSources::default()
    };
    let mut merged = toml_edit::DocumentMut::new();
    let mut inherited_doc = toml_edit::DocumentMut::new();
    let mut user_patch = SettingsPatch::default();
    let mut has_project = false;
    let mut layers = Vec::new();
    for (path, is_project) in [(user, false), (project, true)] {
        let Some(path) = path else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let text = read_settings_text(path)?;
        let document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| format!("{}: invalid settings TOML: {error}", path.display()))?;
        let file: SettingsPatch = toml_edit::de::from_str(&text)
            .map_err(|error| format!("{}: invalid settings TOML: {error}", path.display()))?;
        if is_project {
            if !file.lsp.commands.is_empty() {
                return Err(format!(
                    "{}: [lsp.commands] is not allowed in project settings",
                    path.display()
                ));
            }
            has_project = true;
            merge_document(&mut inherited_doc, &document);
        } else {
            user_patch = file;
        }
        let origin = if is_project {
            SettingsOrigin::Project
        } else {
            SettingsOrigin::User
        };
        if is_project {
            sources.project = Some(path.to_path_buf());
        } else {
            sources.user = Some(path.to_path_buf());
        }
        mark_layer_sources(&document, origin, &mut sources);
        merge_document(&mut merged, &document);
        layers.push(path);
    }
    let settings = if layers.is_empty() {
        Settings::default()
    } else {
        let names = layers
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        parse(&merged.to_string()).map_err(|error| format!("{names}: {error}"))?
    };
    let settings = apply_env(settings, |name| std::env::var(name).ok())?;
    let inherited = if has_project {
        parse(&inherited_doc.to_string()).map_err(|error| format!("project settings: {error}"))?
    } else {
        Settings::default()
    };
    let inherited = apply_env(inherited, |name| std::env::var(name).ok())?;
    mark_env_sources(&mut sources, |name| std::env::var(name).ok());
    if std::env::var_os("ANVIL_THEME").is_some() {
        sources.set(SettingsKey::Theme, SettingsOrigin::StartupOverride);
    }
    Ok(LoadedSettings {
        settings,
        sources,
        user: user_patch,
        inherited,
    })
}

fn mark_layer_sources(
    document: &toml_edit::DocumentMut,
    origin: SettingsOrigin,
    sources: &mut SettingsSources,
) {
    if ["mode", "name", "dark", "light"]
        .iter()
        .any(|key| has_layer_key(document, &["theme", key]))
    {
        sources.set(SettingsKey::Theme, origin);
    }
    for (key, path) in [
        (SettingsKey::ScrollMargin, ["editor", "scroll_margin"]),
        (SettingsKey::AutoSave, ["editor", "auto_save"]),
        (SettingsKey::SoftWrap, ["editor", "soft_wrap"]),
        (SettingsKey::FormatOnSave, ["editor", "format_on_save"]),
        (SettingsKey::MaxBuffers, ["editor", "max_buffers"]),
        (SettingsKey::SidebarPercent, ["editor", "sidebar_percent"]),
        (SettingsKey::CaseSensitive, ["search", "case_sensitive"]),
        (SettingsKey::InlayHints, ["lsp", "inlay_hints"]),
        (SettingsKey::ReducedMotion, ["ui", "reduced_motion"]),
        (SettingsKey::HighContrast, ["ui", "high_contrast"]),
        (SettingsKey::FileWatching, ["editor", "file_watching"]),
    ] {
        if has_layer_key(document, &path) {
            sources.set(key, origin);
        }
    }
}

fn has_layer_key(document: &toml_edit::DocumentMut, path: &[&str]) -> bool {
    let mut table = document.as_table();
    for (index, key) in path.iter().enumerate() {
        let Some(item) = table.get(key) else {
            return false;
        };
        if index + 1 == path.len() {
            return true;
        }
        let Some(next) = item.as_table() else {
            return false;
        };
        table = next;
    }
    true
}

fn mark_env_sources(sources: &mut SettingsSources, get: impl Fn(&str) -> Option<String>) {
    for def in SETTINGS {
        if let Some(name) = def.env
            && get(name).is_some()
        {
            sources.set(def.key, SettingsOrigin::Environment);
        }
    }
}

fn read_settings_text(path: &Path) -> Result<String, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if metadata.len() > MAX_BYTES {
        return Err(format!("{} exceeds {} bytes", path.display(), MAX_BYTES));
    }
    fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn merge_document(base: &mut toml_edit::DocumentMut, overlay: &toml_edit::DocumentMut) {
    merge_table(base.as_table_mut(), overlay.as_table());
}

fn merge_table(base: &mut toml_edit::Table, overlay: &toml_edit::Table) {
    for (key, value) in overlay.iter() {
        match (base.get_mut(key), value) {
            (Some(toml_edit::Item::Table(base)), toml_edit::Item::Table(overlay)) => {
                merge_table(base, overlay);
            }
            _ => {
                base.insert(key, value.clone());
            }
        }
    }
}

pub(crate) fn load_file_from_path(path: &Path) -> Result<Settings, String> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    parse(&read_settings_text(path)?)
}

pub(crate) fn save_patch(patch: &SettingsPatch) -> Result<PathBuf, String> {
    let path = path()?;
    save_patch_to_path(&path, patch)?;
    Ok(path)
}

pub(crate) fn save_patch_to_path(path: &Path, patch: &SettingsPatch) -> Result<(), String> {
    let mut document = if path.exists() {
        let metadata = fs::metadata(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if metadata.len() > MAX_BYTES {
            return Err(format!("{} exceeds {} bytes", path.display(), MAX_BYTES));
        }
        let text = fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        text.parse::<toml_edit::DocumentMut>()
            .map_err(|error| format!("invalid settings TOML: {error}"))?
    } else {
        toml_edit::DocumentMut::new()
    };
    if patch.theme.is_some() || document.contains_key("theme") {
        patch_table(&mut document, "theme", true, |table| {
            for key in ["mode", "name", "dark", "light"] {
                table.remove(key);
            }
            if let Some(theme) = &patch.theme {
                set_patch_value(table, "mode", theme.mode.as_deref());
                set_patch_value(table, "name", theme.name.as_deref());
                set_patch_value(table, "dark", theme.dark.as_deref());
                set_patch_value(table, "light", theme.light.as_deref());
            }
        })?;
    }
    patch_table(
        &mut document,
        "editor",
        patch.editor.scroll_margin.is_some()
            || patch.editor.max_buffers.is_some()
            || patch.editor.auto_save.is_some()
            || patch.editor.soft_wrap.is_some()
            || patch.editor.format_on_save.is_some()
            || patch.editor.file_watching.is_some()
            || patch.editor.sidebar_percent.is_some(),
        |table| {
            set_patch_value(
                table,
                "scroll_margin",
                patch.editor.scroll_margin.map(|value| value as i64),
            );
            set_patch_value(
                table,
                "max_buffers",
                patch.editor.max_buffers.map(|value| value as i64),
            );
            set_patch_value(table, "auto_save", patch.editor.auto_save);
            set_patch_value(table, "soft_wrap", patch.editor.soft_wrap);
            set_patch_value(table, "format_on_save", patch.editor.format_on_save);
            set_patch_value(table, "file_watching", patch.editor.file_watching);
            set_patch_value(
                table,
                "sidebar_percent",
                patch.editor.sidebar_percent.map(|value| value as i64),
            );
        },
    )?;
    patch_table(
        &mut document,
        "search",
        patch.search.case_sensitive.is_some(),
        |table| set_patch_value(table, "case_sensitive", patch.search.case_sensitive),
    )?;
    patch_table(
        &mut document,
        "lsp",
        patch.lsp.inlay_hints.is_some(),
        |table| set_patch_value(table, "inlay_hints", patch.lsp.inlay_hints),
    )?;
    patch_table(
        &mut document,
        "ui",
        patch.ui.reduced_motion.is_some() || patch.ui.high_contrast.is_some(),
        |table| {
            set_patch_value(table, "reduced_motion", patch.ui.reduced_motion);
            set_patch_value(table, "high_contrast", patch.ui.high_contrast);
        },
    )?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let temporary = path.with_extension(format!("toml.tmp-{}", std::process::id()));
    fs::write(&temporary, document.to_string())
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot replace {}: {error}", path.display()));
    }
    Ok(())
}

fn patch_table(
    document: &mut toml_edit::DocumentMut,
    key: &str,
    create: bool,
    apply: impl FnOnce(&mut toml_edit::Table),
) -> Result<(), String> {
    if let Some(item) = document.get_mut(key) {
        let table = item
            .as_table_mut()
            .ok_or_else(|| format!("[{key}] must be a table"))?;
        apply(table);
        return Ok(());
    }
    if create {
        let mut table = toml_edit::Table::new();
        apply(&mut table);
        document.insert(key, toml_edit::Item::Table(table));
    }
    Ok(())
}

fn set_patch_value<T: Into<toml_edit::Value>>(
    table: &mut toml_edit::Table,
    key: &str,
    value: Option<T>,
) {
    match value {
        Some(value) => table[key] = toml_edit::value(value),
        None => {
            table.remove(key);
        }
    }
}

pub(crate) fn parse(text: &str) -> Result<Settings, String> {
    let document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| format!("invalid settings TOML: {error}"))?;
    for (key, _) in document.iter() {
        if !matches!(key, "theme" | "editor" | "search" | "lsp" | "ui") {
            return Err(format!("unknown settings section [{key}]"));
        }
    }
    let file: SettingsPatch =
        toml_edit::de::from_str(text).map_err(|error| format!("invalid settings TOML: {error}"))?;
    let mut settings = Settings {
        theme: parse_theme(file.theme)?,
        ..Settings::default()
    };
    if let Some(scroll_margin) = file.editor.scroll_margin {
        if scroll_margin > MAX_SCROLL_MARGIN {
            return Err(format!(
                "editor.scroll_margin must be 0..={MAX_SCROLL_MARGIN}, got {scroll_margin}"
            ));
        }
        settings.editor.scroll_margin = scroll_margin;
    }
    if let Some(max_buffers) = file.editor.max_buffers {
        if !(1..=MAX_MAX_BUFFERS).contains(&max_buffers) {
            return Err(format!(
                "editor.max_buffers must be 1..={MAX_MAX_BUFFERS}, got {max_buffers}"
            ));
        }
        settings.editor.max_buffers = max_buffers;
    }
    if let Some(auto_save) = file.editor.auto_save {
        settings.editor.auto_save = auto_save;
    }
    if let Some(soft_wrap) = file.editor.soft_wrap {
        settings.editor.soft_wrap = soft_wrap;
    }
    if let Some(format_on_save) = file.editor.format_on_save {
        settings.editor.format_on_save = format_on_save;
    }
    if let Some(sidebar_percent) = file.editor.sidebar_percent {
        if !(MIN_SIDEBAR_PERCENT..=MAX_SIDEBAR_PERCENT).contains(&sidebar_percent) {
            return Err(format!(
                "editor.sidebar_percent must be {MIN_SIDEBAR_PERCENT}..={MAX_SIDEBAR_PERCENT}, got {sidebar_percent}"
            ));
        }
        settings.editor.sidebar_percent = sidebar_percent;
    }
    if let Some(file_watching) = file.editor.file_watching {
        settings.editor.file_watching = file_watching;
    }
    if let Some(case_sensitive) = file.search.case_sensitive {
        settings.search.case_sensitive = case_sensitive;
    }
    if let Some(inlay_hints) = file.lsp.inlay_hints {
        settings.lsp.inlay_hints = inlay_hints;
    }
    if let Some(reduced_motion) = file.ui.reduced_motion {
        settings.ui.reduced_motion = reduced_motion;
    }
    if let Some(high_contrast) = file.ui.high_contrast {
        settings.ui.high_contrast = high_contrast;
    }
    Ok(settings)
}
fn parse_theme(theme: Option<ThemeFile>) -> Result<Choice, String> {
    let Some(theme) = theme else {
        return Ok(Choice::Fixed("anvil-dark"));
    };
    match theme.mode.as_deref().unwrap_or("fixed") {
        "fixed" => {
            if theme.dark.is_some() || theme.light.is_some() {
                return Err("fixed theme mode must not set dark or light".to_owned());
            }
            let name = theme::canonical(theme.name.as_deref().unwrap_or("anvil-dark"))?;
            Ok(Choice::Fixed(name))
        }
        "auto" => {
            if theme.name.is_some() {
                return Err("auto theme mode must not set name".to_owned());
            }
            let dark = theme::canonical(theme.dark.as_deref().unwrap_or("anvil-dark"))?;
            let light = theme::canonical(theme.light.as_deref().unwrap_or("solarized-light"))?;
            Ok(Choice::Auto { dark, light })
        }
        mode => Err(format!(
            "unknown theme mode {mode:?}; expected fixed or auto"
        )),
    }
}

fn apply_env(
    mut settings: Settings,
    get: impl Fn(&str) -> Option<String>,
) -> Result<Settings, String> {
    if let Some(value) = get(ENV_SCROLL_MARGIN) {
        let margin = value.parse::<usize>().map_err(|_| {
            format!("{ENV_SCROLL_MARGIN} must be an integer 0..={MAX_SCROLL_MARGIN}")
        })?;
        if margin > MAX_SCROLL_MARGIN {
            return Err(format!(
                "{ENV_SCROLL_MARGIN} must be 0..={MAX_SCROLL_MARGIN}, got {margin}"
            ));
        }
        settings.editor.scroll_margin = margin;
    }
    if let Some(value) = get(ENV_MAX_BUFFERS) {
        let max_buffers = value
            .parse::<usize>()
            .map_err(|_| format!("{ENV_MAX_BUFFERS} must be an integer 1..={MAX_MAX_BUFFERS}"))?;
        if !(1..=MAX_MAX_BUFFERS).contains(&max_buffers) {
            return Err(format!(
                "{ENV_MAX_BUFFERS} must be 1..={MAX_MAX_BUFFERS}, got {max_buffers}"
            ));
        }
        settings.editor.max_buffers = max_buffers;
    }
    if let Some(value) = get(ENV_AUTO_SAVE) {
        settings.editor.auto_save = parse_bool(ENV_AUTO_SAVE, &value)?;
    }
    if let Some(value) = get(ENV_SOFT_WRAP) {
        settings.editor.soft_wrap = parse_bool(ENV_SOFT_WRAP, &value)?;
    }
    if let Some(value) = get(ENV_FORMAT_ON_SAVE) {
        settings.editor.format_on_save = parse_bool(ENV_FORMAT_ON_SAVE, &value)?;
    }
    if let Some(value) = get(ENV_FILE_WATCHING) {
        settings.editor.file_watching = parse_bool(ENV_FILE_WATCHING, &value)?;
    }
    if let Some(value) = get(ENV_SIDEBAR_PERCENT) {
        let percent = value.parse::<u16>().map_err(|_| {
            format!(
                "{ENV_SIDEBAR_PERCENT} must be an integer {MIN_SIDEBAR_PERCENT}..={MAX_SIDEBAR_PERCENT}"
            )
        })?;
        if !(MIN_SIDEBAR_PERCENT..=MAX_SIDEBAR_PERCENT).contains(&percent) {
            return Err(format!(
                "{ENV_SIDEBAR_PERCENT} must be {MIN_SIDEBAR_PERCENT}..={MAX_SIDEBAR_PERCENT}, got {percent}"
            ));
        }
        settings.editor.sidebar_percent = percent;
    }
    if let Some(value) = get(ENV_SEARCH_CASE_SENSITIVE) {
        settings.search.case_sensitive = parse_bool(ENV_SEARCH_CASE_SENSITIVE, &value)?;
    }
    if let Some(value) = get(ENV_INLAY_HINTS) {
        settings.lsp.inlay_hints = parse_bool(ENV_INLAY_HINTS, &value)?;
    }
    if let Some(value) = get(ENV_REDUCED_MOTION) {
        settings.ui.reduced_motion = parse_bool(ENV_REDUCED_MOTION, &value)?;
    }
    if let Some(value) = get(ENV_HIGH_CONTRAST) {
        settings.ui.high_contrast = parse_bool(ENV_HIGH_CONTRAST, &value)?;
    }
    Ok(settings)
}

fn parse_bool(name: &str, value: &str) -> Result<bool, String> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(format!("{name} must be true or false, got {value:?}")),
    }
}

fn explicit_settings_path() -> Option<PathBuf> {
    let explicit = std::env::var_os("ANVIL_SETTINGS")?;
    let explicit = explicit.to_string_lossy();
    (!explicit.trim().is_empty()).then(|| PathBuf::from(explicit.into_owned()))
}

pub(crate) fn path() -> Result<PathBuf, String> {
    if let Some(explicit) = explicit_settings_path() {
        return Ok(explicit);
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        let config = config.to_string_lossy();
        if !config.trim().is_empty() {
            return Ok(PathBuf::from(config.into_owned()).join("anvil/settings.toml"));
        }
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| "cannot resolve settings path: HOME is unset".to_owned())?;
    Ok(PathBuf::from(home).join(".config/anvil/settings.toml"))
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct SettingsPatch {
    theme: Option<ThemeFile>,
    editor: EditorFile,
    search: SearchFile,
    lsp: LspFile,
    ui: UiFile,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ThemeFile {
    mode: Option<String>,
    name: Option<String>,
    dark: Option<String>,
    light: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EditorFile {
    scroll_margin: Option<usize>,
    max_buffers: Option<usize>,
    auto_save: Option<bool>,
    soft_wrap: Option<bool>,
    format_on_save: Option<bool>,
    file_watching: Option<bool>,
    sidebar_percent: Option<u16>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SearchFile {
    case_sensitive: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LspFile {
    inlay_hints: Option<bool>,
    commands: BTreeMap<String, LspCommandFile>,
    roots: BTreeMap<String, BTreeMap<String, LspRootProfileFile>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum LspCommandFile {
    Command(String),
    Argv(Vec<String>),
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LspRootProfileFile {
    command: Option<LspCommandFile>,
    args: Option<Vec<String>>,
    root_markers: Option<Vec<String>>,
    env: BTreeMap<String, String>,
    settings: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UiFile {
    reduced_motion: Option<bool>,
    high_contrast: Option<bool>,
    #[allow(dead_code)]
    icons: BTreeMap<String, serde::de::IgnoredAny>,
}

impl SettingsPatch {
    pub(crate) fn from_settings(settings: Settings) -> Self {
        let mut patch = Self::default();
        for def in SETTINGS {
            debug_assert_eq!(def.label, def.path);
            patch.set_field(def.key, &settings);
        }
        patch
    }

    pub(crate) fn set_field(&mut self, key: SettingsKey, settings: &Settings) {
        match key {
            SettingsKey::Theme => self.set_theme(settings.theme),
            SettingsKey::ScrollMargin => {
                self.editor.scroll_margin = Some(settings.editor.scroll_margin)
            }
            SettingsKey::AutoSave => self.editor.auto_save = Some(settings.editor.auto_save),
            SettingsKey::SoftWrap => self.editor.soft_wrap = Some(settings.editor.soft_wrap),
            SettingsKey::FormatOnSave => {
                self.editor.format_on_save = Some(settings.editor.format_on_save)
            }
            SettingsKey::MaxBuffers => self.editor.max_buffers = Some(settings.editor.max_buffers),
            SettingsKey::SidebarPercent => {
                self.editor.sidebar_percent = Some(settings.editor.sidebar_percent)
            }
            SettingsKey::CaseSensitive => {
                self.search.case_sensitive = Some(settings.search.case_sensitive)
            }
            SettingsKey::InlayHints => self.lsp.inlay_hints = Some(settings.lsp.inlay_hints),
            SettingsKey::ReducedMotion => self.ui.reduced_motion = Some(settings.ui.reduced_motion),
            SettingsKey::HighContrast => self.ui.high_contrast = Some(settings.ui.high_contrast),
            SettingsKey::FileWatching => {
                self.editor.file_watching = Some(settings.editor.file_watching)
            }
        }
    }

    pub(crate) fn clear_field(&mut self, key: SettingsKey) {
        match key {
            SettingsKey::Theme => self.theme = None,
            SettingsKey::ScrollMargin => self.editor.scroll_margin = None,
            SettingsKey::AutoSave => self.editor.auto_save = None,
            SettingsKey::SoftWrap => self.editor.soft_wrap = None,
            SettingsKey::FormatOnSave => self.editor.format_on_save = None,
            SettingsKey::MaxBuffers => self.editor.max_buffers = None,
            SettingsKey::SidebarPercent => self.editor.sidebar_percent = None,
            SettingsKey::CaseSensitive => self.search.case_sensitive = None,
            SettingsKey::InlayHints => self.lsp.inlay_hints = None,
            SettingsKey::ReducedMotion => self.ui.reduced_motion = None,
            SettingsKey::HighContrast => self.ui.high_contrast = None,
            SettingsKey::FileWatching => self.editor.file_watching = None,
        }
    }

    pub(crate) fn set_theme(&mut self, choice: Choice) {
        self.theme = Some(theme_file(choice));
    }
}

fn theme_file(choice: Choice) -> ThemeFile {
    match choice {
        Choice::Fixed(name) => ThemeFile {
            mode: Some("fixed".to_owned()),
            name: Some(name.to_owned()),
            dark: None,
            light: None,
        },
        Choice::Auto { dark, light } => ThemeFile {
            mode: Some("auto".to_owned()),
            name: None,
            dark: Some(dark.to_owned()),
            light: Some(light.to_owned()),
        },
    }
}

impl crate::Model {
    pub(crate) fn export_settings(&mut self, argument: &str) {
        let argument = argument.trim();
        if argument.is_empty() {
            self.status.text = "E: usage: :settings-export <path>".to_owned();
            self.file.dirty = true;
            return;
        }
        let path = match self.workspace.absolute_path(Path::new(argument)) {
            Ok(path) => path,
            Err(error) => {
                self.status.text = format!("E: settings-export: {error}");
                self.file.dirty = true;
                return;
            }
        };
        let patch = SettingsPatch::from_settings(self.config.settings);
        match save_patch_to_path(&path, &patch) {
            Ok(()) => {
                self.status.text = format!("settings exported: {}", path.display());
            }
            Err(error) => self.status.text = format!("E: settings-export: {error}"),
        }
        self.file.dirty = true;
    }

    pub(crate) fn import_settings(&mut self, argument: &str) {
        let argument = argument.trim();
        if argument.is_empty() {
            self.status.text = "E: usage: :settings-import <path>".to_owned();
            self.file.dirty = true;
            return;
        }
        let source = match self.workspace.absolute_path(Path::new(argument)) {
            Ok(path) => path,
            Err(error) => {
                self.status.text = format!("E: settings-import: {error}");
                self.file.dirty = true;
                return;
            }
        };
        if !source.is_file() {
            self.status.text = format!("E: settings-import: {} is not a file", source.display());
            self.file.dirty = true;
            return;
        }
        let imported = match load_file_from_path(&source) {
            Ok(settings) => settings,
            Err(error) => {
                self.status.text = format!("E: settings-import: {error}");
                self.file.dirty = true;
                return;
            }
        };
        let patch = SettingsPatch::from_settings(imported);
        let target = match save_patch(&patch) {
            Ok(path) => path,
            Err(error) => {
                self.status.text = format!("E: settings-import: {error}");
                self.file.dirty = true;
                return;
            }
        };
        self.reload_config();
        let reload_status = self.status.text.clone();
        self.status.text = format!(
            "settings imported: {} -> {}; {reload_status}",
            source.display(),
            target.display()
        );
        self.file.dirty = true;
    }

    pub(crate) fn apply_settings(&mut self, settings: Settings) {
        let theme_changed = self.config.settings.theme != settings.theme;
        let watch_changed =
            self.config.settings.editor.file_watching != settings.editor.file_watching;
        self.config.settings = settings;
        self.search.state.options.case_sensitive = settings.search.case_sensitive;
        self.language.inlay_hints.hints.clear();
        self.language.inlay_hints.pending = None;
        self.language.inlay_hints.checked = None;
        if theme_changed {
            if let Err(error) = theme::apply_choice(settings.theme, false) {
                self.status.text = format!("E: theme: {error}");
            }
        }
        theme::set_high_contrast(settings.ui.high_contrast);
        if watch_changed {
            self.workspace
                .set_watch_enabled(settings.editor.file_watching);
            if settings.editor.file_watching {
                self.sync_watch_paths();
                self.sync_tree_watches();
            }
        }
        self.file.dirty = true;
    }

    pub(crate) fn persist_theme_status(&mut self, status: String) -> String {
        if let Some(choice) = theme::runtime_choice() {
            self.config.settings.theme = choice;
        }
        if !self.appearance.persist_theme {
            return status;
        }
        let mut patch = self.config.user_settings.clone();
        patch.set_theme(self.config.settings.theme);
        match save_patch(&patch) {
            Ok(_) => {
                self.config.user_settings = patch;
                self.config.sources.mark_theme_saved();
                format!("{status} (saved)")
            }
            Err(error) => format!("{status}; E: {error}"),
        }
    }
}

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_explicit() {
        assert_eq!(
            Settings::default(),
            Settings {
                theme: Choice::Fixed("anvil-dark"),
                editor: EditorSettings {
                    scroll_margin: 3,
                    max_buffers: DEFAULT_MAX_BUFFERS,
                    auto_save: false,
                    soft_wrap: false,
                    format_on_save: false,
                    file_watching: true,
                    sidebar_percent: DEFAULT_SIDEBAR_PERCENT,
                },
                search: SearchSettings {
                    case_sensitive: false,
                },
                lsp: LspSettings { inlay_hints: true },
                ui: UiSettings {
                    reduced_motion: false,
                    high_contrast: false,
                },
            }
        );
    }

    #[test]
    fn registry_covers_every_setting_once() {
        let mut keys = std::collections::BTreeSet::new();
        let mut envs = std::collections::BTreeSet::new();
        for def in SETTINGS {
            assert!(
                keys.insert(def.key),
                "duplicate registry key: {:?}",
                def.key
            );
            assert!(!def.path.is_empty());
            assert_eq!(def.label, def.path);
            assert!(!def.description.is_empty());
            assert!(!def.group.is_empty());
            assert!(!def.tab.is_empty());
            if let Some(env) = def.env {
                assert!(envs.insert(env), "duplicate environment name: {env}");
            }
        }
        for key in [
            SettingsKey::Theme,
            SettingsKey::ScrollMargin,
            SettingsKey::AutoSave,
            SettingsKey::SoftWrap,
            SettingsKey::FormatOnSave,
            SettingsKey::MaxBuffers,
            SettingsKey::SidebarPercent,
            SettingsKey::CaseSensitive,
            SettingsKey::InlayHints,
            SettingsKey::ReducedMotion,
            SettingsKey::HighContrast,
            SettingsKey::FileWatching,
        ] {
            assert!(keys.contains(&key), "missing registry key: {key:?}");
        }
        assert_eq!(keys.len(), 12);
    }
    #[test]
    fn settings_export_and_import_round_trip() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let previous_settings = std::env::var_os("ANVIL_SETTINGS");
        let previous_config = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::remove_var("ANVIL_SETTINGS");
            std::env::set_var("XDG_CONFIG_HOME", directory.path().join("config"));
        }
        let mut model = crate::Model::new(crate::EditorState::from_text(""), None);
        model.workspace.path = workspace.clone();
        model.workspace.roots =
            crate::workspace_roots::WorkspaceRoots::new(&workspace).expect("roots");
        model.config.settings.editor.scroll_margin = 6;
        model.config.settings.editor.soft_wrap = true;

        model.execute_command("settings-export export.toml");
        let exported = workspace.join("export.toml");
        assert!(
            model.status.text.contains("settings exported"),
            "{}",
            model.status.text
        );
        let parsed = parse(&fs::read_to_string(&exported).expect("export")).expect("valid export");
        assert_eq!(parsed.editor.scroll_margin, 6);
        assert!(parsed.editor.soft_wrap);

        let imported = directory.path().join("import.toml");
        fs::write(
            &imported,
            "[editor]\nscroll_margin = 9\nsoft_wrap = false\n[search]\ncase_sensitive = true\n",
        )
        .expect("import source");
        model.execute_command(&format!("settings-import {}", imported.display()));
        assert_eq!(model.config.settings.editor.scroll_margin, 9);
        assert!(!model.config.settings.editor.soft_wrap);
        assert!(model.config.settings.search.case_sensitive);
        assert!(
            model.status.text.contains("settings imported"),
            "{}",
            model.status.text
        );
        assert!(
            directory
                .path()
                .join("config/anvil/settings.toml")
                .is_file()
        );

        match previous_settings {
            Some(value) => unsafe { std::env::set_var("ANVIL_SETTINGS", value) },
            None => unsafe { std::env::remove_var("ANVIL_SETTINGS") },
        }
        match previous_config {
            Some(value) => unsafe { std::env::set_var("XDG_CONFIG_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }
    }

    #[test]
    fn parses_and_validates_sidebar_percent() {
        let settings = parse("[editor]\nsidebar_percent = 30\n").expect("sidebar width");
        assert_eq!(settings.editor.sidebar_percent, 30);
        assert!(parse("[editor]\nsidebar_percent = 5\n").is_err());
        assert!(parse("[editor]\nsidebar_percent = 41\n").is_err());
    }

    #[test]
    fn project_layer_merges_over_user_settings() {
        let directory = tempfile::tempdir().expect("tempdir");
        let user = directory.path().join("user.toml");
        let project = directory.path().join(PROJECT_SETTINGS);
        fs::write(
            &user,
            "[theme]\nname = \"anvil-dark\"\n[editor]\nscroll_margin = 2\nsoft_wrap = false\n",
        )
        .expect("user settings");
        fs::write(
            &project,
            "[theme]\nname = \"nord\"\n[editor]\nsoft_wrap = true\n",
        )
        .expect("project settings");

        let settings = load_from_layers(Some(&user), Some(&project)).expect("merged settings");
        assert_eq!(settings.theme, Choice::Fixed("nord"));
        assert_eq!(settings.editor.scroll_margin, 2);
        assert!(settings.editor.soft_wrap);
    }

    #[test]
    fn project_icon_table_does_not_block_settings() {
        let directory = tempfile::tempdir().expect("tempdir");
        let user = directory.path().join("user.toml");
        let project = directory.path().join(PROJECT_SETTINGS);
        fs::write(&user, "[theme]\nname = \"anvil-dark\"\n").expect("user settings");
        fs::write(
            &project,
            "[ui.icons]\nrust = \"A\"\n[editor]\nsoft_wrap = true\n",
        )
        .expect("project settings");
        let settings = load_from_layers(Some(&user), Some(&project)).expect("settings");
        assert!(settings.editor.soft_wrap);
    }

    #[test]
    fn tracks_per_field_settings_origins() {
        let directory = tempfile::tempdir().expect("tempdir");
        let user = directory.path().join("user.toml");
        let project = directory.path().join(PROJECT_SETTINGS);
        fs::write(
            &user,
            "[theme]\nname = \"anvil-dark\"\n[editor]\nscroll_margin = 2\n",
        )
        .expect("user settings");
        fs::write(&project, "[editor]\nsoft_wrap = true\n").expect("project settings");

        let loaded =
            load_from_layers_with_sources(Some(&user), Some(&project)).expect("loaded settings");
        assert_eq!(
            loaded.sources.origin(SettingsKey::Theme),
            SettingsOrigin::User
        );
        assert_eq!(
            loaded.sources.origin(SettingsKey::ScrollMargin),
            SettingsOrigin::User
        );
        assert_eq!(
            loaded.sources.origin(SettingsKey::SoftWrap),
            SettingsOrigin::Project
        );
        assert_eq!(
            loaded.sources.origin(SettingsKey::FileWatching),
            SettingsOrigin::Default
        );
        assert_eq!(loaded.sources.layer_files(), vec![user, project]);
    }

    #[test]
    fn environment_marks_field_origin() {
        let mut sources = SettingsSources::default();
        mark_env_sources(&mut sources, |name| {
            (name == ENV_MAX_BUFFERS).then(|| "12".to_owned())
        });
        assert_eq!(
            sources.origin(SettingsKey::MaxBuffers),
            SettingsOrigin::Environment
        );
        assert_eq!(
            sources.origin(SettingsKey::ScrollMargin),
            SettingsOrigin::Default
        );
    }

    #[test]
    fn reload_reports_effective_settings_file() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let settings_path = directory.path().join("settings.toml");
        fs::write(&settings_path, "[editor]\nscroll_margin = 2\n").expect("settings");

        unsafe { std::env::set_var("ANVIL_SETTINGS", &settings_path) };
        let mut model = crate::Model::new(crate::EditorState::from_text(""), None);
        model.workspace.path = workspace.clone();
        model.workspace.roots =
            crate::workspace_roots::WorkspaceRoots::new(&workspace).expect("roots");
        model.reload_config();
        unsafe { std::env::remove_var("ANVIL_SETTINGS") };

        assert!(
            model
                .status
                .text
                .contains(&settings_path.display().to_string()),
            "{}",
            model.status.text
        );
        assert_eq!(
            model.config.sources.origin(SettingsKey::ScrollMargin),
            SettingsOrigin::User
        );
        assert_eq!(model.config.settings.editor.scroll_margin, 2);
    }

    #[test]
    fn project_layer_requires_trust() {
        let workspace = Path::new("/workspace");
        assert_eq!(project_layer(workspace, false), None);
        assert_eq!(
            project_layer(workspace, true),
            Some(workspace.join(PROJECT_SETTINGS))
        );
    }

    #[test]
    fn untrusted_project_is_reported_as_ignored() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::write(
            workspace.join(PROJECT_SETTINGS),
            "[editor]\nsoft_wrap = true\n",
        )
        .expect("project settings");
        let previous_settings = std::env::var_os("ANVIL_SETTINGS");
        let previous_config = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::remove_var("ANVIL_SETTINGS");
            std::env::set_var("XDG_CONFIG_HOME", directory.path().join("config"));
        }
        let loaded = load_for_workspace_with_sources(&workspace, false).expect("untrusted project");
        match previous_settings {
            Some(value) => unsafe { std::env::set_var("ANVIL_SETTINGS", value) },
            None => unsafe { std::env::remove_var("ANVIL_SETTINGS") },
        }
        match previous_config {
            Some(value) => unsafe { std::env::set_var("XDG_CONFIG_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }
        assert!(loaded.sources.ignored().iter().any(|(path, reason)| {
            path == &workspace.join(PROJECT_SETTINGS) && reason.contains("untrusted")
        }));
        assert_eq!(
            loaded.sources.origin(SettingsKey::SoftWrap),
            SettingsOrigin::Default
        );
    }

    #[test]
    fn reload_reports_untrusted_project_file() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::write(
            workspace.join(PROJECT_SETTINGS),
            "[editor]\nsoft_wrap = true\n",
        )
        .expect("project settings");
        let previous_settings = std::env::var_os("ANVIL_SETTINGS");
        let previous_config = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::remove_var("ANVIL_SETTINGS");
            std::env::set_var("XDG_CONFIG_HOME", directory.path().join("config"));
        }
        let mut model = crate::Model::new(crate::EditorState::from_text(""), None);
        model.workspace.path = workspace.clone();
        model.workspace.roots =
            crate::workspace_roots::WorkspaceRoots::new(&workspace).expect("roots");
        model
            .workspace
            .roots
            .set_trusted(&workspace, false)
            .expect("untrusted");
        model.reload_config();
        match previous_settings {
            Some(value) => unsafe { std::env::set_var("ANVIL_SETTINGS", value) },
            None => unsafe { std::env::remove_var("ANVIL_SETTINGS") },
        }
        match previous_config {
            Some(value) => unsafe { std::env::set_var("XDG_CONFIG_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }
        assert!(
            model.status.text.contains("ignored"),
            "{}",
            model.status.text
        );
        assert!(
            model.status.text.contains("untrusted"),
            "{}",
            model.status.text
        );
    }

    #[test]
    fn invalid_project_settings_report_path_and_parse_error() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let project = workspace.join(PROJECT_SETTINGS);
        fs::write(&project, "[editor\nsoft_wrap = true\n").expect("invalid project");
        let previous_settings = std::env::var_os("ANVIL_SETTINGS");
        let previous_config = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::remove_var("ANVIL_SETTINGS");
            std::env::set_var("XDG_CONFIG_HOME", directory.path().join("config"));
        }
        let error = load_for_workspace_with_sources(&workspace, true).expect_err("invalid TOML");
        match previous_settings {
            Some(value) => unsafe { std::env::set_var("ANVIL_SETTINGS", value) },
            None => unsafe { std::env::remove_var("ANVIL_SETTINGS") },
        }
        match previous_config {
            Some(value) => unsafe { std::env::set_var("XDG_CONFIG_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }
        assert!(error.contains(&project.display().to_string()), "{error}");
        assert!(error.contains("invalid settings TOML"), "{error}");
    }

    #[test]
    fn project_layer_rejects_lsp_commands() {
        let directory = tempfile::tempdir().expect("tempdir");
        let project = directory.path().join(PROJECT_SETTINGS);
        fs::write(&project, "[lsp.commands]\nrust = \"/tmp/evil\"\n").expect("project settings");

        let error = load_from_layers(None, Some(&project)).expect_err("lsp commands");
        assert!(error.contains("[lsp.commands]"), "{error}");
    }

    #[test]
    fn explicit_settings_replace_the_project_layer() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::write(
            workspace.join(PROJECT_SETTINGS),
            "[theme]\nname = \"nord\"\n",
        )
        .expect("project settings");
        let explicit = directory.path().join("explicit.toml");
        fs::write(&explicit, "[theme]\nname = \"omp-kde\"\n").expect("explicit settings");

        unsafe { std::env::set_var("ANVIL_SETTINGS", &explicit) };
        let result = load_for_workspace(&workspace, true);
        unsafe { std::env::remove_var("ANVIL_SETTINGS") };
        let settings = result.expect("explicit settings");
        assert_eq!(settings.theme, Choice::Fixed("omp-kde"));
    }

    #[test]
    fn parses_lsp_command_overrides() {
        let (tool, args) = resolve_lsp_command(
            &LspCommandFile::Command("/usr/bin/json-server".to_owned()),
            &["--stdio".to_owned()],
        )
        .expect("string command");
        assert_eq!(tool, "/usr/bin/json-server");
        assert_eq!(args, ["--stdio"]);

        let settings = parse(
            "[lsp]\ninlay_hints = true\n[lsp.commands]\njson = [\"node\", \"server.js\", \"--stdio\"]\nrust = \"/usr/bin/rust-analyzer\"\n",
        )
        .expect("settings with lsp commands");
        assert!(settings.lsp.inlay_hints);
        let file: SettingsPatch = toml_edit::de::from_str(
            "[lsp.commands]\njson = [\"node\", \"server.js\", \"--stdio\"]\nrust = \"/usr/bin/rust-analyzer\"\n",
        )
        .expect("lsp commands");
        let (tool, args) =
            resolve_lsp_command(&file.lsp.commands["json"], &[]).expect("json command");
        assert_eq!(tool, "node");
        assert_eq!(args, ["server.js", "--stdio"]);
        let (tool, args) = resolve_lsp_command(&file.lsp.commands["rust"], &["--stdio".to_owned()])
            .expect("rust command");
        assert_eq!(tool, "/usr/bin/rust-analyzer");
        assert_eq!(args, ["--stdio"]);
    }

    #[test]
    fn applies_per_root_lsp_profiles() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path().canonicalize().expect("canonical");
        let text = format!(
            "[lsp.roots.\"{}\".rust]\n\
             command = \"/opt/rust-analyzer\"\n\
             args = [\"--log-file\", \"/tmp/ra.log\"]\n\
             root_markers = [\"Cargo.toml\", \"rust-toolchain.toml\"]\n\
             env = {{ RA_LOG = \"info\" }}\n\
             settings = {{ check = {{ command = \"clippy\" }} }}\n",
            root.display()
        );
        let file: SettingsPatch = toml_edit::de::from_str(&text).expect("profile");
        let mut settings = anvil_lsp_client::LspSettings {
            tool: "rust-analyzer".to_owned(),
            args: Vec::new(),
            language_id: "rust".to_owned(),
            root_markers: vec!["Cargo.toml".to_owned()],
            label: "rust-analyzer".to_owned(),
            initialization_options: None,
            env: BTreeMap::new(),
            settings: serde_json::Value::Null,
        };

        apply_lsp_root_profile_file(&file, &root, &mut settings);
        assert_eq!(settings.tool, "/opt/rust-analyzer");
        assert_eq!(settings.args, ["--log-file", "/tmp/ra.log"]);
        assert_eq!(settings.root_markers, ["Cargo.toml", "rust-toolchain.toml"]);
        assert_eq!(settings.env["RA_LOG"], "info");
        assert_eq!(
            settings.settings,
            serde_json::json!({"check": {"command": "clippy"}})
        );
    }

    #[test]
    fn string_json_override_keeps_builtin_stdio_args() {
        let file: SettingsPatch = toml_edit::de::from_str(
            "[lsp.commands]\njson = \"/usr/bin/vscode-json-language-server\"\n",
        )
        .expect("json command");
        let (tool, args) = resolve_lsp_command(&file.lsp.commands["json"], &["--stdio".to_owned()])
            .expect("json command");
        assert_eq!(tool, "/usr/bin/vscode-json-language-server");
        assert_eq!(args, ["--stdio"]);
    }

    #[test]
    fn parses_all_domains_and_theme() {
        let settings = parse(
            "[theme]\nmode = \"fixed\"\n[editor]\nscroll_margin = 1\nmax_buffers = 4\nauto_save = true\nsoft_wrap = true\nformat_on_save = true\n[search]\ncase_sensitive = true\n[lsp]\ninlay_hints = false\n",
        )
        .expect("settings");
        assert_eq!(settings.theme, Choice::Fixed("anvil-dark"));
        assert_eq!(settings.editor.scroll_margin, 1);
        assert_eq!(settings.editor.max_buffers, 4);
        assert!(settings.editor.auto_save);
        assert!(settings.editor.soft_wrap);
        assert!(settings.editor.format_on_save);
        assert!(settings.search.case_sensitive);
        assert!(!settings.lsp.inlay_hints);
    }

    #[test]
    fn parses_file_watching_and_env_override() {
        let settings = parse("[editor]\nfile_watching = false\n").expect("file watching");
        assert!(!settings.editor.file_watching);
        let settings = apply_env(settings, |name| {
            (name == ENV_FILE_WATCHING).then(|| "true".to_owned())
        })
        .expect("env");
        assert!(settings.editor.file_watching);
    }

    #[test]
    fn loads_file_values_and_missing_files_as_defaults() {
        let directory =
            std::env::temp_dir().join(format!("anvil-settings-registry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("directory");
        let path = directory.join("settings.toml");
        assert_eq!(
            load_file_from_path(&path).expect("missing"),
            Settings::default()
        );
        fs::write(
            &path,
            "[editor]\nscroll_margin = 2\n[search]\ncase_sensitive = true\n[lsp]\ninlay_hints = false\n",
        )
        .expect("write");
        let settings = load_file_from_path(&path).expect("settings");
        assert_eq!(settings.editor.scroll_margin, 2);
        assert!(settings.search.case_sensitive);
        assert!(!settings.lsp.inlay_hints);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn save_writes_theme_and_domains() {
        let directory =
            std::env::temp_dir().join(format!("anvil-settings-save-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("directory");
        let path = directory.join("settings.toml");
        fs::write(
            &path,
            "[theme]\nmode = \"fixed\"\nname = \"nord\"\n[editor]\nscroll_margin = 1\n",
        )
        .expect("write");
        let mut settings = Settings {
            theme: Choice::Fixed("nord"),
            ..Settings::default()
        };
        settings.editor.scroll_margin = 5;
        settings.editor.max_buffers = 12;
        settings.search.case_sensitive = true;
        settings.lsp.inlay_hints = false;
        save_patch_to_path(&path, &SettingsPatch::from_settings(settings)).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("[theme]"));
        assert!(text.contains("name = \"nord\""));
        assert!(text.contains("max_buffers = 12"));
        assert_eq!(load_file_from_path(&path).expect("load"), settings);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn sparse_save_does_not_persist_environment_overrides() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let directory = tempfile::tempdir().expect("tempdir");
        let user = directory.path().join("settings.toml");
        fs::write(&user, "[editor]\nsoft_wrap = true\n").expect("write");
        let previous = std::env::var_os(ENV_SCROLL_MARGIN);
        unsafe { std::env::set_var(ENV_SCROLL_MARGIN, "9") };
        let loaded = load_from_layers_with_sources(Some(&user), None).expect("load");
        unsafe {
            match previous {
                Some(value) => std::env::set_var(ENV_SCROLL_MARGIN, value),
                None => std::env::remove_var(ENV_SCROLL_MARGIN),
            }
        }
        assert_eq!(loaded.settings.editor.scroll_margin, 9);
        save_patch_to_path(&user, &loaded.user).expect("save");
        let text = fs::read_to_string(&user).expect("read");
        assert!(!text.contains("scroll_margin"), "{text}");
        assert!(text.contains("soft_wrap = true"), "{text}");
    }

    #[test]
    fn clearing_a_user_key_reveals_the_project_value() {
        let directory = tempfile::tempdir().expect("tempdir");
        let user = directory.path().join("settings.toml");
        let project = directory.path().join(PROJECT_SETTINGS);
        fs::write(&user, "[editor]\nsoft_wrap = false\n").expect("user");
        fs::write(&project, "[editor]\nsoft_wrap = true\n").expect("project");
        let loaded = load_from_layers_with_sources(Some(&user), Some(&project)).expect("load");
        assert!(loaded.settings.editor.soft_wrap);
        assert!(loaded.inherited.editor.soft_wrap);
        let mut patch = loaded.user.clone();
        patch.clear_field(SettingsKey::SoftWrap);
        save_patch_to_path(&user, &patch).expect("save");
        let text = fs::read_to_string(&user).expect("read");
        assert!(!text.contains("soft_wrap"), "{text}");
        let reloaded = load_from_layers_with_sources(Some(&user), Some(&project)).expect("reload");
        assert!(reloaded.settings.editor.soft_wrap);
        assert_eq!(
            reloaded.sources.origin(SettingsKey::SoftWrap),
            SettingsOrigin::Project
        );
    }

    #[test]
    fn sparse_save_preserves_comments_and_unknown_keys() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("settings.toml");
        fs::write(
            &path,
            "# keep this comment\n[editor]\nsoft_wrap = true\n[future]\nvalue = 1\n",
        )
        .expect("write");
        save_patch_to_path(&path, &SettingsPatch::default()).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("# keep this comment"), "{text}");
        assert!(text.contains("[future]"), "{text}");
        assert!(text.contains("value = 1"), "{text}");
        assert!(parse(&text).is_err(), "unknown keys remain load errors");
    }

    #[test]
    fn theme_patch_does_not_rewrite_other_user_keys() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("settings.toml");
        fs::write(&path, "[editor]\nsoft_wrap = true\n").expect("write");
        let loaded = load_from_layers_with_sources(Some(&path), None).expect("load");
        let mut patch = loaded.user.clone();
        patch.set_theme(Choice::Fixed("nord"));
        save_patch_to_path(&path, &patch).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("soft_wrap = true"), "{text}");
        assert!(text.contains("name = \"nord\""), "{text}");
    }

    #[test]
    fn settings_overlay_save_keeps_env_out_of_user_file() {
        let _guard = TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("settings.toml");
        fs::write(&path, "[editor]\nsoft_wrap = true\n").expect("write");
        let previous_settings = std::env::var_os("ANVIL_SETTINGS");
        let previous_margin = std::env::var_os(ENV_SCROLL_MARGIN);
        unsafe {
            std::env::set_var("ANVIL_SETTINGS", &path);
            std::env::set_var(ENV_SCROLL_MARGIN, "9");
        }
        let mut model = crate::Model::new(crate::EditorState::from_text(""), None);
        model.reload_config();
        assert_eq!(model.config.settings.editor.scroll_margin, 9);
        model.open_settings();
        model
            .overlays
            .settings_overlay
            .as_mut()
            .expect("overlay")
            .selected = 1;
        model.adjust_settings(1);
        model.save_settings();
        let text = fs::read_to_string(&path).expect("read");
        assert!(!text.contains("scroll_margin"), "{text}");
        assert!(text.contains("auto_save = true"), "{text}");
        assert!(text.contains("soft_wrap = true"), "{text}");
        unsafe {
            match previous_settings {
                Some(value) => std::env::set_var("ANVIL_SETTINGS", value),
                None => std::env::remove_var("ANVIL_SETTINGS"),
            }
            match previous_margin {
                Some(value) => std::env::set_var(ENV_SCROLL_MARGIN, value),
                None => std::env::remove_var(ENV_SCROLL_MARGIN),
            }
        }
    }

    #[test]
    fn theme_parser_rejects_ambiguous_and_unknown_values() {
        assert!(parse("[theme]\nmode = \"auto\"\nname = \"nord\"\n").is_err());
        assert!(parse("[theme]\ndark = \"nord\"\n").is_err());
        assert!(parse("[theme]\nname = \"missing\"\n").is_err());
        assert!(parse("[theme]\nmode = \"system\"\n").is_err());
    }
    #[test]
    fn parses_auto_theme_mode() {
        let settings =
            parse("[theme]\nmode = \"auto\"\ndark = \"nord\"\nlight = \"solarized-light\"\n")
                .expect("auto theme");
        assert_eq!(
            settings.theme,
            Choice::Auto {
                dark: "nord",
                light: "solarized-light",
            }
        );
    }

    #[test]
    fn rejects_unknown_sections_fields_and_values() {
        assert!(parse("[ui]\nmode = \"x\"\n").is_err());
        assert!(parse("[editor]\ntab_width = 2\n").is_err());
        assert!(parse("[editor]\nscroll_margin = 99\n").is_err());
        assert!(parse("[search]\ncase_sensitive = \"yes\"\n").is_err());
        assert!(parse("[editor]\nmax_buffers = 0\n").is_err());
        assert!(parse("[editor]\nmax_buffers = 100\n").is_err());
    }

    #[test]
    fn environment_overrides_file_values() {
        let settings = apply_env(
            parse("[editor]\nscroll_margin = 1\n[search]\ncase_sensitive = true\n[lsp]\ninlay_hints = false\n")
                .expect("file"),
            |name| match name {
                ENV_SCROLL_MARGIN => Some("4".to_owned()),
                ENV_MAX_BUFFERS => Some("12".to_owned()),
                ENV_SEARCH_CASE_SENSITIVE => Some("false".to_owned()),
                ENV_INLAY_HINTS => Some("true".to_owned()),
                _ => None,
            },
        )
        .expect("env");
        assert_eq!(settings.editor.max_buffers, 12);
        assert_eq!(settings.editor.scroll_margin, 4);
        assert!(!settings.search.case_sensitive);
        assert!(settings.lsp.inlay_hints);
    }

    #[test]
    fn environment_errors_are_visible() {
        assert!(
            apply_env(Settings::default(), |name| {
                (name == ENV_SCROLL_MARGIN).then(|| "99".to_owned())
            })
            .is_err()
        );
        assert!(
            apply_env(Settings::default(), |name| {
                (name == ENV_MAX_BUFFERS).then(|| "100".to_owned())
            })
            .is_err()
        );
        assert!(
            apply_env(Settings::default(), |name| {
                (name == ENV_INLAY_HINTS).then(|| "maybe".to_owned())
            })
            .is_err()
        );
    }

    #[test]
    fn applying_settings_updates_model_defaults() {
        let mut model = crate::Model::new(crate::EditorState::from_text("x"), None);
        let mut settings = Settings::default();
        settings.search.case_sensitive = true;
        settings.lsp.inlay_hints = false;
        model.apply_settings(settings);
        assert!(model.search.state.options.case_sensitive);
        assert!(!model.config.settings.lsp.inlay_hints);
    }
}

#[cfg(test)]
mod accessibility_tests {
    use super::*;

    #[test]
    fn parses_and_overrides_ui_accessibility_settings() {
        let settings = parse("[ui]\nreduced_motion = true\nhigh_contrast = true\n").expect("parse");
        assert!(settings.ui.reduced_motion);
        assert!(settings.ui.high_contrast);
        let settings = apply_env(settings, |name| {
            (name == ENV_REDUCED_MOTION).then(|| "false".to_owned())
        })
        .expect("env");
        assert!(!settings.ui.reduced_motion);
        assert!(settings.ui.high_contrast);
    }
}
