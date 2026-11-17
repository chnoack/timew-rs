//! `x`: the CSV export of the table.

use super::*;

#[test]
fn x_exports_the_filtered_table_with_a_heading() {
    let root = sample_db("export");
    let mut app = App::new(root.clone());
    press(&mut app, '1');
    press(&mut app, 'x');
    let today = local_date(app.range.start);
    assert_eq!(app.export.as_ref().unwrap().path, format!("timew-{today}.csv"));
    let screen = render_at(&mut app, 100, 24);
    assert!(screen.contains("Export as CSV") && screen.contains("2 rows of the table"), "{screen}");

    let file = root.join("today.csv");
    clear_field(&mut app);
    type_text(&mut app, file.to_str().unwrap());
    key(&mut app, KeyCode::Enter);
    assert!(app.export.is_none());
    assert!(app.notice.as_deref().unwrap().starts_with("Exported 2 rows to "), "{:?}", app.notice);
    let csv = fs::read_to_string(&file).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "Wk,Date,Day,Tags,Start,End,Time,Total,Annotation");
    assert_eq!(lines.len(), 3, "{csv}");
    assert!(lines[1].contains(&format!(",{today},")) && lines[1].contains(",meetings,8:00,10:00,2:00,,"), "{csv}");
    assert!(lines[2].contains(&format!(",{today},")) && lines[2].ends_with(",email,10:00,11:00,1:00,3.00,"), "{csv}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn export_asks_before_overwriting_and_names_the_tag() {
    let root = sample_db("export-overwrite");
    let mut app = App::new(root.clone());
    press(&mut app, '2');
    app.tag = Some("planning".into());
    app.load();
    press(&mut app, 'x');
    let day = local_date(app.range.start);
    assert_eq!(app.export.as_ref().unwrap().path, format!("timew-{day}-planning.csv"));

    let file = root.join("out.csv");
    fs::write(&file, "old").unwrap();
    clear_field(&mut app);
    type_text(&mut app, file.to_str().unwrap());
    key(&mut app, KeyCode::Enter);
    assert!(app.export.as_ref().unwrap().error.as_deref().unwrap().contains("exists"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "old");
    key(&mut app, KeyCode::Enter);
    assert!(app.export.is_none());
    let csv = fs::read_to_string(&file).unwrap();
    assert_eq!(csv.lines().count(), 2, "{csv}");
    assert!(csv.contains(",planning,8:00,12:00,4:00,4.00,"), "{csv}");

    // Esc cancels without writing, a directory is refused
    press(&mut app, 'x');
    clear_field(&mut app);
    type_text(&mut app, root.to_str().unwrap());
    key(&mut app, KeyCode::Enter);
    assert!(app.export.as_ref().unwrap().error.as_deref().unwrap().contains("directory"));
    key(&mut app, KeyCode::Esc);
    assert!(app.export.is_none() && app.notice.is_none());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn export_uses_the_separator_setting() {
    let root = sample_db("export-separator");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    fs::write(root.join("timewarrior.cfg"), "export.csv.separator = ;\n").unwrap();
    press(&mut app, '1');
    press(&mut app, 'x');
    let screen = render_at(&mut app, 100, 24);
    assert!(screen.contains("Separator: ; (export.csv.separator)"), "{screen}");
    let file = root.join("semi.csv");
    clear_field(&mut app);
    type_text(&mut app, file.to_str().unwrap());
    key(&mut app, KeyCode::Enter);
    let csv = fs::read_to_string(&file).unwrap();
    assert!(csv.starts_with("Wk;Date;Day;Tags;Start;End;Time;Total;Annotation\n"), "{csv}");
    assert!(csv.contains(";meetings;8:00;10:00;2:00;;"), "{csv}");

    // an invalid value is reported in the dialog, nothing is written
    fs::write(root.join("timewarrior.cfg"), "export.csv.separator = ;;\n").unwrap();
    press(&mut app, 'x');
    clear_field(&mut app);
    type_text(&mut app, root.join("bad.csv").to_str().unwrap());
    key(&mut app, KeyCode::Enter);
    assert!(app.export.as_ref().unwrap().error.as_deref().unwrap().contains("export.csv.separator"));
    assert!(!root.join("bad.csv").exists());
    let _ = fs::remove_dir_all(&root);
}
