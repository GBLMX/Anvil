//! Buffer snapshots, activation, and quit guards.

use super::*;

use anvil_session::provider::{
    CancellationToken, PickerBuffer, PickerContext, PickerEntry, PickerQuery, PickerSource,
    ProviderError, ProviderId, ProviderOrigin, ProviderPriority, ProviderRegistry,
};
use serde_json::json;
use std::sync::Arc;
impl Model {
    pub(crate) fn snapshot(&self) -> Buffer {
        Buffer {
            editor: self.session.editor.clone(),
            path: self.session.path.clone(),
            cursor: self.session.cursor,
            scroll: self.view.scroll,
            missing: self.file.missing,
            hscroll: self.view.hscroll,
            visual_anchor: self.edit.visual_anchor,
            visual_line: self.edit.visual_line,
            modified: self.file.modified,
            search_query: self.search.query.clone(),
            syntax: self.syntax_cache.highlights.borrow().active_cache(),
        }
    }

    pub(crate) fn sync_active(&mut self) {
        let snapshot = self.snapshot();
        if let Some(buffer) = self.buffers.entries.get_mut(self.buffers.active) {
            *buffer = snapshot;
        }
    }

    pub(crate) fn switch_to_buffer(&mut self, index: usize) {
        if self.pager.back.is_none() {
            self.sync_active();
        }
        self.load_buffer(index);
    }

    pub(crate) fn select_buffer(&mut self, index: usize) {
        if self.pager.back.is_some() {
            let _ = self.close_help();
        }
        let len = self.buffers.entries.len();
        if index >= len {
            self.status.text = format!("E: no buffer {}", index + 1);
            self.file.dirty = true;
            return;
        }
        if index != self.buffers.active {
            if !self.auto_save_before_focus_change() {
                return;
            }
            self.sync_active();
            self.load_buffer(index);
        }
        let name = self.buffer_name(index);
        self.status.text = format!("buffer {}/{}: {name}", index + 1, len);
        self.file.dirty = true;
    }

    pub(crate) fn open_buffer_picker(&mut self) {
        self.cancel_completion();
        self.ensure_current_buffer();
        let context = self.buffer_picker_context();
        let (cancel, result) = self.query_buffer_picker(&context, "");
        let entries = result.unwrap_or_else(|error| {
            self.status.text = format!("E: {error}");
            Vec::new()
        });
        let selected = entries
            .iter()
            .position(|entry| entry.value.as_u64() == Some(self.buffers.active as u64))
            .unwrap_or(0);
        self.overlays.buffer_picker = Some(BufferPickerState {
            query: String::new(),
            filtered: (0..entries.len()).collect(),
            entries,
            selected,
            cancel,
            mode: BufferPickerMode::Buffers,
        });
        self.push_overlay(OverlayKind::BufferPicker);
        self.status.text = "buffers".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn open_plugin_picker(
        &mut self,
        provider_id: ProviderId,
        items: Vec<anvil_api::ProviderItem>,
    ) {
        if items.is_empty() {
            self.status.text = "provider returned no items".to_owned();
            self.file.dirty = true;
            return;
        }
        let entries = items
            .into_iter()
            .map(|item| {
                let display = item.label;
                let value = item.value.unwrap_or_else(|| display.clone());
                PickerEntry {
                    value: serde_json::Value::String(value),
                    ordinal: display.clone(),
                    display,
                }
            })
            .collect::<Vec<_>>();
        self.overlays.buffer_picker = Some(BufferPickerState {
            query: String::new(),
            filtered: (0..entries.len()).collect(),
            entries,
            selected: 0,
            cancel: CancellationToken::new(),
            mode: BufferPickerMode::Plugin { provider_id },
        });
        self.push_overlay(OverlayKind::BufferPicker);
        self.status.text = "provider items".to_owned();
        self.file.dirty = true;
    }

    pub(crate) fn refresh_buffer_picker(&mut self) {
        let Some(picker) = self.overlays.buffer_picker.as_ref() else {
            return;
        };
        let query = picker.query.clone();
        if matches!(picker.mode, BufferPickerMode::Plugin { .. }) {
            let needle = query.to_lowercase();
            let Some(picker) = self.overlays.buffer_picker.as_mut() else {
                return;
            };
            picker.filtered = picker
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.display.to_lowercase().contains(&needle))
                .map(|(index, _)| index)
                .collect();
            picker.selected = picker.selected.min(picker.filtered.len().saturating_sub(1));
            self.file.dirty = true;
            return;
        }
        let Some(picker) = self.overlays.buffer_picker.as_ref() else {
            return;
        };
        picker.cancel.cancel();
        let context = self.buffer_picker_context();
        let (cancel, result) = self.query_buffer_picker(&context, &query);
        let entries = result.unwrap_or_else(|error| {
            self.status.text = format!("E: {error}");
            Vec::new()
        });
        let Some(picker) = self.overlays.buffer_picker.as_mut() else {
            return;
        };
        picker.entries = entries;
        picker.filtered = (0..picker.entries.len()).collect();
        picker.selected = picker.selected.min(picker.filtered.len().saturating_sub(1));
        picker.cancel = cancel;
        self.file.dirty = true;
    }

