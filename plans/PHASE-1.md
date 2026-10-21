# Layout Fixer — Phase 1: Core Engine

> **For Claude Code:** This file is the full spec for Phase 1. Read it all before writing code.
> Work through the tasks in order. After each task: run `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test`, then commit with the task title as the message. Do not start the next task until the current one is green.
> If something in this spec is ambiguous or contradicts itself, stop and ask instead of guessing.

## 1. What we are building

Layout Fixer is a background app. When the user types in the wrong keyboard layout (e.g. meant Persian, typed with English active) and then types the trigger keys `F U C K K K`, the app deletes the wrong text and the trigger, retypes the text in the other layout, and switches the system layout.

Example: the user meant `سلام` but the US layout was active, so the screen shows `sghl`. They type `fuckkk`. The app sends 10 backspaces, types `سلام`, and switches to Persian.

**Phase 1 builds only the platform-independent core**: key types, layout maps, buffer, trigger matching, the correction engine, a screen simulator for tests, and a CLI harness. No OS APIs. Everything must build and pass tests on Linux, Windows and macOS.

**Core design rule:** the buffer stores *physical keys* (key code + modifiers), never just characters. Converting text to the other layout is "render the same keys under the other layout". Trigger matching is on key codes, so it works whatever layout is active.

## 2. Out of scope for Phase 1

- Any OS integration (hooks, injection, layout switching). Only the `Backend` trait is defined.
- The real split scorer (n-gram models). Phase 1 ships a trait plus a simple `LastWordScorer`.
- Tauri app, tray, settings UI, config files, installers.
- Reading key maps from the OS. Phase 1 uses built-in maps.

## 3. Hard constraints

- Rust stable, edition 2021, Cargo workspace.
- `#![forbid(unsafe_code)]` in every crate.
- Allowed dependencies: `thiserror` (errors), `clap` with `derive` (CLI only). Dev-dependencies: none beyond std unless asked. Do not add others without asking.
- No network code, no file I/O in library crates, no logging of key contents anywhere.
- The engine is deterministic: time is passed in as `now_ms: u64`, never read from the clock inside library code.
- All public items have doc comments.

## 4. Workspace layout

```
layout-fixer/
├── Cargo.toml              # workspace: members = ["crates/*"]
├── PHASE-1.md              # this file
├── .github/workflows/ci.yml
└── crates/
    ├── lf-core/            # types, buffer, trigger, split trait, engine, simulator
    │   └── src/
    │       ├── lib.rs
    │       ├── key.rs
    │       ├── layout.rs
    │       ├── event.rs
    │       ├── buffer.rs
    │       ├── trigger.rs
    │       ├── split.rs
    │       ├── engine.rs
    │       └── sim.rs
    ├── lf-layouts/         # built-in layout tables: US English, Persian Standard
    │   └── src/
    │       ├── lib.rs
    │       ├── us.rs
    │       └── ir_pes.rs
    ├── lf-platform/        # Backend trait only (implementations come in Phase 3+)
    │   └── src/lib.rs
    └── lf-cli/             # binary `lf-cli`: simulate typing through the engine
        └── src/main.rs
```

Dependency direction: `lf-layouts → lf-core`, `lf-platform → lf-core`, `lf-cli → lf-core + lf-layouts`. `lf-core` depends on nothing internal.

## 5. Types and APIs

These signatures are the contract. Small additions (helpers, `Debug`/`Clone`/`PartialEq`/`Eq`/`Hash` derives) are fine; changing the shape is not without asking.

### 5.1 `key.rs`

```rust
/// Physical key, named after W3C UI Events `KeyboardEvent.code` values.
/// Platform backends (later phases) map OS scan codes to these.
pub enum KeyCode {
    KeyA, KeyB, /* … */ KeyZ,
    Digit0, Digit1, /* … */ Digit9,
    Backquote, Minus, Equal, BracketLeft, BracketRight, Backslash,
    Semicolon, Quote, Comma, Period, Slash, Space,
}

impl KeyCode {
    /// Parse a W3C code string, e.g. "KeyF" -> KeyCode::KeyF.
    pub fn from_w3c(s: &str) -> Option<KeyCode>;
    pub fn as_w3c(&self) -> &'static str;
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers { pub shift: bool, pub altgr: bool }

/// One printable key press as seen by the engine.
pub struct KeyPress {
    pub code: KeyCode,
    pub mods: Modifiers,
    /// Layout active when the key was pressed.
    pub layout: LayoutId,
    /// What actually appeared on screen (None = dead key / nothing typed).
    pub produced: Option<char>,
}
```

