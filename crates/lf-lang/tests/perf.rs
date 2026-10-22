//! Speed gate: scoring a full 256-key buffer takes under 1 ms.
//!
//! Only meaningful in release mode, so it is ignored by default. CI runs it
//! with `cargo test --release -p lf-lang -- --ignored`.

use std::time::{Duration, Instant};

use lf_core::{build_words, Candidate, KeyPress, SplitScorer, BUFFER_CAPACITY};

#[test]
#[ignore = "timing test; run with --release -- --ignored"]
fn scoring_full_buffer_under_1ms() {
    let us = lf_layouts::us();
    let ir = lf_layouts::ir_pes();
    let text: String = "I'm at work nhvl ldhl "
        .chars()
        .cycle()
        .take(BUFFER_CAPACITY)
        .collect();
    let keys: Vec<KeyPress> = text
        .chars()
        .map(|ch| {
            let (code, mods) = us.key_for(ch).unwrap();
            KeyPress {
                code,
                mods,
                layout: us.id.clone(),
                produced: Some(ch),
            }
        })
        .collect();
    assert_eq!(keys.len(), 256);
    let scorer = lf_lang::default_scorer();

    // Best of several runs, so a busy CI machine does not cause a failure.
    let mut best = Duration::MAX;
    for _ in 0..50 {
        let t = Instant::now();
        let words = build_words(&keys, &us, &ir);
        let split = scorer.first_wrong_word(&Candidate {
            words: &words,
            from: &us,
            to: &ir,
        });
        best = best.min(t.elapsed());
        assert!(split.is_some());
    }
    assert!(best < Duration::from_millis(1), "best run took {best:?}");
    println!("256 keys scored in {best:?}");
}
