//! Acceptance tests from PHASE-1.md section 11.

use lf_core::{
    AllWordsScorer, Command, Config, Engine, Harness, LastWordScorer, LayoutId, SplitScorer,
};

fn harness(start: &str, scorer: Box<dyn SplitScorer>) -> Harness {
    let layouts = lf_layouts::all();
    let engine = Engine::new(
        Config::default(),
        layouts.clone(),
        scorer,
        LayoutId::new(start),
    )
    .expect("valid engine");
    Harness::new(engine, layouts)
}

/// Run `script` and return every command the engine emitted.
fn run(h: &mut Harness, script: &str) -> Vec<Command> {
    let mut cmds = Vec::new();
    h.run_with(script, |c| cmds.extend_from_slice(c))
        .expect("script runs");
    cmds
}

fn check(h: &Harness, screen: &str, layout: &str) {
    assert_eq!(h.screen.text, screen);
    assert_eq!(h.screen.layout, LayoutId::new(layout));
    assert_eq!(h.engine.current_layout(), &LayoutId::new(layout));
}

#[test]
fn t01_basic_fix() {
    let mut h = harness("us", Box::new(LastWordScorer));
    let cmds = run(&mut h, "sghl{trigger}");
    assert_eq!(
        cmds,
        vec![
            Command::Replace {
                backspaces: 10,
                text: "سلام".into()
            },
            Command::SetLayout(LayoutId::new("ir")),
        ]
    );
    check(&h, "سلام", "ir");
}

#[test]
fn t02_last_word_only() {
    let mut h = harness("us", Box::new(LastWordScorer));
    run(&mut h, "hello sghl{trigger}");
    check(&h, "hello سلام", "ir");
}

#[test]
fn t03_all_words() {
    let mut h = harness("us", Box::new(AllWordsScorer));
    run(&mut h, "sghl n,sj lk{trigger}");
    check(&h, "سلام دوست من", "ir");
}

#[test]
fn t04_trigger_on_persian_keys() {
    let mut h = harness("ir", Box::new(LastWordScorer));
    let cmds = run(&mut h, "اثممخ{trigger}");
    assert_eq!(
        cmds[0],
        Command::Replace {
            backspaces: 11,
            text: "hello".into()
        }
    );
    check(&h, "hello", "us");
}

#[test]
fn t05_four_key_word_is_not_trigger() {
    let mut h = harness("us", Box::new(LastWordScorer));
    assert!(run(&mut h, "fuck").is_empty());
    check(&h, "fuck", "us");
}

#[test]
fn t06_backspace_keeps_buffer_in_sync() {
    let mut h = harness("us", Box::new(LastWordScorer));
    run(&mut h, "sghx{bs}l{trigger}");
    check(&h, "سلام", "ir");
}

#[test]
fn t07_click_resets() {
    let mut h = harness("us", Box::new(LastWordScorer));
    assert!(run(&mut h, "sghl{click}{trigger}").is_empty());
    check(&h, "sghlfuckkk", "us");
}

#[test]
fn t08_chord_resets() {
    let mut h = harness("us", Box::new(LastWordScorer));
    assert!(run(&mut h, "sghl{chord}{trigger}").is_empty());
    check(&h, "sghlfuckkk", "us");
}

#[test]
fn t09_idle_resets() {
    let mut h = harness("us", Box::new(LastWordScorer));
    assert!(run(&mut h, "sghl{wait:31000}{trigger}").is_empty());
    check(&h, "sghlfuckkk", "us");
}

#[test]
fn t10_second_trigger_undoes() {
    let mut h = harness("us", Box::new(LastWordScorer));
    let cmds = run(&mut h, "sghl{trigger}{trigger}");
    assert_eq!(
        cmds[2],
        Command::Replace {
            backspaces: 10,
            text: "sghl".into()
        }
    );
    check(&h, "sghl", "us");
    assert!(!h.engine.has_undo_record());
}

#[test]
fn t11_undo_window_passed_is_normal_fix() {
    let mut h = harness("us", Box::new(LastWordScorer));
    run(&mut h, "sghl{trigger}{wait:11000}{trigger}");
    check(&h, "sghl", "us");
    // An undo drops the record; a fix leaves a fresh one.
    assert!(h.engine.has_undo_record());
}

#[test]
fn t12_other_key_clears_undo_record() {
    // `ط` is KeyX on the Persian layout, which is active after the first fix.
    let mut h = harness("us", Box::new(LastWordScorer));
    run(&mut h, "sghl{trigger}ط{trigger}");
    check(&h, "sghlx", "us");
    assert!(h.engine.has_undo_record());
}

#[test]
fn t13_range_stops_at_layout_switch() {
    let mut h = harness("us", Box::new(LastWordScorer));
    run(&mut h, "hello {layout:ir}اثممخ{trigger}");
    check(&h, "hello hello", "us");
}

#[test]
fn t14_literal_braces() {
    let mut h = harness("us", Box::new(LastWordScorer));
    assert!(run(&mut h, "ldo{{}}").is_empty());
    check(&h, "ldo{}", "us");
}

#[test]
fn t15_buffer_capacity() {
    let mut h = harness("us", Box::new(AllWordsScorer));
    let script = format!("{}{{trigger}}", "a".repeat(300));
    run(&mut h, &script);
    let expected = format!("{}{}", "a".repeat(50), "ش".repeat(250));
    check(&h, &expected, "ir");
}