### 5.2 `layout.rs`

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct LayoutId(pub String);          // e.g. "us", "ir"

pub struct Layout {
    pub id: LayoutId,
    pub name: String,                      // e.g. "English (US)"
    map: HashMap<(KeyCode, Modifiers), char>,
}

impl Layout {
    pub fn new(id: &str, name: &str, entries: &[(KeyCode, Modifiers, char)]) -> Self;
    /// Char this key produces in this layout.
    pub fn char_for(&self, code: KeyCode, mods: Modifiers) -> Option<char>;
    /// Reverse lookup, used by the simulator to "type" a string.
    /// Prefer no-shift over shift when a char appears twice.
    pub fn key_for(&self, ch: char) -> Option<(KeyCode, Modifiers)>;
}

/// Render keys as text under `layout`. If a key has no mapping in `layout`,
/// fall back to the key's original `produced` char; skip keys with neither.
pub fn render(keys: &[KeyPress], layout: &Layout) -> String;
```

### 5.3 `event.rs`

```rust
pub enum ResetReason {
    FocusChanged, MouseClick, Navigation /* arrows, Home, End, PgUp, PgDn */,
    Submit /* Enter, Tab, Esc */, PasswordField, Manual,
}

pub enum InputEvent {
    Key(KeyPress),
    Backspace,
    /// Any key pressed with Ctrl/Alt/Super/Cmd held (copy, paste, undo…).
    Chord,
    Reset(ResetReason),
    /// User (or the system) changed the active layout.
    LayoutChanged(LayoutId),
}
```

### 5.4 `buffer.rs`

```rust
pub const BUFFER_CAPACITY: usize = 256;

/// Ring buffer of recent key presses mirroring what is on screen.
pub struct KeyBuffer { /* VecDeque<KeyPress> */ }

impl KeyBuffer {
    pub fn new() -> Self;
    pub fn push(&mut self, k: KeyPress);       // drops oldest past capacity
    pub fn backspace(&mut self);               // pops last; no-op when empty
    pub fn clear(&mut self);
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn as_slice(&self) -> Vec<&KeyPress>;  // or an iterator; your choice
    /// Remove and return the last `n` entries (oldest first).
    pub fn pop_tail(&mut self, n: usize) -> Vec<KeyPress>;
    pub fn extend(&mut self, keys: impl IntoIterator<Item = KeyPress>);
}
```

On `clear`, overwrite entries before dropping them (simple manual zeroing of the `produced` field is enough; no `zeroize` crate in Phase 1).

### 5.5 `trigger.rs`

```rust
pub struct Trigger { keys: Vec<KeyCode> }

impl Trigger {
    /// Default: [KeyF, KeyU, KeyC, KeyK, KeyK, KeyK].
    pub fn default_trigger() -> Self;
    /// Errors if fewer than 4 keys.
    pub fn new(keys: Vec<KeyCode>) -> Result<Self, TriggerError>;
    pub fn len(&self) -> usize;
    /// True if the buffer ends with the trigger keys, all pressed with no modifiers.
    pub fn matches_tail(&self, buffer: &KeyBuffer) -> bool;
}
```

### 5.6 `split.rs`

```rust
/// One word of the candidate range, rendered both ways.
pub struct WordView {
    pub start: usize,        // index into the candidate key slice (inclusive)
    pub end: usize,          // exclusive
    pub text_from: String,   // as typed (active layout A)
    pub text_to: String,     // rendered in the other layout B
}

pub struct Candidate<'a> {
    pub words: &'a [WordView],
    pub from: &'a Layout,
    pub to: &'a Layout,
}

/// Decides where the wrong-layout suffix starts.
pub trait SplitScorer: Send {
    /// Return the index (into `words`) of the first word to convert,
    /// or None to convert nothing.
    fn first_wrong_word(&self, c: &Candidate) -> Option<usize>;
}

