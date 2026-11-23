//! Model construction, configuration reload, and input helpers.

use super::*;
const LEADER_TIMEOUT: Duration = Duration::from_millis(1200);

#[cfg(feature = "official-git")]
fn default_git_service() -> Option<std::sync::Arc<dyn anvil_module::git::GitService>> {
    Some(std::sync::Arc::new(anvil_mod_git::SystemGitService))
}

#[cfg(not(feature = "official-git"))]
fn default_git_service() -> Option<std::sync::Arc<dyn anvil_module::git::GitService>> {
    None
}

fn reload_status(prefix: &str, files: &[PathBuf]) -> String {
    if files.is_empty() {
        format!("{prefix} (defaults)")
    } else {
        format!(
            "{prefix} from {}",
            files
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Model {
    /// Builds a model from text and an optional source path.
    pub fn new(editor: EditorState, path: Option<PathBuf>) -> Self {
        let welcome = path.is_none() && editor.text().len_bytes() == 0;
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let workspace_roots = workspace_roots::WorkspaceRoots::new(&workspace).unwrap_or_default();
        #[cfg(test)]
        let (symbol_cache, _symbol_cache_temp) = match tempfile::tempdir() {
            Ok(directory) => (
                Some(anvil_runtime::SymbolCache::at(
                    directory.path().join("symbols.json"),
                )),
                Some(directory),
            ),
            Err(_) => (None, None),
        };
        #[cfg(not(test))]
        let symbol_cache = None;
        Self {
            session: EditorSessionState {
                editor: DocumentSession::from(editor),
                path,
                cursor: 0,
                jump_list: jump::JumpList::default(),
                change_list: jump::JumpList::default(),
            },
            start_page: StartPageState {
                welcome,
                ..Default::default()
            },
            edit: EditSessionState::default(),
            view: ViewState::default(),
            ui: UiState::default(),
            config: ConfigState {
                effects: EffectQueue::new(EFFECT_QUEUE_CAPACITY),
                keymap: keymap::KeymapConfig::default(),
                settings: Settings::default(),
                user_settings: settings::SettingsPatch::default(),
                project_settings: settings::SettingsPatch::default(),
                inherited_settings: Settings::default(),
                sources: settings::SettingsSources::default(),
                language_overlays: settings::LanguageOverlays::default(),
                language_revision: 0,
                buffer_settings: std::cell::RefCell::default(),
            },
            file: FileState {
                dirty: true,
                ..Default::default()
            },
            status: StatusState::default(),
            input: InputState::default(),
            search: SearchSession::default(),
            pager: ReadOnlyState::default(),
            buffers: BufferState::default(),
            workspace: WorkspaceState {
                path: workspace,
                roots: workspace_roots,
                git_service: default_git_service(),
                recent: RecentState {
                    persist: !cfg!(test),
                    ..Default::default()
                },
                ..Default::default()
            },
            language: LanguageState::default(),
            surfaces: surface::FocusStack::default(),
            overlays: OverlayState::default(),
            appearance: AppearanceState {
                ascii_icons: icon_theme::ascii_mode(),
                persist_theme: !cfg!(test),
                ..Default::default()
            },
            project: ProjectState::default(),
            syntax_cache: SyntaxCacheState {
                highlights: RefCell::new(SyntaxState::default()),
                bracket: RefCell::new(bracket::BracketCache::default()),
            },
            runtime: RuntimeState::default(),
            symbol_cache,
            #[cfg(test)]
            _symbol_cache_temp,
        }
    }

    pub(crate) fn reload_config(&mut self) {
        let project_trusted = self
            .workspace
            .roots
            .primary()
            .is_some_and(|root| root.trusted);
        let loaded = (|| -> Result<_, String> {
            let settings =
                settings::load_for_workspace_with_sources(&self.workspace.path, project_trusted)?;
            let keymap = keymap::KeymapConfig::load_default()?;
            Ok((settings, keymap))
        })();
        match loaded {
            Ok((settings, keymap)) => self.apply_loaded_config_with_sources(settings, keymap),
            Err(error) => {
                self.status.text = format!("E: config-reload: {error}");
                self.file.dirty = true;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn apply_loaded_config(
        &mut self,
        settings: settings::Settings,
        keymap: keymap::KeymapConfig,
    ) {
        self.apply_settings(settings);
        self.notify_lsp_configuration();
        self.config.keymap = keymap;
        self.status.text = "configuration reloaded".to_owned();
        self.file.dirty = true;
    }

    fn install_loaded_settings(
        &mut self,
        loaded: settings::LoadedSettings,
    ) -> (Vec<PathBuf>, String, Vec<String>) {
        let settings::LoadedSettings {
            settings,
            sources,
            user,
            project,
            inherited,
            languages,
            warnings,
        } = loaded;
        let files = sources.layer_files();
        let ignored = sources
            .ignored()
            .iter()
            .map(|(path, reason)| format!("{} ({reason})", path.display()))
            .collect::<Vec<_>>()
            .join(", ");
        self.apply_settings(settings);
        self.config.user_settings = user;
        self.config.project_settings = project;
        self.config.inherited_settings = inherited;
        self.config.sources = sources;
        self.config.language_overlays = languages;
        self.config.language_revision = self.config.language_revision.wrapping_add(1);
        self.notify_lsp_configuration();
        self.file.dirty = true;
        (files, ignored, warnings)
    }

    fn apply_loaded_config_with_sources(
        &mut self,
        loaded: settings::LoadedSettings,
        keymap: keymap::KeymapConfig,
    ) {
        let (files, ignored, warnings) = self.install_loaded_settings(loaded);
        self.config.keymap = keymap;
        self.status.text = reload_status("configuration reloaded", &files);
        if !ignored.is_empty() {
            self.status.text.push_str(&format!("; ignored {ignored}"));
        }
        if !warnings.is_empty() {
            self.status
                .text
                .push_str(&format!("; warnings {}", warnings.join("; ")));
        }
        self.file.dirty = true;
    }

    pub(crate) fn reload_settings_from_files(&mut self) -> Result<(), String> {
        let project_trusted = self
            .workspace
            .roots
            .primary()
            .is_some_and(|root| root.trusted);
        let loaded =
            settings::load_for_workspace_with_sources(&self.workspace.path, project_trusted)?;
        let (files, ignored, warnings) = self.install_loaded_settings(loaded);
        self.status.text = reload_status("settings reloaded", &files);
        if !ignored.is_empty() {
            self.status.text.push_str(&format!("; ignored {ignored}"));
        }
        if !warnings.is_empty() {
            self.status
                .text
                .push_str(&format!("; warnings {}", warnings.join("; ")));
        }
        Ok(())
    }

    pub(crate) fn poll_settings_watch(
        &mut self,
        watch: &mut settings::SettingsWatch,
        pending: &mut bool,
    ) {
        let project_trusted = self
            .workspace
            .roots
            .primary()
            .is_some_and(|root| root.trusted);
        let paths = settings::watch_paths(&self.workspace.path, project_trusted);
        let paths_changed = watch.sync(paths);
        if !(*pending || paths_changed || watch.changed()) {
            return;
        }
        if self.overlays.settings_overlay.is_some() {
            *pending = true;
            return;
        }
        *pending = false;
        if let Err(error) = self.reload_settings_from_files() {
            self.status.text = format!("E: settings reload: {error}");
            self.file.dirty = true;
        }
        watch.refresh();
    }

    pub(crate) fn load_keymap_config_from(&mut self, path: &Path) -> Result<(), String> {
        let config = keymap::KeymapConfig::from_path(path)?;
        self.config.keymap = config;
        Ok(())
    }

    pub(crate) fn load_keymap_config(&mut self) -> Result<(), String> {
        if let Some(path) = keymap::KeyChord::default_path()
            && path.exists()
        {
            return self.load_keymap_config_from(&path);
        }
        if let Some(preset) = std::env::var_os("ANVIL_KEYMAP_PRESET") {
            self.config.keymap = keymap::KeymapConfig::from_preset(&preset.to_string_lossy())?;
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn load_plugins(&mut self) -> Result<(), String> {
        let mut plugins = plugin_discovery::discover_bundled()?;
        let local = plugin_discovery::discover()?;
        let mut conflicts = plugin_discovery::merge_local(&mut plugins, local);
        let managed = if let Some(gateway) = self.workspace.gateway.as_mut() {
            match gateway.call("pkg.installed", serde_json::json!({})) {
                Ok(value) => {
                    self.runtime.plugins.store_size = value
                        .get("store_size")
                        .and_then(|size| size.as_u64())
                        .unwrap_or(0);
                    plugin_discovery::discover_managed(&value)
                }
                Err(error) => Err(format!("pkg.installed: {error}")),
            }
        } else {
            Ok(Vec::new())
        };
        let managed_conflicts = match managed {
            Ok(managed) => plugin_discovery::merge_managed(&mut plugins, managed),
            Err(error) => {
                self.runtime.plugins.entries = plugins;
                return Err(error);
            }
        };
        conflicts.extend(managed_conflicts);
        self.runtime.plugins.entries = plugins;
        if let Some(manager) = self.runtime.plugins.manager.as_mut() {
            manager.selected = manager
                .selected
                .min(self.runtime.plugins.entries.len().saturating_sub(1));
        }
        if !conflicts.is_empty() {
            return Err(format!("duplicate plugin ids: {}", conflicts.join(", ")));
        }
        Ok(())
    }

    /// Builds a model that uses `gateway` for all file I/O.
    pub fn with_gateway(gateway: GatewayClient, workspace: PathBuf) -> Self {
        let mut model = Self::new(EditorState::from_text(""), None);
        model.workspace.gateway = Some(gateway);
        model.workspace.path = workspace;
        model.project.platform = None;
        model.workspace.roots =
            workspace_roots::WorkspaceRoots::new(&model.workspace.path).unwrap_or_default();
        model.workspace.root_gateways.clear();
        model.workspace.root_file_workers.clear();
        model.reload_icons();
        model.workspace.require_gateway = true;
        model
    }

    /// Opens a workspace file, preserving an edited active buffer as a snapshot.
    /// Builds a gateway-backed model by reading a UTF-8 file.
    /// Builds a model by reading a UTF-8 file.
    ///
    /// Invalid UTF-8 is rejected instead of being lossily replaced, because
    /// saving a lossy view would silently corrupt the original bytes.
    /// Dispatches one terminal event through the normal input path.
    ///
    /// Used by tests and benchmarks; terminal loops read events themselves.
    pub fn dispatch(&mut self, event: Event) {
        handle_event(self, event);
    }

    pub(crate) fn push_count(&mut self, character: char) {
        self.input.count =
            self.input.count.saturating_mul(10) + usize::from(character as u8 - b'0');
        self.file.dirty = true;
    }

    pub(crate) fn take_count(&mut self) -> usize {
        let count = self.input.count.max(1);
        self.input.count = 0;
        count
    }

    pub(crate) fn begin_pending(&mut self, pending: char) {
        self.input.pending = Some(pending);
        self.input.leader_deadline = (pending == ' ').then(|| Instant::now() + LEADER_TIMEOUT);
        self.file.dirty = true;
    }

    pub(crate) fn expire_leader(&mut self) {
        let Some(deadline) = self.input.leader_deadline else {
            return;
        };
        if self.input.pending == Some(' ') && Instant::now() >= deadline {
            self.input.pending = None;
            self.input.leader_deadline = None;
            self.file.dirty = true;
        }
    }

    /// Moves the cursor by repeated vi word motions.
    /// Moves the cursor one line while keeping the viewport independent.
    /// Moves the cursor one grapheme-cluster boundary.
    /// Moves to the start of the cursor line.
    /// Moves to the end of the cursor line.
    /// Keeps the cursor inside the viewport.
    pub(crate) fn reject_read_only(&mut self) -> bool {
        if self.file.read_only {
            self.status.text = "E: read-only buffer".to_owned();
            self.file.dirty = true;
            true
        } else if self.pager.back.is_some() {
            self.status.text = "E: read-only view".to_owned();
            self.file.dirty = true;
            true
        } else {
            false
        }
    }

    pub(crate) fn clear_git_gutter(&mut self) {
        self.workspace.git.marks.clear();
        self.workspace.git.diff_request = None;
        self.workspace.git.diff_checked = None;
        self.workspace.git.diff_due = None;
    }
    pub fn poll_syntax(&mut self) {
        let text = self.session.editor.text();
        let revision = self.session.editor.revision();
        let path = self.session.path.clone();
        if self
            .syntax_cache
            .highlights
            .borrow_mut()
            .poll(text, revision, path.as_deref())
        {
            self.file.dirty = true;
        }
    }
}
