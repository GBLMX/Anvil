//! Filtering and selection state shared by pickers and command palettes.
//!
//! The table deliberately has no preview or activation policy. Callers own
//! what selecting an item means.

pub(crate) trait SelectionItem {
    fn label(&self) -> &str;
}

#[derive(Debug, Clone)]
pub(crate) struct SelectionTable<T> {
    query: String,
    items: Vec<T>,
    filtered: Vec<usize>,
    selected: usize,
}

impl<T: SelectionItem> SelectionTable<T> {
    pub(crate) fn new(items: Vec<T>) -> Self {
        Self {
            query: String::new(),
            filtered: (0..items.len()).collect(),
            items,
            selected: 0,
        }
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn query_mut(&mut self) -> &mut String {
        &mut self.query
    }

    pub(crate) fn items(&self) -> &[T] {
        &self.items
    }

    pub(crate) fn filtered(&self) -> &[usize] {
        &self.filtered
    }

    pub(crate) fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn selected(&self) -> Option<&T> {
        self.items.get(*self.filtered.get(self.selected)?)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }

    pub(crate) fn refresh(&mut self) {
        self.refresh_with(|item, query| {
            query.is_empty() || item.label().to_ascii_lowercase().contains(query)
        });
    }

    pub(crate) fn refresh_with(&mut self, matches: impl Fn(&T, &str) -> bool) {
        let query = self.query.to_ascii_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches(item, &query))
            .map(|(index, _)| index)
            .collect();
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    pub(crate) fn sort_filtered_by(&mut self, key: impl Fn(&T) -> usize) {
        let items = &self.items;
        self.filtered.sort_by_key(|index| key(&items[*index]));
    }

    pub(crate) fn move_by(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let last = self.filtered.len() - 1;
        self.selected = if delta < 0 {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            (self.selected + delta.unsigned_abs()).min(last)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Item {
        label: &'static str,
        rank: usize,
    }

    impl SelectionItem for Item {
        fn label(&self) -> &str {
            self.label
        }
    }

    fn table() -> SelectionTable<Item> {
        SelectionTable::new(vec![
            Item {
                label: "alpha",
                rank: 2,
            },
            Item {
                label: "beta",
                rank: 0,
            },
            Item {
                label: "gamma",
                rank: 1,
            },
        ])
    }

    #[test]
    fn filters_and_clamps_selection() {
        let mut table = table();
        table.query_mut().push('a');
        table.refresh();
        assert_eq!(table.filtered(), &[0, 1, 2]);
        table.query_mut().clear();
        table.query_mut().push_str("mm");
        table.refresh();
        assert_eq!(table.filtered(), &[2]);
        table.move_by(5);
        assert_eq!(table.selected().map(|item| item.label), Some("gamma"));
    }

    #[test]
    fn sorts_filtered_indices_without_reordering_items() {
        let mut table = table();
        table.sort_filtered_by(|item| item.rank);
        assert_eq!(table.filtered(), &[1, 2, 0]);
        assert_eq!(table.items()[0].label, "alpha");
    }
}
