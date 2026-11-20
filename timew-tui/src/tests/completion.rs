//! Tag completion in the tag fields of the start and edit forms.

use super::*;
use crate::forms::tag_completion;

fn known() -> Vec<(String, usize)> {
    [("meetings", 5), ("email", 3), ("meta", 1), ("planning", 2), ("deep work", 4), ("team", 2), ("teamwork", 1)]
        .into_iter()
        .map(|(t, c)| (t.to_string(), c))
        .collect()
}

/// What appears after the cursor for `text`.
fn complete(text: &str) -> Option<String> {
    tag_completion(text, text.chars().count(), &known()).map(|c| c.rest)
}

/// The field after taking the suggestion for `text`, with `known` tags.
fn accepted(text: &str, known: &[(&str, usize)]) -> Option<String> {
    let known: Vec<(String, usize)> = known.iter().map(|(t, c)| (t.to_string(), *c)).collect();
    let completion = tag_completion(text, text.chars().count(), &known)?;
    let mut form = crate::forms::StartForm { suggest: true, ..Default::default() };
    form.fields[0] = text.to_string();
    form.cursor = text.chars().count();
    form.accept(&completion);
    assert_eq!(form.cursor, form.fields[0].chars().count(), "the cursor ends up at the end");
    Some(form.fields[0].clone())
}

#[test]
fn completes_the_most_used_matching_tag() {
    assert_eq!(complete("m").as_deref(), Some("eetings"), "meetings (5) before meta (1)");
    assert_eq!(complete("met").as_deref(), Some("a"));
    assert_eq!(complete("email pl").as_deref(), Some("anning"), "the last tag counts");
    assert_eq!(complete("te").as_deref(), Some("am"), "equally used: the first by name");
    assert_eq!(complete("team").as_deref(), Some("work"), "a longer tag completes a whole one");
}

#[test]
fn no_completion_when_nothing_fits() {
    assert_eq!(complete(""), None);
    assert_eq!(complete("email "), None, "after a space, between tags");
    assert_eq!(complete("x"), None, "no tag starts with it");
    assert_eq!(complete("meetings"), None, "already complete");
    assert_eq!(complete("meetings me").as_deref(), Some("ta"), "meetings is already there, meta is next");
    assert_eq!(complete("meetings meta email m"), None, "both m-tags are already there");
    assert_eq!(tag_completion("me", 1, &known()), None, "only with the cursor at the end");
}

#[test]
fn case_is_ignored_and_the_stored_case_is_used() {
    for typed in ["ab", "AB", "aB", "abc", "ABC", "Abc"] {
        for tag in ["ABC", "Abc", "aBc"] {
            // typed exactly as stored: nothing to complete
            let expected = (typed != tag).then(|| tag.to_string());
            assert_eq!(accepted(typed, &[(tag, 1)]), expected, "{typed} → {tag}");
        }
    }
    assert_eq!(accepted("email ab", &[("ABC", 1)]).as_deref(), Some("email ABC"), "only the last tag changes");
    assert_eq!(accepted("\"Deep W", &[("deep work", 1)]).as_deref(), Some("\"deep work\""));
    // shown after the cursor: the rest in the stored case; nothing if only the case differs
    let known = vec![("ABC".to_string(), 1)];
    let c = tag_completion("ab", 2, &known).unwrap();
    assert_eq!((c.tag.as_str(), c.rest.as_str()), ("ABC", "C"));
    assert_eq!(tag_completion("abc", 3, &known).unwrap().rest, "");
}

#[test]
fn case_variants_choose_by_use_and_respect_exact_tags() {
    let known = [("ABC", 1), ("abc", 3), ("Abc", 2)];
    assert_eq!(accepted("a", &known).as_deref(), Some("abc"), "the most used spelling");
    // typed exactly as a known tag: other spellings are not offered, longer tags are
    assert_eq!(accepted("ABC", &known), None);
    assert_eq!(accepted("ABC", &[("ABC", 1), ("abc", 3), ("abcd", 1)]).as_deref(), Some("abcd"));
    // a tag already in the field, in any case, is not offered again
    assert_eq!(accepted("abc a", &[("ABC", 1)]), None);
    // characters that are not letters must match exactly
    assert_eq!(accepted("a-", &[("a_b", 1)]), None);
}

