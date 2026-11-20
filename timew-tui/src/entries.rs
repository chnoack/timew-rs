//! Changing the data: editing, starting, stopping and deleting entries,
//! and undoing the last change. All writes go through `timew-core`.

use timew_core::database::Database;
use timew_core::interval::Interval;
use timew_core::{start_interval_at, this_minute};
use timew_core::paths::Paths;

use crate::app::App;
use crate::filter::{Filter, day_range};
use crate::format::{format_hm, local_moment};
use crate::forms::{DeleteDialog, EditForm, StartForm, parse_tags, tags_to_text};

impl App {
    /// Edit the start and end of the selected entry in the details panel.
    pub(crate) fn open_edit(&mut self) {
        let Some(t) = self.selected() else { return };
        self.edit = Some(EditForm::new(t));
        if !self.wide {
            self.chart = false;
            self.details = true;
        }
    }

    /// `a`: the edit form for a new entry right after the selected one:
    /// from its end, 30 minutes long (shorter if the next entry starts
    /// sooner).
    pub(crate) fn add_entry(&mut self) {
        let Some(t) = self.selected() else { return };
        let iv = &t.interval;
        if iv.is_open() {
            self.notice = Some("The selected entry is still running; a new entry can only follow a finished one.".into());
            self.notice_failed = true;
            return;
        }
        let start = iv.range.end;
        let mut end = start + 30 * 60;
        // the next stored entry, also one hidden by a filter
        let next = Database::open(self.db_root.join("data"))
            .ok()
            .and_then(|mut db| db.intervals().ok().map(|all| all.iter().map(|i| i.range.start).filter(|s| *s > start).min()))
            .flatten();
        if let Some(next) = next {
            end = end.min(next);
        }
        self.edit = Some(EditForm::new_entry(start, end));
        if !self.wide {
            self.chart = false;
            self.details = true;
        }
    }

    pub(crate) fn save_edit(&mut self, adjust: bool) {
        let Some(form) = &mut self.edit else { return };
        let edited = match form.edited() {
            Ok(e) => e,
            Err(e) => return form.error = Some(e),
        };
        let mut paths = Paths::from_env();
        paths.db_dir = self.db_root.clone();
        paths.config_dir = self.config_dir.clone();
        let (adjusted, actions) = match timew_core::modify_interval(&paths, &form.original, &edited, adjust) {
            Ok(result) => result,
            Err(e) if e.starts_with("You cannot overlap") => {
                return form.error = Some("Overlaps another entry. Shift+Enter adjusts it.".into());
            }
            Err(e) => return form.error = Some(e),
        };
        let changed = !actions.is_empty();
        let added = form.is_new();
        if changed {
            self.last_edit = Some((form.original.clone(), actions));
        }
        self.edit = None;
        self.load();
        let row = self.select_interval(&edited);
        let others = match adjusted {
            0 => String::new(),
            1 => " 1 overlapping entry adjusted.".into(),
            n => format!(" {n} overlapping entries adjusted."),
        };
        let done = if added { "Added" } else { "Saved" };
        self.notice = Some(match (changed, row) {
            (false, _) => "No changes.".into(),
            (true, Some(_)) => format!("{done}.{others} u undoes it."),
            (true, None) => format!("{done}; the entry is outside the shown period.{others} u undoes it."),
        });
        self.notice_failed = false;
    }

    /// `d`: ask whether to delete the selected entry.
    pub(crate) fn ask_delete(&mut self) {
        let Some(t) = self.selected() else { return };
        let (interval, id) = (t.interval.clone(), t.id);
        // the entry before it may lie outside the shown period
        let previous = Database::open(self.db_root.join("data"))
            .ok()
            .and_then(|mut db| timew_core::previous_interval(&mut db, &interval).ok().flatten());
        self.confirm_delete = Some(DeleteDialog { interval, id, previous });
    }

    /// Delete the entry of the confirmation dialog. The view stays as it
    /// is; the selection stays on the same row.
    pub(crate) fn delete_entry(&mut self) {
        let Some(DeleteDialog { interval, .. }) = self.confirm_delete.take() else { return };
        let paths = self.edit_paths();
        match timew_core::delete_interval(&paths, &interval) {
            Ok(changes) => {
                let selected = self.table.selected();
                self.last_edit = Some((interval, changes));
                self.load();
                if let Some(i) = selected
                    && !self.summary.rows.is_empty()
                {
                    self.table.select(Some(i.min(self.summary.rows.len() - 1)));
                }
                self.notice = Some("Deleted. u undoes it.".to_string());
                self.notice_failed = false;
            }
            Err(e) => {
                self.notice = Some(format!("Not deleted: {e}"));
                self.notice_failed = true;
                self.load();
            }
        }
    }

    /// `e` in the delete dialog: delete the entry and extend the one before
    /// it to its end. The extended entry is selected.
    pub(crate) fn delete_extending_previous(&mut self) {
        let Some(dialog) = &self.confirm_delete else { return };
        if dialog.previous.is_none() {
            return;
        }
        let interval = dialog.interval.clone();
        self.confirm_delete = None;
        let paths = self.edit_paths();
        match timew_core::delete_extending_previous(&paths, &interval) {
            Ok((extended, changes)) => {
                self.last_edit = Some((interval, changes));
                self.load();
                self.select_interval(&extended);
                let to = if extended.is_open() { "runs on".to_string() } else { format!("ends at {}", local_moment(extended.range.end)) };
                self.notice = Some(format!("Deleted; the previous entry now {to}. u undoes it."));
                self.notice_failed = false;
            }
            Err(e) => {
                self.notice = Some(format!("Not deleted: {e}"));
                self.notice_failed = true;
                self.load();
            }
        }
    }

