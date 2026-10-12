import { useEffect, useState } from "react";
import { api, type ModelInfo, type Settings, type Status } from "./api";

type Toggle = { key: keyof Settings["sources"]; label: string; note: string; disabled?: boolean };

const TOGGLES: Toggle[] = [
  { key: "app", label: "Foreground app", note: "The name of the app in front, and when you are away." },
  { key: "window_titles", label: "Window titles", note: "Titles can show email subjects and chat names. Not available yet.", disabled: true },
  { key: "fs", label: "File changes", note: "Which files change in your repositories. Never their contents." },
  { key: "git", label: "Git", note: "Your commits on every branch, read from local .git folders." },
  { key: "agents", label: "Claude Code and Codex", note: "Your prompts, tool names and test results from local session logs." },
  { key: "shell", label: "Terminal", note: "Command name, exit code and folder from the zsh hook. Never the arguments." },
];

const lines = (s: string) => s.split("\n").map((l) => l.trim()).filter(Boolean);

export default function Sources({ status, onSaved }: { status: Status; onSaved: () => void }) {
  const [s, setS] = useState<Settings>(status.settings);
  const [roots, setRoots] = useState(s.repo_roots.join("\n"));
  const [excluded, setExcluded] = useState(s.excluded_paths.join("\n"));
  const [optOut, setOptOut] = useState(s.cloud_opt_out.join("\n"));
  const [key, setKey] = useState("");
  const [hookLine, setHookLine] = useState<string | null>(null);
  const [togglToken, setTogglToken] = useState("");
  const [togglError, setTogglError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);

  const save = async () => {
    await api.saveSettings({ ...s, repo_roots: lines(roots), excluded_paths: lines(excluded), cloud_opt_out: lines(optOut) });
    setSaved("Settings saved. Collector changes apply the next time the app opens.");
    onSaved();
  };

  const connectToggl = async () => {
    setTogglError(null);
    try {
      const account = await api.connectToggl(togglToken);
      setTogglToken("");
      setS({ ...s, toggl: { ...s.toggl, workspace_id: account.workspace_id, email: s.toggl.email || account.email } });
      setSaved(`Connected to Toggl as ${account.email}. The token is in your Keychain.`);
      onSaved();
    } catch (e) {
      setTogglError(String(e));
    }
  };

  const saveKey = async () => {
    await api.saveApiKey(key);
    setKey("");
    setSaved("API key saved to your Keychain.");
    onSaved();
  };

  return (
    <div className="sources">
      <header className="day-head">
        <h1>Sources</h1>
        <p className="day-total">All collected data stays in a local database on this Mac.</p>
      </header>

      <section>
        <h2>What the app collects</h2>
        <ul className="toggles">
          {TOGGLES.map((t) => (
            <li key={t.key}>
              <label className={t.disabled ? "off" : ""}>
                <input
                  type="checkbox"
                  role="switch"
                  disabled={t.disabled}
                  checked={s.sources[t.key]}
                  onChange={(e) => setS({ ...s, sources: { ...s.sources, [t.key]: e.target.checked } })}
                />
                <span>
                  <b>{t.label}</b>
                  <small>{t.note}</small>
                </span>
              </label>
            </li>
          ))}
        </ul>
        <div className="hook">
          <button onClick={async () => setHookLine(await api.installShellHook())}>Set up the terminal hook</button>
          {hookLine && (
            <p>
              Add this line to <code>~/.zshrc</code>, then open a new terminal: <code className="copy">{hookLine}</code>
            </p>
          )}
        </div>
      </section>

      <section>
        <h2>Where to look</h2>
        <label className="area">
          Code folders, one per line
          <textarea rows={3} value={roots} onChange={(e) => setRoots(e.target.value)} />
        </label>
        <label className="area">
          Never collect from these paths
          <textarea rows={3} value={excluded} placeholder="/Users/you/Documents/private" onChange={(e) => setExcluded(e.target.value)} />
        </label>
      </section>

      <section>
        <h2>AI features</h2>
        <p className="muted">
          The journal and AI labels send one short, redacted summary per day to the Claude API: session times, project and branch names,
          commit subjects, test counts and the first 200 characters of your prompts, with secrets removed. Raw logs, file contents and command output stay here.
        </p>
        <label className="area">
          Projects that never leave this Mac, one per line
          <textarea rows={2} value={optOut} onChange={(e) => setOptOut(e.target.value)} />
        </label>
        <ModelPicker value={s.model} onChange={(model) => setS({ ...s, model })} />
        <label className="area">
          Claude API key {status.has_api_key && <span className="tag ok">Saved</span>}
          <span className="inline">
            <input type="password" autoComplete="off" value={key} placeholder={status.has_api_key ? "Replace the saved key" : "sk-ant-…"} onChange={(e) => setKey(e.target.value)} />
            <button disabled={!key.trim()} onClick={saveKey}>Save key</button>
          </span>
        </label>
      </section>

      <section>
        <h2>Toggl</h2>
        <p className="muted">
          Reports export approved entries as a Toggl CSV. With a token, they can also go straight to your workspace. The
          token is on your Toggl profile page.
        </p>
        <label className="area">
          Email for CSV imports
          <input value={s.toggl.email} placeholder="you@company.com" onChange={(e) => setS({ ...s, toggl: { ...s.toggl, email: e.target.value } })} />
        </label>
        <label className="check-line">
          <input type="checkbox" checked={s.toggl.default_billable} onChange={(e) => setS({ ...s, toggl: { ...s.toggl, default_billable: e.target.checked } })} />
          New projects are billable
        </label>
        <label className="area">
          Toggl API token {status.has_toggl_token && <span className="tag ok">Connected</span>}
          <span className="inline">
            <input type="password" autoComplete="off" value={togglToken} placeholder={status.has_toggl_token ? "Replace the saved token" : "Paste your API token"} onChange={(e) => setTogglToken(e.target.value)} />
            <button disabled={!togglToken.trim()} onClick={connectToggl}>Connect</button>
          </span>
        </label>
        {togglError && <p className="error">{togglError}</p>}
      </section>

      <footer className="save">
        <button className="primary" onClick={save}>Save settings</button>
        {saved && <span className="muted">{saved}</span>}
      </footer>
    </div>
  );
}

/** Every AI feature uses this model. The list comes from the models the saved key can use. */
function ModelPicker({ value, onChange }: { value: string; onChange: (model: string) => void }) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  useEffect(() => {
    api.aiModels().then(setModels).catch(() => setModels([]));
  }, []);
  const options = models.some((m) => m.id === value) ? models : [{ id: value, display_name: value, note: "" }, ...models];
  const note = options.find((m) => m.id === value)?.note;

  return (
    <label className="area">
      Model for the journal and labels
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        {options.map((m) => (
          <option key={m.id} value={m.id}>{m.display_name}</option>
        ))}
      </select>
      {note && <small className="muted">{note}</small>}
    </label>
  );
}
