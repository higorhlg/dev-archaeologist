import { useEffect, useState } from "react";
import {
  api,
  stratum,
  type CalendarStatus,
  type Context,
  type ContextRules,
  type DescriptionRules,
  type ModelInfo,
  type ProjectContext,
  type RecurringEntry,
  type Settings as AppSettings,
  type Status,
  type TogglProject,
  type TogglProjectInfo,
  type TrackerStatus,
  type TrackerSettings,
} from "./api";
import ScheduleStep from "./ScheduleStep";

export type SettingsTab = "connections" | "timesheet" | "hours" | "collection" | "ai";

const TABS: { id: SettingsTab; label: string }[] = [
  { id: "connections", label: "Connections" },
  { id: "timesheet", label: "Timesheet" },
  { id: "hours", label: "Work hours" },
  { id: "collection", label: "Collection" },
  { id: "ai", label: "AI and automation" },
];

type Toggle = { key: keyof AppSettings["sources"]; label: string; note: string; disabled?: boolean };

const TOGGLES: Toggle[] = [
  { key: "app", label: "Foreground app", note: "The name of the app in front, and when you are away." },
  { key: "window_titles", label: "Window titles", note: "Titles can show email subjects and chat names. Not available yet.", disabled: true },
  { key: "fs", label: "File changes", note: "Which files change in your repositories. Never their contents." },
  { key: "git", label: "Git", note: "Your commits on every branch, read from local .git folders." },
  { key: "agents", label: "Claude Code and Codex", note: "Your prompts, tool names and test results from local session logs." },
  { key: "shell", label: "Terminal", note: "Command name, exit code and folder from the zsh hook. Never the arguments." },
  { key: "calendar", label: "Calendar meetings", note: "Meetings from the macOS Calendar app, after they end. Set up in Connections." },
];

const WEEKDAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

const lines = (s: string) => s.split("\n").map((l) => l.trim()).filter(Boolean);
const words = (s: string) => s.split(/[\s,]+/).map((w) => w.trim().toUpperCase()).filter(Boolean);

type Props = { status: Status; tab: SettingsTab; onTab: (tab: SettingsTab) => void; onSaved: () => void };

/** Settings grouped by purpose, one tab each. Each section saves on its own. */
export default function Settings({ status, tab, onTab, onSaved }: Props) {
  return (
    <div className="page settings">
      <header className="toolbar">
        <h1>Settings</h1>
        <nav className="tabs" role="tablist" aria-label="Settings sections">
          {TABS.map((t) => (
            <button key={t.id} role="tab" aria-selected={tab === t.id} className={tab === t.id ? "on" : ""} onClick={() => onTab(t.id)}>
              {t.label}
            </button>
          ))}
        </nav>
      </header>
      <p className="muted lead">Everything the app collects stays in a local database on this Mac.</p>
      {tab === "connections" && (
        <>
          <TogglConnection status={status} onSaved={onSaved} />
          <TicketSources onSaved={onSaved} />
          <Calendar onSaved={onSaved} />
        </>
      )}
      {tab === "timesheet" && (
        <>
          <TogglDefaults status={status} onSaved={onSaved} />
          <DescriptionRulesForm status={status} onSaved={onSaved} />
          <RecurringEntries status={status} onSaved={onSaved} />
          <ProjectMapping status={status} onSaved={onSaved} />
          <TogglHistory />
        </>
      )}
      {tab === "hours" && (
        <>
          <section>
            <h2>Work hours</h2>
            <ScheduleStep compact initial={status.settings.schedule} projects={Object.keys(status.settings.toggl.project_map)} onDone={onSaved} />
          </section>
          <WorkAndPersonal rules={status.settings.contexts} onSaved={onSaved} />
        </>
      )}
      {tab === "collection" && <Collection status={status} onSaved={onSaved} />}
      {tab === "ai" && <AiAndAutomation status={status} onSaved={onSaved} />}
    </div>
  );
}

/** A section's save button with what it last did. */
function SaveRow({ onSave, label = "Save", note }: { onSave: () => void; label?: string; note: string | null }) {
  return (
    <div className="actions">
      <button className="primary" onClick={onSave}>{label}</button>
      {note && <span className="muted" role="status">{note}</span>}
    </div>
  );
}