    pub(crate) fn move_buffer_picker(&mut self, delta: isize) {
        let Some(picker) = self.overlays.buffer_picker.as_mut() else {
            return;
        };
        picker.selected = picker
            .selected
            .saturating_add_signed(delta)
            .min(picker.filtered.len().saturating_sub(1));
        self.file.dirty = true;
    }

    pub(crate) fn close_buffer_picker(&mut self) {
        if let Some(picker) = self.overlays.buffer_picker.take() {
            picker.cancel.cancel();
        }
        self.close_overlay(OverlayKind::BufferPicker);
        self.file.dirty = true;
    }

    pub(crate) fn apply_buffer_picker(&mut self) {
        let Some(picker) = self.overlays.buffer_picker.take() else {
            return;
        };
        picker.cancel.cancel();
        self.close_overlay(OverlayKind::BufferPicker);
        let Some(entry) = picker
            .filtered
            .get(picker.selected)
            .and_then(|index| picker.entries.get(*index))
        else {
            self.status.text = "no matching item".to_owned();
            self.file.dirty = true;
            return;
        };
        if let BufferPickerMode::Plugin { provider_id } = picker.mode {
            let Some(value) = entry.value.as_str() else {
                self.status.text = "E: invalid provider picker value".to_owned();
                self.file.dirty = true;
                return;
            };
            self.provider_action(provider_id.as_str(), value);
            return;
        }
        let Some(index) = entry
            .value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
        else {
            self.status.text = "E: invalid buffer picker value".to_owned();
            self.file.dirty = true;
            return;
        };
        self.select_buffer(index);
    }

    fn buffer_picker_context(&self) -> PickerContext {
        PickerContext {
            root: self.workspace.path.clone(),
            revision: self.session.editor.revision(),
            buffers: self
                .buffers
                .entries
                .iter()
                .enumerate()
                .map(|(index, buffer)| PickerBuffer {
                    index,
                    name: self.buffer_name(index),
                    active: index == self.buffers.active,
                    modified: buffer.modified,
                })
                .collect(),
        }
    }

    fn query_buffer_picker(
        &self,
        context: &PickerContext,
        text: &str,
    ) -> (CancellationToken, Result<Vec<PickerEntry>, ProviderError>) {
        let cancel = CancellationToken::new();
        let registry = buffer_picker_registry();
        let Some(provider) = registry.providers().into_iter().next() else {
            return (cancel, Ok(Vec::new()));
        };
        let query = PickerQuery {
            text: text.to_owned(),
            limit: 512,
        };
        let mut entries = Vec::new();
        let result = provider
            .provider
            .query(context, &query, &cancel, &mut |batch| {
                entries.extend(batch);
            });
        (cancel, result.map(|()| entries))
    }

    pub(crate) fn load_buffer(&mut self, index: usize) {
        self.clear_lsp_requests();
        self.close_hover();
        self.close_diagnostic_peek();
        self.overlays.lsp_results.code_actions = None;
        self.close_overlay(OverlayKind::CodeActions);
        self.overlays.lsp_results.document_symbols = None;
        self.close_overlay(OverlayKind::DocumentSymbols);
        let Some(buffer) = self.buffers.entries.get(index).cloned() else {
            return;
        };
        let syntax = buffer.syntax.clone();
        self.file.missing = buffer.missing;
        self.edit.visual_anchor = buffer.visual_anchor;
        self.edit.visual_line = buffer.visual_line;
        self.session.editor = buffer.editor;
        self.session.path = buffer.path;
        self.session.cursor = buffer.cursor;
        self.view.scroll = buffer.scroll;
        self.view.hscroll = buffer.hscroll;
        self.file.modified = buffer.modified;
        self.search.query = buffer.search_query;
        self.search.state.reset_for_buffer();
        self.start_page.welcome = false;
        self.start_page.welcome_back = false;
        self.syntax_cache.highlights.borrow_mut().switch_path_with(
            self.session.path.as_deref(),
            false,
            Some(syntax),
        );
        self.buffers.active = index;
        self.ui.input_mode = InputMode::Normal;
        self.input.pending = None;
        self.sync_lsp_active();
        self.file.dirty = true;
    }

