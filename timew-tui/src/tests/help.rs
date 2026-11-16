use super::*;

#[test]
fn help_line_always_ends_with_quit() {
    let root = sample_db("help");
    let mut app = App::new(root.clone());
    for w in [40u16, 60, 90, 140] {
        let screen = render_at(&mut app, w, 24);
        let help = screen.lines().last().unwrap();
        assert!(help.ends_with("q quit"), "width {w}: {help}");
        assert!(help.chars().count() <= w as usize);
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn question_mark_opens_the_key_legend() {
    let root = sample_db("legend");
    let mut app = App::new(root.clone());

    press(&mut app, '?');
    assert_eq!(app.legend, Some(0));
    let screen = render_at(&mut app, 100, 60);
    println!("{screen}");
    assert!(screen.contains(" Keys ") && screen.contains("Periods") && screen.contains("hours-per-tag chart"));
    // every line fits at this height: no scrolling
    press(&mut app, 'j');
    render_at(&mut app, 100, 60);
    assert_eq!(app.legend, Some(0));

    // the legend takes all keys: these don't change the filter or quit
    press(&mut app, '1');
    assert_eq!(app.filter, Filter::Month);
    key(&mut app, KeyCode::Esc);
    assert!(app.legend.is_none() && !app.quit);

    // in a small window it scrolls, and the offset is clamped
    press(&mut app, '?');
    let screen = render_at(&mut app, 60, 20);
    println!("{screen}");
    assert!(screen.contains("Moving") && !screen.contains("Ctrl+C"));
    press(&mut app, 'G');
    let screen = render_at(&mut app, 60, 20);
    println!("{screen}");
    assert!(screen.contains("Ctrl+C") && !screen.contains("Moving"));
    press(&mut app, 'g');
    assert_eq!(app.legend, Some(0));
    press(&mut app, '?');
    assert!(app.legend.is_none());

    // a click anywhere closes it
    press(&mut app, '?');
    app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 0, row: 0, modifiers: KeyModifiers::NONE });
    assert!(app.legend.is_none());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn footer_always_starts_with_help() {
    let root = sample_db("footer-help");
    let mut app = App::new(root.clone());
    let footer = |app: &mut App, w| render_at(app, w, 34).lines().last().unwrap().to_string();
    let wide = footer(&mut app, 140);
    assert!(wide.starts_with(" ? help  ") && wide.contains("r reload"), "{wide}");
    for w in [20u16, 40, 60, 90] {
        let narrow = footer(&mut app, w);
        assert!(narrow.starts_with(" ? help  ") && narrow.ends_with("q quit"), "width {w}: {narrow}");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn i_shows_version_license_and_project_page() {
    let root = sample_db("info");
    let mut app = App::new(root.clone());
    press(&mut app, '1');
    assert!(footer(&mut app).contains("i info"), "{}", footer(&mut app));
    press(&mut app, 'i');
    let screen = render_at(&mut app, 140, 24);
    println!("{screen}");
    assert!(screen.contains(" Info "));
    assert!(screen.contains(&format!("timew-tui {}", env!("CARGO_PKG_VERSION"))));
    assert!(screen.contains(&format!("timew-core {}", timew_core::VERSION)));
    assert!(screen.contains("https://github.com/chnoack/timew-rs"));
    assert!(screen.contains("License   MIT"));
    assert!(screen.contains("Copyright 2026, Christian Noack"));
    assert!(screen.contains("2015 - 2023, Thomas Lauf, Paul Beckingham, Federico Hernandez (Timewarrior)"));
    // it takes the keys: q closes it instead of quitting
    press(&mut app, 'q');
    assert!(!app.info && !app.quit);
    assert!(!render_at(&mut app, 140, 24).contains(" Info "));
    // a click closes it too
    press(&mut app, 'i');
    app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 0, row: 0, modifiers: KeyModifiers::NONE });
    assert!(!app.info);
    let _ = fs::remove_dir_all(&root);
}
