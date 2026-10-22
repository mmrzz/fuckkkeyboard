//! End-to-end runs of `lf-train prepare`, `train` and `inspect` on the
//! fixture corpora.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use lf_lang::{Language, LanguageModel};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A fresh, empty folder; tests run in parallel, so each call gets its own.
fn out_dir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn lf_train(args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_lf-train"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "lf-train {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn prepare_and_train(lang: &str) -> (PathBuf, Vec<u8>) {
    let dir = out_dir(&format!("trainer-{lang}"));
    let clean = dir.join("clean.txt");
    let model = dir.join("model.lfng");
    let input = fixtures().join(lang);
    lf_train(&[
        "prepare",
        "--lang",
        lang,
        "--in",
        input.to_str().unwrap(),
        "--out",
        clean.to_str().unwrap(),
    ]);
    lf_train(&[
        "train",
        "--lang",
        lang,
        "--in",
        clean.to_str().unwrap(),
        "--out",
        model.to_str().unwrap(),
        "--prune",
        "1",
        "--heldout",
        "0.1",
    ]);
    (clean, fs::read(&model).unwrap())
}

#[test]
fn prepare_cleans_fixture_corpus() {
    let (clean, _) = prepare_and_train("en");
    let text = fs::read_to_string(clean).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    // 25 lines minus one short, one duplicate and one Persian line.
    assert_eq!(lines.len(), 22);
    assert!(lines.iter().all(|l| !l.contains('\t')));
    assert!(!text.contains("Short."));
    assert!(!text.contains("فارسی"));
    assert_eq!(text.matches("The weather is nice").count(), 1);

    let (clean, _) = prepare_and_train("fa");
    let text = fs::read_to_string(clean).unwrap();
    assert_eq!(text.lines().count(), 22);
    assert!(!text.contains("English"));
}

#[test]
fn prepare_is_reproducible() {
    let (a, _) = prepare_and_train("fa");
    let a = fs::read_to_string(a).unwrap();
    let (b, _) = prepare_and_train("fa");
    assert_eq!(a, fs::read_to_string(b).unwrap());
}

#[test]
fn trained_models_load_and_prefer_their_language() {
    let (_, en) = prepare_and_train("en");
    let (_, fa) = prepare_and_train("fa");
    let en = LanguageModel::from_bytes(&en).unwrap();
    let fa = LanguageModel::from_bytes(&fa).unwrap();
    assert_eq!(en.language(), Language::En);
    assert_eq!(fa.language(), Language::Fa);
    assert_eq!(en.order(), 4);
    for m in [&en, &fa] {
        assert!(m.h() < 0.0 && m.h().is_finite());
    }
    // Seen words beat their keyboard-garbled counterparts.
    assert!(en.word_log2prob("meeting") > en.word_log2prob("lvsd"));
    assert!(fa.word_log2prob("جلسه") > fa.word_log2prob("ثپسی"));
}

#[test]
fn inspect_prints_header() {
    let dir = out_dir("trainer-inspect");
    let (_, bytes) = prepare_and_train("en");
    let model = dir.join("en.lfng");
    fs::write(&model, bytes).unwrap();
    let out = lf_train(&["inspect", model.to_str().unwrap()]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("language    en"), "{text}");
    assert!(text.contains("order       4"), "{text}");
    assert!(text.contains("top contexts"), "{text}");
}

#[test]
fn inspect_rejects_corrupt_model() {
    let dir = out_dir("trainer-corrupt");
    let (_, mut bytes) = prepare_and_train("en");
    let n = bytes.len();
    bytes[n / 2] ^= 0xFF;
    let model = dir.join("bad.lfng");
    fs::write(&model, bytes).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lf-train"))
        .args(["inspect", model.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("checksum"));
}

#[test]
fn train_with_word_list() {
    let dir = out_dir("trainer-words");
    let (clean, _) = prepare_and_train("en");
    let model = dir.join("en.lfng");
    let words = fixtures().join("words");
    lf_train(&[
        "train",
        "--lang",
        "en",
        "--in",
        clean.to_str().unwrap(),
        "--out",
        model.to_str().unwrap(),
        "--words",
        words.to_str().unwrap(),
        "--top-words",
        "2",
    ]);
    let m = LanguageModel::from_bytes(&fs::read(&model).unwrap()).unwrap();
    // "the" + "The" + "meeting" + "NV" count; "!", "on the", "café", "42" do not.
    assert_eq!(m.data().word_total, 1042);
    assert_eq!(m.data().words.len(), 2);
    assert!((m.word_ppm("the").unwrap() - 1000.0 * 1e6 / 1042.0).abs() < 1e-6);
    assert!(m.word_ppm("meeting").is_some());
    assert_eq!(m.word_ppm("nv"), None);
}
