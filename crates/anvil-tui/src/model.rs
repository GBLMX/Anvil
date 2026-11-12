//! Model construction, configuration reload, and input helpers.

use super::*;

impl Model {
    /// Builds a model from text and an optional source path.
    pub fn new(editor: EditorState, path: Option<PathBuf>) -> Self {
        let welcome = path.is_none() && editor.text().len_bytes() == 0;
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let workspace_roots = workspace_roots::WorkspaceRoots::new(&workspace).unwrap_or_default();
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
                welcome_back: false,
            },
            edit: EditSessionState::default(),
            view: ViewState::default(),
            ui: UiState::default(),
            config: ConfigState {
                effects: EffectQueue::new(EFFECT_QUEUE_CAPACITY),
                keymap: keymap::KeymapConfig::default(),
                settings: Settings::default(),
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
                ..Default::default()
            },
            language: LanguageState::default(),
            git: GitState::default(),
            recent: RecentState {
                persist: !cfg!(test),
                ..Default::default()
            },
            tree: TreeState::default(),
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
            plugins: PluginState::default(),
            runtime: RuntimeState::default(),
        }
    }

    pub(crate) fn reload_config(&mut self) {
        let loaded = (|| -> Result<_, String> {
            let settings = settings::load()?;
            let keymap = keymap::KeymapConfig::load_default()?;
            Ok((settings, keymap))
        })();
        match loaded {
            Ok((settings, keymap)) => self.apply_loaded_config(settings, keymap),
            Err(error) => {
                self.status.text = format!("E: config-reload: {error}");
                self.file.dirty = true;
            }
        }
    }

    pub(crate) fn apply_loaded_config(
        &mut self,
        settings: settings::Settings,
        keymap: keymap::KeymapConfig,
    ) {
        self.apply_settings(settings);
        self.config.keymap = keymap;
        self.status.text = "configuration reloaded".to_owned();
        self.file.dirty = true;
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
                    self.plugins.store_size = value
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
                self.plugins.entries = plugins;
                return Err(error);
            }
        };
        conflicts.extend(managed_conflicts);
        self.plugins.entries = plugins;
        if let Some(manager) = self.plugins.manager.as_mut() {
            manager.selected = manager
                .selected
                .min(self.plugins.entries.len().saturating_sub(1));
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
        self.input.leader_deadline =
            (pending == ' ').then(|| Instant::now() + Duration::from_millis(600));
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
        self.git.marks.clear();
        self.git.diff_request = None;
        self.git.diff_checked = None;
        self.git.diff_due = None;
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
