use super::*;

#[test]
fn details_popup_follows_selection() {
    let root = sample_db("details");
    let mut app = App::new(root.clone());
    press(&mut app, '4');
    key(&mut app, KeyCode::Enter);
    assert!(app.details);
    let first = render(&mut app);
    println!("{first}");
    assert!(first.contains("Entry details") && first.contains("planning") && first.contains("Duration   4:00"));
    assert!(first.contains("Start      ") && !first.contains(":00:00"), "no seconds anywhere: {first}");
    assert!(first.contains("@3"));
    let start = jiff::Zoned::now().date().yesterday().unwrap();
    assert!(first.contains(&start.strftime("%A").to_string()), "weekday shown");
    assert!(first.contains(&format!("W{:02} {}", start.iso_week_date().week(), start.iso_week_date().year())), "week shown");

    press(&mut app, 'j');
    let second = render(&mut app);
    assert!(second.contains("@2") && second.contains("Duration   2:00"));

    key(&mut app, KeyCode::Esc);
    assert!(!app.details && !app.quit, "Esc closes the details first");
    key(&mut app, KeyCode::Esc);
    assert!(!app.quit, "then it resets the filters, it never quits");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn wide_window_shows_details_on_the_right_automatically() {
    let root = sample_db("wide");
    let mut app = App::new(root.clone());
    press(&mut app, '4');
    let screen = render_at(&mut app, 140, 24);
    println!("{screen}");
    assert!(app.wide);
    assert_eq!(app.table.selected(), Some(0), "top entry is selected");
    // details panel sits on the same line as the table header, to its right
    let header = screen.lines().find(|l| l.contains("Summary · Year")).unwrap();
    assert!(header.contains("Entry details"));
    assert!(screen.contains("@3") && !screen.contains("Enter details"));

    // Enter does nothing in the wide layout; q quits
    key(&mut app, KeyCode::Enter);
    assert!(render_at(&mut app, 140, 24).contains("Entry details"));
    press(&mut app, 'q');
    assert!(app.quit);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn narrow_window_needs_enter_for_details() {
    let root = sample_db("narrow");
    let mut app = App::new(root.clone());
    press(&mut app, '4');
    let screen = render(&mut app);
    assert!(!app.wide);
    assert!(!screen.contains("Entry details"), "no details before Enter");
    key(&mut app, KeyCode::Enter);
    let screen = render(&mut app);
    let header = screen.lines().find(|l| l.contains("Summary · Year")).unwrap();
    assert!(!header.contains("Entry details"), "narrow details go below the table");
    assert!(screen.contains("Entry details"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn clicking_selected_row_opens_details() {
    let root = sample_db("click-row");
    let mut app = App::new(root.clone());
    press(&mut app, '4');
    render(&mut app);
    let a = app.table_area;
    let click = |app: &mut App, y| {
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: a.x + 5,
            row: y,
            modifiers: KeyModifiers::NONE,
        })
    };
    let second_row = a.y + 1 + PAD.top + 1 + 1; // border, padding, header, first row
    click(&mut app, second_row);
    assert_eq!(app.table.selected(), Some(1));
    assert!(!app.details);
    click(&mut app, second_row);
    assert!(app.details);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn week_colors_alternate_per_iso_week() {
    let row = |date: Option<&str>| summary::Row {
        week: None,
        date: date.map(str::to_string),
        day_name: None,
        id: String::new(),
        tags: String::new(),
        annotation: String::new(),
        start: String::new(),
        end: String::new(),
        time: String::new(),
        total: String::new(),
        source: 0,
    };
    let rows = [
        row(Some("2026-11-06")), // Fri W45
        row(None),               // same day
        row(Some("2026-11-09")), // Mon W46
        row(Some("2026-11-13")), // Fri W46
        row(Some("2026-11-16")), // Mon W47
        row(None),
        row(Some("2025-11-17")), // W47 of another year -> still a new week
    ];
    assert_eq!(week_parity(&rows), vec![0, 0, 1, 1, 0, 0, 1]);
}

#[test]
fn seconds_are_not_shown() {
    assert_eq!(without_seconds("8:50:00"), "8:50");
    assert_eq!(without_seconds("10:20:59"), "10:20");
    assert_eq!(without_seconds("2086:35:00"), "2086:35");
    assert_eq!(without_seconds("-"), "-");
    assert_eq!(without_seconds(""), "");
    assert_eq!(format_hm(0), "0:00");
    assert_eq!(format_hm(7 * 3600 + 59 * 60 + 2), "7:59");
    assert_eq!(format_hm(2086 * 3600 + 35 * 60), "2086:35");
}

#[test]
fn totals_are_decimal_hours() {
    assert_eq!(parse_hms("7:30:00"), Some(7 * 3600 + 30 * 60));
    assert_eq!(parse_hms("2086:35:00"), Some(2086 * 3600 + 35 * 60));
    assert_eq!(parse_hms(""), None);
    assert_eq!(parse_hms("-"), None);
    assert_eq!(decimal_hours(7 * 3600 + 30 * 60), "7.50");
    assert_eq!(decimal_hours(8 * 3600 + 25 * 60), "8.42");
    assert_eq!(decimal_hours(2086 * 3600 + 35 * 60), "2086.58");
    assert_eq!(decimal_hours(0), "0.00");
}

#[test]
fn empty_database_shows_message() {
    let root = env::temp_dir().join(format!("timew-tui-test-empty-{}", std::process::id()));
    fs::create_dir_all(root.join("data")).unwrap();
    let mut app = App::new(root.clone());
    assert!(render(&mut app).contains("No data found in the range"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn focused_view_has_a_yellow_border() {
    let root = sample_db("focus");
    let mut app = App::new(root.clone());
    press(&mut app, '1');
    // color of the summary's top-left corner
    let corner = |app: &mut App| {
        let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let y = (0..buf.area.height).find(|&y| (0..20).any(|x| buf[(x, y)].symbol() == "S" && buf[(x + 1, y)].symbol() == "u")).unwrap();
        assert_eq!(buf[(0, y)].symbol(), "╭");
        buf[(0, y)].fg
    };
    assert_eq!(corner(&mut app), FOCUS, "the table has the focus");
    press(&mut app, 'e');
    assert_ne!(corner(&mut app), FOCUS, "the edit form has it");
    key(&mut app, KeyCode::Esc);
    assert_eq!(corner(&mut app), FOCUS);
    press(&mut app, 't');
    assert_ne!(corner(&mut app), FOCUS, "the tag list has it");
    key(&mut app, KeyCode::Esc);
    press(&mut app, '?');
    assert_ne!(corner(&mut app), FOCUS, "the legend has it");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn messages_have_their_own_panel() {
    let root = sample_db("messages");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    let status = |screen: &str| screen.lines().rev().nth(1).unwrap().to_string();
    // wide: no message, no panel (the chart gets the room)
    let screen = render_at(&mut app, 140, 34);
    assert!(!screen.contains("Messages"));
    edit_end(&mut app, "0930", false);
    let screen = render_at(&mut app, 140, 34);
    println!("{screen}");
    let messages = screen.lines().position(|l| l.contains("Messages")).unwrap();
    let chart = screen.lines().position(|l| l.contains("Hours per tag")).unwrap();
    assert!(messages > chart, "below the chart");
    assert!(screen.lines().nth(messages + 2).unwrap().contains("Saved."));
    assert!(status(&screen).starts_with(" Total "), "the status line keeps the totals: {}", status(&screen));
    // narrow: a panel below the list, only while there is a message
    press(&mut app, 'u');
    let screen = render_at(&mut app, 90, 24);
    println!("{screen}");
    assert!(screen.contains("Messages") && screen.contains("Undone."));
    assert!(status(&screen).starts_with(" Total "));
    press(&mut app, 'j');
    assert!(!render_at(&mut app, 90, 24).contains("Messages"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn gaps_within_a_day_are_marked_with_a_line() {
    // yesterday: a 08-09, gap, b 09:30-10, c 10-11 (no gap), today: d 08-09, gap, e 10-11
    let root = env::temp_dir().join(format!("timew-tui-test-gaps-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("data")).unwrap();
    let tz = jiff::tz::TimeZone::system();
    let today = jiff::Zoned::now().date();
    let yesterday = today.yesterday().unwrap();
    let at = |d: jiff::civil::Date, h: i8, m: i8| {
        timew_core::interval::format_iso(d.at(h, m, 0, 0).to_zoned(tz.clone()).unwrap().timestamp().as_second())
    };
    let mut lines: Vec<(jiff::civil::Date, String)> = [
        (yesterday, (8, 0), (9, 0), "a"),
        (yesterday, (9, 30), (10, 0), "b"),
        (yesterday, (10, 0), (11, 0), "c"),
        (today, (8, 0), (9, 0), "d"),
        (today, (10, 0), (11, 0), "e"),
    ]
    .iter()
    .map(|(d, (h1, m1), (h2, m2), t)| (*d, format!("inc {} - {} # {t}", at(*d, *h1, *m1), at(*d, *h2, *m2))))
    .collect();
    lines.sort();
    for (d, line) in lines {
        let file = root.join("data").join(format!("{:04}-{:02}.data", d.year(), d.month()));
        let mut content = fs::read_to_string(&file).unwrap_or_default();
        content.push_str(&line);
        content.push('\n');
        fs::write(file, content).unwrap();
    }
    let mut app = App::new(root.clone());
    app.filter = Filter::Custom;
    app.custom_range = Range::new(
        yesterday.to_zoned(tz.clone()).unwrap().timestamp().as_second(),
        today.tomorrow().unwrap().to_zoned(tz.clone()).unwrap().timestamp().as_second(),
    );
    app.load();
    let tags: Vec<String> = app.summary.rows.iter().map(|r| r.tags.clone()).collect();
    assert_eq!(tags, vec!["a", "b", "c", "d", "e"]);
    // a|b gap; b|c touch; c is the day's last; d|e gap; e is the last
    assert_eq!(gap_after(&app), vec![true, false, false, true, false]);

    // drawn as a grey underline across the whole row
    let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let row_y = |tag: &str| {
        (0..buf.area.height)
            .find(|&y| {
                let line: String = (0..60).map(|x| buf[(x, y)].symbol()).collect();
                line.contains(&format!(" {tag}  "))
            })
            .unwrap()
    };
    // dashed, from the Tags column to the end of the Total column
    let header_y = (0..buf.area.height).find(|&y| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>().contains("Wk  Date")).unwrap();
    let from = col_of(&buf, header_y, "Tags");
    let to = col_of(&buf, header_y, "Total") + "Total".len() as u16;
    let on = |x: u16, y: u16| buf[(x, y)].modifier.contains(Modifier::UNDERLINED) && buf[(x, y)].underline_color == GAP_COLOR;
    let underlined = |y: u16| {
        (3..from).all(|x| !on(x, y)) && (from..to).all(|x| on(x, y) == ((x - from) % 3 != 2)) && !on(to, y)
    };
    assert!(underlined(row_y("a")) && underlined(row_y("d")));
    assert!(!buf[(10, row_y("b"))].modifier.contains(Modifier::UNDERLINED));
    assert!(!buf[(10, row_y("c"))].modifier.contains(Modifier::UNDERLINED));
    assert!(!buf[(10, row_y("e"))].modifier.contains(Modifier::UNDERLINED));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn gap_lines_follow_scrolling() {
    // 30 days, each 08-09 and 10-11 (a gap between them)
    let root = env::temp_dir().join(format!("timew-tui-test-gapscroll-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("data")).unwrap();
    let tz = jiff::tz::TimeZone::system();
    let first = jiff::civil::date(2025, 3, 1);
    let at = |d: jiff::civil::Date, h: i8| timew_core::interval::format_iso(d.at(h, 0, 0, 0).to_zoned(tz.clone()).unwrap().timestamp().as_second());
    let mut content = String::new();
    for n in 0..30 {
        let d = first.checked_add(jiff::Span::new().days(n)).unwrap();
        content.push_str(&format!("inc {} - {} # first\ninc {} - {} # second\n", at(d, 8), at(d, 9), at(d, 10), at(d, 11)));
    }
    fs::write(root.join("data/2025-03.data"), content).unwrap();
    let mut app = App::new(root.clone());
    app.filter = Filter::Month;
    app.month = Some((2025, 3));
    app.load();
    assert_eq!(app.summary.rows.len(), 60);
    for _ in 0..41 {
        press(&mut app, 'j');
    }
    let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut checked = 0;
    for y in 0..buf.area.height {
        let line: String = (0..40).map(|x| buf[(x, y)].symbol()).collect();
        let is_first = line.contains(" first ");
        let is_second = line.contains(" second ");
        if !(is_first || is_second) {
            continue;
        }
        let tags_x = col_of(&buf, y, if is_first { "first" } else { "second" });
        let underlined = buf[(tags_x, y)].modifier.contains(Modifier::UNDERLINED);
        assert!(!buf[(3, y)].modifier.contains(Modifier::UNDERLINED), "not under the week");
        assert_eq!(underlined, is_first, "row {y}: {line}");
        checked += 1;
    }
    assert!(checked >= 10 && app.table.offset() > 0, "scrolled: {checked} rows, offset {}", app.table.offset());
    let _ = fs::remove_dir_all(&root);
}
