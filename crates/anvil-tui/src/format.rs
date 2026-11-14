//! External formatter request path; LSP formatting remains the fallback.

use super::*;
use anvil_session::format::FormatService;
use anvil_session::provider::{FormattedText, ProviderId};
use anvil_workspace::FileRequest;

fn formatter_language(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("rs") => Some("rust"),
        Some("go") => Some("go"),
        _ => None,
    }
}

impl Model {
    /// Requests an external formatter, falling back to LSP formatting.
    pub(crate) fn request_format(&mut self) -> bool {
        let Some(path) = self.session.path.clone() else {
            return self.request_formatting();
        };
        let Some(language_id) = formatter_language(&path) else {
            return self.request_formatting();
        };
        self.language
            .lsp_requests
            .pending
            .retain(|_, kind| *kind != LspRequestKind::Formatting);
        self.language.lsp_requests.format_on_save_pending = false;
        if !self.ensure_file_worker() {
            return self.request_formatting();
        }
        let revision = self.session.editor.revision();
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes())
            .to_owned();
        if text.len() > 4 * 1024 * 1024 {
            self.status.text = "E: format: buffer exceeds 4 MiB".to_owned();
            self.file.dirty = true;
            return false;
        }
        let generation = self.file.format_generation.wrapping_add(1);
        self.file.format_generation = generation;
        let Some(worker) = self.file_worker_for_path(&path) else {
            return self.request_formatting();
        };
        if worker
            .request(FileRequest::Format {
                path,
                text,
                language_id: language_id.to_owned(),
                revision,
                generation,
            })
            .is_err()
        {
            return self.request_formatting();
        }
        self.file.format_pending = Some(generation);
        self.status.text = "formatting".to_owned();
        self.file.dirty = true;
        true
    }

    pub(crate) fn handle_format(
        &mut self,
        path: PathBuf,
        revision: u64,
        generation: u64,
        result: Result<Option<String>, String>,
    ) {
        if self.file.format_pending != Some(generation) {
            return;
        }
        self.file.format_pending = None;
        if self.session.path.as_ref() != Some(&path) {
            return;
        }
        match result {
            Ok(None) => {
                if !self.request_formatting() {
                    self.status.text = "E: no formatter available".to_owned();
                }
            }
            Ok(Some(text)) => {
                if self.session.editor.revision() != revision {
                    self.status.text = "E: format: stale revision".to_owned();
                    self.file.dirty = true;
                    return;
                }
                let formatted = FormattedText {
                    provider: ProviderId::new("builtin.exec"),
                    text,
                };
                match FormatService::apply(&mut self.session.editor, revision, &formatted) {
                    Ok(true) => self.status.text = "formatted".to_owned(),
                    Ok(false) => self.status.text = "format: no changes".to_owned(),
                    Err(error) => self.status.text = format!("E: format: {error}"),
                }
            }
            Err(error) => self.status.text = format!("E: format: {error}"),
        }
        self.file.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_editor::EditorState;

    fn model(text: &str) -> (Model, PathBuf) {
        let path = std::env::temp_dir().join("anvil-format-test.rs");
        let model = Model::new(EditorState::from_text(text), Some(path.clone()));
        (model, path)
    }

    #[test]
    fn applies_current_format_result() {
        let (mut model, path) = model("fn main(){}");
        let revision = model.session.editor.revision();
        model.file.format_pending = Some(3);
        model.handle_format(path, revision, 3, Ok(Some("fn main() {}\n".to_owned())));
        assert_eq!(model.session.editor.text().slice(0..13), "fn main() {}\n");
        assert_eq!(model.status.text, "formatted");
        assert_eq!(model.file.format_pending, None);
    }

    #[test]
    fn drops_stale_format_result() {
        let (mut model, path) = model("fn main(){}");
        let stale = model.session.editor.revision().wrapping_sub(1);
        model.file.format_pending = Some(4);
        model.handle_format(path, stale, 4, Ok(Some("changed".to_owned())));
        assert_eq!(model.session.editor.text().slice(0..11), "fn main(){}");
        assert_eq!(model.status.text, "E: format: stale revision");
    }
}
