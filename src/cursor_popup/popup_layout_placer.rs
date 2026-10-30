use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{glib, prelude::*};

use crate::clipboard::clipboard_entry::ClipboardEntry;
use crate::common::i18n::translate_native;

use super::image_preview::ImagePreview;
use super::popup_local_shortcuts::PopupLocalShortcuts;
use super::popup_search_controller::PopupSearchController;
use super::popup_selection_controller::PopupSelectionController;
use super::popup_ui::PopupUi;

const DEFAULT_LINES_PER_ITEM: i32 = 3;
const MIN_LINES_PER_ITEM: i32 = 1;
const MIN_WIDTH: i32 = 400;
const MAX_WIDTH: i32 = 600;

pub struct PopupLayoutPlacer {
    available_width: i32,
    available_height: i32,
    natural_width: Cell<i32>,
    natural_height: Cell<i32>,
    pending_scroll: Rc<RefCell<Option<glib::SourceId>>>,
}

impl PopupLayoutPlacer {
    pub fn new() -> Self {
        Self {
            available_width: MIN_WIDTH,
            available_height: i32::MAX,
            natural_width: Cell::new(MIN_WIDTH),
            natural_height: Cell::new(0),
            pending_scroll: Rc::new(RefCell::new(None)),
        }
    }

    pub fn set_available_size(&mut self, width: i32, height: i32) {
        self.available_width = width;
        self.available_height = height;
    }

    pub fn set_available_height(&mut self, height: i32) {
        self.available_height = height;
    }

    pub fn natural_size(&self) -> (i32, i32) {
        (self.natural_width.get(), self.natural_height.get())
    }

    pub fn clear(&mut self) {
        self.available_width = MIN_WIDTH;
        self.available_height = i32::MAX;
        self.natural_width.set(MIN_WIDTH);
        self.natural_height.set(0);
        self.cancel_pending_scroll();
    }

    pub fn render(
        &self,
        ui: &PopupUi,
        selection: &PopupSelectionController,
        _search: &PopupSearchController,
        shortcuts: &PopupLocalShortcuts,
        image_preview_size: i32,
        image_preview: &ImagePreview,
        on_select: &Rc<dyn Fn(ClipboardEntry)>,
        on_favorites: &Rc<dyn Fn()>,
    ) {
        let available_width = self.available_width.max(1);
        let width_cap = available_width.min(MAX_WIDTH);
        let width_floor = MIN_WIDTH.min(width_cap);
        let checksums: Vec<_> = selection
            .get_current_page_state()
            .entries
            .iter()
            .map(|entry| entry.is_image().then(|| entry.checksum()))
            .collect();
        ui.root.set_size_request(width_floor, -1);
        self.build_page(
            ui,
            selection,
            shortcuts,
            image_preview_size,
            image_preview,
            &checksums,
            DEFAULT_LINES_PER_ITEM,
            on_select,
            on_favorites,
        );
        let (_, natural_width, _, _) = ui.root.measure(gtk::Orientation::Horizontal, -1);
        let width = natural_width.clamp(width_floor, width_cap);
        ui.root.set_size_request(width, -1);

        let mut lines = DEFAULT_LINES_PER_ITEM;
        let mut total_height;
        let mut selected_row;

        loop {
            ui.list_scroll
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
            ui.list_scroll.set_max_content_height(-1);
            selected_row = self.build_page(
                ui,
                selection,
                shortcuts,
                image_preview_size,
                image_preview,
                &checksums,
                lines,
                on_select,
                on_favorites,
            );
            total_height = ui.root.measure(gtk::Orientation::Vertical, width).1;
            if total_height <= self.available_height || lines <= MIN_LINES_PER_ITEM {
                break;
            }
            lines -= 1;
        }

        if total_height > self.available_height.max(1) {
            let list_height = ui.list_scroll.measure(gtk::Orientation::Vertical, width).1;
            let non_list_height = total_height.saturating_sub(list_height);
            let max_list_height = self.available_height.saturating_sub(non_list_height).max(0);
            ui.list_scroll
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            ui.list_scroll.set_max_content_height(max_list_height);
        } else {
            ui.list_scroll
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
            ui.list_scroll.set_max_content_height(-1);
        }

        let height = ui.root.measure(gtk::Orientation::Vertical, width).1;
        self.natural_width.set(width);
        self.natural_height.set(height);
        ui.root.queue_resize();
        if let Some(row) = selected_row {
            self.schedule_scroll(ui, row);
        }
    }

