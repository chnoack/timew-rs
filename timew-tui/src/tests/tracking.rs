use super::*;

#[test]
fn s_starts_an_entry_and_shows_it() {
    let root = yesterday_db("start");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    // a year view filtered by a tag the new entry won't have
    press(&mut app, '4');
    app.tag = Some("planning".into());
    app.load();
    assert!(footer(&mut app).starts_with(" ? help  s start "), "{}", footer(&mut app));

    press(&mut app, 's');
    assert!(render_at(&mut app, 140, 24).contains("Start tracking"));
    type_text(&mut app, "focus \"deep work");
    key(&mut app, KeyCode::Enter);
    assert!(render_at(&mut app, 140, 24).contains("Tags: a quote is not closed."));
    type_text(&mut app, "\"");
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "writing");
    key(&mut app, KeyCode::Enter);

    assert!(app.start_form.is_none());
    assert_eq!((app.filter, app.tag.clone()), (Filter::Day, None), "today, without the tag filter");
    let iv = app.selected().unwrap().interval.clone();
    assert!(iv.is_open(), "the new entry is selected");
    assert_eq!(iv.tags, vec!["deep work", "focus"]);
    assert_eq!(iv.annotation.as_deref(), Some("writing"));
    let line = format!("inc {} # \"deep work\" focus # \"writing\"\n", timew_core::interval::format_iso(iv.range.start));
    assert!(all_lines(&root).contains(&line), "{}", all_lines(&root));
    let screen = render_at(&mut app, 140, 24);
    assert!(screen.contains("Entry details · running") && screen.contains("Started"), "{screen}");
    assert_eq!(panel_border(&mut app, "Entry details"), Color::Red, "running: red border");
    assert!(footer(&mut app).starts_with(" ? help  s stop "));

    // a stopped entry has the normal border
    press(&mut app, '2');
    assert!(!app.selected().unwrap().interval.is_open());
    assert_eq!(panel_border(&mut app, "Entry details"), ACCENT);
    // starting again is not offered while one runs; s stops instead
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn s_stops_the_running_entry_and_keeps_the_view() {
    let root = yesterday_db("stop");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, 's');
    type_text(&mut app, "work");
    key(&mut app, KeyCode::Enter);
    let started = app.selected().unwrap().interval.clone();
    std::thread::sleep(std::time::Duration::from_millis(1100));

    press(&mut app, '4'); // a different view while it runs
    press(&mut app, 's');
    assert_eq!(app.filter, Filter::Year, "stopping keeps the filters");
    let stopped = app.selected().unwrap().interval.clone();
    assert!(stopped.is_ended() && stopped.range.start == started.range.start, "the stopped entry stays selected");
    assert!(render_at(&mut app, 140, 24).contains("Stopped after"));
    assert!(footer(&mut app).starts_with(" ? help  s start "));
    assert_ne!(panel_border(&mut app, "Entry details"), Color::Red);
    assert!(!all_lines(&root).contains(&format!("inc {} # work\n", timew_core::interval::format_iso(started.range.start))));

    // u undoes the stop: running again
    press(&mut app, 'u');
    assert!(all_lines(&root).contains(&format!("inc {} # work\n", timew_core::interval::format_iso(started.range.start))));
    assert!(footer(&mut app).starts_with(" ? help  s stop "));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn s_in_a_narrow_window_opens_the_details() {
    let root = yesterday_db("start-narrow");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    render(&mut app);
    press(&mut app, 's');
    key(&mut app, KeyCode::Enter); // no tags, no annotation
    let screen = render(&mut app);
    assert!(!app.wide && app.details);
    assert!(screen.contains("Entry details · running"), "{screen}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn d_deletes_the_selected_entry_after_asking() {
    let root = sample_db("delete");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    assert_eq!(app.filter, Filter::Day);
    assert!(footer(&mut app).contains("d delete"), "{}", footer(&mut app));
    let before = today_file(&root);
    assert_eq!(app.selected().unwrap().interval.tags, vec!["meetings".to_string()]);

    // n cancels
    press(&mut app, 'd');
    let screen = render_at(&mut app, 140, 24);
    assert!(screen.contains("Delete this entry?") && screen.contains("y delete"), "{screen}");
    press(&mut app, 'n');
    assert!(app.confirm_delete.is_none());
    assert_eq!(today_file(&root), before);

    // only y deletes: Enter, other keys and clicks don't
    press(&mut app, 'd');
    key(&mut app, KeyCode::Enter);
    press(&mut app, 'x');
    app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 1, row: 1, modifiers: KeyModifiers::NONE });
    assert!(app.confirm_delete.is_some());
    assert_eq!(today_file(&root), before);
    press(&mut app, 'y');
    assert!(app.confirm_delete.is_none());
    assert!(!today_file(&root).contains("meetings"));
    assert!(render_at(&mut app, 140, 24).contains("Deleted. u undoes it."));
    assert_eq!(app.filter, Filter::Day, "the view stays");
    assert_eq!(app.selected().unwrap().interval.tags, vec!["email".to_string()], "the next entry is selected");

    // u brings it back
    press(&mut app, 'u');
    assert_eq!(today_file(&root), before);
    assert_eq!(app.selected().unwrap().interval.tags, vec!["meetings".to_string()]);

    // nothing selected: nothing to delete
    app.filter = Filter::Month;
    app.month = Some((2000, 1));
    app.load();
    press(&mut app, 'd');
    assert!(app.confirm_delete.is_none());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn e_in_the_delete_dialog_extends_the_previous_entry() {
    let root = sample_db("delete-prev");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    let before = today_file(&root);
    // today: meetings 08-10, email 10-11; select email
    press(&mut app, 'j');
    assert_eq!(app.selected().unwrap().interval.tags, vec!["email".to_string()]);
    press(&mut app, 'd');
    let screen = render_at(&mut app, 140, 30);
    println!("{screen}");
    assert!(screen.contains("e instead deletes it and extends the previous entry"), "{screen}");
    assert!(screen.contains("meetings,") && screen.contains("e delete + extend previous"));
    press(&mut app, 'a'); // not the key for it
    assert!(app.confirm_delete.is_some());
    press(&mut app, 'e');
    assert!(app.confirm_delete.is_none());
    let data = today_file(&root);
    assert!(!data.contains("email"));
    assert!(data.contains(&format!("inc {} - {} # meetings", today_at(8, 0), today_at(11, 0))), "{data}");
    let iv = app.selected().unwrap().interval.clone();
    assert_eq!((iv.tags.clone(), iv.range.end), (vec!["meetings".to_string()], timew_core::interval::parse_iso(&today_at(11, 0)).unwrap()));
    assert!(render_at(&mut app, 140, 24).contains("Deleted; the previous entry now ends at"));
    press(&mut app, 'u');
    assert_eq!(today_file(&root), before, "u restores both entries");

    // the first entry of all has no previous one: no option, e does nothing
    press(&mut app, '2');
    press(&mut app, 'd');
    let screen = render_at(&mut app, 140, 30);
    assert!(!screen.contains("extend") && screen.contains("y delete · n/Esc cancel"), "{screen}");
    press(&mut app, 'e');
    assert!(app.confirm_delete.is_some(), "still asking");
    press(&mut app, 'n');
    let _ = fs::remove_dir_all(&root);
}
