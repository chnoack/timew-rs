use super::*;

#[test]
fn filters_switch_the_summary() {
    let root = sample_db("filters");
    let mut app = App::new(root.clone());
    assert_eq!(app.filter, Filter::Month, "month is the default filter");
    let month = render(&mut app);
    println!("{month}");
    assert!(month.contains("Summary · Month"));
    assert!(!month.contains("Annotation") && !month.contains("@1"), "ID/Annotation are hidden");

    press(&mut app, '1');
    let day = render(&mut app);
    println!("{day}");
    assert!(day.contains("meetings") && day.contains("email") && !day.contains("planning"));
    assert!(day.contains("Total 3.00 h"));

    press(&mut app, '2');
    let yday = render(&mut app);
    assert!(yday.contains("planning") && !yday.contains("meetings"));
    assert!(yday.contains("Total 4.00 h"));

    press(&mut app, '4');
    assert!(render(&mut app).contains("Total 7.00 h"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn mouse_click_on_button_selects_filter() {
    let root = sample_db("mouse");
    let mut app = App::new(root.clone());
    render(&mut app);
    let (rect, f) = app.buttons[0].clone();
    assert_eq!(f, Button::Range(Filter::Day));
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x + 2,
        row: rect.y + 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.filter, Filter::Day);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn scrolling_is_clamped() {
    let root = sample_db("scroll");
    let mut app = App::new(root.clone());
    press(&mut app, '4');
    press(&mut app, 'G');
    assert_eq!(app.table.selected(), Some(2));
    press(&mut app, 'j');
    assert_eq!(app.table.selected(), Some(2));
    press(&mut app, 'g');
    assert_eq!(app.table.selected(), Some(0));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn tag_picker_filters_by_tag() {
    let root = sample_db("tag-picker");
    let mut app = App::new(root.clone());
    press(&mut app, '4');
    assert_eq!(
        app.all_tags,
        vec![("email".into(), 1), ("meetings".into(), 1), ("planning".into(), 1)]
    );

    press(&mut app, '6');
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("Filter by tag") && screen.contains("All tags"));
    assert!(screen.contains("│ 5 Range │") && screen.contains("│ 6 Tag │"), "no value shown when nothing is selected");

    // entry 0 = All tags, 1 = email, 2 = meetings
    press(&mut app, 'j');
    press(&mut app, 'j');
    key(&mut app, KeyCode::Enter);
    assert!(app.picker.is_none());
    assert_eq!(app.tag.as_deref(), Some("meetings"));
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("6 meetings ▾") && screen.contains("tag meetings"));
    assert!(screen.contains("Total 2.00 h") && !screen.contains("planning  "));

    // the tag filter combines with the range filter
    press(&mut app, '2');
    assert!(render(&mut app).contains("No data found in the range") );
    assert!(render(&mut app).contains("tagged with meetings"));

    // Backspace outside the picker does nothing
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.tag.as_deref(), Some("meetings"));

    // Backspace inside the picker clears the tag filter and closes it
    press(&mut app, '6');
    key(&mut app, KeyCode::Backspace);
    assert!(app.picker.is_none());
    assert_eq!(app.tag, None);
    assert!(render(&mut app).contains("Total 4.00 h"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn tag_picker_by_mouse_and_esc() {
    let root = sample_db("tag-mouse");
    let mut app = App::new(root.clone());
    press(&mut app, '4');
    render(&mut app);
    let (rect, _) = app.buttons.iter().find(|(_, b)| *b == Button::Tag).unwrap().clone();
    let click = |app: &mut App, column, row| {
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    click(&mut app, rect.x + 2, rect.y + 1);
    assert!(app.picker.is_some());
    render(&mut app);
    let a = app.picker_area;
    click(&mut app, a.x + 1, a.y + 3); // "planning"
    assert_eq!(app.tag.as_deref(), Some("planning"));

    press(&mut app, '6');
    key(&mut app, KeyCode::Esc);
    assert!(app.picker.is_none() && !app.quit, "Esc only closes the picker");
    assert_eq!(app.tag.as_deref(), Some("planning"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn range_input_parsing() {
    // bare dates: To is inclusive
    let r = parse_range_input("2025-03-01", "2025-03-31").unwrap();
    assert_eq!((r.start, r.end), (local(2025, 3, 1, 0, 0), local(2025, 4, 1, 0, 0)));
    assert_eq!(range_label(&r), "2025-03-01 – 2025-03-31");
    // a single day
    let r = parse_range_input("2025-03-05", "2025-03-05").unwrap();
    assert_eq!(range_label(&r), "2025-03-05");
    // times, with a space or a T, with or without seconds
    let r = parse_range_input("2025-03-05 08:30", "2025-03-05T12:00:00").unwrap();
    assert_eq!((r.start, r.end), (local(2025, 3, 5, 8, 30), local(2025, 3, 5, 12, 0)));
    assert_eq!(range_label(&r), "2025-03-05 08:30 – 2025-03-05 12:00");
    // open end
    let r = parse_range_input("2025-03-01", "").unwrap();
    assert!(!r.is_ended());
    assert_eq!(range_label(&r), "since 2025-03-01");
    // hints
    assert_eq!(parse_range_input(":lastyear", "").unwrap(), expand_range_hint("lastyear").unwrap());
    // errors
    assert!(parse_range_input("", "").unwrap_err().contains("start date"));
    assert!(parse_range_input("2025-03-05", "2025-03-01").unwrap_err().contains("after"));
    assert!(parse_range_input("yesterday-ish", "").is_err());
    assert!(parse_range_input(":nope", "").unwrap_err().contains("Unknown"));
    assert!(parse_range_input(":week", "2025-01-01").is_err());
}

#[test]
fn custom_range_form() {
    let root = sample_db("custom-range");
    let mut app = App::new(root.clone());
    let today = jiff::Zoned::now().date();
    let yesterday = today.yesterday().unwrap();

    // prefilled with the current (month) range
    press(&mut app, 'c');
    let input = app.range_input.clone().unwrap();
    assert_eq!(input.from, local_date(app.range.start));
    assert_eq!(input.to, local_date(app.range.end - 1));
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("Date range") && screen.contains("From") && screen.contains("To"));

    // invalid input shows an error and keeps the form open; typing 'q' doesn't quit
    clear_field(&mut app);
    type_text(&mut app, "qqq");
    key(&mut app, KeyCode::Enter);
    assert!(!app.quit && app.range_input.as_ref().unwrap().error.is_some());
    assert!(render(&mut app).contains("not a valid date"));

    // yesterday only
    clear_field(&mut app);
    type_text(&mut app, &yesterday.to_string());
    key(&mut app, KeyCode::Tab);
    clear_field(&mut app);
    type_text(&mut app, &yesterday.to_string());
    key(&mut app, KeyCode::Enter);
    assert!(app.range_input.is_none());
    assert_eq!(app.filter, Filter::Custom);
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains(&format!("5 {yesterday} ▾")));
    assert!(screen.contains("Summary · Range") && screen.contains("Total 4.00 h"));

    // reopening shows what was typed; Esc cancels without changes
    press(&mut app, 'c');
    assert_eq!(app.range_input.as_ref().unwrap().from, yesterday.to_string());
    key(&mut app, KeyCode::Esc);
    assert!(app.range_input.is_none() && !app.quit);
    assert_eq!(app.filter, Filter::Custom);

    // custom range combines with the tag filter, presets still work
    press(&mut app, '1');
    assert_eq!(app.filter, Filter::Day);
    assert!(render(&mut app).contains("Total 3.00 h"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn custom_range_button_opens_form() {
    let root = sample_db("custom-button");
    let mut app = App::new(root.clone());
    render(&mut app);
    let (rect, _) = app.buttons.iter().find(|(_, b)| *b == Button::CustomRange).unwrap().clone();
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x + 2,
        row: rect.y + 1,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.range_input.is_some());
    render(&mut app);
    let to = app.range_fields[1];
    app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: to.x + 1, row: to.y, modifiers: KeyModifiers::NONE });
    assert_eq!(app.range_input.as_ref().unwrap().field, 1);
    app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 0, row: 0, modifiers: KeyModifiers::NONE });
    assert!(app.range_input.is_none(), "click outside closes the form");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn shift_3_opens_month_list_of_shown_year() {
    let root = sample_db("months");
    let mut app = App::new(root.clone());
    let now = jiff::Zoned::now();
    let yesterday = now.date().yesterday().unwrap();

    for c in ['§', '#'] {
        press(&mut app, c);
        assert!(app.month_picker.is_some(), "{c} opens the month list");
        key(&mut app, KeyCode::Esc);
        assert!(app.month_picker.is_none() && !app.quit);
    }
    app.on_key(KeyEvent {
        code: KeyCode::Char('3'),
        modifiers: KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    assert!(app.month_picker.is_some(), "Shift+3 reported with modifier");

    let p = app.month_picker.as_ref().unwrap();
    assert_eq!(p.year, now.year());
    assert_eq!(p.state.selected(), Some(now.month() as usize - 1));
    let screen = render(&mut app);
    println!("{screen}");
    assert!(MONTH_NAMES.iter().all(|m| screen.contains(m)));

    // previous year: pick January there
    key(&mut app, KeyCode::Left);
    assert_eq!(app.month_picker.as_ref().unwrap().year, now.year() - 1);
    press(&mut app, 'g');
    key(&mut app, KeyCode::Enter);
    assert!(app.month_picker.is_none());
    assert_eq!((app.filter, app.month), (Filter::Month, Some((now.year() - 1, 1))));
    assert_eq!(app.range, month_range(now.year() - 1, 1));
    let screen = render(&mut app);
    assert!(screen.contains(&format!("3 {}-01 ▾", now.year() - 1)), "button shows the month number");
    assert!(screen.contains("No data found"));

    // opening again shows that year; plain 3 goes back to the current month
    press(&mut app, '#');
    assert_eq!(app.month_picker.as_ref().unwrap().year, now.year() - 1);
    key(&mut app, KeyCode::Esc);
    press(&mut app, '3');
    assert_eq!(app.month, None);
    assert_eq!(app.range, expand_range_hint("month").unwrap());

    // Backspace in the list behaves like pressing 3
    press(&mut app, '#');
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.month, Some((now.year() - 1, now.month())));
    press(&mut app, '#');
    key(&mut app, KeyCode::Backspace);
    assert!(app.month_picker.is_none());
    assert_eq!((app.filter, app.month), (Filter::Month, None));
    assert_eq!(app.range, expand_range_hint("month").unwrap());
    press(&mut app, '4');
    press(&mut app, '#');
    key(&mut app, KeyCode::Backspace);
    assert_eq!((app.filter, app.month), (Filter::Month, None), "also from another filter");

    // pick yesterday's month by mouse
    press(&mut app, '#');
    if yesterday.year() != now.year() {
        key(&mut app, KeyCode::Left);
    }
    render(&mut app);
    let a = app.month_picker_area;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: a.x + 2,
        row: a.y + yesterday.month() as u16 - 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.month, Some((yesterday.year(), yesterday.month())));
    assert!(render(&mut app).contains("planning"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn shift_4_opens_list_of_last_five_years() {
    let root = sample_db("years");
    let mut app = App::new(root.clone());
    let this_year = jiff::Zoned::now().year();

    press(&mut app, '$');
    let p = app.year_picker.as_ref().expect("$ opens the year list");
    assert_eq!(p.years, (0..5).map(|i| this_year - i).collect::<Vec<_>>());
    assert_eq!(p.state.selected(), Some(0), "the shown (current) year is preselected");
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains(" Year ") && screen.contains(&(this_year - 4).to_string()));
    key(&mut app, KeyCode::Esc);
    assert!(app.year_picker.is_none() && !app.quit);

    app.on_key(KeyEvent {
        code: KeyCode::Char('4'),
        modifiers: KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    assert!(app.year_picker.is_some(), "Shift+4 reported with modifier");

    // pick last year
    press(&mut app, 'j');
    key(&mut app, KeyCode::Enter);
    assert_eq!((app.filter, app.year), (Filter::Year, Some(this_year - 1)));
    assert_eq!(app.range, year_range(this_year - 1));
    assert!(render(&mut app).contains(&format!("4 {} ▾", this_year - 1)));

    // the month list now opens on that year
    press(&mut app, '#');
    assert_eq!(app.month_picker.as_ref().unwrap().year, this_year - 1);
    key(&mut app, KeyCode::Esc);

    // reopening preselects the shown year; Backspace = pressing 4
    press(&mut app, '$');
    assert_eq!(app.year_picker.as_ref().unwrap().state.selected(), Some(1));
    key(&mut app, KeyCode::Backspace);
    assert!(app.year_picker.is_none());
    assert_eq!((app.filter, app.year), (Filter::Year, None));
    assert_eq!(app.range, expand_range_hint("year").unwrap());

    // plain 4 also resets a picked year; mouse picking works
    press(&mut app, '$');
    render(&mut app);
    let a = app.year_picker_area;
    app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: a.x + 2, row: a.y + 2, modifiers: KeyModifiers::NONE });
    assert_eq!(app.year, Some(this_year - 2));
    press(&mut app, '4');
    assert_eq!(app.year, None);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn esc_resets_all_filters() {
    let root = sample_db("esc-reset");
    let mut app = App::new(root.clone());
    let default_range = app.range;
    // year, a tag, then Esc: back to the current month without a tag
    press(&mut app, '4');
    press(&mut app, '6');
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert!(app.tag.is_some() && app.filter == Filter::Year);
    key(&mut app, KeyCode::Esc);
    assert!(!app.quit);
    assert_eq!((app.filter, app.month, app.year, app.tag.clone()), (Filter::Month, None, None, None));
    assert_eq!(app.range, default_range);
    assert!(render(&mut app).contains("All filters reset"));
    // a picked month and a custom range are reset too
    app.month = Some((2020, 5));
    app.load();
    key(&mut app, KeyCode::Esc);
    assert_eq!((app.filter, app.month), (Filter::Month, None));
    app.filter = Filter::Custom;
    app.load();
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.filter, Filter::Month);
    // nothing to reset: nothing happens (and no quitting)
    key(&mut app, KeyCode::Esc);
    assert!(!app.quit && app.notice.is_none());
    let _ = fs::remove_dir_all(&root);
}