    fn build_page(
        &self,
        ui: &PopupUi,
        selection: &PopupSelectionController,
        shortcuts: &PopupLocalShortcuts,
        image_preview_size: i32,
        image_preview: &ImagePreview,
        checksums: &[Option<String>],
        lines: i32,
        on_select: &Rc<dyn Fn(ClipboardEntry)>,
        on_favorites: &Rc<dyn Fn()>,
    ) -> Option<gtk::Widget> {
        while let Some(child) = ui.list.first_child() {
            ui.list.remove(&child);
        }

        let page = selection.get_current_page_state();
        if page.entries.is_empty() {
            ui.list
                .append(&ui.create_empty_label(&empty_text(selection)));
        }

        let mut selected_row: Option<gtk::Widget> = None;
        for (index, entry) in page.entries.iter().enumerate() {
            let entry_for_click = entry.clone();
            let select = Rc::clone(on_select);
            let shortcut = shortcuts.quick_slot_display(index);
            let row = ui.create_item_row(
                entry,
                checksums[index].as_deref().map(|checksum| {
                    image_preview.widget(
                        entry,
                        checksum,
                        image_preview_size,
                        ui.root.scale_factor(),
                    )
                }),
                index,
                shortcut.as_deref(),
                lines,
                move || select(entry_for_click.clone()),
            );
            if page.selected_index == Some(index) {
                row.add_css_class("selected");
                selected_row = Some(row.clone().upcast());
            }
            ui.list.append(&row);
        }

        if selection.favorites_enabled() {
            let favorites = Rc::clone(on_favorites);
            let row = ui.create_favorites_row(selection.is_favorites_view(), move || favorites());
            if selection.is_favorites_row_selected() {
                row.add_css_class("selected");
                selected_row = Some(row.clone().upcast());
            }
            ui.list.append(&row);
        }

        ui.set_page_indicator(
            page.current_page,
            page.total_pages,
            !selection.items_to_show().is_empty(),
        );
        ui.set_favorite_hint_active(
            selection
                .get_selected_entry()
                .is_some_and(ClipboardEntry::is_favorite),
        );

        selected_row
    }

    fn schedule_scroll(&self, ui: &PopupUi, row: gtk::Widget) {
        self.cancel_pending_scroll();
        let scroll = ui.list_scroll.clone();
        let list = ui.list.clone();
        let pending = Rc::clone(&self.pending_scroll);
        let source = glib::idle_add_local_once(move || {
            pending.borrow_mut().take();
            let Some(bounds) = row.compute_bounds(&list) else {
                return;
            };
            let adjustment = scroll.vadjustment();
            let page_size = adjustment.page_size();
            if page_size >= adjustment.upper() {
                return;
            }
            let top = bounds.y() as f64;
            let bottom = top + bounds.height() as f64;
            let value = adjustment.value();
            if top < value {
                adjustment.set_value(top.max(adjustment.lower()));
            } else if bottom > value + page_size {
                adjustment.set_value((bottom - page_size).min(adjustment.upper() - page_size));
            }
        });
        self.pending_scroll.replace(Some(source));
    }

    fn cancel_pending_scroll(&self) {
        if let Some(source) = self.pending_scroll.borrow_mut().take() {
            source.remove();
        }
    }
}

fn empty_text(selection: &PopupSelectionController) -> String {
    if selection.original_count() == 0 {
        translate_native("Clipboard history is empty")
    } else if selection.is_favorites_view() {
        translate_native("No favorites yet")
    } else {
        translate_native("No matching clipboard items")
    }
}
