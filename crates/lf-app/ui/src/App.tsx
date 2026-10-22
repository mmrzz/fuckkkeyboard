import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  Config,
  LayoutView,
  MIN_TRIGGER_KEYS,
  Status,
  TRIGGER_KEYS,
  TriggerView,
} from "./api";

type Page = "general" | "trigger" | "blocklist" | "privacy";

const PAGES: [Page, string][] = [
  ["general", "General"],
  ["trigger", "Trigger"],
  ["blocklist", "Blocklist"],
  ["privacy", "What this app can see"],
];

const STATUS_POLL_MS = 1000;

export default function App() {
  const [page, setPage] = useState<Page>("general");
  const [status, setStatus] = useState<Status | null>(null);
  const [config, setConfig] = useState<Config | null>(null);
  const [layouts, setLayouts] = useState<LayoutView[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.status());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
    api.getConfig().then(setConfig, (e) => setError(String(e)));
    api.layouts().then(setLayouts, (e) => setError(String(e)));
    const timer = window.setInterval(refreshStatus, STATUS_POLL_MS);
    return () => window.clearInterval(timer);
  }, [refreshStatus]);

  // The tray's "What this app can see" item opens that page.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<string>("show-page", (e) => {
      if (PAGES.some(([p]) => p === e.payload)) setPage(e.payload as Page);
    })
      .then((f) => (unlisten = f))
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  const update = async (patch: Partial<Config>) => {
    if (!config) return;
    const next = { ...config, ...patch };
    try {
      await api.setConfig(next);
      setConfig(next);
      setError(null);
      void refreshStatus();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="app">
      {status?.preview && (
        <div className="banner preview" role="status">
          <strong>Preview.</strong> This system has no input backend yet, so Layout Fixer
          sees no typing here. Everything below works and is saved, but fixes only happen
          on Windows for now.
        </div>
      )}
      {status?.notice && (
        <div className="banner notice" role="alert">
          {status.notice}{" "}
          <button className="link" onClick={() => api.dismissNotice().then(refreshStatus)}>
            Dismiss
          </button>
        </div>
      )}
      {error && (
        <div className="banner error" role="alert">
          {error}
        </div>
      )}
      <nav className="tabs">
        {PAGES.map(([p, label]) => (
          <button
            key={p}
            className={p === page ? "tab active" : "tab"}
            aria-current={p === page ? "page" : undefined}
            onClick={() => setPage(p)}
          >
            {label}
          </button>
        ))}
      </nav>
      <main>
        {page === "general" && (
          <General status={status} config={config} layouts={layouts} update={update} refresh={refreshStatus} />
        )}
        {page === "trigger" && config && <TriggerPage config={config} update={update} />}
        {page === "blocklist" && config && <Blocklist config={config} update={update} />}
        {page === "privacy" && <Privacy />}
      </main>
    </div>
  );
}

type Update = (patch: Partial<Config>) => Promise<void>;

function stateLabel(s: Status): string {
  if (!s.running) return "Not running";
  if (s.paused) return "Paused";
  if (s.focused_app_blocked) return "Off in the current app (blocklist)";
  return "On";
}

