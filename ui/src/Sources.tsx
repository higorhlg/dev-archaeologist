import { useState } from "react";
import { api, type Settings, type Status } from "./api";

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
  const [saved, setSaved] = useState<string | null>(null);

  const save = async () => {
    await api.saveSettings({ ...s, repo_roots: lines(roots), excluded_paths: lines(excluded), cloud_opt_out: lines(optOut) });
    setSaved("Settings saved. Collector changes apply the next time the app opens.");
    onSaved();
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
        <h2>Journal writing</h2>
        <p className="muted">
          The journal sends one short, redacted summary per day to the Claude API: session times, project and branch names,
          commit subjects, test counts and the first 200 characters of your prompts, with secrets removed. Raw logs, file contents and command output stay here.
        </p>
        <label className="area">
          Projects that never leave this Mac, one per line
          <textarea rows={2} value={optOut} onChange={(e) => setOptOut(e.target.value)} />
        </label>
        <label className="area">
          Model
          <input value={s.model} onChange={(e) => setS({ ...s, model: e.target.value })} />
        </label>
        <label className="area">
          Claude API key {status.has_api_key && <span className="tag ok">Saved</span>}
          <span className="inline">
            <input type="password" autoComplete="off" value={key} placeholder={status.has_api_key ? "Replace the saved key" : "sk-ant-…"} onChange={(e) => setKey(e.target.value)} />
            <button disabled={!key.trim()} onClick={saveKey}>Save key</button>
          </span>
        </label>
      </section>

      <footer className="save">
        <button className="primary" onClick={save}>Save settings</button>
        {saved && <span className="muted">{saved}</span>}
      </footer>
    </div>
  );
}
