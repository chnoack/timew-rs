use super::*;

#[test]
fn edit_changes_start_and_end() {
    let root = sample_db("edit-save");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    render_at(&mut app, 140, 24);
    assert_eq!(app.selected().unwrap().interval.tags, vec!["meetings".to_string()]);

    press(&mut app, 'e');
    let screen = render_at(&mut app, 140, 24);
    println!("{screen}");
    assert!(screen.contains("Edit entry") && !screen.contains("Entry details"));
    let today = local_date(jiff::Timestamp::now().as_second());
    assert!(screen.lines().any(|l| l.contains("Start ") && l.contains(&today) && l.contains("08:00")));
    assert!(screen.lines().any(|l| l.contains("End ") && l.contains(&today) && l.contains("10:00")));

    // start 07:30, end 09:45: typing overwrites the digits
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "0730");
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "0945");
    assert_eq!(app.edit.as_ref().unwrap().fields[1], "07:30");
    assert_eq!(app.edit.as_ref().unwrap().fields[3], "09:45");
    assert!(render_at(&mut app, 140, 24).contains("2:15"), "duration preview");
    key(&mut app, KeyCode::Enter);

    let screen = render_at(&mut app, 140, 24);
    assert!(app.edit.is_none());
    assert!(screen.contains("Saved."), "{screen}");
    let iv = &app.selected().unwrap().interval;
    assert_eq!(iv.tags, vec!["meetings".to_string()], "the edited entry stays selected");
    let data = today_file(&root);
    assert!(data.contains(&format!("inc {} - {} # meetings", today_at(7, 30), today_at(9, 45))), "{data}");
    assert!(!root.join("data/undo.data").exists(), "no undo journal is written");
    // the notice goes away with the next key
    press(&mut app, 'j');
    assert!(!render_at(&mut app, 140, 24).contains("Saved."));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn edit_refuses_overlaps_and_bad_input() {
    let root = sample_db("edit-refuse");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    render(&mut app);
    let before = today_file(&root);

    // narrow window: the form opens below the table
    press(&mut app, 'e');
    let screen = render(&mut app);
    assert!(!app.wide && screen.contains("Edit entry"));
    let header = screen.lines().find(|l| l.contains("Summary · Day")).unwrap();
    assert!(!header.contains("Edit entry"));

    key(&mut app, KeyCode::BackTab); // annotation
    key(&mut app, KeyCode::BackTab); // tags
    key(&mut app, KeyCode::BackTab); // end time
    ctrl_u(&mut app);
    type_text(&mut app, "25:00");
    key(&mut app, KeyCode::Enter);
    assert!(render(&mut app).contains("End time: use HH:MM."));

    ctrl_u(&mut app);
    type_text(&mut app, "10:30"); // runs into the email entry at 10:00
    key(&mut app, KeyCode::Enter);
    let screen = render(&mut app);
    assert!(screen.contains("Overlaps another entry. Shift+Enter adjusts it."), "{screen}");
    assert!(app.edit.is_some(), "the form stays open");

    ctrl_u(&mut app);
    type_text(&mut app, "07:00"); // before the start
    key(&mut app, KeyCode::Enter);
    assert!(render(&mut app).contains("The end must be after the start."));

    key(&mut app, KeyCode::Esc);
    assert!(app.edit.is_none() && !app.quit);
    assert_eq!(today_file(&root), before, "nothing was written");
    assert!(!root.join("data/undo.data").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn edit_fields_overwrite_and_move_the_cursor() {
    let root = sample_db("edit-cursor");
    let mut app = App::new(root.clone());
    press(&mut app, '1');
    render_at(&mut app, 140, 24);
    press(&mut app, 'e');
    // the date field: retype it digit by digit
    type_text(&mut app, "20251231");
    let form = app.edit.as_ref().unwrap();
    assert_eq!((form.fields[0].as_str(), form.field, form.cursor), ("2025-12-31", 0, 10));
    // → at the end goes to the time field, ← at its start back again
    key(&mut app, KeyCode::Right);
    assert_eq!((app.edit.as_ref().unwrap().field, app.edit.as_ref().unwrap().cursor), (1, 0));
    key(&mut app, KeyCode::Left);
    assert_eq!((app.edit.as_ref().unwrap().field, app.edit.as_ref().unwrap().cursor), (0, 10));
    // backspace deletes before the cursor, typing then appends
    key(&mut app, KeyCode::Backspace);
    key(&mut app, KeyCode::Backspace);
    type_text(&mut app, "30");
    assert_eq!(app.edit.as_ref().unwrap().fields[0], "2025-12-30");
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Delete);
    assert_eq!(app.edit.as_ref().unwrap().fields[0], "025-12-30");
    // the terminal cursor follows
    key(&mut app, KeyCode::End);
    let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let field = app.edit_fields[0];
    assert_eq!(terminal.get_cursor_position().unwrap(), Position::new(field.x + 1 + 9, field.y));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn shift_enter_adjusts_overlapping_entries() {
    for (name, modifier) in [("shift", KeyModifiers::SHIFT), ("alt", KeyModifiers::ALT)] {
        let root = sample_db(&format!("edit-adjust-{name}"));
        let mut app = App::new(root.clone());
        app.config_dir = root.clone();
        press(&mut app, '1');
        render_at(&mut app, 140, 24);
        // meetings 08-10 now ends 10:30, into email 10-11
        press(&mut app, 'e');
        key(&mut app, KeyCode::Up); // annotation
        key(&mut app, KeyCode::Up); // tags
        key(&mut app, KeyCode::Up); // end time
        type_text(&mut app, "1030");
        key(&mut app, KeyCode::Enter);
        assert!(app.edit.as_ref().unwrap().error.is_some(), "plain Enter refuses the overlap");
        key_with(&mut app, KeyCode::Enter, modifier);
        assert!(app.edit.is_none());
        let screen = render_at(&mut app, 140, 24);
        assert!(screen.contains("Saved. 1 overlapping entry adjusted."), "{screen}");
        let data = today_file(&root);
        assert!(data.contains(&format!("inc {} - {} # meetings", today_at(8, 0), today_at(10, 30))), "{data}");
        assert!(data.contains(&format!("inc {} - {} # email", today_at(10, 30), today_at(11, 0))), "{data}");
        let _ = fs::remove_dir_all(&root);
    }
}

#[test]
fn tags_as_text() {
    let tags = vec!["client a".to_string(), "meetings".to_string(), "say \"hi\"".to_string()];
    let text = tags_to_text(&tags);
    assert_eq!(text, "\"client a\" meetings \"say \\\"hi\\\"\"");
    assert_eq!(parse_tags(&text).unwrap(), tags);
    assert_eq!(parse_tags("  a   b-c  ").unwrap(), vec!["a", "b-c"]);
    assert_eq!(parse_tags("").unwrap(), Vec::<String>::new());
    assert!(parse_tags("a \"b c").is_err());
}

#[test]
fn edit_changes_tags() {
    let root = sample_db("edit-tags");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    render_at(&mut app, 140, 24);
    press(&mut app, 'e');
    key(&mut app, KeyCode::Up); // from the start date up to the annotation
    key(&mut app, KeyCode::Up); // and the tags
    assert_eq!(app.edit.as_ref().unwrap().field, TAGS);
    let screen = render_at(&mut app, 140, 24);
    assert!(screen.contains("separated by spaces"), "{screen}");
    key(&mut app, KeyCode::End);
    type_text(&mut app, " \"client a");
    key(&mut app, KeyCode::Enter);
    assert!(render_at(&mut app, 140, 24).contains("Tags: a quote is not closed."));
    type_text(&mut app, "\" planning");
    key(&mut app, KeyCode::Enter);
    assert!(app.edit.is_none());
    assert_eq!(app.selected().unwrap().interval.tags, vec!["client a", "meetings", "planning"]);
    let data = today_file(&root);
    assert!(data.contains(&format!("inc {} - {} # \"client a\" meetings planning", today_at(8, 0), today_at(10, 0))), "{data}");
    let tags_db = fs::read_to_string(root.join("data/tags.data")).unwrap();
    assert!(tags_db.contains("\"client a\":{\"count\":1}") && tags_db.contains("\"planning\":{\"count\":2}"), "{tags_db}");

    // remove all tags
    press(&mut app, 'e');
    key(&mut app, KeyCode::BackTab);
    key(&mut app, KeyCode::BackTab);
    ctrl_u(&mut app);
    key(&mut app, KeyCode::Enter);
    assert!(app.selected().unwrap().interval.tags.is_empty());
    assert!(today_file(&root).contains(&format!("inc {} - {}\n", today_at(8, 0), today_at(10, 0))));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn edit_changes_the_annotation() {
    let root = sample_db("edit-annotation");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    render_at(&mut app, 140, 24);
    press(&mut app, 'e');
    key(&mut app, KeyCode::Up); // the annotation is just above the start
    assert_eq!(app.edit.as_ref().unwrap().field, ANNOTATION);
    assert!(render_at(&mut app, 140, 24).contains("any text; empty removes the annotation"));
    type_text(&mut app, "weekly sync, \"room 2\"");
    key(&mut app, KeyCode::Enter);
    assert!(app.edit.is_none());
    let iv = &app.selected().unwrap().interval;
    assert_eq!(iv.annotation.as_deref(), Some("weekly sync, \"room 2\""));
    let data = today_file(&root);
    assert!(data.contains(&format!("inc {} - {} # meetings # \"weekly sync, \\\"room 2\\\"\"", today_at(8, 0), today_at(10, 0))), "{data}");
    // the details panel shows it
    assert!(render_at(&mut app, 140, 24).contains("weekly sync"));

    // the form shows the current annotation; clearing removes it
    press(&mut app, 'e');
    assert_eq!(app.edit.as_ref().unwrap().fields[ANNOTATION], "weekly sync, \"room 2\"");
    key(&mut app, KeyCode::Up);
    ctrl_u(&mut app);
    type_text(&mut app, "   ");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.selected().unwrap().interval.annotation, None);
    assert!(today_file(&root).contains(&format!("inc {} - {} # meetings\n", today_at(8, 0), today_at(10, 0))));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn u_undoes_the_last_saved_edit() {
    let root = sample_db("edit-undo");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    let original = today_file(&root);
    assert!(!footer(&mut app).contains("u undo"), "nothing to undo yet");
    press(&mut app, 'u'); // does nothing
    assert_eq!(today_file(&root), original);

    // two edits: meetings ends 09:30, then it ends 10:30 adjusting email
    edit_end(&mut app, "0930", false);
    let first = today_file(&root);
    assert!(footer(&mut app).starts_with(" ? help  s start  u undo  "), "{}", footer(&mut app));
    edit_end(&mut app, "1030", true);
    assert!(today_file(&root).contains(&format!("inc {} - {} # email", today_at(10, 30), today_at(11, 0))));

    // u reverts the last edit only, the adjusted email entry included
    press(&mut app, 'u');
    assert!(render_at(&mut app, 140, 24).contains("Undone."));
    assert_eq!(today_file(&root), first);
    assert_eq!(app.selected().unwrap().interval.tags, vec!["meetings".to_string()]);
    assert!(!footer(&mut app).contains("u undo"), "only one level of undo");
    press(&mut app, 'u');
    assert_eq!(today_file(&root), first);
    assert!(!root.join("data/undo.data").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn u_refuses_when_the_entry_changed_since() {
    let root = sample_db("edit-undo-stale");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    edit_end(&mut app, "0930", false);

    // another program changes the edited entry afterwards
    let mut paths = Paths::from_env();
    paths.db_dir = root.clone();
    paths.config_dir = root.clone();
    let mut db = Database::open(root.join("data")).unwrap();
    let meetings = db.all_tracked().unwrap().remove(1).interval;
    assert_eq!(meetings.tags, vec!["meetings".to_string()]);
    let mut changed = meetings.clone();
    changed.add_tag("x");
    timew_core::modify_interval(&paths, &meetings, &changed, false).unwrap();
    let after_other = today_file(&root);

    press(&mut app, 'u');
    let screen = render_at(&mut app, 140, 24);
    assert!(screen.contains("Not undone: The entry was changed by another program since."), "{screen}");
    assert_eq!(today_file(&root), after_other, "nothing was reverted");
    assert!(!footer(&mut app).contains("u undo"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn u_refuses_to_restore_into_a_new_entry() {
    let root = sample_db("edit-undo-overlap");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    edit_end(&mut app, "0930", false); // meetings 08:00-09:30, free until 10:00

    // another program records 09:40-09:50
    let mut paths = Paths::from_env();
    paths.db_dir = root.clone();
    paths.config_dir = root.clone();
    let mut db = Database::open(root.join("data")).unwrap();
    let mut new = db.all_tracked().unwrap().remove(1).interval;
    let start = new.range.end + 600;
    new.range = Range::new(start, start + 600);
    new.tags = vec!["new".into()];
    db.add_interval(&new).unwrap();
    db.commit().unwrap();
    let before = today_file(&root);

    press(&mut app, 'u');
    assert!(render_at(&mut app, 140, 24).contains("Not undone: It would overlap an entry added since."));
    assert_eq!(today_file(&root), before);
    let _ = fs::remove_dir_all(&root);
}
