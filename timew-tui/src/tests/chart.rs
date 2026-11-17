use super::*;

#[test]
fn bar_chart_of_hours_per_tag() {
    let root = sample_db("chart");
    let mut app = App::new(root.clone());
    press(&mut app, '4');
    assert_eq!(
        simple(&tag_totals(&app)),
        vec![("planning".into(), 4 * 3600, 0), ("meetings".into(), 2 * 3600, 0), ("email".into(), 3600, 0)]
    );

    // wide: the chart is always shown, below the details on the right
    let screen = render_at(&mut app, 140, 34);
    println!("{screen}");
    let title = screen.lines().find(|l| l.contains("Summary · Year")).unwrap();
    assert!(title.contains("Entry details"));
    let lines: Vec<&str> = screen.lines().collect();
    let row = |s: &str| lines.iter().position(|l| l.contains(s)).unwrap();
    assert!(row("Hours per tag ─") > row("Duration"), "chart below the details");
    assert!(!screen.contains("Hours per tag ·"), "all tags shown: no count");
    // b and Esc don't hide it in the wide layout; Esc quits
    press(&mut app, 'b');
    assert!(render_at(&mut app, 140, 34).contains("Hours per tag"));
    let planning = chart_line(&screen, "planning");
    assert!(planning.contains("4.00 h") && planning.contains("57 %"));
    assert!(screen.contains("Total 7.00 h"));
    // the longest bar belongs to the largest tag
    let bar_len = |tag: &str| chart_line(&screen, tag).matches('█').count();
    assert!(bar_len("planning") > bar_len("meetings") && bar_len("meetings") > bar_len("email"));

    // the chart follows the filters
    press(&mut app, '1');
    assert_eq!(simple(&tag_totals(&app)), vec![("meetings".into(), 2 * 3600, 0), ("email".into(), 3600, 0)]);
    assert!(chart_line(&render_at(&mut app, 140, 34), "email").contains('█'));

    // narrow: b shows the chart below the table; Esc closes it, Enter switches to details
    render(&mut app); // the window is now narrow
    assert!(!app.wide && !render(&mut app).contains("Hours per tag"));
    press(&mut app, 'b');
    let screen = render(&mut app);
    println!("{screen}");
    let title = screen.lines().find(|l| l.contains("Summary · Day")).unwrap();
    assert!(!title.contains("Hours per tag") && screen.contains("Hours per tag ─"));
    key(&mut app, KeyCode::Esc);
    assert!(!app.chart && !app.quit);
    press(&mut app, 'b');
    key(&mut app, KeyCode::Enter);
    assert!(!app.chart && app.details);
    assert!(render(&mut app).contains("Entry details"));
    let _ = fs::remove_dir_all(&root);
}

/// The chart's line for `tag` (the part right of the summary table, if any).
fn chart_line(screen: &str, tag: &str) -> String {
    screen
        .lines()
        .map(|l| l.rsplit("││").next().unwrap_or(l))
        .find(|l| l.contains(tag) && l.contains('█'))
        .unwrap_or_else(|| panic!("no chart bar for {tag}:\n{screen}"))
        .to_string()
}