    pub(crate) fn ensure_current_buffer(&mut self) -> usize {
        self.sync_active();
        if self.buffers.entries.is_empty() {
            self.buffers.entries.push(self.snapshot());
            self.buffers.active = 0;
        }
        self.buffers.active
    }

    pub(crate) fn ensure_buffer_capacity(&mut self) -> Result<(), String> {
        let max_buffers = self.config.settings.editor.max_buffers;
        if self.buffers.entries.len() < max_buffers {
            return Ok(());
        }
        let message = format!("buffer limit reached ({max_buffers})");
        self.status.text = format!("E: {message}");
        self.file.dirty = true;
        Err(message)
    }

    pub(crate) fn new_buffer(&mut self) {
        if self.pager.back.is_some() {
            let _ = self.close_help();
        }
        if self.ensure_buffer_capacity().is_err() {
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        self.ensure_current_buffer();
        self.buffers.entries.push(Buffer {
            editor: EditorState::from_text("").into(),
            path: None,
            cursor: 0,
            missing: false,
            scroll: 0,
            visual_anchor: None,
            visual_line: false,
            hscroll: 0,
            modified: false,
            search_query: String::new(),
            syntax: Default::default(),
        });
        let index = self.buffers.entries.len() - 1;
        self.load_buffer(index);
        self.status.text = "new buffer".to_owned();
        self.sync_watch_paths();
        self.file.dirty = true;
    }

    pub(crate) fn buffer_name(&self, index: usize) -> String {
        if index == self.buffers.active && self.file.stdin_buffer {
            return "[stdin]".to_owned();
        }
        self.buffers
            .entries
            .get(index)
            .and_then(|buffer| buffer.path.as_ref())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "[no file]".to_owned())
    }

    pub(crate) fn save_buffer(&mut self, index: usize) -> bool {
        self.sync_active();
        if index >= self.buffers.entries.len() {
            self.status.text = "E: invalid buffer index".to_owned();
            self.file.dirty = true;
            return false;
        }
        if index == self.buffers.active {
            return self.save();
        }
        let Some(path) = self.buffers.entries[index].path.clone() else {
            self.status.text = "E: buffer has no file name".to_owned();
            self.file.dirty = true;
            return false;
        };
        let text = self.buffers.entries[index]
            .editor
            .text()
            .slice(0..self.buffers.entries[index].editor.text().len_bytes());
        let display = path.display().to_string();
        match self.write_file(&path, &text) {
            Ok(()) => {
                if let Some(buffer) = self.buffers.entries.get_mut(index) {
                    buffer.modified = false;
                    buffer.missing = false;
                }
                self.status.text = format!("\"{display}\" written");
                self.file.dirty = true;
                true
            }
            Err(error) => {
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                false
            }
        }
    }

