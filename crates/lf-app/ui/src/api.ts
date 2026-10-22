// Typed wrappers over the app's Tauri commands. The settings window talks to
// the runtime only through these; it never sees keystrokes.
import { invoke } from "@tauri-apps/api/core";

export interface Status {
  running: boolean;
  paused: boolean;
  current_layout: string | null;
  layout_pair: [string, string] | null;
  focused_app_blocked: boolean;
  fixes: number;
  undos: number;
  last_error: string | null;
  preview: boolean;
  notice: string | null;
}

export interface Config {
  trigger: string[];
  layout_pair: [string, string] | null;
  undo_window_ms: number;
  idle_reset_ms: number;
  switch_layout_after_fix: boolean;
  blocklist: string[];
  start_at_login: boolean;
}

export interface LayoutView {
  id: string;
  name: string;
  language: string | null;
}

export interface TriggerView {
  id: string;
  name: string;
  text: string;
}

export const api = {
  status: () => invoke<Status>("status"),
  getConfig: () => invoke<Config>("get_config"),
  setConfig: (config: Config) => invoke<void>("set_config", { config }),
  pause: () => invoke<void>("pause"),
  resume: () => invoke<void>("resume"),
  layouts: () => invoke<LayoutView[]>("layouts"),
  recentApps: () => invoke<string[]>("recent_apps"),
  triggerPreview: (keys: string[]) => invoke<TriggerView[]>("trigger_preview", { keys }),
  dismissNotice: () => invoke<void>("dismiss_notice"),
};

/// Physical keys a trigger may use (W3C `KeyboardEvent.code` values), the
/// same set as lf_core::KeyCode.
export const TRIGGER_KEYS: ReadonlySet<string> = new Set([
  ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("").map((c) => `Key${c}`),
  ..."0123456789".split("").map((d) => `Digit${d}`),
  "Backquote",
  "Minus",
  "Equal",
  "BracketLeft",
  "BracketRight",
  "Backslash",
  "Semicolon",
  "Quote",
  "Comma",
  "Period",
  "Slash",
  "Space",
]);

export const MIN_TRIGGER_KEYS = 4;