#[test]
fn tags_with_spaces_need_a_quote() {
    assert_eq!(complete("de"), None, "\"deep work\" can't be typed without quotes");
    assert_eq!(complete("\"de").as_deref(), Some("ep work\""), "inside a quote, with the closing quote");
    assert_eq!(complete("\"deep work").as_deref(), Some("\""), "just the closing quote");
    assert_eq!(complete("\"deep work\""), None, "the quote is closed");
    assert_eq!(complete("email \"pl").as_deref(), Some("anning\""));
}

/// The cells of `text` on the screen: their foreground and background.
fn colors_of(app: &mut App, text: &str) -> Vec<(Color, Color)> {
    let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buf = terminal.backend().buffer();
    for y in 0..buf.area.height {
        let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        if let Some(i) = line.find(text) {
            let x = line[..i].chars().count() as u16;
            return (x..x + text.chars().count() as u16).map(|x| (buf[(x, y)].fg, buf[(x, y)].bg)).collect();
        }
    }
    panic!("{text} is not on the screen");
}

#[test]
fn start_form_suggests_and_tab_takes_it() {
    let root = sample_db("complete-start");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    press(&mut app, 's');
    type_text(&mut app, "pl");
    let screen = render_at(&mut app, 140, 24);
    assert!(screen.contains(" planning") && screen.contains("Tab or → takes the suggested tag"), "{screen}");
    // typed text normal, the cursor after it, then the rest of the
    // suggestion dark gray (the field is reversed)
    let colors = colors_of(&mut app, "planning");
    assert!(colors[..2].iter().all(|(_, bg)| *bg == Color::Reset), "{colors:?}");
    assert_eq!(colors[2], (Color::White, crate::ui::CURSOR), "{colors:?}");
    assert!(colors[3..].iter().all(|(_, bg)| *bg == Color::DarkGray), "{colors:?}");

    key(&mut app, KeyCode::Tab);
    let form = app.start_form.as_ref().unwrap();
    assert_eq!((form.fields[0].as_str(), form.field, form.cursor), ("planning", 0, 8));
    // nothing more to suggest: Tab moves on again
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.start_form.as_ref().unwrap().field, 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn start_form_suggests_only_while_typing() {
    let root = sample_db("complete-typing");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    press(&mut app, 's');
    type_text(&mut app, "em");
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Right);
    // moved, not typed: no suggestion, so → and Tab do what they always do
    assert!(app.start_form.as_ref().unwrap().suggestion(&app.all_tags).is_none());
    assert!(!render_at(&mut app, 140, 24).contains("suggested tag"));
    key(&mut app, KeyCode::Tab);
    let form = app.start_form.as_ref().unwrap();
    assert_eq!((form.fields[0].as_str(), form.field), ("em", 1));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn edit_form_suggests_tags_and_right_takes_it() {
    let root = sample_db("complete-edit");
    let mut app = App::new(root.clone());
    app.config_dir = root.clone();
    press(&mut app, '1');
    render_at(&mut app, 140, 24);
    press(&mut app, 'e');
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::End);
    assert_eq!(app.edit.as_ref().unwrap().fields[TAGS], "meetings");
    type_text(&mut app, " e");
    assert!(render_at(&mut app, 140, 24).contains("meetings email"));
    key(&mut app, KeyCode::Right);
    let form = app.edit.as_ref().unwrap();
    assert_eq!((form.fields[TAGS].as_str(), form.field), ("meetings email", TAGS));
    // a tag that is already there is not suggested
    type_text(&mut app, " me");
    assert!(app.edit.as_ref().unwrap().suggestion(&app.all_tags).is_none());
    // the date fields never suggest
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.edit.as_ref().unwrap().field, 0);
    press(&mut app, '2');
    assert!(app.edit.as_ref().unwrap().suggestion(&app.all_tags).is_none());
    key(&mut app, KeyCode::Esc);
    let _ = fs::remove_dir_all(&root);
}