/// Phase 1 scorer: always converts only the last word.
pub struct LastWordScorer;
/// Test helper: converts every word in the candidate range.
pub struct AllWordsScorer;
```

**Word rule:** a word is a maximal run of keys that are not a word break. A word break is `KeyCode::Space` with no modifiers. `Space` with Shift is *not* a break (on Persian Standard it types ZWNJ, which belongs inside a word like `می‌خواهم`).

### 5.7 `engine.rs`

```rust
pub struct Config {
    pub trigger: Trigger,
    pub layout_pair: (LayoutId, LayoutId),   // e.g. ("us", "ir")
    pub undo_window_ms: u64,                 // default 10_000
    pub idle_reset_ms: u64,                  // default 30_000
    pub switch_layout_after_fix: bool,       // default true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Send `backspaces` backspaces, then type `text`.
    Replace { backspaces: usize, text: String },
    SetLayout(LayoutId),
}

pub struct Engine { /* config, layouts, scorer, buffer, current layout, last key time, undo record */ }

impl Engine {
    pub fn new(config: Config, layouts: Vec<Layout>, scorer: Box<dyn SplitScorer>,
               current: LayoutId) -> Result<Self, EngineError>;
    /// Feed one event; returns the commands the platform must execute, in order.
    pub fn handle(&mut self, ev: InputEvent, now_ms: u64) -> Vec<Command>;
    pub fn current_layout(&self) -> &LayoutId;
    pub fn buffer_len(&self) -> usize;           // for tests and debugging
}
```

`EngineError` covers: a layout in the pair is missing from `layouts`; `current` is not in the pair.

## 6. Engine behaviour (the important part)

### 6.1 Every event

1. If `now_ms - last_key_ms > idle_reset_ms` and the buffer is not empty, clear the buffer (and drop the undo record) **before** handling the event.
2. Handle the event:

| Event | Action |
| --- | --- |
| `Key(k)` | Push `k`. Update `last_key_ms`. If the trigger matches the tail, run **6.2 Fix** (or **6.3 Undo**). Any other key clears the undo record. |
| `Backspace` | Pop one entry. Clear the undo record. |
| `Chord` | Clear buffer and undo record. |
| `Reset(_)` | Clear buffer and undo record. |
| `LayoutChanged(id)` | Set current layout to `id`. Keep the buffer (each entry remembers its own layout). Clear the undo record. |

Only `Key` events update `last_key_ms`.

### 6.2 Fix (trigger matched, no live undo record)

1. Pop the trigger's keys off the buffer. `trigger_chars` = number of those keys with `produced.is_some()`.
2. **Candidate range** = the trailing run of buffer entries whose `layout` equals the current layout. (Text typed before a deliberate layout switch is assumed correct.) If the range is empty → return `[]` and leave the trigger keys on screen (do not push them back into the buffer).
3. A = current layout, B = the other layout of the pair.
4. Split the range into words (rule in 5.6) and build `WordView`s with `render` under A and B.
5. Ask the scorer. `None`, or no words at all → return `[]`.
6. Suffix = keys from `words[j].start` to the end of the range (includes spaces between and after words).
7. `suffix_chars` = number of suffix keys with `produced.is_some()`.
8. Emit `Replace { backspaces: suffix_chars + trigger_chars, text: render(suffix, B) }`.
9. If `switch_layout_after_fix`, emit `SetLayout(B)` and set current layout to B.
10. Update the buffer to reflect the screen: remove the suffix, then push the suffix keys again with `layout = B` and `produced = B.char_for(...)` (falling back to the old produced char).
11. Store an undo record: `{ original_keys: suffix (as before step 10), from: A, to: B, replaced_len: suffix len, at_ms: now_ms }`.

### 6.3 Undo (trigger matched, undo record exists and `now_ms - at_ms <= undo_window_ms`)

1. Pop the trigger keys; count `trigger_chars` as in 6.2.
2. Emit `Replace { backspaces: <chars of the replaced text> + trigger_chars, text: render(original_keys, A) }`.
3. If `switch_layout_after_fix`, emit `SetLayout(A)` and set current layout to A.
4. Restore the buffer: remove the converted keys, push `original_keys` back unchanged.
5. Drop the undo record. (A second trigger right after an undo is a normal fix.)

If the undo record is older than the window, drop it and do a normal **6.2 Fix**.

### 6.4 Known limitation (document in code, don't solve)

Backspace counts assume one backspace deletes one produced char. Some apps delete a base letter plus combining marks together; Phase 1 ignores this.

## 7. Built-in layouts (`lf-layouts`)

Expose:

```rust
pub fn us() -> Layout;        // id "us", "English (US)"
pub fn ir_pes() -> Layout;    // id "ir", "Persian (Standard)"
pub fn all() -> Vec<Layout>;
```

### 7.1 US English

Standard US QWERTY, unshifted and shifted, for every `KeyCode` in 5.1. Space → `' '` (both shift states).

### 7.2 Persian Standard

**Authoritative unshifted letters** (tests depend on these):

| Key | Char | Key | Char | Key | Char |
| --- | --- | --- | --- | --- | --- |
| q | ض | a | ش | z | ظ |
| w | ص | s | س | x | ط |
| e | ث | d | ی | c | ز |
| r | ق | f | ب | v | ر |
| t | ف | g | ل | b | ذ |
| y | غ | h | ا | n | د |
| u | ع | j | ت | m | پ |
| i | ه | k | ن | , | و |
| o | خ | l | م | | |
| p | ح | ; | ک | | |
| [ | ج | ' | گ | | |
| ] | چ | | | | |

`Space` unshifted → `' '`. `Space` + Shift → ZWNJ `'\u{200C}'`. Digits unshifted → Persian digits `۱۲۳۴۵۶۷۸۹۰`.

**Everything else** (the rest of the number row, `Backquote`, `Minus`, `Equal`, `Backslash`, `Period`, `Slash`, and the whole Shift layer) must be taken from the XKB source on this machine: `/usr/share/X11/xkb/symbols/ir`, the `pes` section (default Persian). Read that file, transcribe the mapping, and put a comment at the top of `ir_pes.rs` naming the source file and section. If the file is missing, stop and ask. Do not guess these from memory.

Skip AltGr (third/fourth level) entries in Phase 1.

## 8. Simulator (`lf-core::sim`)

A tiny model of a text field, used by tests and the CLI.

```rust
pub struct ScreenSim { pub text: String, pub layout: LayoutId }