#[test]
fn neighboring_bars_have_different_colors() {
    let total = |tag: &str| TagTotal { tag: tag.to_string(), share: 1.0, shared: 0.0 };
    // more bars than colors, with "(no tag)" in between
    let mut totals: Vec<TagTotal> = (0..20).map(|i| total(&format!("t{i}"))).collect();
    totals.insert(5, total(NO_TAG));
    let colors = bar_colors(&totals);
    assert_eq!(colors[0], TAG_COLORS[0]);
    assert_eq!(colors[5], GRAY_COLORS, "no tag is gray");
    assert_eq!(colors[6], TAG_COLORS[5], "gray doesn't use up a color");
    for pair in colors.windows(2) {
        assert_ne!(pair[0].0, pair[1].0, "neighbors differ: {pair:?}");
    }
    // the palette itself: no color twice, and it wraps around to a different one
    for (i, c) in TAG_COLORS.iter().enumerate() {
        assert!(TAG_COLORS.iter().skip(i + 1).all(|d| d != c));
    }

    // drawn: the bars top to bottom take the palette in order
    let root = sample_db("colors");
    let mut app = App::new(root.clone());
    press(&mut app, '1');
    let mut terminal = Terminal::new(TestBackend::new(140, 34)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let bar_color = |tag: &str| {
        let line = |y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>();
        let y = (0..buf.area.height).find(|&y| line(y).contains('█') && line(y).contains(tag)).unwrap();
        let x = (0..buf.area.width).find(|&x| buf[(x, y)].symbol() == "█").unwrap();
        buf[(x, y)].fg
    };
    // today: meetings 2 h (first bar), email 1 h (second)
    assert_eq!(bar_color("meetings"), TAG_COLORS[0].0);
    assert_eq!(bar_color("email"), TAG_COLORS[1].0);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn wide_chart_shrinks_in_low_windows() {
    let root = sample_db("low");
    let mut app = App::new(root.clone());
    press(&mut app, '4');
    // room for everything
    let screen = render_at(&mut app, 140, 27);
    assert!(chart_line(&screen, "email").contains('█') && screen.contains("Total 7.00 h  "));
    // enough for the three bars, not for the total line
    let screen = render_at(&mut app, 140, 25);
    println!("{screen}");
    assert!(chart_line(&screen, "email").contains('█'));
    assert!(!screen.lines().any(|l| l.contains("││  Total")));
    // one bar and a "more" line
    let screen = render_at(&mut app, 140, 24);
    println!("{screen}");
    assert!(chart_line(&screen, "planning").contains('█'));
    assert!(screen.contains("… 2 more tags, 3.00 h"));
    assert!(screen.contains("Hours per tag · 3"), "count shown when tags are cut off");
    // too low for the chart: details only
    let screen = render_at(&mut app, 140, 22);
    assert!(screen.contains("Entry details") && !screen.contains("Hours per tag"));
    let _ = fs::remove_dir_all(&root);
}

/// `TagTotal`s as (tag, share, shared) in whole seconds, for comparing.
fn simple(totals: &[TagTotal]) -> Vec<(String, i64, i64)> {
    totals.iter().map(|t| (t.tag.clone(), t.share.round() as i64, t.shared.round() as i64)).collect()
}

#[test]
fn tag_totals_split_time_evenly() {
    let root = env::temp_dir().join(format!("timew-tui-test-multitag-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("data")).unwrap();
    let tz = jiff::tz::TimeZone::system();
    let today = jiff::Zoned::now().date();
    let at = |h: i8| timew_core::interval::format_iso(today.at(h, 0, 0, 0).to_zoned(tz.clone()).unwrap().timestamp().as_second());
    // 8-10 "a b", 10-11 untagged, 11-14 "a b c", 14-15 "c"
    let lines = format!(
        "inc {} - {} # a b\ninc {} - {}\ninc {} - {} # a b c\ninc {} - {} # c\n",
        at(8), at(10), at(10), at(11), at(11), at(14), at(14), at(15)
    );
    fs::write(root.join("data").join(format!("{:04}-{:02}.data", today.year(), today.month())), lines).unwrap();
    let mut app = App::new(root.clone());
    press(&mut app, '1');
    let totals = tag_totals(&app);
    assert_eq!(
        simple(&totals),
        vec![
            // a and b: 2h/2 + 3h/3 = 2h own share; shared 1h + 2h = 3h
            ("a".into(), 7200, 3 * 3600),
            ("b".into(), 7200, 3 * 3600),
            // c: 3h/3 + 1h = 2h own share; shared 2h (same share, less shared: after a and b)
            ("c".into(), 7200, 7200),
            ("(no tag)".into(), 3600, 0),
        ]
    );
    // the own shares add up exactly to the total (7 h)
    let shares: f64 = totals.iter().map(|t| t.share).sum();
    assert!((shares - app.summary.grand_total as f64).abs() < 1e-6);
    assert_eq!(app.summary.grand_total, 7 * 3600);
    // own share + shared = the full time of the entries with that tag
    assert_eq!(totals[0].share + totals[0].shared, 5.0 * 3600.0);

    // the chart shows the share in yellow, the shared time as "+", and a legend
    let screen = render_at(&mut app, 140, 34);
    println!("{screen}");
    let a = chart_line(&screen, "a ");
    assert!(a.contains(" 2.00 h") && a.contains("+3.00 h") && a.contains("29 %"), "{a}");
    let none = chart_line(&screen, "(no tag)");
    assert!(!none.contains('+'), "{none}");
    assert!(screen.contains("own share") && screen.contains("shared (+)"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn stacked_bars_use_eighth_blocks_in_two_shades() {
    let (s, l) = (Color::Indexed(1), Color::Indexed(2));
    let parts = |strong, light| -> Vec<(String, Style)> {
        stacked_bar(strong, light, 4, s, l).into_iter().map(|sp| (sp.content.to_string(), sp.style)).collect()
    };
    let fg = |c| Style::new().fg(c);
    assert_eq!(parts(1.0, 0.0), vec![("████".into(), fg(s))]);
    assert_eq!(parts(0.5, 0.0), vec![("██".into(), fg(s)), ("  ".into(), Style::new())]);
    assert_eq!(parts(0.0, 0.0), vec![("    ".into(), Style::new())]);
    assert_eq!(parts(1.0 / 32.0, 0.0), vec![("▏".into(), fg(s)), ("   ".into(), Style::new())]);
    // 2.25 cells strong
    assert_eq!(parts(0.5625, 0.0), vec![("██▎".into(), fg(s)), (" ".into(), Style::new())]);
    // half strong, a quarter light
    assert_eq!(parts(0.5, 0.25), vec![("██".into(), fg(s)), ("█".into(), fg(l)), (" ".into(), Style::new())]);
    // the strong part ends inside a cell that the light part continues
    assert_eq!(
        parts(0.5625, 0.4375),
        vec![("██".into(), fg(s)), ("▎".into(), Style::new().fg(s).bg(l)), ("█".into(), fg(l))]
    );
    // always exactly `width` cells
    for (a, b) in [(0.0, 0.0), (0.3, 0.3), (0.7, 0.3), (1.0, 0.0), (0.1, 0.05)] {
        let w: usize = parts(a, b).iter().map(|(t, _)| t.chars().count()).sum();
        assert_eq!(w, 4, "{a} {b}");
    }
}

#[test]
fn underscore_tags_are_left_out_of_the_totals() {
    let root = env::temp_dir().join(format!("timew-tui-test-uncounted-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("data")).unwrap();
    let today = jiff::Zoned::now().date();
    let lines = format!(
        "inc {} - {} # meetings\ninc {} - {} # _break meetings\ninc {} - {} # email\n",
        today_at(8, 0),
        today_at(10, 0),
        today_at(10, 0),
        today_at(11, 0),
        today_at(11, 0),
        today_at(12, 0)
    );
    fs::write(root.join("data").join(format!("{:04}-{:02}.data", today.year(), today.month())), lines).unwrap();
    let mut app = App::new(root.clone());
    press(&mut app, '1');
    assert_eq!(app.summary.grand_total, 3 * 3600, "the _break hour doesn't count");

    let mut terminal = Terminal::new(TestBackend::new(160, 30)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let text: Vec<String> =
        (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect();
    let screen = text.join("\n");
    println!("{screen}");
    assert!(screen.contains("Total 3.00 h"), "status line");
    // the day's total (on its last row) is 3.00
    let last = text.iter().find(|l| l.contains("email") && l.contains("12:00")).unwrap();
    assert!(last.contains("1:00  3.00"), "{last}");
    // chart: meetings only with the counted entry, _break without a percentage
    let line = |tag: &str| text.iter().find(|l| l.contains(&format!("  {tag} ")) && l.contains(" h ")).unwrap().clone();
    assert!(line("meetings").contains("2.00 h") && line("meetings").contains("67 %"), "{}", line("meetings"));
    assert!(line("email").contains("33 %"));
    assert!(line("_break").contains("1.00 h") && !line("_break").contains('%'), "{}", line("_break"));
    // in the chart, the hours of _break are light gray, the others yellow
    let hours_color = |tag: &str| {
        let y = text.iter().position(|l| l.contains(&format!("  {tag} ")) && l.contains(" h ")).unwrap();
        let x = text[y][..text[y].find(" h ").unwrap()].chars().count() as u16 - 1;
        buf[(x, y as u16)].fg
    };
    assert_eq!(hours_color("_break"), Color::Gray);
    assert_eq!(hours_color("meetings"), Color::Yellow);
    // the _break row's time is gray, the others are not
    let row = text.iter().position(|l| l.contains("_break, meetings")).unwrap() as u16;
    let time_x = |l: &str| l[..l.find(" 1:00 ").unwrap() + 1].chars().count() as u16;
    let x = time_x(&text[row as usize]);
    assert_eq!(buf[(x, row)].fg, Color::Indexed(244));
    let other = text.iter().position(|l| l.contains("email") && l.contains("12:00")).unwrap() as u16;
    let x2 = time_x(&text[other as usize]);
    assert_ne!(buf[(x2, other)].fg, Color::Indexed(244));
    let _ = fs::remove_dir_all(&root);
}
