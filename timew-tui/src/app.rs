//! The application state ([`App`]): what is loaded and shown, the
//! filters, the selection and the open dialogs, and the actions that
//! change the view (filters, pickers, scrolling).

use std::env;
use std::path::PathBuf;

use ratatui::layout::{Position, Rect};
use ratatui::widgets::{ListState, TableState};
use timew_core::database::{Database, Tracked};
use timew_core::datetime::expand_range_hint;
use timew_core::interval::Interval;
use timew_core::paths::Paths;
use timew_core::range::Range;
use timew_core::summary::{self, Summary};

use crate::filter::{Filter, MonthPicker, RangeInput, YearPicker, day_range, month_range, year_range};
use crate::format::local_date;
use crate::forms::{DeleteDialog, EditForm, FIELDS, StartForm};
use crate::ui::PAD;
use crate::ui::popups::legend_lines;

/// A clickable button in the filter bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Button {
    Range(Filter),
    CustomRange,
    Tag,
}

pub(crate) struct App {
    pub(crate) db_root: PathBuf,
    /// Where `timewarrior.cfg` is (read when saving an edit).
    pub(crate) config_dir: PathBuf,
    /// Open edit form (`e`) for the selected entry.
    pub(crate) edit: Option<EditForm>,
    /// Screen areas of the edit form's fields from the last draw.
    pub(crate) edit_fields: [Rect; FIELDS],
    /// A message for the status line, cleared by the next key.
    pub(crate) notice: Option<String>,
    /// Whether the notice reports a failure (red instead of green).
    pub(crate) notice_failed: bool,
    /// Open delete confirmation (`d`).
    pub(crate) confirm_delete: Option<DeleteDialog>,
    /// Open start dialog (`s`).
    pub(crate) start_form: Option<StartForm>,
    /// Screen areas of the start dialog's fields from the last draw.
    pub(crate) start_fields: [Rect; 2],
    /// The running entry, if any (in the whole database, not only the
    /// shown period).
    pub(crate) running: Option<Interval>,
    /// The last saved edit, for `u`: the entry before the edit and every
    /// change the save made (including adjusted entries). Only in memory.
    pub(crate) last_edit: Option<(Interval, Vec<timew_core::session::Change>)>,
    pub(crate) filter: Filter,
    pub(crate) range: Range,
    /// The range used by `Filter::Custom`, and the text it was entered as.
    pub(crate) custom_range: Range,
    pub(crate) custom_input: (String, String),
    /// Open date-range form.
    pub(crate) range_input: Option<RangeInput>,
    /// Screen areas of the From/To fields from the last draw (for mouse clicks).
    pub(crate) range_fields: [Rect; 2],
    pub(crate) range_popup: Rect,
    /// Only show intervals carrying this tag (None = all tags).
    pub(crate) tag: Option<String>,
    /// Every tag in the whole database with its interval count, sorted by name.
    pub(crate) all_tags: Vec<(String, usize)>,
    /// Open tag picker (entry 0 is "All tags", entry i+1 is `all_tags[i]`).
    pub(crate) picker: Option<ListState>,
    /// Screen area of the tag picker list from the last draw (for mouse clicks).
    pub(crate) picker_area: Rect,
    /// Day shown by `Filter::Day`, picked with ⇧1 (None = today).
    pub(crate) day: Option<jiff::civil::Date>,
    /// Month shown by `Filter::Month` (None = the current month).
    pub(crate) month: Option<(i16, i8)>,
    /// Open month picker.
    pub(crate) month_picker: Option<MonthPicker>,
    /// Year shown by `Filter::Year` (None = the current year).
    pub(crate) year: Option<i16>,
    /// Open year picker.
    pub(crate) year_picker: Option<YearPicker>,
    /// Screen area of the year picker list from the last draw.
    pub(crate) year_picker_area: Rect,
    /// Screen area of the month picker list from the last draw.
    pub(crate) month_picker_area: Rect,
    /// Intervals of the current range; `summary::Row::source` indexes this.
    pub(crate) tracked: Vec<Tracked>,
    pub(crate) summary: Summary,
    pub(crate) error: Option<String>,
    pub(crate) table: TableState,
    /// Whether the bottom details panel is open (narrow layout only; the
    /// side panel in the wide layout is always shown).
    pub(crate) details: bool,
    /// Whether the last draw used the wide (side-by-side) layout.
    pub(crate) wide: bool,
    /// Whether the hours-per-tag bar chart is shown (in place of the details).
    pub(crate) chart: bool,
    /// Whether the info dialog (`i`) is open.
    pub(crate) info: bool,
    /// Open key legend (`?`), with its scroll offset in lines.
    pub(crate) legend: Option<u16>,
    /// Lines of the legend that fit on screen at the last draw (page size).
    pub(crate) legend_page: u16,
    /// Screen area of the summary table from the last draw (for mouse clicks).
    pub(crate) table_area: Rect,
    /// Screen areas of the filter buttons from the last draw (for mouse clicks).
    pub(crate) buttons: Vec<(Rect, Button)>,
    /// Number of data rows visible in the table at the last draw (page size).
    pub(crate) page: usize,
    pub(crate) quit: bool,
}

