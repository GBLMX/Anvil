use super::*;
use anvil_session::provider::{
    CancellationToken as ProviderCancellationToken, CompletionContext as ProviderCompletionContext,
    CompletionItem as ProviderCompletionItem, CompletionKind, CompletionPathEntry,
    CompletionSource, ProviderError, ProviderId, ProviderOrigin, ProviderPriority,
    ProviderRegistry,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct CompletionAdditionalEdit {
    pub(crate) start_line: usize,
    pub(crate) start_character: usize,
    pub(crate) end_line: usize,
    pub(crate) end_character: usize,
    pub(crate) new_text: String,
}

pub(crate) fn completion_additional_edits(
    raw: &serde_json::Value,
) -> Vec<CompletionAdditionalEdit> {
    raw.get("additionalTextEdits")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|edit| {
            let start = edit.pointer("/range/start")?;
            let end = edit.pointer("/range/end")?;
            Some(CompletionAdditionalEdit {
                start_line: start.get("line")?.as_u64()? as usize,
                start_character: start.get("character")?.as_u64()? as usize,
                end_line: end.get("line")?.as_u64()? as usize,
                end_character: end.get("character")?.as_u64()? as usize,
                new_text: edit.get("newText")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

pub(crate) fn completion_auto_import(raw: &serde_json::Value) -> Option<String> {
    completion_additional_edits(raw)
        .into_iter()
        .map(|edit| edit.new_text.trim().to_owned())
        .find(|text| !text.is_empty())
}

pub(crate) fn clean_completion_documentation(text: &str) -> String {
    let mut lines = Vec::new();
    let mut previous_blank = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed.starts_with("```") {
            continue;
        }
        if trimmed.is_empty() {
            if previous_blank {
                continue;
            }
            previous_blank = true;
        } else {
            previous_blank = false;
        }
        lines.push(line);
    }
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

impl Model {
    pub(crate) fn install_completion(&mut self, items: Vec<CompletionItem>) {
        if items.is_empty() {
            return;
        }
        let text = self.session.editor.text();
        let query = completion_prefix(&text.slice(0..text.len_bytes()), self.session.cursor);
        let filtered = (0..items.len()).collect();
        let selected = items.iter().position(|item| item.preselect).unwrap_or(0);
        self.overlays.lsp_results.completion = Some(CompletionState {
            items,
            filtered,
            query,
            selected,
        });
        self.push_overlay_if_idle(OverlayKind::Completion);
        self.filter_completion();
    }

    pub(crate) fn local_completion_items(&mut self) -> Vec<CompletionItem> {
        let text = self
            .session
            .editor
            .text()
            .slice(0..self.session.editor.text().len_bytes());
        let cursor = self.session.cursor.min(text.len());
        if !text.is_char_boundary(cursor) {
            return Vec::new();
        }
        let prefix = completion_prefix(&text, cursor);
        let path_prefix = completion_path_prefix(&text, cursor);
        let path_entries = if path_prefix.contains('/') {
            self.path_completion_entries(&path_prefix)
        } else {
            Vec::new()
        };
        if prefix.len() < 2 && path_entries.is_empty() {
            return Vec::new();
        }
        let registry = completion_source_registry();
        let context = ProviderCompletionContext {
            path: self.session.path.as_deref(),
            language_id: "",
            revision: self.session.editor.revision(),
            text: &text,
            cursor,
            prefix: &prefix,
            path_prefix: &path_prefix,
            path_entries: &path_entries,
        };
        let cancel = ProviderCancellationToken::new();
        let mut items = Vec::new();
        for provider in registry.providers() {
            let Ok(batch) = provider.provider.complete(&context, &cancel) else {
                continue;
            };
            let replace = if provider.id.as_str() == "builtin.paths" {
                Some((cursor.saturating_sub(path_prefix.len()), cursor))
            } else {
                Some((cursor.saturating_sub(prefix.len()), cursor))
            };
            items.extend(
                batch
                    .into_iter()
                    .map(|item| completion_from_provider(provider.id, item, replace)),
            );
        }
        items
    }

    fn path_completion_entries(&mut self, path_prefix: &str) -> Vec<CompletionPathEntry> {
        if path_prefix.starts_with('/') {
            return Vec::new();
        }
        let Some((directory, _)) = path_prefix.rsplit_once('/') else {
            return Vec::new();
        };
        let base = self
            .session
            .path
            .as_deref()
            .and_then(std::path::Path::parent)
            .unwrap_or(&self.workspace.path);
        let directory = if directory.is_empty() {
            base.to_path_buf()
        } else {
            base.join(directory)
        };
        let relative = if directory == self.workspace.path {
            ".".to_owned()
        } else {
            let Ok(relative) = self.workspace.relative_path(&directory) else {
                return Vec::new();
            };
            relative
        };
        let Some(gateway) = self.workspace.gateway.as_mut() else {
            return Vec::new();
        };
        let Ok(entries) = gateway.read_dir(&relative) else {
            return Vec::new();
        };
        let mut entries = entries
            .into_iter()
            .filter(|entry| !entry.name.starts_with('.'))
            .map(|entry| CompletionPathEntry {
                name: entry.name,
                is_dir: entry.is_dir,
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| left.name.cmp(&right.name))
        });
        entries
    }
}

pub(crate) fn completion_from_provider(
    source: &ProviderId,
    item: ProviderCompletionItem,
    replace_bytes: Option<(usize, usize)>,
) -> CompletionItem {
    let label = item.label;
    let filter_text = item.filter_text.clone().unwrap_or_else(|| label.clone());
    let sort_text = item.sort_text.clone().unwrap_or_else(|| label.clone());
    CompletionItem {
        source: source.clone(),
        label,
        filter_text,
        sort_text,
        preselect: false,
        insert: item.insert_text,
        snippet: None,
        detail: item.detail.unwrap_or_default(),
        replace: None,
        replace_bytes,
        documentation: None,
        raw: serde_json::Value::Null,
        resolved: true,
    }
}

pub(crate) fn completion_prefix(text: &str, cursor: usize) -> String {
    let mut start = cursor;
    for (index, character) in text[..cursor].char_indices().rev() {
        if character.is_alphanumeric() || character == '_' {
            start = index;
        } else {
            break;
        }
    }
    text[start..cursor].to_owned()
}

fn completion_path_prefix(text: &str, cursor: usize) -> String {
    let mut start = 0;
    for (index, character) in text[..cursor].char_indices().rev() {
        if character.is_whitespace() || matches!(character, '"' | '\'' | '(' | '[' | '{' | ',') {
            start = index + character.len_utf8();
            break;
        }
    }
    text[start..cursor].to_owned()
}

struct BufferCompletionSource;

impl CompletionSource for BufferCompletionSource {
    fn id(&self) -> ProviderId {
        ProviderId::new("builtin.buffer-words")
    }

    fn complete(
        &self,
        context: &ProviderCompletionContext<'_>,
        cancel: &ProviderCancellationToken,
    ) -> Result<Vec<ProviderCompletionItem>, ProviderError> {
        cancel.check()?;
        if context.prefix.len() < 2 {
            return Ok(Vec::new());
        }
        let needle = context.prefix.to_lowercase();
        let mut seen = std::collections::HashSet::new();
        let mut items = Vec::new();
        for word in context
            .text
            .split(|character: char| !character.is_alphanumeric() && character != '_')
        {
            if items.len() >= 200 {
                break;
            }
            if word.len() < 2 || word == context.prefix {
                continue;
            }
            if !word.to_lowercase().starts_with(&needle) || !seen.insert(word.to_owned()) {
                continue;
            }
            items.push(ProviderCompletionItem {
                label: word.to_owned(),
                detail: Some("buffer".to_owned()),
                insert_text: word.to_owned(),
                filter_text: Some(word.to_owned()),
                sort_text: Some(word.to_owned()),
                kind: CompletionKind::Text,
            });
        }
        cancel.check()?;
        Ok(items)
    }
}

struct PathCompletionSource;

impl CompletionSource for PathCompletionSource {
    fn id(&self) -> ProviderId {
        ProviderId::new("builtin.paths")
    }

    fn complete(
        &self,
        context: &ProviderCompletionContext<'_>,
        cancel: &ProviderCancellationToken,
    ) -> Result<Vec<ProviderCompletionItem>, ProviderError> {
        cancel.check()?;
        let Some((directory, fragment)) = context.path_prefix.rsplit_once('/') else {
            return Ok(Vec::new());
        };
        let prefix = if directory.is_empty() {
            String::new()
        } else {
            format!("{directory}/")
        };
        let needle = fragment.to_lowercase();
        let mut items = Vec::new();
        for entry in context.path_entries {
            if items.len() >= 200 {
                break;
            }
            if !entry.name.to_lowercase().starts_with(&needle) {
                continue;
            }
            let suffix = if entry.is_dir { "/" } else { "" };
            let insert = format!("{prefix}{}{suffix}", entry.name);
            items.push(ProviderCompletionItem {
                label: insert.clone(),
                detail: Some(if entry.is_dir { "directory" } else { "path" }.to_owned()),
                insert_text: insert.clone(),
                filter_text: Some(entry.name.clone()),
                sort_text: Some(insert),
                kind: CompletionKind::Text,
            });
        }
        cancel.check()?;
        Ok(items)
    }
}

fn completion_source_registry() -> ProviderRegistry<dyn CompletionSource> {
    let mut registry = ProviderRegistry::new();
    let buffer: Arc<dyn CompletionSource> = Arc::new(BufferCompletionSource);
    let paths: Arc<dyn CompletionSource> = Arc::new(PathCompletionSource);
    registry
        .register(
            ProviderId::new("builtin.buffer-words"),
            ProviderPriority::new(10),
            ProviderOrigin::Builtin,
            buffer,
        )
        .expect("builtin buffer completion id is valid");
    registry
        .register(
            ProviderId::new("builtin.paths"),
            ProviderPriority::new(20),
            ProviderOrigin::Builtin,
            paths,
        )
        .expect("builtin path completion id is valid");
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_documentation_strips_fences_and_collapses_blanks() {
        let text = "```python\nclass Protocol\n```\n\n---\n\nA typing protocol.\n\n";
        assert_eq!(
            clean_completion_documentation(text),
            "class Protocol\n\nA typing protocol."
        );
    }
}