function General(props: {
  status: Status | null;
  config: Config | null;
  layouts: LayoutView[];
  update: Update;
  refresh: () => Promise<void>;
}) {
  const { status, config, layouts, update, refresh } = props;
  if (!status || !config) return <p>Loading…</p>;

  const togglePause = async () => {
    await (status.paused ? api.resume() : api.pause());
    // The runtime applies the change on its own thread; re-read shortly.
    window.setTimeout(refresh, 100);
  };

  const pair = config.layout_pair ?? status.layout_pair ?? ["", ""];
  const auto = config.layout_pair === null;
  const label = (l: LayoutView) => `${l.name} (${l.id}${l.language ? `, ${l.language}` : ""})`;
  const setPair = (i: 0 | 1, id: string) => {
    const next: [string, string] = [pair[0], pair[1]];
    next[i] = id;
    void update({ layout_pair: next });
  };

  return (
    <>
      <section className="card">
        <h2>Status</h2>
        <p className="state">
          <span className={status.paused || !status.running ? "dot off" : "dot on"} />
          {stateLabel(status)}
        </p>
        <p className="muted">
          Active layout: {status.current_layout ?? "unknown"} · Fixes: {status.fixes} · Undos:{" "}
          {status.undos}
        </p>
        {status.last_error && <p className="error-text">Last problem: {status.last_error}</p>}
        <button onClick={togglePause} disabled={!status.running}>
          {status.paused ? "Resume" : "Pause"}
        </button>
      </section>

      <section className="card">
        <h2>Layout pair</h2>
        <p className="muted">Text typed in one of these layouts is converted to the other.</p>
        <label className="check">
          <input
            type="checkbox"
            checked={auto}
            onChange={(e) =>
              void update({ layout_pair: e.target.checked ? null : status.layout_pair ?? null })
            }
          />
          Choose automatically (the first two installed languages)
        </label>
        <div className="row">
          <label>
            First layout
            <select value={pair[0]} disabled={auto} onChange={(e) => setPair(0, e.target.value)}>
              {layouts.map((l) => (
                <option key={l.id} value={l.id}>
                  {label(l)}
                </option>
              ))}
            </select>
          </label>
          <label>
            Second layout
            <select value={pair[1]} disabled={auto} onChange={(e) => setPair(1, e.target.value)}>
              {layouts.map((l) => (
                <option key={l.id} value={l.id}>
                  {label(l)}
                </option>
              ))}
            </select>
          </label>
        </div>
      </section>

      <section className="card">
        <h2>Behaviour</h2>
        <SecondsField
          label="Undo window"
          help="Typing the trigger again within this time puts the original text back."
          ms={config.undo_window_ms}
          min={0}
          max={60}
          onCommit={(ms) => update({ undo_window_ms: ms })}
        />
        <SecondsField
          label="Forget typing after"
          help="After this long without a key press, earlier typing is not converted."
          ms={config.idle_reset_ms}
          min={1}
          max={600}
          onCommit={(ms) => update({ idle_reset_ms: ms })}
        />
        <label className="check">
          <input
            type="checkbox"
            checked={config.switch_layout_after_fix}
            onChange={(e) => void update({ switch_layout_after_fix: e.target.checked })}
          />
          Switch the keyboard layout after a fix
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={config.start_at_login}
            onChange={(e) => void update({ start_at_login: e.target.checked })}
          />
          Start at login (hidden in the tray)
        </label>
      </section>
    </>
  );
}

function SecondsField(props: {
  label: string;
  help: string;
  ms: number;
  min: number;
  max: number;
  onCommit: (ms: number) => Promise<void>;
}) {
  const [text, setText] = useState(String(props.ms / 1000));
  useEffect(() => setText(String(props.ms / 1000)), [props.ms]);
  const commit = () => {
    const s = Number(text);
    if (Number.isFinite(s) && s >= props.min && s <= props.max) {
      if (Math.round(s * 1000) !== props.ms) void props.onCommit(Math.round(s * 1000));
    } else {
      setText(String(props.ms / 1000));
    }
  };
  return (
    <label className="field">
      {props.label}
      <span>
        <input
          type="number"
          min={props.min}
          max={props.max}
          step="any"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && commit()}
        />{" "}
        seconds
      </span>
      <small className="muted">{props.help}</small>
    </label>
  );
}