impl App {
    pub(crate) fn new(db_root: PathBuf) -> Self {
        let config_dir = if env::var_os("TIMEWARRIORDB").is_some_and(|v| !v.is_empty()) {
            db_root.clone()
        } else {
            Paths::from_env().config_dir
        };
        let mut app = App {
            db_root,
            config_dir,
            edit: None,
            edit_fields: [Rect::default(); FIELDS],
            notice: None,
            notice_failed: false,
            last_edit: None,
            start_form: None,
            confirm_delete: None,
            start_fields: [Rect::default(); 2],
            running: None,
            filter: Filter::Month,
            range: Range::empty(),
            custom_range: Range::empty(),
            custom_input: (String::new(), String::new()),
            range_input: None,
            range_fields: [Rect::default(); 2],
            range_popup: Rect::default(),
            tag: None,
            all_tags: Vec::new(),
            picker: None,
            picker_area: Rect::default(),
            day: None,
            month: None,
            month_picker: None,
            year: None,
            year_picker: None,
            year_picker_area: Rect::default(),
            month_picker_area: Rect::default(),
            tracked: Vec::new(),
            summary: Summary { rows: Vec::new(), grand_total: 0 },
            error: None,
            table: TableState::default(),
            details: false,
            wide: false,
            chart: false,
            info: false,
            legend: None,
            legend_page: 10,
            table_area: Rect::default(),
            buttons: Vec::new(),
            page: 10,
            quit: false,
        };
        app.load();
        app
    }

    /// (Re)read the database for the current filter. Opens the database
    /// fresh each time so external changes are picked up on reload.
    pub(crate) fn load(&mut self) {
        self.range = match (self.filter, self.month) {
            (Filter::Day, _) if self.day.is_some() => day_range(self.day.unwrap()),
            (Filter::Month, Some((y, m))) => month_range(y, m),
            (Filter::Year, _) if self.year.is_some() => year_range(self.year.unwrap()),
            (f, _) => match f.hint() {
                Some(hint) => expand_range_hint(hint).expect("built-in hint"),
                None => self.custom_range,
            },
        };
        let result = Database::open(self.db_root.join("data")).map_err(|e| e.to_string()).and_then(|mut db| {
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for t in db.all_tracked()? {
                for tag in t.interval.tags {
                    *counts.entry(tag).or_default() += 1;
                }
            }
            self.all_tags = counts.into_iter().collect();
            self.running = db.latest()?.filter(|i| i.is_open());
            let mut tracked = db.tracked_in_range(&self.range)?;
            if let Some(tag) = &self.tag {
                tracked.retain(|t| t.interval.has_tag(tag));
            }
            Ok(tracked)
        });
        match result {
            Ok(tracked) => {
                self.summary = summary::build(&self.range, &tracked);
                self.tracked = tracked;
                self.error = None;
            }
            Err(e) => {
                self.tracked = Vec::new();
                self.summary = Summary { rows: Vec::new(), grand_total: 0 };
                self.error = Some(e);
            }
        }
        self.table = TableState::default();
        if !self.summary.rows.is_empty() {
            self.table.select(Some(0));
        } else {
            self.details = false;
        }
    }

