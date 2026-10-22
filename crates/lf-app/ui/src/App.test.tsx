// The settings window against a mock of the app's commands (which, in the
// real app, sit on the runtime's MockBackend on Linux and macOS).
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import App from "./App";
import type { Config, Status } from "./api";

let calls: [string, unknown][] = [];
let status: Status;
let config: Config;

beforeEach(() => {
  calls = [];
  status = {
    running: true,
    paused: false,
    current_layout: "us",
    layout_pair: ["us", "ir"],
    focused_app_blocked: false,
    fixes: 3,
    undos: 1,
    last_error: null,
    preview: true,
    notice: null,
  };
  config = {
    trigger: ["KeyF", "KeyU", "KeyC", "KeyK", "KeyK", "KeyK"],
    layout_pair: null,
    undo_window_ms: 10000,
    idle_reset_ms: 30000,
    switch_layout_after_fix: true,
    blocklist: ["keepassxc"],
    start_at_login: false,
  };
  mockIPC((cmd, args) => {
    calls.push([cmd, args]);
    switch (cmd) {
      case "status":
        return status;
      case "get_config":
        return config;
      case "layouts":
        return [
          { id: "us", name: "English (US)", language: "en" },
          { id: "ir", name: "Persian", language: "fa" },
          { id: "de", name: "German", language: "de" },
        ];
      case "recent_apps":
        return ["notepad.exe"];
      case "trigger_preview":
        return [];
      case "pause":
        status = { ...status, paused: true };
        return null;
      case "resume":
        status = { ...status, paused: false };
        return null;
      case "set_config":
        config = (args as { config: Config }).config;
        return null;
      default:
        return null;
    }
  });
});

afterEach(() => {
  cleanup();
  clearMocks();
});

const called = (cmd: string) => calls.filter(([c]) => c === cmd);

describe("settings window", () => {
  it("shows the preview banner and counts, never text", async () => {
    render(<App />);
    expect(await screen.findByText(/Preview\./)).toBeTruthy();
    expect(await screen.findByText(/Fixes: 3/)).toBeTruthy();
  });

  it("pausing and resuming call the right commands", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Pause" }));
    await waitFor(() => expect(called("pause")).toHaveLength(1));
    fireEvent.click(await screen.findByRole("button", { name: "Resume" }));
    await waitFor(() => expect(called("resume")).toHaveLength(1));
  });

  it("changing the pair calls set_config with the new pair", async () => {
    render(<App />);
    fireEvent.click(await screen.findByLabelText(/Choose automatically/));
    await waitFor(() => expect(called("set_config")).toHaveLength(1));
    expect(config.layout_pair).toEqual(["us", "ir"]);

    fireEvent.change(await screen.findByLabelText("Second layout"), { target: { value: "de" } });
    await waitFor(() => expect(called("set_config")).toHaveLength(2));
    expect(config.layout_pair).toEqual(["us", "de"]);
    expect(config.trigger).toEqual(["KeyF", "KeyU", "KeyC", "KeyK", "KeyK", "KeyK"]);
  });

  it("adds a recent app to the blocklist", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Blocklist" }));
    fireEvent.click(await screen.findByRole("button", { name: "Block" }));
    await waitFor(() => expect(config.blocklist).toEqual(["keepassxc", "notepad.exe"]));
  });

  it("has the what-it-can-see page", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "What this app can see" }));
    expect(await screen.findByText(/No network\./)).toBeTruthy();
  });
});
