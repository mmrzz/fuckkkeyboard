# Manual tests

Checks no CI machine can run: real keyboards, real apps, the real tray.
Run the section for a platform before tagging a release that adds or
changes it, and note the result (date, OS build, app versions) in the
release PR.

## Windows (v0.1)

Setup: install the build under test (NSIS `.exe`), with **English (US)** and
**Persian (Standard)** keyboards installed (Settings → Time & language →
Language & region). Default settings unless a step says otherwise. "Type
`sghl` + trigger" means: switch to English, type `sghl`, then type the
trigger; expect `سلام` and the layout switched to Persian. Then type the
trigger again within 10 s; expect `sghl` back and English active.

| # | Check | Expected | Result |
| --- | --- | --- | --- |
| W1 | Install; SmartScreen **More info → Run anyway** | Installs; settings window opens; tray icon appears | |
| W2 | Browser (Edge or Chrome): address bar and a page text box: `sghl` + trigger, then undo | Fixed, then undone, in both fields | |
| W3 | Electron app (VS Code editor, Telegram Desktop message box): `sghl` + trigger | Fixed; layout switched | |
| W4 | Office app (Word or Outlook body): `sghl` + trigger | Fixed; layout switched | |
| W5 | Native editor (Notepad): `sghl` + trigger; also a mixed line `I'm at work nhvl ldhl` + trigger | `سلام`; mixed line becomes `I'm at work دارم میام` | |
| W6 | Persian typed by mistake: switch to Persian, press the keys `h e l l o` (shows `اثممخ`) + trigger | `hello`; layout switched to English | |
| W7 | Terminal (Windows Terminal or `cmd.exe`): `sghl` + trigger | Nothing happens (blocked by default); tray tooltip says "off in this app" | |
| W8 | Tray → **Pause**; `sghl` + trigger in Notepad | Icon turns grey; nothing is fixed. **Resume**: icon back; fixing works again | |
| W9 | Settings → Trigger → record `jjjj` → **Use this trigger**; `sghl` + `jjjj` in Notepad | Fixed with the new trigger; the old one does nothing | |
| W10 | Settings → choose a different pair (add a third layout first, e.g. German); fix between them | Fixes convert between the chosen pair | |
| W11 | Blocklist: add `notepad.exe` from **Recently used apps**; try a fix in Notepad; remove it | Blocked while listed; works again after removal | |
| W12 | Quit from the tray and start again | Trigger, pair, behaviour and blocklist are as set before | |
| W13 | Turn on **Start at login**; sign out and in | Starts hidden in the tray (no window); fixing works | |
| W14 | Launch the app a second time while it runs | No second tray icon; the settings window of the running one comes forward | |
| W15 | Click in the middle of a word, then type + trigger | Only text typed after the click is converted | |
| W16 | Tray → **What this app can see** | Window opens on that page | |
| W17 | An app running as administrator (e.g. an elevated Notepad) | Nothing happens there (documented limitation); no crash | |
| W18 | Corrupt `%APPDATA%\Layout Fixer\config\config.toml` (type garbage), start the app | Notice about the reset; `config.toml.broken` kept; defaults in use | |