    /// Keys go to the summary table: no form, list or legend is open.
    pub(crate) fn table_has_focus(&self) -> bool {
        self.edit.is_none()
            && self.start_form.is_none()
            && self.confirm_delete.is_none()
            && !self.info
            && self.legend.is_none()
            && self.picker.is_none()
            && self.month_picker.is_none()
            && self.year_picker.is_none()
            && self.range_input.is_none()
    }

    /// Update the times of a running entry (the rows are built for "now").
    pub(crate) fn tick(&mut self) {
        self.summary = summary::build(&self.range, &self.tracked);
    }

    /// The interval behind the currently selected summary row.
    pub(crate) fn selected(&self) -> Option<&Tracked> {
        let row = self.summary.rows.get(self.table.selected()?)?;
        self.tracked.get(row.source)
    }

    pub(crate) fn toggle_details(&mut self) {
        if !self.wide {
            if self.chart {
                // the bottom panel shows the chart: switch it to the details
                self.chart = false;
                self.details = self.selected().is_some();
            } else {
                self.details = !self.details && self.selected().is_some();
            }
        }
    }

    /// Select the row of `interval`, if it is shown. Returns the row.
    pub(crate) fn select_interval(&mut self, interval: &Interval) -> Option<usize> {
        let row = self.summary.rows.iter().position(|r| self.tracked[r.source].interval == *interval);
        if let Some(i) = row {
            self.table.select(Some(i));
        }
        row
    }

    pub(crate) fn edit_paths(&self) -> Paths {
        let mut paths = Paths::from_env();
        paths.db_dir = self.db_root.clone();
        paths.config_dir = self.config_dir.clone();
        paths
    }

    pub(crate) fn open_range_input(&mut self) {
        let (from, to) = if self.filter == Filter::Custom {
            self.custom_input.clone()
        } else {
            // prefill with the current range, end shown inclusive
            let date = |e: i64| local_date(e);
            (date(self.range.start), if self.range.is_ended() { date(self.range.end - 1) } else { String::new() })
        };
        self.range_input = Some(RangeInput { from, to, field: 0, error: None });
    }

    pub(crate) fn open_picker(&mut self) {
        let current = self
            .tag
            .as_ref()
            .and_then(|t| self.all_tags.iter().position(|(name, _)| name == t))
            .map_or(0, |i| i + 1);
        self.picker = Some(ListState::default().with_selected(Some(current)));
    }

    /// Apply picker entry `i` (0 = all tags) and close the picker.
    pub(crate) fn pick(&mut self, i: usize) {
        self.picker = None;
        let tag = i.checked_sub(1).and_then(|i| self.all_tags.get(i)).map(|(t, _)| t.clone());
        if tag != self.tag {
            self.tag = tag;
            self.load();
        }
    }

    pub(crate) fn picker_move(&mut self, delta: isize) {
        let len = self.all_tags.len() as isize + 1;
        if let Some(state) = &mut self.picker {
            let cur = state.selected().unwrap_or(0) as isize;
            state.select(Some((cur + delta).clamp(0, len - 1) as usize));
        }
    }

    /// Back to the start view: the current month, all tags (`Esc`).
    pub(crate) fn reset_filters(&mut self) {
        let default = self.filter == Filter::Month
            && self.day.is_none()
            && self.month.is_none()
            && self.year.is_none()
            && self.tag.is_none();
        if default {
            return;
        }
        self.filter = Filter::Month;
        self.day = None;
        self.month = None;
        self.year = None;
        self.tag = None;
        self.load();
        self.notice = Some("All filters reset: current month, all tags.".to_string());
        self.notice_failed = false;
    }