    /// `s`: stop the running entry, or ask for the tags and annotation of
    /// a new one.
    pub(crate) fn start_or_stop(&mut self) {
        if self.running.is_some() {
            self.stop_entry();
        } else {
            self.start_form = Some(StartForm::default());
        }
    }

    /// `⇧S`: ask for the tags and annotation of a new entry that starts
    /// right after the last one: now if one is running (that one is
    /// stopped now), else when the last entry of today ended, else at 9:00
    /// (or now, if that is earlier).
    pub(crate) fn start_after_last(&mut self) {
        let now = this_minute();
        let hm = |e: i64| local_moment(e).get(11..16).unwrap_or("00:00").to_string();
        let (start, stops_running, note) = if let Some(running) = &self.running {
            let tags = if running.tags.is_empty() { String::new() } else { format!(" ({})", running.tags.join(", ")) };
            (now, true, format!("Stops the running entry{tags}; the new one starts now, {}.", hm(now)))
        } else {
            let today = day_range(jiff::Zoned::now().date());
            let last_end = Database::open(self.db_root.join("data"))
                .ok()
                .and_then(|mut db| db.intervals().ok().map(|all| {
                    all.iter()
                        .filter(|i| i.range.is_ended() && i.range.end > today.start && i.range.end <= now)
                        .map(|i| i.range.end)
                        .max()
                }))
                .flatten();
            match last_end {
                Some(end) => (end, false, format!("The entry starts at {}, when the last one ended.", hm(end))),
                None => {
                    let nine = jiff::Zoned::now().date().at(9, 0, 0, 0).to_zoned(jiff::tz::TimeZone::system())
                        .map_or(today.start + 9 * 3600, |z| z.timestamp().as_second());
                    if nine <= now {
                        (nine, false, "No entry today yet: the entry starts at 09:00.".to_string())
                    } else {
                        (now, false, "No entry today yet: the entry starts now (before 09:00).".to_string())
                    }
                }
            }
        };
        self.start_form = Some(StartForm { start: Some(start), stops_running, note, ..StartForm::default() });
    }

    /// Start the new entry from the start dialog, then show it: the view
    /// switches to today, and the entry is selected and its details open.
    pub(crate) fn start_entry(&mut self) {
        let Some(form) = &mut self.start_form else { return };
        let tags = match parse_tags(&form.fields[0]) {
            Ok(t) => t,
            Err(e) => return form.error = Some(e),
        };
        let annotation = form.fields[1].clone();
        let start = form.start.unwrap_or_else(this_minute);
        let stops_running = form.stops_running;
        let paths = self.edit_paths();
        let (started, changes) = match start_interval_at(&paths, start, stops_running, &tags, Some(&annotation)) {
            Ok(result) => result,
            Err(e) => return self.start_form.as_mut().unwrap().error = Some(e),
        };
        self.start_form = None;
        self.last_edit = Some((Interval::default(), changes));
        self.filter = Filter::Day;
        self.month = None;
        self.year = None;
        if self.tag.as_ref().is_some_and(|t| !started.has_tag(t)) {
            self.tag = None;
        }
        self.load();
        self.select_interval(&started);
        if !self.wide {
            self.chart = false;
            self.details = true;
        }
        let what = if started.tags.is_empty() { String::new() } else { format!(" {}", tags_to_text(&started.tags)) };
        let at = local_moment(started.range.start).get(11..16).unwrap_or("").to_string();
        let stopped = if stops_running { "Stopped the running entry. " } else { "" };
        self.notice = Some(format!("{stopped}Started{what} at {at}. s stops it, u undoes it."));
        self.notice_failed = false;
    }

    /// Stop the running entry now (`s`). The view stays as it is.
    fn stop_entry(&mut self) {
        let paths = self.edit_paths();
        match timew_core::stop_interval(&paths) {
            Ok((before, changes)) => {
                let selected = self.table.selected();
                self.last_edit = Some((before.clone(), changes));
                self.load();
                // keep the stopped entry selected, else the same row
                let row = self.summary.rows.iter().position(|r| {
                    let iv = &self.tracked[r.source].interval;
                    iv.range.start == before.range.start && iv.tags == before.tags
                });
                if let Some(i) = row.or(selected.filter(|i| *i < self.summary.rows.len())) {
                    self.table.select(Some(i));
                }
                let duration = format_hm(this_minute() - before.range.start);
                self.notice = Some(format!("Stopped after {duration}. u undoes it."));
                self.notice_failed = false;
            }
            Err(e) => {
                self.notice = Some(format!("Not stopped: {e}"));
                self.notice_failed = true;
                self.load();
            }
        }
    }

    /// Revert the last saved edit (`u`). Only the last one can be undone.
    pub(crate) fn undo_edit(&mut self) {
        let Some((before, changes)) = self.last_edit.take() else { return };
        let paths = self.edit_paths();
        match timew_core::undo_change(&paths, &changes) {
            Ok(()) => {
                self.load();
                self.select_interval(&before);
                self.notice = Some("Undone.".to_string());
                self.notice_failed = false;
            }
            Err(e) => {
                self.notice = Some(format!("Not undone: {e}"));
                self.notice_failed = true;
                self.load();
            }
        }
    }
}
