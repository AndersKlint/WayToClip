use crate::clipboard::clipboard_entry::ClipboardEntry;
use crate::common::constants::ITEMS_PER_PAGE;

const KEY_0: u32 = 0x30;
const KEY_1: u32 = 0x31;

#[derive(Clone, Debug)]
pub struct PopupSelectionController {
    original_items: Vec<ClipboardEntry>,
    items_to_show: Vec<ClipboardEntry>,
    current_page: usize,
    selected_index: Option<usize>,
    limit_popup_pages: bool,
    max_popup_pages: usize,
    favorites_only: bool,
    favorites_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentPageState<'a> {
    pub entries: &'a [ClipboardEntry],
    pub start: usize,
    pub selected_index: Option<usize>,
    pub current_page: usize,
    pub total_pages: usize,
}

impl Default for PopupSelectionController {
    fn default() -> Self {
        Self {
            original_items: Vec::new(),
            items_to_show: Vec::new(),
            current_page: 0,
            selected_index: None,
            limit_popup_pages: false,
            max_popup_pages: 3,
            favorites_only: false,
            favorites_enabled: true,
        }
    }
}

impl PopupSelectionController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn original_items(&self) -> &[ClipboardEntry] {
        &self.original_items
    }

    pub fn items_to_show(&self) -> &[ClipboardEntry] {
        &self.items_to_show
    }

    pub fn current_page(&self) -> usize {
        self.current_page
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    pub fn original_count(&self) -> usize {
        self.original_items.len()
    }

    pub fn is_favorites_view(&self) -> bool {
        self.favorites_only
    }

    pub fn favorites_enabled(&self) -> bool {
        self.favorites_enabled
    }

    pub fn get_selected_entry(&self) -> Option<&ClipboardEntry> {
        self.selected_target()
    }

    pub fn is_favorites_row_selected(&self) -> bool {
        if !self.favorites_enabled {
            return false;
        }
        let start = self.current_page * ITEMS_PER_PAGE;
        let page_entry_count = ITEMS_PER_PAGE.min(self.items_to_show.len().saturating_sub(start));
        self.selected_index == Some(page_entry_count)
    }

    pub fn get_search_source(&self) -> Vec<ClipboardEntry> {
        self.original_items
            .iter()
            .filter(|entry| !self.favorites_only || entry.is_favorite())
            .cloned()
            .collect()
    }

    pub fn get_current_page_state(&self) -> CurrentPageState<'_> {
        let start = self.current_page * ITEMS_PER_PAGE;
        let end = (start + ITEMS_PER_PAGE).min(self.items_to_show.len());
        CurrentPageState {
            entries: self.items_to_show.get(start..end).unwrap_or(&[]),
            start,
            selected_index: self.selected_index,
            current_page: self.current_page,
            total_pages: self.get_total_pages(),
        }
    }

    pub fn apply_settings(
        &mut self,
        limit_popup_pages: bool,
        max_popup_pages: usize,
        favorites_enabled: bool,
    ) {
        self.limit_popup_pages = limit_popup_pages;
        self.max_popup_pages = max_popup_pages;
        self.favorites_enabled = favorites_enabled;
        if !self.favorites_enabled {
            self.favorites_only = false;
        }
    }

    pub fn reset(&mut self, entries: &[ClipboardEntry]) {
        self.original_items = entries.iter().rev().cloned().collect();
        self.favorites_only = false;
        let source = self.original_items.clone();
        self.show_filtered(&source);
    }

    pub fn show_filtered(&mut self, items: &[ClipboardEntry]) {
        let max_items = self.get_max_items(items.len());
        self.items_to_show = items.iter().take(max_items).cloned().collect();
        self.current_page = 0;
        self.selected_index = self.default_selected_index();
    }

    pub fn confirm_selection(&mut self) -> Option<ClipboardEntry> {
        if self.is_favorites_row_selected() {
            self.toggle_favorites_view();
            return None;
        }
        self.selected_target().cloned()
    }

    pub fn select_by_number_key(&self, key_symbol: u32) -> Option<ClipboardEntry> {
        let index = if key_symbol == KEY_0 {
            9
        } else {
            key_symbol.checked_sub(KEY_1)? as usize
        };
        self.select_by_slot_index(index)
    }

    pub fn select_by_slot_index(&self, index: usize) -> Option<ClipboardEntry> {
        if index >= ITEMS_PER_PAGE {
            return None;
        }
        let start = self.current_page * ITEMS_PER_PAGE;
        self.items_to_show.get(start + index).cloned()
    }

    pub fn navigate_up(&mut self) {
        let row_count = self.row_count();
        if row_count == 0 {
            return;
        }
        let current = self.selected_index.map_or(-1, |index| index as isize);
        let next = if current <= 0 {
            row_count - 1
        } else {
            (current - 1) as usize
        };
        self.selected_index = Some(next);
    }

    pub fn navigate_down(&mut self) {
        let row_count = self.row_count();
        if row_count == 0 {
            return;
        }
        let current = self.selected_index.map_or(-1, |index| index as isize);
        let next = if current >= row_count as isize - 1 {
            0
        } else {
            (current + 1) as usize
        };
        self.selected_index = Some(next);
    }

    pub fn navigate_page_forward(&mut self) {
        let total_pages = self.get_total_pages();
        if total_pages <= 1 {
            return;
        }
        self.current_page = (self.current_page + 1) % total_pages;
        self.selected_index = Some(0);
    }

    pub fn navigate_page_back(&mut self) {
        let total_pages = self.get_total_pages();
        if total_pages <= 1 {
            return;
        }
        self.current_page = (self.current_page + total_pages - 1) % total_pages;
        self.selected_index = Some(0);
    }

    pub fn delete_selected_item(&mut self) -> Option<ClipboardEntry> {
        let target = self.selected_target()?.clone();
        remove_first_equal(&mut self.original_items, &target);
        remove_first_equal(&mut self.items_to_show, &target);

        if self.items_to_show.is_empty() {
            self.current_page = 0;
            self.selected_index = self.default_selected_index();
        } else {
            self.clamp_selection_to_page();
        }
        Some(target)
    }

    pub fn toggle_favorites_view(&mut self) {
        self.set_favorites_view(!self.favorites_only);
    }

    pub fn toggle_favorite_selected(&mut self) -> Option<ClipboardEntry> {
        if !self.favorites_enabled {
            return None;
        }
        let mut target = self.selected_target()?.clone();
        let is_favorite = !target.is_favorite;
        target.is_favorite = is_favorite;

        for entry in &mut self.original_items {
            if entry.equals(&target) {
                entry.is_favorite = is_favorite;
            }
        }
        for entry in &mut self.items_to_show {
            if entry.equals(&target) {
                entry.is_favorite = is_favorite;
            }
        }

        if self.favorites_only && !is_favorite {
            self.items_to_show.retain(|entry| entry.is_favorite());
            if self.items_to_show.is_empty() {
                self.current_page = 0;
                self.selected_index = self.default_selected_index();
            } else {
                self.clamp_selection_to_page();
            }
        } else if let Some(index) = self
            .items_to_show
            .iter()
            .position(|entry| entry.equals(&target))
        {
            self.current_page = index / ITEMS_PER_PAGE;
            self.selected_index = Some(index % ITEMS_PER_PAGE);
        } else if self.items_to_show.is_empty() {
            self.current_page = 0;
            self.selected_index = self.default_selected_index();
        } else {
            self.clamp_selection_to_page();
        }

        Some(target)
    }

    pub fn get_total_pages(&self) -> usize {
        self.items_to_show.len().div_ceil(ITEMS_PER_PAGE).max(1)
    }

    fn set_favorites_view(&mut self, is_on: bool) {
        if !self.favorites_enabled || self.favorites_only == is_on {
            return;
        }
        self.favorites_only = is_on;
        let source = self.get_search_source();
        self.show_filtered(&source);
    }

    fn clamp_selection_to_page(&mut self) {
        let new_page_count = self.get_total_pages();
        if self.current_page >= new_page_count {
            self.current_page = new_page_count - 1;
        }
        let page_start = self.current_page * ITEMS_PER_PAGE;
        let page_count = ITEMS_PER_PAGE.min(self.items_to_show.len() - page_start);
        if let Some(selected_index) = self.selected_index {
            if selected_index >= page_count {
                self.selected_index = Some(page_count.saturating_sub(1));
            }
        }
    }

    fn default_selected_index(&self) -> Option<usize> {
        if !self.items_to_show.is_empty() || self.favorites_enabled {
            Some(0)
        } else {
            None
        }
    }

    fn selected_target(&self) -> Option<&ClipboardEntry> {
        if self.is_favorites_row_selected() {
            return None;
        }
        let index = self.selected_index?;
        let start = self.current_page * ITEMS_PER_PAGE;
        self.items_to_show.get(start + index)
    }

    fn row_count(&self) -> usize {
        let start = self.current_page * ITEMS_PER_PAGE;
        let page_entries = ITEMS_PER_PAGE.min(self.items_to_show.len().saturating_sub(start));
        page_entries + usize::from(self.favorites_enabled)
    }

    fn get_max_items(&self, total_available: usize) -> usize {
        if self.limit_popup_pages {
            self.max_popup_pages.saturating_mul(ITEMS_PER_PAGE)
        } else {
            total_available
        }
    }
}

fn remove_first_equal(items: &mut Vec<ClipboardEntry>, target: &ClipboardEntry) {
    if let Some(index) = items.iter().position(|entry| entry.equals(target)) {
        items.remove(index);
    }
}
