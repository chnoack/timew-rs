//! Keyboard and mouse input: which key does what in the main view and in
//! each dialog.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use ratatui::widgets::ListState;

use crate::app::{App, Button};
use crate::filter::{Filter, parse_range_input};
use crate::forms::{FIELDS, is_text};

impl App {
    fn on_edit_key(&mut self, key: KeyEvent) {
        let Some(form) = &mut self.edit else { return };
        if let Some(suggestion) = form.suggestion(&self.all_tags)
            && accepts_suggestion(key)
        {
            form.accept(&suggestion);
            return;
        }
        let typed = typed_char(key);
        match key.code {
            KeyCode::Esc => self.edit = None,
            KeyCode::Tab | KeyCode::Down => form.focus(form.field + 1, false),
            KeyCode::BackTab | KeyCode::Up => form.focus(form.field + FIELDS - 1, false),
            // the cursor moves into the neighboring field at either end
            KeyCode::Left if form.cursor == 0 => form.focus(form.field + FIELDS - 1, true),
            KeyCode::Left => form.cursor -= 1,
            KeyCode::Right if form.cursor >= form.len() => form.focus(form.field + 1, false),
            KeyCode::Right => form.cursor += 1,
            KeyCode::Home => form.cursor = 0,
            KeyCode::End => form.cursor = form.len(),
            KeyCode::Backspace if form.cursor > 0 => {
                form.cursor -= 1;
                form.remove(form.cursor);
            }
            KeyCode::Delete => form.remove(form.cursor),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.fields[form.field].clear();
                form.cursor = 0;
            }
            // Ctrl+A / Ctrl+E: start / end of the field, as in a shell
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => form.cursor = 0,
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => form.cursor = form.len(),
            KeyCode::Char(c) => form.type_char(c),
            // Shift+Enter (where the terminal reports it) or Alt+Enter: save and
            // adjust overlapping entries, like `timew modify … :adjust`
            KeyCode::Enter if key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => self.save_edit(true),
            KeyCode::Enter => self.save_edit(false),
            _ => {}
        }
        if let Some(form) = &mut self.edit {
            form.suggest = typed;
        }
    }

    fn on_delete_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => self.delete_entry(),
            KeyCode::Char('e') | KeyCode::Char('E') => self.delete_extending_previous(),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Char('q') => self.confirm_delete = None,
            _ => {}
        }
    }

    fn on_start_key(&mut self, key: KeyEvent) {
        let Some(form) = &mut self.start_form else { return };
        if let Some(suggestion) = form.suggestion(&self.all_tags)
            && accepts_suggestion(key)
        {
            form.accept(&suggestion);
            return;
        }
        let typed = typed_char(key);
        match key.code {
            KeyCode::Esc => self.start_form = None,
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => form.focus(form.field + 1),
            KeyCode::Left => form.cursor = form.cursor.saturating_sub(1),
            KeyCode::Right => form.cursor = (form.cursor + 1).min(form.len()),
            KeyCode::Home => form.cursor = 0,
            KeyCode::End => form.cursor = form.len(),
            KeyCode::Backspace if form.cursor > 0 => {
                form.cursor -= 1;
                form.remove(form.cursor);
            }
            KeyCode::Delete => form.remove(form.cursor),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.fields[form.field].clear();
                form.cursor = 0;
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => form.cursor = 0,
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => form.cursor = form.len(),
            KeyCode::Char(c) => form.insert(c),
            KeyCode::Enter => self.start_entry(),
            _ => {}
        }
        if let Some(form) = &mut self.start_form {
            form.suggest = typed;
        }
    }

    fn on_range_key(&mut self, key: KeyEvent) {
        let Some(input) = &mut self.range_input else { return };
        match key.code {
            KeyCode::Esc => self.range_input = None,
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => input.field = 1 - input.field,
            KeyCode::Backspace => {
                input.active().pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => input.active().clear(),
            KeyCode::Char(c) => {
                input.active().push(c);
                input.error = None;
            }
            KeyCode::Enter => match parse_range_input(&input.from, &input.to) {
                Ok(range) => {
                    self.custom_input = (input.from.trim().to_string(), input.to.trim().to_string());
                    self.custom_range = range;
                    self.range_input = None;
                    self.filter = Filter::Custom;
                    self.load();
                }
                Err(e) => input.error = Some(e),
            },
            _ => {}
        }
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('6') => self.picker = None,
            KeyCode::Down | KeyCode::Char('j') => self.picker_move(1),
            KeyCode::Up | KeyCode::Char('k') => self.picker_move(-1),
            KeyCode::PageDown => self.picker_move(10),
            KeyCode::PageUp => self.picker_move(-10),
            KeyCode::Home | KeyCode::Char('g') => self.picker_move(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.picker_move(isize::MAX / 2),
            KeyCode::Backspace => self.pick(0), // clear the tag filter
            KeyCode::Enter | KeyCode::Char(' ') => {
                let i = self.picker.as_ref().and_then(ListState::selected).unwrap_or(0);
                self.pick(i);
            }
            _ => {}
        }
    }

    fn on_year_picker_key(&mut self, key: KeyEvent) {
        let Some(p) = &mut self.year_picker else { return };
        let last = p.years.len() as isize - 1;
        let cur = p.state.selected().unwrap_or(0) as isize;
        let mut go = |d: isize| p.state.select(Some((cur + d).clamp(0, last) as usize));
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.year_picker = None,
            KeyCode::Down | KeyCode::Char('j') => go(1),
            KeyCode::Up | KeyCode::Char('k') => go(-1),
            KeyCode::Home | KeyCode::Char('g') => go(-last - 1),
            KeyCode::End | KeyCode::Char('G') => go(last + 1),
            KeyCode::Enter | KeyCode::Char(' ') => self.pick_year(cur as usize),
            // like pressing 4: back to the current year
            KeyCode::Backspace => {
                self.year_picker = None;
                self.set_filter(Filter::Year);
            }
            _ => {}
        }
    }

    fn on_month_picker_key(&mut self, key: KeyEvent) {
        let Some(p) = &mut self.month_picker else { return };
        let cur = p.state.selected().unwrap_or(0) as isize;
        let mut go = |d: isize| p.state.select(Some((cur + d).clamp(0, 11) as usize));
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.month_picker = None,
            KeyCode::Down | KeyCode::Char('j') => go(1),
            KeyCode::Up | KeyCode::Char('k') => go(-1),
            KeyCode::Home | KeyCode::Char('g') => go(-12),
            KeyCode::End | KeyCode::Char('G') => go(12),
            KeyCode::Left | KeyCode::Char('h') => self.month_picker_year(-1),
            KeyCode::Right | KeyCode::Char('l') => self.month_picker_year(1),
            KeyCode::Enter | KeyCode::Char(' ') => self.pick_month(cur as usize),
            // like pressing 3: back to the current month
            KeyCode::Backspace => {
                self.month_picker = None;
                self.set_filter(Filter::Month);
            }
            _ => {}
        }
    }

    pub(crate) fn on_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        self.notice = None;
        if self.edit.is_some() {
            return self.on_edit_key(key);
        }
        if self.start_form.is_some() {
            return self.on_start_key(key);
        }
        if self.confirm_delete.is_some() {
            return self.on_delete_key(key);
        }
        if self.info {
            // any key closes the info dialog
            self.info = false;
            return;
        }
        if self.legend.is_some() {
            return self.on_legend_key(key);
        }
        if self.picker.is_some() {
            return self.on_picker_key(key);
        }
        if self.month_picker.is_some() {
            return self.on_month_picker_key(key);
        }
        if self.year_picker.is_some() {
            return self.on_year_picker_key(key);
        }
        if self.range_input.is_some() {
            return self.on_range_key(key);
        }
        if self.export.is_some() {
            return self.on_export_key(key);
        }
        let page = self.page.max(1) as isize;
        match key.code {
            KeyCode::Esc if self.chart && !self.wide => self.chart = false,
            KeyCode::Esc if self.details && !self.wide => self.details = false,
            // the wide layout always shows the chart; b toggles it in the narrow one
            // Ctrl+F / Ctrl+B: a page forward / back (as in less and vi)
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => self.scroll_by(page),
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => self.scroll_by(-page),
            KeyCode::Char('b') if !self.wide => self.chart = !self.chart,
            KeyCode::Enter => self.toggle_details(),
            KeyCode::Char('i') => self.info = true,
            KeyCode::Char('e') => self.open_edit(),
            KeyCode::Char('a') => self.add_entry(),
            KeyCode::Char('x') => self.open_export(),
            KeyCode::Char('u') => self.undo_edit(),
            KeyCode::Char('s') => self.start_or_stop(),
            KeyCode::Char('S') => self.start_after_last(),
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => self.reset_filters(),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_by(page),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Home | KeyCode::Char('g') => self.scroll_by(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.scroll_by(isize::MAX / 2),
            // ⇧1 ("!" on US and German keyboards): the day of the selected entry
            KeyCode::Char('!') => self.pick_day(),
            KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::SHIFT) => self.pick_day(),
            KeyCode::Char('1') => self.set_filter(Filter::Day),
            KeyCode::Char('d') => self.ask_delete(),
            KeyCode::Char('2') => self.set_filter(Filter::Yesterday),
            // Shift+3: '§' on German, '#' on US layouts (or '3' + SHIFT where reported)
            KeyCode::Char('§') | KeyCode::Char('#') | KeyCode::Char('M') => self.open_month_picker(),
            KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::SHIFT) => self.open_month_picker(),
            KeyCode::Char('3') | KeyCode::Char('m') => self.set_filter(Filter::Month),
            // Shift+4: '$' on German and US layouts (or '4' + SHIFT where reported)
            KeyCode::Char('$') => self.open_year_picker(),
            KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::SHIFT) => self.open_year_picker(),
            KeyCode::Char('4') | KeyCode::Char('Y') => self.set_filter(Filter::Year),
            KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => self.set_filter(self.filter.cycle(1)),
            KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => self.set_filter(self.filter.cycle(-1)),
            KeyCode::Char('5') | KeyCode::Char('c') => self.open_range_input(),
            KeyCode::Char('6') => self.open_picker(),
            KeyCode::Char('?') => self.legend = Some(0),
            KeyCode::Char('r') => self.load(),
            _ => {}
        }
    }

    fn on_legend_key(&mut self, key: KeyEvent) {
        let page = self.legend_page.max(1) as i32;
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => self.legend = None,
            KeyCode::Down | KeyCode::Char('j') => self.legend_scroll(1),
            KeyCode::Up | KeyCode::Char('k') => self.legend_scroll(-1),
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => self.legend_scroll(page),
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => self.legend_scroll(-page),
            KeyCode::PageDown | KeyCode::Char(' ') => self.legend_scroll(page),
            KeyCode::PageUp => self.legend_scroll(-page),
            KeyCode::Home | KeyCode::Char('g') => self.legend_scroll(i32::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.legend_scroll(i32::MAX / 2),
            _ => {}
        }
    }

    pub(crate) fn on_mouse(&mut self, mouse: MouseEvent) {
        let pos = Position::new(mouse.column, mouse.row);
        if self.confirm_delete.is_some() || self.export.is_some() {
            return; // only keys answer these
        }
        if let Some(form) = &mut self.start_form {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind
                && let Some(i) = self.start_fields.iter().position(|r| r.contains(pos))
            {
                let scroll = if i == form.field { form.scroll } else { 0 };
                form.field = i;
                form.cursor = (scroll + pos.x.saturating_sub(self.start_fields[i].x + 1) as usize).min(form.len());
            }
            return;
        }
        if self.info {
            if let MouseEventKind::Down(_) = mouse.kind {
                self.info = false;
            }
            return;
        }
        if let Some(form) = &mut self.edit {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind
                && let Some(i) = self.edit_fields.iter().position(|r| r.contains(pos))
            {
                let scroll = if i == form.field && is_text(i) { form.scroll } else { 0 };
                form.field = i;
                form.cursor = (scroll + pos.x.saturating_sub(self.edit_fields[i].x + 1) as usize).min(form.len());
            }
            return;
        }
        if self.legend.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollDown => self.legend_scroll(3),
                MouseEventKind::ScrollUp => self.legend_scroll(-3),
                MouseEventKind::Down(_) => self.legend = None,
                _ => {}
            }
            return;
        }
        if self.picker.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollDown => self.picker_move(1),
                MouseEventKind::ScrollUp => self.picker_move(-1),
                MouseEventKind::Down(MouseButton::Left) => {
                    let a = self.picker_area;
                    if a.contains(pos) {
                        let offset = self.picker.as_ref().map_or(0, ListState::offset);
                        let i = offset + (pos.y - a.y) as usize;
                        if i <= self.all_tags.len() {
                            self.pick(i);
                        }
                    } else {
                        self.picker = None; // click outside closes
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(p) = &mut self.year_picker {
            let last = p.years.len() - 1;
            match mouse.kind {
                MouseEventKind::ScrollDown => p.state.select(Some((p.state.selected().unwrap_or(0) + 1).min(last))),
                MouseEventKind::ScrollUp => p.state.select(Some(p.state.selected().unwrap_or(0).saturating_sub(1))),
                MouseEventKind::Down(MouseButton::Left) => {
                    let a = self.year_picker_area;
                    if a.contains(pos) {
                        let i = p.state.offset() + (pos.y - a.y) as usize;
                        if i <= last {
                            self.pick_year(i);
                        }
                    } else {
                        self.year_picker = None;
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(p) = &mut self.month_picker {
            match mouse.kind {
                MouseEventKind::ScrollDown => p.state.select(Some((p.state.selected().unwrap_or(0) + 1).min(11))),
                MouseEventKind::ScrollUp => p.state.select(Some(p.state.selected().unwrap_or(0).saturating_sub(1))),
                MouseEventKind::Down(MouseButton::Left) => {
                    let a = self.month_picker_area;
                    if a.contains(pos) {
                        let i = p.state.offset() + (pos.y - a.y) as usize;
                        if i < 12 {
                            self.pick_month(i);
                        }
                    } else {
                        self.month_picker = None;
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(input) = &mut self.range_input {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                match self.range_fields.iter().position(|r| r.contains(pos)) {
                    Some(i) => input.field = i,
                    None if !self.range_popup.contains(pos) => self.range_input = None,
                    None => {}
                }
            }
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollDown => self.scroll_by(3),
            MouseEventKind::ScrollUp => self.scroll_by(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((_, b)) = self.buttons.iter().find(|(r, _)| r.contains(pos)) {
                    match b.clone() {
                        Button::Range(f) => self.set_filter(f),
                        Button::Tag => self.open_picker(),
                        Button::CustomRange => self.open_range_input(),
                    }
                } else if let Some(i) = self.row_at(pos) {
                    // first click selects, clicking the selected row toggles details
                    if self.table.selected() == Some(i) {
                        self.toggle_details();
                    } else {
                        self.table.select(Some(i));
                    }
                }
            }
            // right-click on the Month/Year button opens the month/year list
            MouseEventKind::Down(MouseButton::Right) => {
                match self.buttons.iter().find(|(r, _)| r.contains(pos)).map(|(_, b)| b) {
                    Some(Button::Range(Filter::Month)) => self.open_month_picker(),
                    Some(Button::Range(Filter::Year)) => self.open_year_picker(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Tab, → and End take a suggested tag.
fn accepts_suggestion(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Tab | KeyCode::Right | KeyCode::End) && key.modifiers.is_empty()
}

/// Whether `key` types a character (so a tag may be suggested after it).
fn typed_char(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char(_)) && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}
