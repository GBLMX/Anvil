//! Workspace file reading, writing, opening, and saving.

use super::*;
use std::io;
use std::time::Instant;

impl Model {
    pub fn open_path(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let timing = std::env::var_os("ANVIL_OPEN_TIMING").is_some();
        let start = Instant::now();
        let absolute = self.absolute_path(path.as_ref())?;
        if self.path.as_ref() == Some(&absolute) {
            return Ok(());
        }
        if let Some(index) = self
            .buffers
            .iter()
            .position(|buffer| buffer.path.as_ref() == Some(&absolute))
        {
            if self.path.is_some() {
                self.sync_active();
            }
            self.load_buffer(index);
            return Ok(());
        }
        let text = self.read_file(&absolute)?;
        let read_done = start.elapsed();
        if self.path.is_some() {
            self.sync_active();
            if self.buffers.is_empty() {
                self.buffers.push(self.snapshot());
                self.active = 0;
            }
            self.buffers.push(Buffer {
                editor: EditorState::from_text(&text),
                path: Some(absolute),
                cursor: 0,
                scroll: 0,
                hscroll: 0,
                modified: false,
                search_query: String::new(),
            });
            let index = self.buffers.len() - 1;
            self.load_buffer(index);
        } else {
            self.editor = EditorState::from_text(&text);
            self.path = Some(absolute);
            self.cursor = 0;
            self.scroll = 0;
            self.hscroll = 0;
            self.modified = false;
            self.syntax.borrow_mut().invalidate();
        }
        if timing {
            eprintln!("open_path read={read_done:?} index={:?}", start.elapsed());
        }
        self.sync_lsp_active();
        self.dirty = true;
        Ok(())
    }

    pub fn open_with_gateway(
        path: impl AsRef<Path>,
        gateway: GatewayClient,
        workspace: PathBuf,
    ) -> Result<Self, String> {
        let mut model = Self::with_gateway(gateway, workspace);
        model.open_path(path)?;
        Ok(model)
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let text = fs::read_to_string(&path)?;
        let path = path.canonicalize().unwrap_or(path);
        Ok(Self::new(EditorState::from_text(&text), Some(path)))
    }

    pub(crate) fn absolute_path(&self, path: &Path) -> Result<PathBuf, String> {
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        Ok(joined.canonicalize().unwrap_or(joined))
    }

    pub(crate) fn relative_path(&self, path: &Path) -> Result<String, String> {
        let relative = path
            .strip_prefix(&self.workspace)
            .map_err(|_| format!("path is outside workspace {}", self.workspace.display()))?;
        let relative = relative
            .to_str()
            .filter(|relative| !relative.is_empty() && *relative != ".")
            .ok_or_else(|| "invalid file path".to_owned())?;
        Ok(relative.to_owned())
    }

    pub(crate) fn read_file(&mut self, path: &Path) -> Result<String, String> {
        if !self.require_gateway {
            let data = fs::read(path).map_err(|error| error.to_string())?;
            return String::from_utf8(data)
                .map_err(|_| format!("{}: invalid UTF-8", path.display()));
        }
        let relative = self.relative_path(path)?;
        let gateway = self
            .gateway
            .as_mut()
            .ok_or_else(|| "gateway unavailable".to_owned())?;
        let size = gateway
            .file_size(&relative)
            .map_err(|error| error.to_string())?;
        let window = std::env::var("ANVIL_TUI_READ_WINDOW")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|window| *window > 0)
            .unwrap_or(4);
        let data = gateway
            .read_all_binary_pipelined(&relative, size, MAX_OPEN_BYTES, window)
            .map_err(|error| error.to_string())?;
        String::from_utf8(data).map_err(|_| format!("{relative}: invalid UTF-8"))
    }

    pub(crate) fn write_file(&mut self, path: &Path, text: &str) -> Result<(), String> {
        if !self.require_gateway {
            return fs::write(path, text).map_err(|error| error.to_string());
        }
        let relative = self.relative_path(path)?;
        let gateway = self
            .gateway
            .as_mut()
            .ok_or_else(|| "gateway unavailable".to_owned())?;
        gateway
            .write_all_binary(&relative, text.as_bytes())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn save_to(&mut self, path: PathBuf) -> bool {
        if self.reject_read_only() {
            return false;
        }
        let path = match self.absolute_path(&path) {
            Ok(path) => path,
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                return false;
            }
        };
        let text = self.editor.text().slice(0..self.editor.text().len_bytes());
        let display = path.display().to_string();
        match self.write_file(&path, &text) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.modified = false;
                self.git_checked = None;
                self.git_due = Some(Instant::now());
                self.sync_lsp_active();
                self.notify_lsp_save(&path, &text);
                self.status = format!("\"{display}\" written");
                self.dirty = true;
                true
            }
            Err(error) => {
                self.status = format!("E: {error}");
                self.dirty = true;
                false
            }
        }
    }

    pub(crate) fn save(&mut self) -> bool {
        if self.reject_read_only() {
            return false;
        }
        let Some(path) = self.path.clone() else {
            self.status = "E: no file name".to_owned();
            self.dirty = true;
            return false;
        };
        self.save_to(path)
    }

    pub(crate) fn save_all(&mut self) -> bool {
        if self.reject_read_only() {
            return false;
        }
        self.sync_active();
        let active_path = self.path.clone();
        let mut failed = false;
        if self.modified && !self.save() {
            failed = true;
        }
        let mut writes = Vec::new();
        for (index, buffer) in self.buffers.iter().enumerate() {
            if !buffer.modified || buffer.path == active_path {
                continue;
            }
            let Some(path) = &buffer.path else {
                self.status = "E: modified buffer has no file name".to_owned();
                failed = true;
                continue;
            };
            let text = buffer
                .editor
                .text()
                .slice(0..buffer.editor.text().len_bytes());
            writes.push((index, path.clone(), text));
        }
        for (index, path, text) in writes {
            match self.write_file(&path, &text) {
                Ok(()) => {
                    if let Some(buffer) = self.buffers.get_mut(index) {
                        buffer.modified = false;
                    }
                }
                Err(error) => {
                    self.status = format!("E: {}: {error}", path.display());
                    failed = true;
                }
            }
        }
        self.sync_active();
        if !failed {
            self.status = "all files written".to_owned();
        }
        self.dirty = true;
        !failed
    }

    pub(crate) fn save_as(&mut self, path: &str) -> bool {
        self.save_to(PathBuf::from(path))
    }
}
