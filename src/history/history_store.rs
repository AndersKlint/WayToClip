use crate::clipboard::clipboard_entry::ClipboardEntry;

#[derive(Clone, Debug)]
pub struct RemovedEntry {
    pub entry: ClipboardEntry,
    pub was_selected: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ClearResult {
    pub removed: Vec<RemovedEntry>,
    pub should_clear_clipboard: bool,
}

#[derive(Clone, Debug, Default)]
pub struct HistoryStore {
    entries: Vec<ClipboardEntry>,
    selected: Option<usize>,
}

impl HistoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[ClipboardEntry] {
        &self.entries
    }

    pub fn selected(&self) -> Option<&ClipboardEntry> {
        self.selected.and_then(|index| self.entries.get(index))
    }

    pub fn length(&self) -> usize {
        self.entries.len()
    }

    pub fn load(&mut self, entries: Vec<ClipboardEntry>) {
        self.entries = entries;
        self.selected = if self.entries.is_empty() {
            None
        } else {
            Some(self.entries.len() - 1)
        };
    }

    pub fn has(&self, entry: &ClipboardEntry) -> bool {
        self.entries.iter().any(|existing| existing.equals(entry))
    }

    pub fn find_equal(&self, entry: &ClipboardEntry) -> Option<&ClipboardEntry> {
        self.entries.iter().find(|existing| existing.equals(entry))
    }

    pub fn add(&mut self, entry: ClipboardEntry) {
        self.entries.push(entry);
    }

    pub fn select(
        &mut self,
        entry: &ClipboardEntry,
        should_move_first: bool,
    ) -> Option<ClipboardEntry> {
        let index = self.resolve_index(entry)?;
        if should_move_first {
            let existing = self.entries.remove(index);
            let selected = existing.clone();
            self.entries.push(existing);
            self.selected = Some(self.entries.len() - 1);
            Some(selected)
        } else {
            self.selected = Some(index);
            Some(self.entries[index].clone())
        }
    }

    pub fn remove(&mut self, entry: &ClipboardEntry) -> Option<RemovedEntry> {
        let index = self.resolve_index(entry)?;
        let (entry, was_selected) = self.remove_at(index);
        Some(RemovedEntry {
            entry,
            was_selected,
        })
    }

    pub fn toggle_favorite(&mut self, entry: &ClipboardEntry) -> bool {
        let Some(index) = self.resolve_index(entry) else {
            return false;
        };
        let entry = &mut self.entries[index];
        entry.is_favorite = !entry.is_favorite;
        entry.is_favorite
    }

    pub fn trim(&mut self, max_size: usize) -> Vec<RemovedEntry> {
        let mut removed = Vec::new();
        let mut non_favorite_count = self
            .entries
            .iter()
            .filter(|entry| !entry.is_favorite())
            .count();
        while non_favorite_count > max_size {
            let Some(index) = self.entries.iter().position(|entry| !entry.is_favorite()) else {
                break;
            };
            let (entry, was_selected) = self.remove_at(index);
            removed.push(RemovedEntry {
                entry,
                was_selected,
            });
            non_favorite_count -= 1;
        }
        removed
    }

    pub fn clear(&mut self, should_keep_selected: bool) -> ClearResult {
        let mut removed = Vec::new();
        let mut should_clear_clipboard = false;
        let mut index = 0;
        while index < self.entries.len() {
            if self.entries[index].is_favorite() {
                index += 1;
                continue;
            }
            if should_keep_selected && self.selected == Some(index) {
                index += 1;
                continue;
            }
            let (entry, was_selected) = self.remove_at(index);
            should_clear_clipboard |= was_selected;
            removed.push(RemovedEntry {
                entry,
                was_selected,
            });
        }
        ClearResult {
            removed,
            should_clear_clipboard,
        }
    }

    pub fn to_persistable(&self, should_cache_only_favorite: bool) -> Vec<ClipboardEntry> {
        self.entries
            .iter()
            .filter(|entry| !should_cache_only_favorite || entry.is_favorite())
            .cloned()
            .collect()
    }

    fn resolve_index(&self, entry: &ClipboardEntry) -> Option<usize> {
        self.entries
            .iter()
            .position(|existing| std::ptr::eq(existing, entry))
            .or_else(|| {
                self.entries
                    .iter()
                    .position(|existing| existing.equals(entry))
            })
    }

    fn remove_at(&mut self, index: usize) -> (ClipboardEntry, bool) {
        let was_selected = self.selected == Some(index);
        let entry = self.entries.remove(index);
        if was_selected {
            self.selected = if self.entries.is_empty() {
                None
            } else {
                Some(self.entries.len() - 1)
            };
        } else if let Some(selected) = self.selected {
            if selected > index {
                self.selected = Some(selected - 1);
            }
        }
        (entry, was_selected)
    }
}
