use super::*;
use anvil_session::provider::{
    CancellationToken as ProviderCancellationToken, CompletionContext as ProviderCompletionContext,
    CompletionItem as ProviderCompletionItem, CompletionKind, CompletionSource, ProviderError,
    ProviderId, ProviderOrigin, ProviderPriority, ProviderRegistry,
};
use std::sync::Arc;

impl Model {
    pub(crate) fn install_completion(&mut self, items: Vec<CompletionItem>, id: u64) {
        if items.is_empty() {
            return;
        }
        let filtered = (0..items.len()).collect();
        let selected = items.iter().position(|item| item.preselect).unwrap_or(0);
        self.overlays.lsp_results.completion = Some(CompletionState {
            id,
            items,
            filtered,
            query: String::new(),
            selected,
        });
        self.push_overlay_if_idle(OverlayKind::Completion);
    }

    pub(crate) fn local_completion_items(&self) -> Vec<CompletionItem> {
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
        if prefix.len() < 2 {
            return Vec::new();
        }
        let registry = completion_source_registry();
        let Some(provider) = registry.providers().into_iter().next() else {
            return Vec::new();
        };
        let context = ProviderCompletionContext {
            path: self.session.path.as_deref(),
            language_id: "",
            revision: self.session.editor.revision(),
            text: &text,
            cursor,
            prefix: &prefix,
        };
        let cancel = ProviderCancellationToken::new();
        let Ok(items) = provider.provider.complete(&context, &cancel) else {
            return Vec::new();
        };
        items
            .into_iter()
            .map(|item| completion_from_provider(provider.id, item))
            .collect()
    }
}

fn completion_from_provider(source: &ProviderId, item: ProviderCompletionItem) -> CompletionItem {
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
        documentation: None,
        raw: serde_json::Value::Null,
        resolved: true,
    }
}

fn completion_prefix(text: &str, cursor: usize) -> String {
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

fn completion_source_registry() -> ProviderRegistry<dyn CompletionSource> {
    let mut registry = ProviderRegistry::new();
    let source: Arc<dyn CompletionSource> = Arc::new(BufferCompletionSource);
    registry
        .register(
            ProviderId::new("builtin.buffer-words"),
            ProviderPriority::new(10),
            ProviderOrigin::Builtin,
            source,
        )
        .expect("builtin completion source id is valid");
    registry
}
