## X11 (Phase 4, v0.2)

Run on a real **Xorg/X11 login** (not Wayland, not a nested server). Record
pass/fail per row, with the date, distro, desktop and app version.

### Setup

1. Install two layouts, e.g. English (US) and Persian, in the desktop's
   keyboard settings (or `setxkbmap -layout us,ir`).
2. `lf-daemon --print-layouts` lists both, with languages `en` and `fa`.
3. Start the tray app (or `lf-daemon --log info` for a headless run).

### Checklist

In each app: with the US layout active, type `sghl`, then the trigger
(`fuckkk` by default). Expect `سلام` and the layout indicator on Persian.
Type the trigger again within 10 s: expect `sghl` back and the layout on US.

| # | Where | Expect | Pass/Fail | Notes |
|---|-------|--------|-----------|-------|
| 1 | Browser text box (Firefox or Chromium) | fix, then undo | | |
| 2 | VS Code editor | fix, then undo | | |
| 3 | Terminal (e.g. GNOME Terminal, Konsole, xterm) | nothing happens: terminals are blocked by default | | |
| 4 | Native editor (gedit, Kate or Mousepad) | fix, then undo | | |
| 5 | Across a layout switch: type `hello ` on US, switch to Persian with the desktop shortcut, type `سلام` correctly, switch back to US, type `sghl` + trigger | only `sghl` is converted; the Persian word is untouched | | |
| 6 | Reverse: with Persian active, type the keys of `hello` (shows as `اثممخ`) + trigger | `hello`, layout back on US | | |
| 7 | Click elsewhere in the text after typing, then the trigger | nothing is replaced (the click resets) | | |
| 8 | Ctrl+A / Ctrl+V then the trigger | nothing is replaced (the shortcut resets) | | |
| 9 | Desktop layout indicator after a fix (KDE: follows at once; GNOME: may lag, XKB-only switch) | indicator shows the new layout | | |
| 10 | Status in the tray: fix/undo counts go up; logs show no typed text | counts only | | |

### Wayland

| # | Where | Expect | Pass/Fail | Notes |
|---|-------|--------|-----------|-------|
| 11 | Start the app in a Wayland session | preview banner ("Wayland isn't supported yet…"), no key capture, no crash | | |
| 12 | `lf-daemon` in a Wayland session | prints the Wayland message, exits with code 2 | | |
