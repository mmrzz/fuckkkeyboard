# Layout Fixer

Fixes text typed in the wrong keyboard layout. Type `sghl` with US English
active when you meant Persian, then type the trigger `fuckkk`: the text is
replaced with `سلام` and the system switches to Persian. Typing the trigger
again within 10 seconds undoes the fix.

Phase 1 built the platform-independent core (spec: `plans/PHASE-1.md`).
Phase 2 adds a split scorer that finds where the wrong-layout text starts,
using character n-gram models of English and Persian (spec:
`plans/PHASE-2.md`, results: `BENCHMARK.md`). There is no OS integration yet.

## Crates

| Crate | What it holds |
| --- | --- |
| `lf-core` | Key types, layouts, the key buffer, trigger matching, the split scorer trait, the correction engine, and a screen simulator (`Harness`) for tests. |
| `lf-layouts` | Built-in layout tables: US English (`us`) and Persian Standard (`ir`, transcribed from XKB `symbols/ir`, section `pes`). |
| `lf-lang` | Language normalization, the `.lfng` character model format, and `NgramScorer` with the models in `models/` embedded (`default_scorer()`). |
| `lf-train` | Offline `lf-train` binary: clean corpora, train models, generate and run the benchmark. |
| `lf-platform` | The `Backend` trait that OS integrations will implement in later phases. |
| `lf-cli` | `lf-cli` binary for driving the engine from the command line. |

The buffer stores physical keys (W3C key codes + modifiers), not characters.
Converting text means rendering the same keys under the other layout, and
the trigger is matched on key codes, so it fires whatever layout is active.

## CLI

```sh
cargo run -p lf-cli -- simulate --layout us "sghl{trigger}"
# Replace { backspaces: 10, text: "سلام" }
# SetLayout("ir")
# screen: "سلام"   layout: ir

cargo run -p lf-cli -- simulate --layout us "I'm at work nhvl ldhl{trigger}"
# screen: "I'm at work دارم میام"   layout: ir

cargo run -p lf-cli -- score --layout us "I'm at work nhvl ldhl"
# word    as-typed  rendered  LLR
# 0       I'm       ّگپ       -8.3
# ...
# 3       nhvl      دارم      33.8
# 4       ldhl      میام      21.8
# split at word 3 → "I'm at work دارم میام"

cargo run -p lf-cli -- keys --layout ir "سلام"
# KeyS KeyG KeyH KeyL
```

`--scorer ngram` (default) finds where the wrong-layout text starts using the
embedded character models, `--scorer last` converts only the last word, and
`--scorer all` converts everything typed since the last layout switch.

Script syntax: plain characters are typed using the current layout. Tokens:
`{bs}` backspace, `{click}` mouse click, `{chord}` Ctrl shortcut, `{enter}`
submit, `{layout:ir}` switch layout, `{wait:31000}` advance the clock (ms),
`{trigger}` type the trigger keys, and `{{` / `}}` for literal braces.

## Development

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same checks on Linux, Windows and macOS, plus two gates:

```sh
cargo test --release -p lf-lang -- --ignored   # 256 keys score in < 1 ms
cargo run --release -p lf-train -- bench       # ≥ 95% split accuracy, all must cases
```

## Models

`models/en.lfng` and `models/fa.lfng` are built offline from the Leipzig
corpora listed in `models/SOURCES.md` (the corpora are not committed). To
rebuild them, put the corpus files in `data/corpus/{en,fa}/` and follow
`models/SOURCES.md`; then regenerate the benchmark with
`lf-train gen-bench --out data/bench/cases.tsv` and record the run in
`BENCHMARK.md`.