impl ScreenSim {
    /// Apply engine commands: remove `backspaces` chars from the end, append `text`,
    /// switch layout on SetLayout.
    pub fn apply(&mut self, cmds: &[Command]);
}

/// Drives an Engine + ScreenSim from a script.
pub struct Harness { pub engine: Engine, pub screen: ScreenSim, pub now_ms: u64, layouts: Vec<Layout> }

impl Harness {
    pub fn new(engine: Engine, layouts: Vec<Layout>) -> Self;
    /// Run a script (syntax below). Each typed char becomes a Key event
    /// using the CURRENT layout's reverse map, is appended to the screen as typed,
    /// then fed to the engine; resulting commands are applied to the screen.
    /// Advances now_ms by 50 per event.
    pub fn run(&mut self, script: &str) -> Result<(), SimError>;
}
```

**Script syntax**

- Plain characters are typed as-is using the current layout's reverse map (`key_for`). A char not in the current layout → `SimError`.
- `{bs}` backspace · `{click}` mouse-click reset · `{chord}` Ctrl chord · `{enter}` submit reset
- `{layout:ir}` user switches layout (emits `LayoutChanged`, updates screen layout)
- `{wait:31000}` advance time by N ms
- `{trigger}` types the trigger keys (`F U C K K K`) in the current layout
- `{{` and `}}` type literal braces

## 9. CLI harness (`lf-cli`)

```
lf-cli simulate [--layout us|ir] [--scorer last|all] "<script>"
```

Prints each command the engine emits, then the final screen text and layout. Example:

```
$ lf-cli simulate --layout us "sghl{trigger}"
Replace { backspaces: 10, text: "سلام" }
SetLayout("ir")
screen: "سلام"   layout: ir
```

Also: `lf-cli keys --layout ir "سلام"` prints the W3C key codes for a string (debugging aid).

## 10. Tasks

Do these in order. Each ends green and committed.

1. **Workspace scaffold.** Create the workspace, the four crates, `#![forbid(unsafe_code)]`, empty modules, and `.github/workflows/ci.yml` (ubuntu-latest, windows-latest, macos-latest; steps: fmt check, clippy `-D warnings`, test).
2. **Key and layout types.** `key.rs`, `layout.rs` with `render`, `key_for`, W3C parsing. Unit tests for round-trips.
3. **US layout.** `lf-layouts::us()` with tests: every `KeyCode` has unshifted and shifted entries; `key_for('A')` is `(KeyA, shift)`.
4. **Persian layout.** `lf-layouts::ir_pes()` from 7.2, including reading the XKB file. Tests: the letter table above, ZWNJ, digits, and that `render(keys of "sghl" in us, ir) == "سلام"`.
5. **Events and buffer.** `event.rs`, `buffer.rs` with tests (push, backspace on empty, capacity 256 drops oldest, `pop_tail`, clear).
6. **Trigger.** `trigger.rs` with tests (see 11).
7. **Split trait.** `split.rs`, word splitting, `LastWordScorer`, `AllWordsScorer`, tests for the word rule including Shift+Space.
8. **Engine: fix.** `engine.rs` sections 6.1 and 6.2.
9. **Engine: undo.** Section 6.3.
10. **Simulator.** `sim.rs` with script parser.
11. **Acceptance tests.** `crates/lf-core/tests/acceptance.rs` with every case in section 11, using `Harness`.
12. **CLI.** `lf-cli` with `simulate` and `keys`.
13. **Backend trait.** `lf-platform` with the trait below and a doc comment explaining that implementations arrive in later phases.

