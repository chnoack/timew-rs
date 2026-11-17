//! Keys of the main view: t and y are free, Ctrl+F / Ctrl+B page, a adds
//! an entry after the selected one.

use super::*;

/// Select the row of the entry tagged `tag`.
fn select_tag(app: &mut App, tag: &str) {
    for _ in 0..app.tracked.len() + 2 {
        if app.selected().is_some_and(|t| t.interval.tags == vec![tag.to_string()]) {
            return;
        }
        key(app, KeyCode::Down);
    }
    panic!("no row tagged {tag}");
}

#[test]
fn t_and_y_do_nothing() {
    let root = sample_db("free-keys");
    let mut app = App::new(root.clone());
    press(&mut app, '1');
    press(&mut app, 't');
    press(&mut app, 'y');
    assert!(app.picker.is_none(), "t no longer opens the tag list");
    assert_eq!(app.filter, Filter::Day, "y no longer shows yesterday");
    // 6 and 2 still do
    press(&mut app, '2');
    assert_eq!(app.filter, Filter::Yesterday);
    press(&mut app, '6');
    assert!(app.picker.is_some());
    press(&mut app, 't');
    assert!(app.picker.is_some(), "t doesn't close the tag list either");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ctrl_f_and_ctrl_b_scroll_a_page() {
    let root = env::temp_dir().join(format!("timew-tui-test-pages-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("data")).unwrap();
    let today = jiff::Zoned::now().date();
    let lines: String = (0..40)
        .map(|i: i8| {
            let (a, b) = (6 * 60 + i as i32 * 10, 6 * 60 + i as i32 * 10 + 10);
            let at = |m: i32| today_at((m / 60) as i8, (m % 60) as i8);
            format!("inc {} - {} # e{i:02}\n", at(a), at(b))
        })
        .collect();
    fs::write(root.join("data").join(format!("{:04}-{:02}.data", today.year(), today.month())), lines).unwrap();
    let mut app = App::new(root.clone());
    press(&mut app, '1');
    render_at(&mut app, 140, 24);
    let page = app.page as usize;
    assert!(page > 1 && page < 40);
    let start = app.table.selected().unwrap();
    key_with(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    assert_eq!(app.table.selected().unwrap(), start + page);
    key_with(&mut app, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(app.table.selected().unwrap(), start);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_adds_an_entry_after_the_selected_one() {
    let root = sample_db("add");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    render_at(&mut app, 140, 24);
    select_tag(&mut app, "email"); // today 10:00-11:00
    press(&mut app, 'a');
    let form = app.edit.as_ref().unwrap();
    assert!(form.is_new());
    // the date of the selected entry (today), from its end, 30 minutes
    let today = local_date(jiff::Timestamp::now().as_second());
    assert_eq!(form.fields[..4], [today.clone(), "11:00".into(), today, "11:30".into()]);
    assert_eq!(form.field, TAGS, "the cursor starts in the tags field");
    let screen = render_at(&mut app, 140, 24);
    assert!(screen.contains("New entry") && screen.contains("new"), "{screen}");

    type_text(&mut app, "focus");
    key(&mut app, KeyCode::Enter);
    assert!(app.edit.is_none());
    let line = format!("inc {} - {} # focus\n", today_at(11, 0), today_at(11, 30));
    assert!(today_file(&root).contains(&line), "{}", today_file(&root));
    assert_eq!(app.selected().unwrap().interval.tags, vec!["focus"], "the new entry is selected");
    assert!(render_at(&mut app, 140, 24).contains("Added. u undoes it."));

    press(&mut app, 'u');
    assert!(!today_file(&root).contains("focus"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_ends_where_the_next_entry_starts() {
    let root = sample_db("add-short");
    // another entry 15 minutes after email ends
    let file = root.join("data").join({
        let t = jiff::Zoned::now().date();
        format!("{:04}-{:02}.data", t.year(), t.month())
    });
    let mut data = fs::read_to_string(&file).unwrap();
    data.push_str(&format!("inc {} - {} # later\n", today_at(11, 15), today_at(12, 0)));
    fs::write(&file, data).unwrap();
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    render_at(&mut app, 140, 24);
    select_tag(&mut app, "email");
    press(&mut app, 'a');
    let form = app.edit.as_ref().unwrap();
    assert_eq!((form.fields[1].as_str(), form.fields[3].as_str()), ("11:00", "11:15"));
    // the end is required for a new entry
    key(&mut app, KeyCode::BackTab); // from the tags to the end time
    ctrl_u(&mut app);
    key(&mut app, KeyCode::BackTab); // end date
    ctrl_u(&mut app);
    key(&mut app, KeyCode::Enter);
    assert!(render_at(&mut app, 140, 24).contains("Enter the end date and time."));
    key(&mut app, KeyCode::Esc);
    assert!(!today_file(&root).contains("# \n"));
    let _ = fs::remove_dir_all(&root);
}
