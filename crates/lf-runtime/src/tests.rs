use std::time::{Duration, Instant};

use lf_core::{KeyPress, Layout};
use lf_platform::MockBackend;

use super::*;

fn mock() -> MockBackend {
    MockBackend::us_ir()
}

fn run(b: &MockBackend, config: RuntimeConfig) -> RuntimeHandle {
    start(Box::new(b.clone()), config).unwrap()
}

/// Wait until every event pushed so far has been processed, by pushing a
/// focus marker and waiting for it to show up in `recent_apps`.
fn settle(b: &MockBackend, h: &RuntimeHandle) {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let marker = AppId(format!(
        "marker-{}",
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    assert!(b.push(PlatformEvent::FocusChanged(Some(marker.clone()))));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !h.recent_apps().contains(&marker) {
        assert!(Instant::now() < deadline, "runtime did not settle");
        thread::sleep(Duration::from_millis(1));
    }
}

/// Wait (without sending anything) until the status satisfies `cond`.
fn wait_for(h: &RuntimeHandle, cond: impl Fn(&Status) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond(&h.status()) {
        assert!(Instant::now() < deadline, "timed out: {:?}", h.status());
        thread::sleep(Duration::from_millis(1));
    }
}

/// Wait until every event pushed so far has been processed, without
/// resetting the engine: bounce the layout to "ir" and back to "us".
fn keep_buffer_barrier(b: &MockBackend, h: &RuntimeHandle) {
    for id in ["ir", "us"] {
        b.push(PlatformEvent::Input(InputEvent::LayoutChanged(
            LayoutId::new(id),
        )));
        wait_for(h, |s| s.current_layout == Some(LayoutId::new(id)));
    }
}

/// Type `text` on the US layout (keys as the backend would report them).
fn type_us(b: &MockBackend, text: &str) {
    let us = lf_layouts::us();
    for ch in text.chars() {
        let (code, mods) = us.key_for(ch).unwrap();
        b.push(PlatformEvent::Input(InputEvent::Key(KeyPress {
            code,
            mods,
            layout: us.id.clone(),
            produced: Some(ch),
        })));
    }
}

fn focus(b: &MockBackend, app: &str) {
    b.push(PlatformEvent::FocusChanged(Some(AppId::new(app))));
}

const TRIGGER: &str = "fuckkk";

#[test]
fn starts_with_auto_pair_and_live_layouts() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    let s = h.status();
    assert!(s.running && !s.paused);
    assert_eq!(
        s.layout_pair,
        Some((LayoutId::new("us"), LayoutId::new("ir")))
    );
    assert_eq!(s.current_layout, Some(LayoutId::new("us")));
    let ids: Vec<_> = h.layouts().into_iter().map(|(id, _, _)| id.0).collect();
    assert_eq!(ids, ["us", "ir"]);
    assert!(b.is_started());
    h.stop();
    assert!(!b.is_started(), "stop stops the backend");
}

#[test]
fn trigger_fixes_and_counts() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    focus(&b, "firefox");
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    assert_eq!(
        b.executed(),
        vec![vec![
            Command::Replace {
                backspaces: 10,
                text: "سلام".into()
            },
            Command::SetLayout(LayoutId::new("ir")),
        ]]
    );
    let s = h.status();
    assert_eq!((s.fixes, s.undos), (1, 0));
    assert_eq!(s.current_layout, Some(LayoutId::new("ir")));
}

#[test]
fn second_trigger_counts_an_undo() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    // A settle marker would reset the engine and drop the undo record.
    wait_for(&h, |s| s.fixes == 1);
    // Now on "ir"; the trigger keys are the same physical keys.
    let ir = lf_layouts::ir_pes();
    for code in Trigger::default_trigger().keys() {
        b.push(PlatformEvent::Input(InputEvent::Key(KeyPress {
            code: *code,
            mods: lf_core::Modifiers::NONE,
            layout: ir.id.clone(),
            produced: ir.char_for(*code, lf_core::Modifiers::NONE),
        })));
    }
    wait_for(&h, |s| s.undos == 1);
    let s = h.status();
    assert_eq!((s.fixes, s.undos), (1, 1));
    assert_eq!(b.executed().len(), 2);
}

#[test]
fn pause_drops_input_and_resume_restores_it() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    type_us(&b, "sg");
    h.pause();
    type_us(&b, "hl");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    assert!(h.status().paused);
    assert!(b.executed().is_empty());

    h.resume();
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    assert!(!h.status().paused);
    assert_eq!(h.status().fixes, 1);
}

#[test]
fn pause_resets_the_buffer() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    type_us(&b, "sghl");
    keep_buffer_barrier(&b, &h);
    h.pause();
    h.resume();
    type_us(&b, TRIGGER);
    settle(&b, &h);
    assert!(b.executed().is_empty(), "nothing left to fix after pause");
}