```rust
pub trait Backend {
    fn start(&mut self, tx: std::sync::mpsc::Sender<(lf_core::InputEvent, u64)>) -> Result<(), BackendError>;
    fn execute(&mut self, cmds: &[lf_core::Command]) -> Result<(), BackendError>;
    fn current_layout(&self) -> Result<lf_core::LayoutId, BackendError>;
}
```

## 11. Acceptance tests

All use `Harness` with layouts `us` + `ir`, pair `("us", "ir")`, default config. Unless noted, scorer is `LastWordScorer` and the start layout is `us`.

| # | Start | Script | Expected screen | Expected layout | Notes |
| --- | --- | --- | --- | --- | --- |
| 1 | us | `sghl{trigger}` | `سلام` | ir | Commands exactly: `Replace{10,"سلام"}`, `SetLayout("ir")` |
| 2 | us | `hello sghl{trigger}` | `hello سلام` | ir | Last word only |
| 3 | us, AllWords | `sghl n,sj lk{trigger}` | `سلام دوست من` | ir | Whole range |
| 4 | ir | `اثممخ{trigger}` | `hello` | us | Trigger typed on Persian keys (`بعزننن`) still fires |
| 5 | us | `fuck` | `fuck` | us | 4-key word is not the trigger; no commands |
| 6 | us | `sghx{bs}l{trigger}` | `سلام` | ir | Backspace keeps buffer in sync |
| 7 | us | `sghl{click}{trigger}` | `sghlfuckkk` | us | Buffer empty after reset → no commands |
| 8 | us | `sghl{chord}{trigger}` | `sghlfuckkk` | us | Chord resets → no commands |
| 9 | us | `sghl{wait:31000}{trigger}` | `sghlfuckkk` | us | Idle reset → no commands |
| 10 | us | `sghl{trigger}{trigger}` | `sghl` | us | Second trigger within 10 s = undo |
| 11 | us | `sghl{trigger}{wait:11000}{trigger}` | `sghl` | us | Undo window passed → normal fix converts `سلام` back (same visible result, different path; assert undo record was not used) |
| 12 | us | `sghl{trigger}x{trigger}` | `sghlx` | us | `x` is typed in ir as `ط`, which clears the undo record; the second trigger is a normal fix of `سلامط` |
| 13 | us | `hello {layout:ir}اثممخ{trigger}` | `hello hello` | us | Candidate range stops at the layout switch |
| 14 | us | `ldo{{}}` | `ldo{}` | us | Literal braces in scripts |
| 15 | us, AllWords | 300 × `a` then `{trigger}` | 50 × `a` then 250 × `ش` | ir | Buffer holds 256 keys; the 6 trigger keys push out 6 more `a`s, so only 250 are converted |

The harness also applies `{bs}` to the screen (removes the last char) and appends trigger chars to the screen as they are typed.

Also unit-test in `trigger.rs`:
- matches `F U C K K K` with layout us and with layout ir
- does not match `F U C K K` (too short) or `F U C K K K` with Shift on any key
- matches when preceded by other text (`hi fuckkk`)
- `Trigger::new` with 3 keys → error

## 12. Definition of done

- All 13 tasks committed; CI green on all three OSes.
- `cargo test` runs every unit test plus all 15 acceptance cases.
- `lf-cli simulate --layout us "sghl n,sj lk{trigger}" --scorer all` prints `سلام دوست من`.
- No `unsafe`, no extra dependencies, no clippy warnings.
- A short `README.md` explaining the crates and how to run the CLI.