/** The Toggl API token, kept in the Keychain; connecting also stores the workspace and email. */
function TogglConnection({ status, onSaved }: { status: Status; onSaved: () => void }) {
  const [token, setToken] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const connect = async () => {
    setError(null);
    try {
      const account = await api.connectToggl(token);
      setToken("");
      setNote(`Connected to Toggl as ${account.email}. The token is in your Keychain.`);
      onSaved();
    } catch (e) {
      setError(String(e));
    }
  };
  return (
    <section>
      <h2>Toggl {status.has_toggl_token && <span className="tag ok">Connected</span>}</h2>
      <p className="muted">With a token, the timesheet can send approved entries straight to your workspace. The token is on your Toggl profile page.</p>
      <label className="area">
        API token
        <span className="inline">
          <input type="password" autoComplete="off" value={token} placeholder={status.has_toggl_token ? "Replace the saved token" : "Paste your API token"} onChange={(e) => setToken(e.target.value)} />
          <button disabled={!token.trim()} onClick={connect}>Connect</button>
        </span>
      </label>
      {note && <p className="muted">{note}</p>}
      {error && <p className="error">{error}</p>}
    </section>
  );
}

/** The CSV email and whether new projects are billable. */
function TogglDefaults({ status, onSaved }: { status: Status; onSaved: () => void }) {
  const [toggl, setToggl] = useState(status.settings.toggl);
  const [note, setNote] = useState<string | null>(null);
  const save = async () => {
    await api.patchSettings({ toggl: { ...status.settings.toggl, email: toggl.email, default_billable: toggl.default_billable } });
    setNote("Saved.");
    onSaved();
  };
  return (
    <section>
      <h2>Toggl entries</h2>
      <label className="area">
        Email for CSV imports
        <input value={toggl.email} placeholder="you@company.com" onChange={(e) => setToggl({ ...toggl, email: e.target.value })} />
      </label>
      <label className="check-line">
        <input type="checkbox" checked={toggl.default_billable} onChange={(e) => setToggl({ ...toggl, default_billable: e.target.checked })} />
        New projects are billable
      </label>
      <SaveRow onSave={save} note={note} />
    </section>
  );
}

