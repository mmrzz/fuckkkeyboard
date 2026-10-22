# Layout Fixer v0.1.0

First release. Type in the wrong keyboard layout, then type the trigger
(`fuckkk` by default, on the same physical keys in any layout): the text is
replaced with what you meant, and the layout is switched. Type the trigger
again within 10 seconds to undo.

## Supported

- **Windows 10 and 11** (x64): tray app with settings window, installers
  below.

## Coming

- Linux X11 (v0.2), Wayland (v0.3), macOS (v1.0). The Linux and macOS
  builds of this version run as a preview without an input backend.

## Features

- Converts between any two installed layouts; English and Persian are
  scored with built-in language models to find where the wrong-layout text
  starts.
- Settings: trigger (record it by pressing keys), layout pair, undo window,
  idle reset, switch layout after a fix, blocklist, start at login.
- Pause/resume from the tray.
- Privacy: typed text stays in memory, is never saved, logged or sent; no
  network access. See "What this app can see" in the app.

## Install

The installers are **not code-signed**, so Windows SmartScreen warns on
first run: click **More info → Run anyway**. Details and checksums: see
`packaging/INSTALL.md`.

- `Layout Fixer_0.1.0_x64-setup.exe`: per-user installer (recommended).
- `Layout Fixer_0.1.0_x64_en-US.msi`: all-users MSI.

## Known limitations

- Apps running as administrator are out of reach unless Layout Fixer also
  runs as administrator.
- No auto-update yet: download new versions from the releases page.
