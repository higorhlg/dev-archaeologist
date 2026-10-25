import { useEffect, useState } from "react";
import {
  api,
  type CalendarStatus,
  type Context,
  type ContextRules,
  type ModelInfo,
  type ProjectContext,
  type Settings,
  type Status,
  type TrackerStatus,
  type TrackerSettings,
} from "./api";
import ScheduleStep from "./ScheduleStep";

type Toggle = { key: keyof Settings["sources"]; label: string; note: string; disabled?: boolean };

const TOGGLES: Toggle[] = [
  { key: "app", label: "Foreground app", note: "The name of the app in front, and when you are away." },
  { key: "window_titles", label: "Window titles", note: "Titles can show email subjects and chat names. Not available yet.", disabled: true },
  { key: "fs", label: "File changes", note: "Which files change in your repositories. Never their contents." },
  { key: "git", label: "Git", note: "Your commits on every branch, read from local .git folders." },
  { key: "agents", label: "Claude Code and Codex", note: "Your prompts, tool names and test results from local session logs." },
  { key: "shell", label: "Terminal", note: "Command name, exit code and folder from the zsh hook. Never the arguments." },
  { key: "calendar", label: "Calendar meetings", note: "Meetings from the macOS Calendar app, after they end. Set up below." },
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
    await api.patchSettings({
      sources: s.sources,
      repo_roots: lines(roots),
      excluded_paths: lines(excluded),
      cloud_opt_out: lines(optOut),
      model: s.model,
      toggl: s.toggl,
      automation: s.automation,
    });
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
        <h2>Work hours</h2>
        <ScheduleStep compact initial={status.settings.schedule} projects={Object.keys(status.settings.toggl.project_map)} onDone={onSaved} />
      </section>

      <Calendar onSaved={onSaved} />

      <WorkAndPersonal rules={status.settings.contexts} onSaved={onSaved} />

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
        <label className="check-line">
          <input type="checkbox" checked={s.automation.journals} onChange={(e) => setS({ ...s, automation: { ...s.automation, journals: e.target.checked } })} />
          Write the journal each morning for finished days of the past week that have none (needs the API key)
        </label>
      </section>

      <section>
        <h2>Weekly report</h2>
        <label className="check-line">
          <input type="checkbox" checked={s.automation.weekly_report} onChange={(e) => setS({ ...s, automation: { ...s.automation, weekly_report: e.target.checked } })} />
          Write last week's work report on Monday, as a Markdown file: time per project and ticket, each day against your hours,
          the journal, and what still needs a ticket or approval
        </label>
        <p className="inline">
          <button onClick={() => api.openWeeklyReports()}>Open weekly reports</button>
        </p>
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

      <TicketSources onSaved={onSaved} />

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

/** Work and personal folders and emails, seeded from the git config and editable here.
 * Saves only the side rules, then shows how each project falls out. */
function WorkAndPersonal({ rules, onSaved }: { rules: ContextRules; onSaved: () => void }) {
  const [projects, setProjects] = useState<ProjectContext[]>([]);
  const [work, setWork] = useState(rules.work_paths.join("\n"));
  const [personal, setPersonal] = useState(rules.personal_paths.join("\n"));
  const [emails, setEmails] = useState(rules.work_emails.join("\n"));
  const [note, setNote] = useState<string | null>(null);

  const load = () => api.projectContexts().then(setProjects);
  useEffect(() => {
    load();
  }, []);

  const saveRules = async (next: Partial<ContextRules>) => {
    await api.patchSettings({ contexts: { ...rules, ...next } });
    onSaved();
    load();
  };
  const save = async () => {
    await saveRules({ work_paths: lines(work), personal_paths: lines(personal), work_emails: lines(emails) });
    setNote("Saved. Reports and the side switch use these rules now.");
  };
  const detect = async () => {
    const found = await api.detectContexts();
    setWork(found.work_paths.join("\n"));
    setEmails(found.work_emails.join("\n"));
    setNote(
      found.work_paths.length
        ? `Found ${found.work_paths.length} work ${found.work_paths.length === 1 ? "folder" : "folders"} in your git config. Save to use them.`
        : "Your global git config has no includeIf work identity. Add folders by hand.",
    );
  };
  const pin = async (project: string, context: Context | null) => {
    await api.setProjectContext(project, context);
    onSaved();
    load();
  };

  return (
    <section>
      <h2>Work and personal</h2>
      <p className="muted">
        A project is work when it sits in a work folder, or when its repo commits with a work email. Personal folders win
        over both. Anything else is personal. Personal time never goes to Toggl.
      </p>
      <label className="area">
        Work folders, one per line
        <textarea rows={2} value={work} placeholder="~/work" onChange={(e) => setWork(e.target.value)} />
      </label>
      <label className="area">
        Personal folders, one per line
        <textarea rows={2} value={personal} placeholder="~/side-projects" onChange={(e) => setPersonal(e.target.value)} />
      </label>
      <label className="area">
        Work emails, one per line
        <textarea rows={1} value={emails} placeholder="you@company.com" onChange={(e) => setEmails(e.target.value)} />
      </label>
      <div className="actions">
        <button className="primary" onClick={save}>Save sides</button>
        <button onClick={detect}>Find in my git config</button>
        {note && <span className="muted">{note}</span>}
      </div>
      <ul className="contexts">
        {projects.map((p) => (
          <li key={p.project}>
            <span>{p.project}{p.chosen && <small className="muted"> your choice</small>}</span>
            <span className="seg" role="group" aria-label={`${p.project} side`}>
              <button className={p.context === "work" ? "on" : ""} onClick={() => pin(p.project, "work")}>Work</button>
              <button className={p.context === "personal" ? "on" : ""} onClick={() => pin(p.project, "personal")}>Personal</button>
              {p.chosen && <button onClick={() => pin(p.project, null)} title="Use the folder and email rules">Auto</button>}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Calendar access, and which calendars count. Meetings come from accounts already in
 * the macOS Calendar app, so there is no sign-in here. */
function Calendar({ onSaved }: { onSaved: () => void }) {
  const [status, setStatus] = useState<CalendarStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    api.calendarStatus().then(setStatus);
  }, []);

  const connect = async () => {
    setBusy("Waiting for macOS…");
    const s = await api.connectCalendar();
    setStatus(s);
    setBusy(null);
    setNote(s.access === "granted" ? "Connected. Meetings from the last 90 days are in your timeline." : null);
    onSaved();
  };
  const toggle = async (title: string, include: boolean) => {
    if (!status) return;
    const excluded = include ? status.excluded.filter((t) => t !== title) : [...status.excluded, title];
    await api.patchSettings({ calendar_excluded: excluded });
    setStatus({ ...status, excluded });
  };
  const reimport = async () => {
    setBusy("Reading meetings…");
    const n = await api.reimportMeetings();
    setBusy(null);
    setNote(`Read the last 90 days again. ${n} new meetings.`);
    onSaved();
  };

  const byAccount = new Map<string, string[]>();
  for (const c of status?.calendars ?? []) byAccount.set(c.account, [...(byAccount.get(c.account) ?? []), c.title]);

  return (
    <section>
      <h2>Calendar</h2>
      <p className="muted">
        A meeting is a timed, busy event you did not decline, with other people or a video link. Each one becomes a session
        that covers its gap. Accounts with a work email are work; others are personal.
      </p>
      {status?.access === "unsupported" && <p className="muted">Calendar reading needs macOS.</p>}
      {status?.access === "not_asked" && (
        <div className="actions">
          <button className="primary" disabled={!!busy} onClick={connect}>{busy ?? "Allow calendar access"}</button>
        </div>
      )}
      {status?.access === "denied" && (
        <p className="error">
          macOS denied Calendar access. Turn on Dev Archaeologist in System Settings › Privacy &amp; Security › Calendars,
          then open this screen again.
        </p>
      )}
      {status?.access === "granted" && (
        <>
          {[...byAccount.entries()].map(([account, titles]) => (
            <div key={account} className="cal-account">
              <b>{account || "On this Mac"}</b>
              <ul className="toggles compact">
                {titles.map((t) => (
                  <li key={t}>
                    <label>
                      <input type="checkbox" role="switch" checked={!status.excluded.includes(t)} onChange={(e) => toggle(t, e.target.checked)} />
                      <span>{t}</span>
                    </label>
                  </li>
                ))}
              </ul>
            </div>
          ))}
          <div className="actions">
            <button disabled={!!busy} onClick={reimport}>{busy ?? "Read meetings again"}</button>
            {note && <span className="muted">{note}</span>}
          </div>
        </>
      )}
    </section>
  );
}

/** Jira (API token) and GitHub (the gh CLI's accounts), which "Find tickets" in Reports asks.
 * Both only read. Tokens stay in the Keychain or in gh. */
function TicketSources({ onSaved }: { onSaved: () => void }) {
  const [status, setStatus] = useState<TrackerStatus | null>(null);
  const [site, setSite] = useState("");
  const [email, setEmail] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.trackerStatus().then((t) => {
      setStatus(t);
      setSite(t.jira_site || t.jira_site_guess || "");
      setEmail(t.jira_email || t.jira_email_guess || "");
    });
  }, []);

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      const account = await api.connectJira(site, email, token);
      setToken("");
      setNote(`Connected to Jira as ${account.display_name || account.email}. The token is in your Keychain.`);
      setStatus(await api.trackerStatus());
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  /** Saves the tracker settings with one field changed; the section is saved whole. */
  const saveTrackers = async (change: Partial<TrackerSettings>) => {
    if (!status) return;
    const current: TrackerSettings = {
      jira_site: status.jira_site,
      jira_email: status.jira_email,
      github: status.github_enabled,
      auto_sync_min: status.auto_sync_min,
    };
    const next = { ...current, ...change };
    await api.patchSettings({ trackers: next });
    setStatus({ ...status, github_enabled: next.github, auto_sync_min: next.auto_sync_min });
    onSaved();
  };
  const setGithub = (github: boolean) => saveTrackers({ github });

  return (
    <section>
      <h2>Ticket sources</h2>
      <p className="muted">
        Find tickets in Reports asks these for your own tickets in the report range and suggests one for each entry
        without a ticket. A Jira ticket a session names exactly, or one its own pull request leads to, is applied
        to that session on its own and marked "auto"; "Not this" takes it back. Other suggestions wait for you. Nothing is
        written to Jira or GitHub.
      </p>
      <label className="area">
        Sync tickets on their own
        <select value={status?.auto_sync_min ?? 30} disabled={!status} onChange={(e) => saveTrackers({ auto_sync_min: Number(e.target.value) })}>
          <option value={0}>Never</option>
          <option value={15}>Every 15 minutes</option>
          <option value={30}>Every 30 minutes</option>
          <option value={60}>Every hour</option>
          <option value={240}>Every 4 hours</option>
        </select>
      </label>
      <p className="muted">
        Runs at launch and then on this interval while the app is open, over the last two weeks of work, without the AI.
        To keep syncing while the app is closed, run <code>dev-arch sync-agent install</code> once in a terminal.
      </p>

      <h3>Jira {status?.jira_connected && <span className="tag ok">Connected</span>}</h3>
      <p className="muted">
        Jira uses an API token. Get a token opens the Atlassian page in your browser: create a token, copy it, and paste it here.
      </p>
      <label className="area">
        Jira site
        <input value={site} placeholder="your-company.atlassian.net" onChange={(e) => setSite(e.target.value)} />
      </label>
      <label className="area">
        Atlassian email
        <input value={email} placeholder="you@company.com" onChange={(e) => setEmail(e.target.value)} />
      </label>
      <label className="area">
        API token
        <span className="inline">
          <input type="password" autoComplete="off" value={token} placeholder={status?.jira_connected ? "Replace the saved token" : "Paste your API token"} onChange={(e) => setToken(e.target.value)} />
          <button onClick={() => api.openJiraTokenPage()}>Get a token</button>
          <button className="primary" disabled={busy || !token.trim() || !site.trim() || !email.trim()} onClick={connect}>
            {busy ? "Connecting…" : "Connect"}
          </button>
        </span>
      </label>
      {error && <p className="error">{error}</p>}

      <h3>GitHub</h3>
      {status && status.github_accounts.length === 0 ? (
        <p className="muted">
          GitHub goes through the GitHub CLI. Install it, run <code>gh auth login</code> in a terminal, then open this screen again.
        </p>
      ) : (
        <label className="check-line">
          <input type="checkbox" checked={status?.github_enabled ?? true} onChange={(e) => setGithub(e.target.checked)} />
          Use the GitHub CLI accounts {status?.github_accounts.join(" and ")}
        </label>
      )}
      {note && <p className="muted">{note}</p>}
    </section>
  );
}