/** How "Epic and ticket" descriptions are worded, for tickets, loose work and meetings. */
function DescriptionRulesForm({ status, onSaved }: { status: Status; onSaved: () => void }) {
  const [rules, setRules] = useState<DescriptionRules>(status.settings.toggl.descriptions);
  const [jiraProjects, setJiraProjects] = useState(rules.jira_projects.join(" "));
  const [mineOnly, setMineOnly] = useState(rules.mine_only_projects.join(" "));
  const [note, setNote] = useState<string | null>(null);
  const setMeeting = (i: number, patch: Partial<DescriptionRules["meetings"][number]>) =>
    setRules({ ...rules, meetings: rules.meetings.map((m, j) => (j === i ? { ...m, ...patch } : m)) });
  const save = async () => {
    const descriptions = { ...rules, jira_projects: words(jiraProjects), mine_only_projects: words(mineOnly), meetings: rules.meetings.filter((m) => m.title_contains.trim()) };
    await api.patchSettings({ toggl: { ...status.settings.toggl, descriptions } });
    setRules(descriptions);
    setNote("Saved. The timesheet words entries with these rules now.");
    onSaved();
  };
  return (
    <section>
      <h2>Descriptions</h2>
      <p className="muted">With "Epic and ticket", entries read <code>EPIC TICKET</code>. The epic comes from your Toggl history, then from Jira, then from the fallback below.</p>
      <div className="form-grid">
        <label className="area">
          Fallback epic
          <input value={rules.fallback_epic} placeholder="SHOP-1126" onChange={(e) => setRules({ ...rules, fallback_epic: e.target.value })} />
          <small className="muted">For a ticket in the same Jira project that has no epic.</small>
        </label>
        <label className="area">
          Work without a ticket
          <input value={rules.no_ticket} placeholder="SHOP-1126 KTLO" onChange={(e) => setRules({ ...rules, no_ticket: e.target.value })} />
        </label>
        <label className="area">
          Jira projects to file under
          <input value={jiraProjects} placeholder="All projects" onChange={(e) => setJiraProjects(e.target.value)} />
          <small className="muted">Space-separated prefixes. Empty takes every project.</small>
        </label>
        <label className="area">
          Only my tickets in
          <input value={mineOnly} placeholder="PLAT" onChange={(e) => setMineOnly(e.target.value)} />
          <small className="muted">Tickets here count only when you are the assignee or reporter.</small>
        </label>
      </div>
      <label className="check-line">
        <input type="checkbox" checked={rules.fold_no_ticket} onChange={(e) => setRules({ ...rules, fold_no_ticket: e.target.checked })} />
        File work without a ticket under the ticket you worked on nearest in time that day
      </label>
      <label className="check-line">
        <input type="checkbox" checked={rules.use_suggestions} onChange={(e) => setRules({ ...rules, use_suggestions: e.target.checked })} />
        Use tickets suggested through a pull request
      </label>
      <h3>Meetings</h3>
      <ul className="rule-list">
        {rules.meetings.map((m, i) => (
          <li key={i}>
            <span>Title contains</span>
            <input value={m.title_contains} placeholder="Squad daily" aria-label="Meeting title contains" onChange={(e) => setMeeting(i, { title_contains: e.target.value })} />
            <span>→</span>
            <input value={m.description} placeholder="MEETING-1 - Squad Meeting" aria-label="Description" onChange={(e) => setMeeting(i, { description: e.target.value })} />
            <button className="small" aria-label="Remove rule" onClick={() => setRules({ ...rules, meetings: rules.meetings.filter((_, j) => j !== i) })}>Remove</button>
          </li>
        ))}
        <li>
          <span>Any other meeting</span>
          <input value={rules.other_meeting} placeholder="Meeting title" aria-label="Other meetings" onChange={(e) => setRules({ ...rules, other_meeting: e.target.value })} />
        </li>
      </ul>
      <div className="actions">
        <button onClick={() => setRules({ ...rules, meetings: [...rules.meetings, { title_contains: "", description: "" }] })}>Add meeting rule</button>
      </div>
      <SaveRow onSave={save} label="Save descriptions" note={note} />
    </section>
  );
}

