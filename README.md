# Layout Fixer

Fixes text typed in the wrong keyboard layout. Type `sghl` with US English
active when you meant Persian, then type the trigger `fuckkk`: the text is
replaced with `سلام` and the system switches to Persian. Typing the trigger
again within 10 seconds undoes the fix.

This is Phase 1: the platform-independent core only (spec: `plans/PHASE-1.md`).
There is no OS integration yet.

## Crates

| Crate | What it holds |
| --- | --- |
| `lf-core` | Key types, layouts, the key buffer, trigger matching, the split scorer trait, the correction engine, and a screen simulator (`Harness`) for tests. |
| `lf-layouts` | Built-in layout tables: US English (`us`) and Persian Standard (`ir`, transcribed from XKB `symbols/ir`, section `pes`). |
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

CI runs the same checks on Linux, Windows and macOS.