#[test]
fn blocked_app_drops_input_case_insensitively() {
    let b = mock();
    let config = RuntimeConfig {
        blocklist: vec!["KeePassXC".into()],
        ..RuntimeConfig::default()
    };
    let h = run(&b, config);
    focus(&b, "keepassxc");
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    settle_without_focus(&b, &h, "keepassxc");
    assert!(h.status().focused_app_blocked);
    assert!(b.executed().is_empty());

    focus(&b, "firefox");
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    assert_eq!(h.status().fixes, 1);
}

/// Like `settle`, but refocus `app` afterwards so the blocked state can be checked.
fn settle_without_focus(b: &MockBackend, h: &RuntimeHandle, app: &str) {
    settle(b, h);
    focus(b, app);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.recent_apps().first() != Some(&AppId::new(app)) {
        assert!(Instant::now() < deadline, "runtime did not settle");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn focus_change_resets_the_buffer() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    type_us(&b, "sghl");
    focus(&b, "code");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    assert!(b.executed().is_empty());
}

#[test]
fn execute_error_resets_and_sets_last_error() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    b.fail_next_execute(BackendError::Injection("denied".into()));
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    let s = h.status();
    assert_eq!(s.last_error.as_deref(), Some("injection failed: denied"));
    assert_eq!(s.fixes, 0);

    // The buffer was reset, so a trigger alone does nothing...
    type_us(&b, TRIGGER);
    settle(&b, &h);
    assert!(b.executed().is_empty());
    // ...and a fresh fix works and clears the error.
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    let s = h.status();
    assert_eq!(s.last_error, None);
    assert_eq!(s.fixes, 1);
}

#[test]
fn recent_apps_are_distinct_most_recent_first_and_capped() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    for i in 0..12 {
        focus(&b, &format!("app{i}"));
    }
    focus(&b, "app5");
    settle(&b, &h);
    let recent = h.recent_apps();
    assert_eq!(recent.len(), RECENT_APPS);
    assert_eq!(recent[1], AppId::new("app5"));
    assert_eq!(recent.iter().filter(|a| a.0 == "app5").count(), 1);
}

#[test]
fn set_config_rebuilds_the_engine() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    let config = RuntimeConfig {
        switch_layout_after_fix: false,
        blocklist: vec!["firefox".into()],
        ..RuntimeConfig::default()
    };
    h.set_config(config).unwrap();
    focus(&b, "code");
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    assert_eq!(b.executed().len(), 1);
    assert_eq!(b.executed()[0].len(), 1, "no SetLayout after the rebuild");
    assert_eq!(h.status().current_layout, Some(LayoutId::new("us")));

    focus(&b, "firefox");
    settle_without_focus(&b, &h, "firefox");
    assert!(h.status().focused_app_blocked);
}

#[test]
fn set_config_with_unknown_pair_keeps_the_old_engine() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    let bad = RuntimeConfig {
        layout_pair: Some((LayoutId::new("us"), LayoutId::new("de"))),
        ..RuntimeConfig::default()
    };
    assert_eq!(
        h.set_config(bad),
        Err(RuntimeError::Engine(EngineError::MissingLayout(
            LayoutId::new("de")
        )))
    );
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    assert_eq!(h.status().fixes, 1);
}

#[test]
fn layouts_changed_reloads_layouts() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    let mut layouts = b.layouts().unwrap();
    layouts.insert(
        0,
        LayoutInfo {
            layout: Layout::new("de", "German", &[]),
            language: Some("de".into()),
        },
    );
    b.set_layouts(layouts);
    b.push(PlatformEvent::LayoutsChanged);
    settle(&b, &h);
    let ids: Vec<_> = h.layouts().into_iter().map(|(id, _, _)| id.0).collect();
    assert_eq!(ids, ["de", "us", "ir"]);
    // Auto pair: first two with different languages.
    assert_eq!(
        h.status().layout_pair,
        Some((LayoutId::new("de"), LayoutId::new("us")))
    );
}

#[test]
fn no_pair_without_two_languages() {
    let one = vec![LayoutInfo {
        layout: Layout::new("de", "German", &[]),
        language: Some("de".into()),
    }];
    // Built-ins still give a usable pair.
    let b = MockBackend::new(one, LayoutId::new("de"));
    let h = run(&b, RuntimeConfig::default());
    assert_eq!(
        h.status().layout_pair,
        Some((LayoutId::new("de"), LayoutId::new("us")))
    );

    let same = vec![
        LayoutInfo {
            layout: lf_layouts::us(),
            language: Some("en".into()),
        },
        LayoutInfo {
            layout: lf_layouts::ir_pes(),
            language: None,
        },
    ];
    let b = MockBackend::new(same, LayoutId::new("us"));
    assert!(auto_pair(&b.layouts().unwrap()).is_none());
}

#[test]
fn status_never_contains_typed_text() {
    let b = mock();
    let h = run(&b, RuntimeConfig::default());
    type_us(&b, "sghl");
    type_us(&b, TRIGGER);
    settle(&b, &h);
    let dump = format!("{:?}", h.status());
    assert!(!dump.contains("sghl") && !dump.contains("سلام"), "{dump}");
}