/** Entries added to every worked day of a weekday, such as a weekly company standup. */
function RecurringEntries({ status, onSaved }: { status: Status; onSaved: () => void }) {
  const [items, setItems] = useState<RecurringEntry[]>(status.settings.toggl.recurring);
  const [note, setNote] = useState<string | null>(null);
  const set = (i: number, patch: Partial<RecurringEntry>) => setItems(items.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const save = async () => {
    const recurring = items.filter((r) => r.description.trim() && r.minutes > 0);
    await api.patchSettings({ toggl: { ...status.settings.toggl, recurring } });
    setItems(recurring);
    setNote("Saved.");
    onSaved();
  };
  return (
    <section>
      <h2>Every week</h2>
      <p className="muted">Added to each worked day that falls on the weekday, already approved, and held in place like a meeting.</p>
      <ul className="rule-list recurring">
        {items.map((r, i) => (
          <li key={i}>
            <select value={r.weekday} aria-label="Weekday" onChange={(e) => set(i, { weekday: Number(e.target.value) })}>
              {WEEKDAYS.map((d, n) => <option key={d} value={n}>{d}</option>)}
            </select>
            <input type="time" value={r.start} aria-label="Starts" onChange={(e) => set(i, { start: e.target.value })} />
            <input type="number" min={5} step={5} value={r.minutes} aria-label="Minutes" onChange={(e) => set(i, { minutes: Number(e.target.value) })} />
            <span>min</span>
            <input value={r.description} placeholder="Company Standup" aria-label="Description" onChange={(e) => set(i, { description: e.target.value })} />
            <input value={r.project} placeholder="Toggl project" aria-label="Toggl project" onChange={(e) => set(i, { project: e.target.value })} />
            <input value={r.client} placeholder="Client" aria-label="Client" onChange={(e) => set(i, { client: e.target.value })} />
            <label className="check-line"><input type="checkbox" checked={r.billable} onChange={(e) => set(i, { billable: e.target.checked })} />Billable</label>
            <button className="small" onClick={() => setItems(items.filter((_, j) => j !== i))}>Remove</button>
          </li>
        ))}
      </ul>
      <div className="actions">
        <button onClick={() => setItems([...items, { weekday: 0, start: "15:30", minutes: 30, description: "", project: "", client: "", billable: false }])}>Add weekly entry</button>
      </div>
      <SaveRow onSave={save} note={note} />
    </section>
  );
}

/** Local project → Toggl project, client, billable, or "never send", for every known project. */
function ProjectMapping({ status, onSaved }: { status: Status; onSaved: () => void }) {
  const settings = status.settings;
  const [map, setMap] = useState<Record<string, TogglProject>>(settings.toggl.project_map);
  const [local, setLocal] = useState<string[]>(Object.keys(settings.toggl.project_map));
  const [remote, setRemote] = useState<TogglProjectInfo[]>([]);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    api.projectContexts().then((ps) => setLocal([...new Set([...ps.filter((p) => p.context === "work").map((p) => p.project), ...Object.keys(settings.toggl.project_map)])].sort()));
    if (status.has_toggl_token) api.togglProjects().then(setRemote).catch(() => setRemote([]));
  }, [status.has_toggl_token, settings.toggl.project_map]);

  const row = (p: string): TogglProject => map[p] ?? { name: p, id: null, client: "", billable: settings.toggl.default_billable, skip: false };
  const set = (p: string, patch: Partial<TogglProject>) => {
    const next = { ...row(p), ...patch };
    if (patch.name !== undefined) next.id = remote.find((r) => r.name === patch.name)?.id ?? null;
    setMap({ ...map, [p]: next });
  };
  const save = async () => {
    await api.patchSettings({ toggl: { ...settings.toggl, project_map: map } });
    setNote("Saved.");
    onSaved();
  };

  return (
    <section>
      <h2>Projects</h2>
      <p className="muted">Each local work project becomes a Toggl project. {remote.length > 0 ? "Pick from your workspace or type a name." : "Type the Toggl project name."}</p>
      <datalist id="toggl-projects">{remote.map((r) => <option key={r.id} value={r.name} />)}</datalist>
      <div className="table-wrap">
        <table className="mapping">
          <thead>
            <tr><th>Local project</th><th>Toggl project</th><th>Client</th><th>Billable</th><th>Never send</th></tr>
          </thead>
          <tbody>
            {local.map((p) => (
              <tr key={p}>
                <td><i className="dot" style={{ background: stratum(p) }} /> {p}</td>
                <td><input list="toggl-projects" value={row(p).name} aria-label={`${p} Toggl project`} onChange={(e) => set(p, { name: e.target.value })} /></td>
                <td><input value={row(p).client} aria-label={`${p} client`} onChange={(e) => set(p, { client: e.target.value })} /></td>
                <td><input type="checkbox" checked={row(p).billable} aria-label={`${p} billable`} onChange={(e) => set(p, { billable: e.target.checked })} /></td>
                <td><input type="checkbox" checked={row(p).skip} aria-label={`${p} never send`} onChange={(e) => set(p, { skip: e.target.checked })} /></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <SaveRow onSave={save} label="Save projects" note={note} />
    </section>
  );
}

/** Learns the epic each ticket is filed under from a Toggl "Detailed report" CSV export. */
function TogglHistory() {
  const [path, setPath] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const learn = async () => {
    setError(null);
    try {
      const n = await api.importTogglHistory(path);
      setNote(`Learned the epic of ${n} tickets.`);
    } catch (e) {
      setError(String(e));
    }
  };
  return (
    <section>
      <h2>Learn from your Toggl history</h2>
      <p className="muted">
        In Toggl, export Reports › Detailed as CSV for the year, then give its path here. Descriptions then use the epic you file each
        ticket under. Import again now and then to pick up new tickets.
      </p>
      <label className="area">
        Detailed report CSV
        <span className="inline">
          <input value={path} placeholder="~/Downloads/TogglTrack_Report_Detailed_report.csv" onChange={(e) => setPath(e.target.value)} />
          <button disabled={!path.trim()} onClick={learn}>Import</button>
        </span>
      </label>
      {note && <p className="muted" role="status">{note}</p>}
      {error && <p className="error">{error}</p>}
    </section>
  );
}

/** What the app reads, the terminal hook, and where it looks for code. */
function Collection({ status, onSaved }: { status: Status; onSaved: () => void }) {
  const [sources, setSources] = useState(status.settings.sources);
  const [roots, setRoots] = useState(status.settings.repo_roots.join("\n"));
  const [excluded, setExcluded] = useState(status.settings.excluded_paths.join("\n"));
  const [hookLine, setHookLine] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const save = async () => {
    await api.patchSettings({ sources, repo_roots: lines(roots), excluded_paths: lines(excluded) });
    setNote("Saved. Collector changes apply the next time the app opens.");
    onSaved();
  };
  return (
    <>
      <section>
        <h2>What the app collects</h2>
        <ul className="toggles">
          {TOGGLES.map((t) => (
            <li key={t.key}>
              <label className={t.disabled ? "off" : ""}>
                <input type="checkbox" role="switch" disabled={t.disabled} checked={sources[t.key]} onChange={(e) => setSources({ ...sources, [t.key]: e.target.checked })} />
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
        <SaveRow onSave={save} note={note} />
      </section>
    </>
  );
}

/** The Claude API key and model, what never leaves the Mac, and what runs on its own. */
function AiAndAutomation({ status, onSaved }: { status: Status; onSaved: () => void }) {
  const [model, setModel] = useState(status.settings.model);
  const [optOut, setOptOut] = useState(status.settings.cloud_opt_out.join("\n"));
  const [automation, setAutomation] = useState(status.settings.automation);
  const [key, setKey] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const saveKey = async () => {
    await api.saveApiKey(key);
    setKey("");
    setNote("API key saved to your Keychain.");
    onSaved();
  };
  const save = async () => {
    await api.patchSettings({ model, cloud_opt_out: lines(optOut), automation });
    setNote("Saved.");
    onSaved();
  };
  return (
    <>
      <section>
        <h2>Claude API</h2>
        <p className="muted">
          The journal and AI labels send one short, redacted summary per day to the Claude API: session times, project and branch names,
          commit subjects, test counts and the first 200 characters of your prompts, with secrets removed. Raw logs, file contents and command output stay here.
        </p>
        <label className="area">
          API key {status.has_api_key && <span className="tag ok">Saved</span>}
          <span className="inline">
            <input type="password" autoComplete="off" value={key} placeholder={status.has_api_key ? "Replace the saved key" : "sk-ant-…"} onChange={(e) => setKey(e.target.value)} />
            <button disabled={!key.trim()} onClick={saveKey}>Save key</button>
          </span>
        </label>
        <ModelPicker value={model} onChange={setModel} />
        <label className="area">
          Projects that never leave this Mac, one per line
          <textarea rows={2} value={optOut} onChange={(e) => setOptOut(e.target.value)} />
        </label>
      </section>
      <section>
        <h2>On their own</h2>
        <label className="check-line">
          <input type="checkbox" checked={automation.journals} onChange={(e) => setAutomation({ ...automation, journals: e.target.checked })} />
          Write the journal each morning for finished days of the past week that have none (needs the API key)
        </label>
        <label className="check-line">
          <input type="checkbox" checked={automation.weekly_report} onChange={(e) => setAutomation({ ...automation, weekly_report: e.target.checked })} />
          Write last week's work report on Monday, as a Markdown file
        </label>
        <label className="check-line">
          <input type="checkbox" checked={automation.status_updates} onChange={(e) => setAutomation({ ...automation, status_updates: e.target.checked })} />
          Write the daily standup each work day, and the weekly status each
          <select value={automation.weekly_status_day} aria-label="Weekly status day" onChange={(e) => setAutomation({ ...automation, weekly_status_day: Number(e.target.value) })}>
            {WEEKDAYS.map((d, n) => <option key={d} value={n}>{d}</option>)}
          </select>
          (needs the API key)
        </label>
        <div className="actions">
          <button className="primary" onClick={save}>Save</button>
          <button onClick={() => api.openWeeklyReports()}>Open weekly reports</button>
          {note && <span className="muted" role="status">{note}</span>}
        </div>
      </section>
    </>
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
        Find tickets in the timesheet asks these for your own tickets in the report range and suggests one for each entry
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