function TriggerPage({ config, update }: { config: Config; update: Update }) {
  const [recording, setRecording] = useState(false);
  const [keys, setKeys] = useState<string[]>(config.trigger);
  const [preview, setPreview] = useState<TriggerView[]>([]);
  const recorder = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.triggerPreview(keys).then(setPreview, () => setPreview([]));
  }, [keys]);

  // Only keys pressed in this box while recording are read, to build the
  // trigger. They are never stored except as the trigger itself.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!recording) return;
    e.preventDefault();
    if (e.key === "Enter" || e.key === "Escape") {
      setRecording(false);
    } else if (e.code === "Backspace") {
      setKeys((k) => k.slice(0, -1));
    } else if (TRIGGER_KEYS.has(e.code) && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
      setKeys((k) => (k.length < 16 ? [...k, e.code] : k));
    }
  };

  const changed = keys.join(" ") !== config.trigger.join(" ");
  const valid = keys.length >= MIN_TRIGGER_KEYS;

  return (
    <>
      <section className="card">
        <h2>Trigger</h2>
        <p className="muted">
          The trigger is a sequence of physical keys. Type it right after text in the wrong layout to
          convert it; type it again soon after to undo. It works whichever layout is active.
        </p>
        <div
          ref={recorder}
          className={recording ? "recorder recording" : "recorder"}
          tabIndex={0}
          role="textbox"
          aria-label="Trigger keys"
          onKeyDown={onKeyDown}
          onBlur={() => setRecording(false)}
        >
          {keys.length === 0 && <span className="muted">Press keys…</span>}
          {keys.map((k, i) => (
            <kbd key={i}>{k.replace(/^Key|^Digit/, "")}</kbd>
          ))}
        </div>
        <div className="row">
          {!recording ? (
            <button
              onClick={() => {
                setKeys([]);
                setRecording(true);
                recorder.current?.focus();
              }}
            >
              Record new trigger
            </button>
          ) : (
            <button onClick={() => setRecording(false)}>Done</button>
          )}
          <button disabled={!changed || !valid} onClick={() => void update({ trigger: keys })}>
            Use this trigger
          </button>
          <button disabled={!changed} onClick={() => setKeys(config.trigger)}>
            Cancel
          </button>
        </div>
        {recording && (
          <p className="muted">
            Press at least {MIN_TRIGGER_KEYS} keys without modifiers. Backspace removes the last
            one; Enter finishes.
          </p>
        )}
        {!valid && !recording && (
          <p className="error-text">A trigger needs at least {MIN_TRIGGER_KEYS} keys.</p>
        )}
        <h3>How it looks in each layout</h3>
        <table className="preview">
          <tbody>
            {preview.map((p) => (
              <tr key={p.id}>
                <td>{p.name}</td>
                <td dir="auto">
                  <code>{p.text}</code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="card">
        <h2>Try it</h2>
        <p className="muted">
          Type here in the wrong layout, then the trigger. Nothing typed here leaves this box or is
          saved.
        </p>
        <textarea className="testbox" rows={4} placeholder="e.g. sghl followed by the trigger" />
      </section>
    </>
  );
}

function Blocklist({ config, update }: { config: Config; update: Update }) {
  const [entry, setEntry] = useState("");
  const [recent, setRecent] = useState<string[]>([]);

  useEffect(() => {
    api.recentApps().then(setRecent, () => setRecent([]));
  }, []);

  const add = (app: string) => {
    const a = app.trim().toLowerCase();
    if (a && !config.blocklist.includes(a)) void update({ blocklist: [...config.blocklist, a] });
    setEntry("");
  };
  const remove = (app: string) =>
    void update({ blocklist: config.blocklist.filter((b) => b !== app) });

  const candidates = recent.filter((r) => !config.blocklist.includes(r.toLowerCase()));

  return (
    <>
      <section className="card">
        <h2>Blocklist</h2>
        <p className="muted">
          Layout Fixer ignores typing in these apps. Password managers and terminals are blocked by
          default.
        </p>
        <ul className="blocklist">
          {config.blocklist.map((b) => (
            <li key={b}>
              <code>{b}</code>
              <button className="link" aria-label={`Remove ${b}`} onClick={() => remove(b)}>
                Remove
              </button>
            </li>
          ))}
        </ul>
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            add(entry);
          }}
        >
          <input
            value={entry}
            placeholder="app id, e.g. notepad.exe"
            aria-label="App to block"
            onChange={(e) => setEntry(e.target.value)}
          />
          <button type="submit" disabled={!entry.trim()}>
            Add
          </button>
        </form>
      </section>
      <section className="card">
        <h2>Recently used apps</h2>
        {candidates.length === 0 && <p className="muted">No other apps seen yet.</p>}
        <ul className="blocklist">
          {candidates.map((r) => (
            <li key={r}>
              <code>{r}</code>
              <button className="link" onClick={() => add(r)}>
                Block
              </button>
            </li>
          ))}
        </ul>
      </section>
    </>
  );
}

export function Privacy() {
  return (
    <section className="card prose">
      <h2>What this app can see</h2>
      <p>
        To notice the trigger, Layout Fixer reads every key you press in other apps, the way a
        keyboard shortcut tool does. Here is exactly what happens with it.
      </p>
      <ul>
        <li>
          <strong>Keys stay in memory, briefly.</strong> The last few hundred key presses are kept
          in RAM so the text before the trigger can be converted. They are cleared when you click,
          press Enter, Tab or Esc, switch apps, pause, or stop typing for a while.
        </li>
        <li>
          <strong>Nothing is written down.</strong> Typed text is never saved to disk, logged, or
          shown in this window. The settings file holds only your settings: the trigger, the layout
          pair, timings and the blocklist.
        </li>
        <li>
          <strong>No network.</strong> Layout Fixer never connects to the internet. The language
          models it uses to decide what to convert are built into the app.
        </li>
        <li>
          <strong>What it shows.</strong> This window and the tray only show counts (fixes, undos),
          layout names and app names, never what you typed.
        </li>
        <li>
          <strong>Blocked apps.</strong> In apps on the blocklist, key presses are dropped as soon
          as they arrive. Pause from the tray to stop everywhere.
        </li>
        <li>
          <strong>What it changes.</strong> On a fix it sends backspaces and the converted text to
          the app you are typing in, and switches the keyboard layout if you asked it to.
        </li>
      </ul>
      <p className="muted">
        On Windows, apps running as administrator are out of reach unless Layout Fixer itself runs
        as administrator.
      </p>
    </section>
  );
}