    pub(crate) fn set_filter(&mut self, filter: Filter) {
        // choosing Day/Month/Year directly always means today/the current month/year
        if filter != self.filter
            || (filter == Filter::Day && self.day.is_some())
            || (filter == Filter::Month && self.month.is_some())
            || (filter == Filter::Year && self.year.is_some())
        {
            self.filter = filter;
            self.day = None;
            self.month = None;
            self.year = None;
            self.load();
        }
    }

    /// ⇧1: show the day of the selected row (the day the row is listed
    /// under), keeping the entry selected.
    pub(crate) fn pick_day(&mut self) {
        let Some(i) = self.table.selected() else { return };
        let Some(date) = self.summary.rows.get(..=i).and_then(|rows| rows.iter().rev().find_map(|r| r.date.clone()))
        else {
            return;
        };
        let Ok(date) = date.parse::<jiff::civil::Date>() else { return };
        let interval = self.selected().map(|t| t.interval.clone());
        self.filter = Filter::Day;
        self.day = Some(date);
        self.month = None;
        self.year = None;
        self.load();
        if let Some(interval) = interval {
            self.select_interval(&interval);
        }
    }

    /// Open the list of the last 5 years (current year first).
    pub(crate) fn open_year_picker(&mut self) {
        let this_year = jiff::Zoned::now().year();
        let years: Vec<i16> = (0..5).map(|i| this_year - i).collect();
        let shown = jiff::Timestamp::from_second(self.range.start).unwrap().to_zoned(jiff::tz::TimeZone::system()).year();
        let selected = years.iter().position(|y| *y == shown).unwrap_or(0);
        self.year_picker = Some(YearPicker { years, state: ListState::default().with_selected(Some(selected)) });
    }

    pub(crate) fn pick_year(&mut self, i: usize) {
        if let Some(p) = self.year_picker.take() {
            self.filter = Filter::Year;
            self.day = None;
            self.month = None;
            self.year = p.years.get(i).copied();
            self.load();
        }
    }

    /// Open the month list for the year currently shown.
    pub(crate) fn open_month_picker(&mut self) {
        let shown = jiff::Timestamp::from_second(self.range.start).unwrap().to_zoned(jiff::tz::TimeZone::system());
        let year = shown.year();
        let selected = shown.month() as usize - 1;
        self.month_picker = Some(MonthPicker { year, state: ListState::default().with_selected(Some(selected)) });
    }

    pub(crate) fn month_picker_year(&mut self, delta: i16) {
        if let Some(p) = &mut self.month_picker {
            p.year += delta;
        }
    }

    pub(crate) fn pick_month(&mut self, m: usize) {
        if let Some(p) = self.month_picker.take() {
            self.filter = Filter::Month;
            self.day = None;
            self.month = Some((p.year, m as i8 + 1));
            self.load();
        }
    }

    pub(crate) fn scroll_by(&mut self, delta: isize) {
        let len = self.summary.rows.len();
        if len == 0 {
            return;
        }
        let cur = self.table.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, len as isize - 1) as usize;
        self.table.select(Some(next));
    }

    pub(crate) fn legend_scroll(&mut self, delta: i32) {
        if let Some(offset) = &mut self.legend {
            let max = legend_lines().len().saturating_sub(self.legend_page as usize) as i32;
            *offset = (*offset as i32 + delta).clamp(0, max) as u16;
        }
    }

    /// Summary row index under a screen position, if any.
    pub(crate) fn row_at(&self, pos: Position) -> Option<usize> {
        let a = self.table_area;
        // border + top padding + header row above the first data row
        let first = a.y + 1 + PAD.top + 1;
        if pos.x <= a.x || pos.x + 1 >= a.right() || pos.y < first || pos.y + 1 >= a.bottom() {
            return None;
        }
        let i = self.table.offset() + (pos.y - first) as usize;
        (i < self.summary.rows.len()).then_some(i)
    }
}