    pub(crate) fn close_buffer(&mut self, index: usize, force: bool) {
        self.ensure_current_buffer();
        if index >= self.buffers.entries.len() {
            self.status.text = "E: invalid buffer index".to_owned();
            self.file.dirty = true;
            return;
        }
        if self.buffers.entries[index].modified && !force {
            if index == self.buffers.active && self.config.settings.editor.auto_save {
                if !self.auto_save_before_focus_change() {
                    return;
                }
                self.sync_active();
            }
            if self.buffers.entries[index].modified {
                self.ui.input_mode = InputMode::ConfirmClose { index };
                self.status.text =
                    format!("Save {} before closing? [Y/n/c]", self.buffer_name(index));
                self.file.dirty = true;
                return;
            }
        }
        self.finish_close_buffer(index);
        self.sync_watch_paths();
    }
    fn finish_close_buffer(&mut self, index: usize) {
        let was_active = index == self.buffers.active;
        let target_path = self.buffers.entries[index].path.clone();
        let split_same = self.buffers.split.as_ref().is_some_and(|pane| {
            match (&target_path, &pane.buffer.path) {
                (Some(target), Some(split)) => target == split,
                (None, None) => was_active,
                _ => false,
            }
        });
        if split_same
            && self
                .buffers
                .split
                .as_ref()
                .is_some_and(|pane| pane.buffer.modified)
        {
            self.status.text = "E: split pane has unsaved changes".to_owned();
            self.file.dirty = true;
            return;
        }
        if split_same {
            self.buffers.split = None;
            self.ui.focus.split = false;
        }
        self.buffers.entries.remove(index);
        if self.buffers.entries.is_empty() {
            self.reset_empty_buffer();
            self.status.text = "buffer closed".to_owned();
            self.file.dirty = true;
            return;
        }
        if was_active {
            let next = index.min(self.buffers.entries.len() - 1);
            self.load_buffer(next);
        } else if index < self.buffers.active {
            self.buffers.active -= 1;
        }
        self.status.text = format!(
            "buffer {}/{} closed",
            index + 1,
            self.buffers.entries.len() + 1
        );
        self.file.dirty = true;
    }

    fn reset_empty_buffer(&mut self) {
        self.session.editor = EditorState::from_text("").into();
        self.session.path = None;
        self.session.cursor = 0;
        self.view.scroll = 0;
        self.view.hscroll = 0;
        self.file.modified = false;
        self.file.missing = false;
        self.edit.visual_anchor = None;
        self.edit.visual_line = false;
        self.search.query.clear();
        self.search.state.reset_for_buffer();
        self.syntax_cache
            .highlights
            .borrow_mut()
            .switch_path(None, false);
        self.start_page.welcome = false;
        self.start_page.welcome_back = false;
        self.ui.input_mode = InputMode::Normal;
        self.input.pending = None;
        self.buffers.split = None;
        self.ui.focus.split = false;
        self.buffers.active = 0;
        self.sync_lsp_active();
        self.file.dirty = true;
    }

