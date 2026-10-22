# Installing Layout Fixer

v0.1 supports **Windows 10 and 11**. Linux and macOS builds exist but are
previews: they run without an input backend, so they show the settings
window and tray but do not fix anything yet (Linux X11 is next).

## Windows

Download one of these from the release page:

| File | What it is |
| --- | --- |
| `Layout Fixer_<version>_x64-setup.exe` | NSIS installer. Installs for the current user, no administrator rights needed. Recommended. |
| `Layout Fixer_<version>_x64_en-US.msi` | MSI installer, for managed machines and `msiexec`. Installs for all users (needs administrator rights). |

Both need Microsoft Edge WebView2, which Windows 10 (recent updates) and 11
already have; the installer downloads it if it is missing.

### "Windows protected your PC" (SmartScreen)

v0.1 is **not code-signed**, so Windows does not know the publisher and
SmartScreen warns before running the installer:

1. Click **More info**.
2. Check that the file name is the one you downloaded from the release page.
3. Click **Run anyway**.

Some browsers also flag unsigned downloads ("not commonly downloaded"):
choose **Keep**. Antivirus tools may flag it too, because it watches the
keyboard to notice the trigger; see [What it can see](#what-it-can-see). If
you would rather check before running, compare the file's SHA-256 with the
one on the release page: `Get-FileHash '.\Layout Fixer_0.1.0_x64-setup.exe'`.

### First run

The settings window opens and the icon appears in the tray (it may be
under the **^** overflow arrow; drag it onto the taskbar to keep it
visible). Type in the wrong layout, then the trigger (`fuckkk` by default,
typed on the same physical keys whatever the layout). Type the trigger
again within 10 seconds to undo.

Closing the window keeps Layout Fixer running in the tray; use **Quit** in
the tray menu to stop it. Turn on **Start at login** in the settings to
start it hidden in the tray when you log in.

### Known limitations

- Apps running **as administrator** are out of reach: Windows hides their
  keyboard input from, and blocks synthetic input into them for, normal
  apps. Run Layout Fixer as administrator too if you need it there.
- Store (UWP) apps are identified as `applicationframehost.exe` in the
  blocklist.
- Terminals and password managers are on the blocklist by default.

### Uninstall

Settings → Apps → Installed apps → Layout Fixer → Uninstall. Your settings
stay in `%APPDATA%\Layout Fixer\config\config.toml`; delete that folder
to remove them too.

## Linux (preview)

`.deb`, `.rpm` and `.AppImage` builds can be made with
`npm run tauri build` in `crates/lf-app`. They show a **Preview** banner:
until the X11 backend lands (v0.2), they do not see typing. Settings are
in `~/.config/layout-fixer/config.toml`.

## What it can see

The same summary as the app's **What this app can see** page:

- To notice the trigger, Layout Fixer reads every key you press in other
  apps, like a keyboard shortcut tool.
- The last few hundred key presses are kept **in memory only**, so the text
  before the trigger can be converted. They are cleared when you click,
  press Enter, Tab or Esc, switch apps, pause, or stop typing for a while.
- Typed text is **never saved, logged, or shown**. The settings file holds
  only settings: trigger, layout pair, timings, blocklist, start at login.
- **No network**: the app never connects to the internet; its language
  models are built in.
- The window and tray show only counts, layout names and app names.
- In blocklisted apps, key presses are dropped as they arrive.

## Building the installers

CI builds the Windows installers when a `v*` tag is pushed (see
`.github/workflows/ci.yml`, job `windows-installer`) and attaches them to a
draft GitHub release. To build them by hand on Windows:

```powershell
cd crates\lf-app
npm ci
npm run tauri build -- --bundles msi,nsis
# target\release\bundle\msi\*.msi and target\release\bundle\nsis\*.exe
```

Bundler settings are in `crates/lf-app/tauri.conf.json` (`bundle`); icon
sources are in `packaging/icons/` (regenerate with
`npx tauri icon ../../packaging/icons/app-icon.svg` from `crates/lf-app`,
then delete the mobile icon folders).