    pub(crate) fn open_file(&mut self, path: &str) {
        if path.is_empty() {
            self.status.text = "E: file name required".to_owned();
            self.file.dirty = true;
            return;
        }
        if self.pager.back.is_some() {
            let _ = self.close_help();
        }
        let path = match self.workspace.absolute_path(Path::new(path)) {
            Ok(path) => path,
            Err(error) => {
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                return;
            }
        };
        if self.session.path.as_ref() == Some(&path) {
            self.status.text = format!("Already editing {}", path.display());
            self.file.dirty = true;
            return;
        }
        if self.session.path.is_none() && self.file.modified {
            self.status.text =
                "E: save or discard the unnamed buffer before opening another file".to_owned();
            self.file.dirty = true;
            return;
        }
        let existing = self
            .buffers
            .entries
            .iter()
            .position(|buffer| buffer.path.as_ref() == Some(&path));
        if existing.is_none() && self.ensure_buffer_capacity().is_err() {
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        if let Some(index) = existing {
            self.sync_active();
            self.load_buffer(index);
            self.status.text = format!(
                "buffer {}/{}: {}",
                index + 1,
                self.buffers.entries.len(),
                path.display()
            );
            self.file.dirty = true;
            return;
        }
        if let Some(mut buffer) = self.buffers.background.remove(&path) {
            if self.session.path.is_some() {
                self.ensure_current_buffer();
            }
            buffer.syntax = Default::default();
            self.buffers.entries.push(buffer);
            let index = self.buffers.entries.len() - 1;
            self.load_buffer(index);
            self.status.text = format!(
                "buffer {}/{}: {}",
                index + 1,
                self.buffers.entries.len(),
                path.display()
            );
            self.file.dirty = true;
            return;
        }
        let text = match self.read_file(&path) {
            Ok(text) => text,
            Err(error) => {
                self.status.text = format!("E: {error}");
                self.file.dirty = true;
                return;
            }
        };
        self.sync_active();
        if self.buffers.entries.is_empty() {
            self.buffers.entries.push(self.snapshot());
        }
        self.buffers.entries.push(Buffer {
            editor: EditorState::from_text(&text).into(),
            path: Some(path.clone()),
            cursor: 0,
            missing: false,
            visual_anchor: None,
            visual_line: false,
            scroll: 0,
            hscroll: 0,
            modified: false,
            search_query: String::new(),
            syntax: Default::default(),
        });
        let index = self.buffers.entries.len() - 1;
        self.load_buffer(index);
        self.status.text = format!(
            "buffer {}/{}: {}",
            index + 1,
            self.buffers.entries.len(),
            path.display()
        );
        self.file.dirty = true;
    }

    pub(crate) fn switch_buffer(&mut self, delta: isize) {
        if self.pager.back.is_some() {
            let _ = self.close_help();
        }
        if self.buffers.entries.is_empty() {
            self.status.text = "E: only one buffer".to_owned();
            self.file.dirty = true;
            return;
        }
        let len = self.buffers.entries.len();
        let target = (self.buffers.active as isize + delta).rem_euclid(len as isize) as usize;
        if target == self.buffers.active {
            self.status.text = "E: only one buffer".to_owned();
            self.file.dirty = true;
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        self.sync_active();
        self.load_buffer(target);
        let name = self
            .session
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "[no file]".to_owned());
        self.status.text = format!("buffer {}/{}: {name}", target + 1, len);
    }

    pub(crate) fn any_modified(&self) -> bool {
        self.file.modified
            || self
                .buffers
                .split
                .as_ref()
                .is_some_and(|pane| pane.buffer.modified)
            || self
                .buffers
                .entries
                .iter()
                .enumerate()
                .any(|(index, buffer)| index != self.buffers.active && buffer.modified)
            || self
                .buffers
                .background
                .values()
                .any(|buffer| buffer.modified)
    }

    pub(crate) fn request_quit(&mut self) {
        if self.close_help() {
            return;
        }
        if !self.auto_save_before_focus_change() {
            return;
        }
        if self.any_modified() {
            self.status.text = "No write since last change (:q! to override)".to_owned();
            self.file.dirty = true;
        } else {
            self.file.quit = true;
        }
    }

    pub(crate) fn force_quit(&mut self) {
        if !self.close_help() {
            self.file.quit = true;
        }
    }

    pub(crate) fn set_editor_mode(&mut self, enabled: bool) {
        self.file.editor_mode = enabled;
    }

    pub(crate) fn exit_code(&self) -> i32 {
        self.file.exit_code
    }

    pub(crate) fn quit_with_code(&mut self, code: i32) {
        self.file.exit_code = code;
        self.file.quit = true;
    }

    pub(crate) fn set_stdin_buffer(&mut self, read_only: bool) {
        self.file.stdin_buffer = true;
        self.file.read_only = read_only;
        self.start_page.welcome = false;
    }

    pub(crate) fn set_read_only(&mut self, read_only: bool) {
        self.file.read_only = read_only;
    }
}

struct BufferPickerSource;

impl PickerSource for BufferPickerSource {
    fn id(&self) -> ProviderId {
        ProviderId::new("builtin.buffers")
    }

    fn query(
        &self,
        context: &PickerContext,
        query: &PickerQuery,
        cancel: &CancellationToken,
        emit: &mut dyn FnMut(Vec<PickerEntry>),
    ) -> Result<(), ProviderError> {
        cancel.check()?;
        let needle = query.text.to_lowercase();
        let mut entries = Vec::new();
        for buffer in &context.buffers {
            if entries.len() >= query.limit {
                break;
            }
            let name = std::path::Path::new(&buffer.name)
                .strip_prefix(&context.root)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| buffer.name.clone());
            if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                continue;
            }
            entries.push(PickerEntry {
                value: json!(buffer.index),
                ordinal: name.to_lowercase(),
                display: format!(
                    "{} {:>2} {}{}",
                    if buffer.active { "*" } else { " " },
                    buffer.index + 1,
                    if buffer.modified { "● " } else { "" },
                    name
                ),
            });
        }
        cancel.check()?;
        emit(entries);
        Ok(())
    }
}

fn buffer_picker_registry() -> ProviderRegistry<dyn PickerSource> {
    let mut registry = ProviderRegistry::new();
    let source: Arc<dyn PickerSource> = Arc::new(BufferPickerSource);
    registry
        .register(
            ProviderId::new("builtin.buffers"),
            ProviderPriority::new(100),
            ProviderOrigin::Builtin,
            source,
        )
        .expect("builtin buffer picker id is valid");
    registry
}
