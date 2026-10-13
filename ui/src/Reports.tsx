import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  dayKey,
  hhmm,
  longDay,
  minutes,
  shiftDay,
  stratum,
  type ContextFilter,
  type DaySchedule,
  type Grouping,
  type Report,
  type ReportEntry,
  type ReportPrefs,
  type ReportQuery,
  type Status,
  type FindResult,
  type Ticket,
  type TicketInfo,
  type TicketMode,
  type TogglProject,
  type TogglProjectInfo,
  type TrackerStatus,
} from "./api";

const RANGES: { id: string; label: string }[] = [
  { id: "this_week", label: "This week" },
  { id: "last_week", label: "Last week" },
  { id: "this_month", label: "This month" },
  { id: "last_month", label: "Last month" },
];
const GROUPINGS: { id: Grouping; label: string }[] = [
  { id: "ticket_day", label: "Per ticket" },
  { id: "session", label: "Per session" },
  { id: "project_day", label: "Per project" },
];
const TICKET_MODES: { id: TicketMode; label: string }[] = [
  { id: "prefix", label: "Ticket in description" },
  { id: "tag", label: "Ticket as tag" },
  { id: "off", label: "No tickets" },
];

/** Weeks start on Monday. */
function rangeDays(range: string): [string, string] {
  const now = new Date();
  const today = dayKey(now);
  const monday = shiftDay(today, -((now.getDay() + 6) % 7));
  const firstOfMonth = dayKey(new Date(now.getFullYear(), now.getMonth(), 1));
  switch (range) {
    case "last_week":
      return [shiftDay(monday, -7), shiftDay(monday, -1)];
    case "this_month":
      return [firstOfMonth, today];
    case "last_month":
      return [dayKey(new Date(now.getFullYear(), now.getMonth() - 1, 1)), shiftDay(firstOfMonth, -1)];
    default:
      return [monday, today];
  }
}

type Props = { status: Status; side: ContextFilter; onSaved: () => void; onOpenDay: (day: string) => void; goToSources: () => void };

export default function Reports({ status, side, onSaved, onOpenDay, goToSources }: Props) {
  const [prefs, setPrefs] = useState<ReportPrefs>(status.settings.report_prefs);
  const [report, setReport] = useState<Report | null>(null);
  const [unlabelled, setUnlabelled] = useState<string[]>([]);
  const [schedule, setSchedule] = useState<DaySchedule[]>([]);
  const [labelling, setLabelling] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showMapping, setShowMapping] = useState(false);
  const [trackers, setTrackers] = useState<TrackerStatus | null>(null);
  const [finding, setFinding] = useState(false);

  useEffect(() => {
    api.trackerStatus().then(setTrackers).catch(() => setTrackers(null));
  }, []);

  const [from, to] = rangeDays(prefs.range);
  const query: ReportQuery = useMemo(
    () => ({ from, to, grouping: prefs.grouping, tickets: prefs.tickets, context: side }),
    [from, to, prefs.grouping, prefs.tickets, side],
  );

  const load = useCallback(() => {
    setError(null);
    api.report(query).then(setReport).catch((e) => setError(String(e)));
    api.daysNeedingLabels(query).then(setUnlabelled);
    api.reportDays(query).then(setSchedule);
  }, [query]);
  useEffect(load, [load]);

  /** The last applied options become the saved preference. */
  const applyPrefs = async (next: ReportPrefs) => {
    setPrefs(next);
    await api.patchSettings({ report_prefs: { ...next, context: side } });
    onSaved();
  };

  const run = async (fn: () => Promise<string | void>) => {
    setError(null);
    setNotice(null);
    try {
      const msg = await fn();
      if (msg) setNotice(msg);
    } catch (e) {
      setError(String(e));
    }
    load();
  };

  const labelDays = () =>
    run(async () => {
      for (const d of unlabelled) {
        setLabelling(d);
        await api.generateJournal(d);
      }
      setLabelling(null);
      return `Labelled ${unlabelled.length} ${unlabelled.length === 1 ? "day" : "days"} and wrote their journal entries.`;
    }).finally(() => setLabelling(null));

  const findTickets = () => {
    setFinding(true);
    run(async () => findSummary(await api.findTickets(query))).finally(() => setFinding(false));
  };

  const suggested = (report?.entries ?? []).filter((e) => e.suggestion);
  const acceptAll = () =>
    run(async () => {
      for (const e of suggested) await api.setTicket(e.session_ids, e.suggestion!.info.key);
      return `Set ${suggested.length} suggested ${suggested.length === 1 ? "ticket" : "tickets"}.`;
    });

  const byDay = useMemo(() => {
    const m = new Map<string, ReportEntry[]>();
    for (const e of report?.entries ?? []) m.set(e.day, [...(m.get(e.day) ?? []), e]);
    return [...m.entries()];
  }, [report]);

  return (
    <div className="reports">
      <header className="day-head">
        <h1>Reports</h1>
        <p className="day-total">
          {side === "all" ? "Work and personal" : side === "work" ? "Work" : "Personal"}, {longDay(from)} to {longDay(to)}
        </p>
      </header>

      <div className="report-controls">
        <Seg label="Range" options={RANGES} value={prefs.range} onChange={(range) => applyPrefs({ ...prefs, range })} />
        <Seg label="Entries" options={GROUPINGS} value={prefs.grouping} onChange={(grouping) => applyPrefs({ ...prefs, grouping })} />
        <Seg label="Tickets" options={TICKET_MODES} value={prefs.tickets} onChange={(tickets) => applyPrefs({ ...prefs, tickets })} />
      </div>

      {report && <Summary report={report} />}

      <div className="report-actions">
        <button onClick={() => run(async () => `Confirmed ${await api.approveConfident(query)} high-confidence sessions.`)}>
          Approve confident sessions
        </button>
        {unlabelled.length > 0 &&
          (status.has_api_key ? (
            <button disabled={!!labelling} onClick={labelDays}>
              {labelling ? `Labelling ${labelling}…` : `Label ${unlabelled.length} ${unlabelled.length === 1 ? "day" : "days"} with AI`}
            </button>
          ) : (
            <button onClick={goToSources}>Add an API key to label sessions</button>
          ))}
        {schedule.some((d) => d.pending_bridge_min > 0) && (
          <button onClick={() => run(async () => `Approved ${await api.approveBridges(from, to)} bridged gaps.`)}>
            Approve bridges ({minutes(schedule.reduce((n, d) => n + d.pending_bridge_min, 0))})
          </button>
        )}
        {trackers && (trackers.jira_connected || (trackers.github_enabled && trackers.github_accounts.length > 0)) ? (
          <button disabled={finding} onClick={findTickets}>{finding ? "Looking for tickets…" : "Find tickets"}</button>
        ) : (
          <button onClick={goToSources}>Connect Jira or GitHub to find tickets</button>
        )}
        {suggested.length > 1 && <button onClick={acceptAll}>Use {suggested.length} suggested tickets</button>}
        <button className="link" onClick={() => setShowMapping(!showMapping)}>
          {showMapping ? "Hide Toggl projects" : "Toggl projects"}
        </button>
        <span className="spacer" />
        {report && <ExportButtons report={report} query={query} hasToken={status.has_toggl_token} run={run} goToSources={goToSources} />}
      </div>
      {notice && <p className="notice">{notice}</p>}
      {error && <p className="error">{error}</p>}

      {showMapping && report && <MappingPanel status={status} report={report} onSaved={() => { onSaved(); load(); }} />}

      {report && report.entries.length === 0 && <p className="empty">No sessions in this range.</p>}
      {byDay.map(([day, entries]) => (
        <section className="report-day" key={day}>
          <h2>
            <button className="link" onClick={() => onOpenDay(day)}>{longDay(day)}</button>
            <span className="num">{minutes(entries.filter((e) => !e.skip).reduce((n, e) => n + e.duration_min, 0))}</span>
          </h2>
          {schedule.find((d) => d.day === day) && (
            <DayAgainstSchedule d={schedule.find((d) => d.day === day)!} fillDefault={status.settings.schedule?.fill_project ?? null}
              projects={[...new Set(entries.map((e) => e.project).filter((p): p is string => !!p))]} run={run} />
          )}
          <ol className="entries">
            {entries.map((e) => (
              <EntryRow key={e.key} e={e} grouping={prefs.grouping} run={run} />
            ))}
          </ol>
        </section>
      ))}
    </div>
  );
}

/** One line on what "Find tickets" did, with where it looked and what failed. */
function findSummary(r: FindResult): string {
  const where = r.looked_in.length ? ` Looked in ${r.looked_in.join("; ")}.` : "";
  const failed = r.errors.length ? ` ${r.errors.join(" ")}` : "";
  if (r.checked === 0) return "Every entry already has a ticket.";
  const ai = r.by_ai ? `, ${r.by_ai} picked by AI between close matches` : "";
  return `Suggested tickets for ${r.suggested} of ${r.checked} entries without one${ai}.${where}${failed}`;
}

function Seg<T extends string>({ label, options, value, onChange }: { label: string; options: { id: T; label: string }[]; value: T; onChange: (v: T) => void }) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.id} className={o.id === value ? "on" : ""} onClick={() => onChange(o.id)}>{o.label}</button>
      ))}
    </div>
  );
}

function Summary({ report }: { report: Report }) {
  const total = Math.max(1, report.total_min);
  return (
    <section className="summary-strip">
      <p>
        <b className="num">{minutes(report.total_min)}</b> to report, <b className="num">{minutes(report.approved_min)}</b> approved
        {report.review_count > 0 && <>, {report.review_count} {report.review_count === 1 ? "entry needs" : "entries need"} review</>}.
        {report.parallel_min > 0 && (
          <span className="muted"> Parallel agent sessions shared {minutes(report.parallel_min)}, counted once.</span>
        )}
      </p>
      <div className="stack" aria-label="Time by project">
        {report.by_project.map((p) => (
          <span key={p.project} title={`${p.project}: ${minutes(p.min)}`} style={{ flexGrow: p.min / total, background: stratum(p.project) }} />
        ))}
      </div>
      <ul className="legend">
        {report.by_project.slice(0, 8).map((p) => (
          <li key={p.project}>
            <i style={{ background: stratum(p.project) }} />
            {p.project} <span className="num muted">{minutes(p.min)}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

type Run = (fn: () => Promise<string | void>) => Promise<void>;

function EntryRow({ e, grouping, run }: { e: ReportEntry; grouping: Grouping; run: Run }) {
  const [label, setLabel] = useState(e.label);
  const [editing, setEditing] = useState<"jira" | "github" | null>(null);
  useEffect(() => setLabel(e.label), [e.label]);

  const single = grouping === "session" && e.session_ids.length === 1;
  const saveLabel = () => {
    if (label.trim() && label !== e.label) run(() => api.setEntryLabel(e.session_ids[0], label.trim()));
  };
  const saveTicket = (value: string | null) => {
    const slot = editing;
    setEditing(null);
    run(() => (slot === "github" ? api.setGithubRef(e.session_ids, value) : api.setTicket(e.session_ids, value)));
  };

  return (
    <li className={`report-entry ${e.approved ? "approved" : ""} ${e.skip ? "skipped" : ""}`}>
      <button
        className="approve"
        aria-label={e.approved ? "Approved" : "Approve this entry"}
        aria-pressed={e.approved}
        disabled={e.approved}
        onClick={() => run(() => api.approveSessions(e.session_ids))}
      >
        {e.approved ? "✓" : ""}
      </button>
      <span className="when num">
        {hhmm(e.start_ts)}
        <b>{minutes(e.duration_min)}</b>
      </span>
      <span className="body">
        <span className="line1">
          <span className="proj" style={{ color: stratum(e.project) }}>{e.toggl_project || "No project"}</span>
          {editing === "jira" ? (
            <TicketEditor initial={e.jira?.key ?? ""} placeholder="SHOP-1261 or repo#12" onSave={saveTicket} onCancel={() => setEditing(null)} />
          ) : e.jira ? (
            <TicketChip ticket={e.jira} info={e.jira_info} run={run} onEdit={() => setEditing("jira")} />
          ) : e.suggestion ? (
            <SuggestionChip e={e} run={run} onOther={() => setEditing("jira")} />
          ) : (
            <button className="ticket add" onClick={() => setEditing("jira")}>{e.github ? "Add Jira ticket" : "Add ticket"}</button>
          )}
          {editing === "github" ? (
            <TicketEditor initial={e.github?.key ?? ""} placeholder="repo#12" onSave={saveTicket} onCancel={() => setEditing(null)} />
          ) : (
            e.github && <TicketChip ticket={e.github} info={e.github_info} run={run} onEdit={() => setEditing("github")} />
          )}
          {e.sent && <span className="tag ok">In Toggl</span>}
          {e.context === "personal" && <span className="tag">Personal, not sent to Toggl</span>}
          {e.skip && e.context === "work" && <span className="tag">Not sent to Toggl</span>}
          {e.billable && !e.skip && <span className="tag">Billable</span>}
        </span>
        {single ? (
          <input className="desc" value={label} aria-label="Description" onChange={(ev) => setLabel(ev.target.value)} onBlur={saveLabel}
            onKeyDown={(ev) => ev.key === "Enter" && (ev.target as HTMLInputElement).blur()} />
        ) : (
          <span className="desc-text">{e.label}</span>
        )}
        {(e.reasons.length > 0 || e.parallel_min > 0 || e.gap_min > 0) && (
          <span className="chips">
            {e.reasons.map((r) => (
              <span key={r} className={`tag ${r === "Low confidence" || r === "No project" ? "low" : ""}`}>{r}</span>
            ))}
            {e.parallel_min > 0 && <span className="chip">{minutes(e.parallel_min)} shared with parallel work</span>}
            {e.gap_min > 0 && <span className="chip">includes {minutes(e.gap_min)} filled</span>}
          </span>
        )}
      </span>
      {grouping !== "session" && e.session_ids.length > 1 && <span className="muted count">{e.session_ids.length} sessions</span>}
    </li>
  );
}

/**
 * A ticket chip: the key, with the title and epic on hover. Click to change it; "Open" opens it.
 * @param ticket the Jira ticket or the GitHub ref of an entry.
 * @param info the stored title, link and epic, when a tracker returned them.
 */
function TicketChip({ ticket, info, run, onEdit }: { ticket: Ticket; info: TicketInfo | null; run: Run; onEdit: () => void }) {
  const github = ticket.key.includes("#");
  const epic = info?.epic;
  const title = [info?.title, epic && `Epic ${epic.key}: ${epic.title}`, `Found in ${ticket.source}. Click to change.`].filter(Boolean).join("\n");
  return (
    <>
      <button className={`ticket ${github ? "github" : ""}`} title={title} onClick={onEdit}>{ticket.key}</button>
      {epic && <span className="tag epic" title={`Epic ${epic.key}`}>{epic.title || epic.key}</span>}
      {info?.url && <button className="link small" onClick={() => run(() => api.openTicket(ticket.key))}>Open</button>}
    </>
  );
}

/** A key input with Set, Auto (back to detection) and Escape to cancel. */
function TicketEditor({ initial, placeholder, onSave, onCancel }: {
  initial: string;
  placeholder: string;
  onSave: (value: string | null) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <span className="ticket-edit">
      <input autoFocus value={value} placeholder={placeholder} onChange={(ev) => setValue(ev.target.value)}
        onKeyDown={(ev) => { if (ev.key === "Enter") onSave(value.trim()); if (ev.key === "Escape") onCancel(); }} />
      <button onClick={() => onSave(value.trim())}>Set</button>
      <button onClick={() => onSave(null)} title="Find it from evidence again">Auto</button>
    </span>
  );
}

/**
 * A ticket "Find tickets" proposed: use it, say no, or type another.
 * Next to a GitHub ref it is always a Jira key; using it keeps the GitHub ref.
 */
function SuggestionChip({ e, run, onOther }: { e: ReportEntry; run: Run; onOther: () => void }) {
  const s = e.suggestion!;
  const epic = s.info.epic ? `\nEpic ${s.info.epic.key}: ${s.info.epic.title}` : "";
  return (
    <span className="suggestion">
      <button className="ticket suggested" title={`Suggested: ${s.reason}.${epic}\nClick to open.`} onClick={() => run(() => api.openTicket(s.info.key))}>
        {s.info.key}
      </button>
      {s.by_ai && <span className="tag">AI</span>}
      {s.info.title && <span className="sugg-title" title={s.reason}>{s.info.title}</span>}
      <button className="small" onClick={() => run(() => api.setTicket(e.session_ids, s.info.key))}>Use</button>
      <button className="small" onClick={() => run(() => api.dismissSuggestion(e.session_ids))}>Not this</button>
      <button className="link small" onClick={onOther}>Other</button>
    </span>
  );
}

function ExportButtons({ report, query, hasToken, run, goToSources }: { report: Report; query: ReportQuery; hasToken: boolean; run: Run; goToSources: () => void }) {
  const [confirming, setConfirming] = useState(false);
  const ready = report.entries.filter((e) => e.approved && !e.skip);
  const unsent = ready.filter((e) => !e.sent);
  const unsentMin = unsent.reduce((n, e) => n + e.duration_min, 0);

  return (
    <span className="export">
      <button disabled={!ready.length} onClick={() => run(async () => `Saved ${await api.exportTogglCsv(query)}`)}>
        Export CSV
      </button>
      {!hasToken ? (
        <button onClick={goToSources}>Connect Toggl</button>
      ) : confirming ? (
        <>
          <button className="primary" onClick={() => { setConfirming(false); run(async () => {
            const r = await api.sendToToggl(query);
            return `Sent ${r.sent} ${r.sent === 1 ? "entry" : "entries"} to Toggl.${r.failed.length ? ` ${r.failed.length} failed: ${r.failed[0]}` : ""}`;
          }); }}>
            Send {unsent.length} ({minutes(unsentMin)})
          </button>
          <button onClick={() => setConfirming(false)}>Cancel</button>
        </>
      ) : (
        <button className="primary" disabled={!unsent.length} onClick={() => setConfirming(true)}>
          Send to Toggl
        </button>
      )}
    </span>
  );
}

/** Local project → Toggl project, client, billable, or "never send". */
function MappingPanel({ status, report, onSaved }: { status: Status; report: Report; onSaved: () => void }) {
  const settings = status.settings;
  const local = [...new Set(report.entries.map((e) => e.project).filter((p): p is string => !!p))].sort();
  const [map, setMap] = useState<Record<string, TogglProject>>(settings.toggl.project_map);
  const [remote, setRemote] = useState<TogglProjectInfo[]>([]);

  useEffect(() => {
    if (status.has_toggl_token) api.togglProjects().then(setRemote).catch(() => setRemote([]));
  }, [status.has_toggl_token]);

  const row = (p: string): TogglProject => map[p] ?? { name: p, id: null, client: "", billable: settings.toggl.default_billable, skip: false };
  const set = (p: string, patch: Partial<TogglProject>) => {
    const next = { ...row(p), ...patch };
    if (patch.name !== undefined) next.id = remote.find((r) => r.name === patch.name)?.id ?? null;
    setMap({ ...map, [p]: next });
  };
  const save = async () => {
    await api.patchSettings({ toggl: { ...settings.toggl, project_map: map } });
    onSaved();
  };

  return (
    <section className="mapping">
      <p className="muted">
        Each local project becomes a Toggl project. {remote.length > 0 ? "Pick from your workspace or type a name." : "Type the Toggl project name."}
      </p>
      <datalist id="toggl-projects">
        {remote.map((r) => <option key={r.id} value={r.name} />)}
      </datalist>
      <table>
        <thead>
          <tr><th>Local project</th><th>Toggl project</th><th>Client</th><th>Billable</th><th>Never send</th></tr>
        </thead>
        <tbody>
          {local.map((p) => (
            <tr key={p}>
              <td><span className="proj" style={{ color: stratum(p) }}>{p}</span></td>
              <td><input list="toggl-projects" value={row(p).name} onChange={(e) => set(p, { name: e.target.value })} /></td>
              <td><input value={row(p).client} onChange={(e) => set(p, { client: e.target.value })} /></td>
              <td><input type="checkbox" checked={row(p).billable} onChange={(e) => set(p, { billable: e.target.checked })} /></td>
              <td><input type="checkbox" checked={row(p).skip} onChange={(e) => set(p, { skip: e.target.checked })} /></td>
            </tr>
          ))}
        </tbody>
      </table>
      <button className="primary" onClick={save}>Save projects</button>
    </section>
  );
}

/** One day measured against the schedule, with "Fill to schedule" for open blocks. */
function DayAgainstSchedule({ d, fillDefault, projects, run }: { d: DaySchedule; fillDefault: string | null; projects: string[]; run: Run }) {
  const [picking, setPicking] = useState(false);
  const [project, setProject] = useState(fillDefault ?? projects[0] ?? "");
  const total = Math.max(1, d.schedule_min, d.observed_min + d.filled_min + d.unaccounted_min);
  return (
    <div className="day-schedule">
      <span className="bar" aria-hidden>
        <i className="o" style={{ flexGrow: d.observed_min / total }} />
        <i className="f" style={{ flexGrow: (d.filled_min + d.pending_bridge_min) / total }} />
        <i className="u" style={{ flexGrow: d.unaccounted_min / total }} />
      </span>
      <span>
        {minutes(d.observed_min)} observed
        {d.filled_min > 0 && `, ${minutes(d.filled_min)} filled`}
        {d.pending_bridge_min > 0 && `, ${minutes(d.pending_bridge_min)} bridged to approve`}
        {d.unaccounted_min > 0 && <>, <span className="warn">{minutes(d.unaccounted_min)} unaccounted</span></>}
        {` of ${minutes(d.schedule_min)}`}
      </span>
      {d.unaccounted_min > 0 &&
        (picking ? (
          <span className="fill-picker">
            <input list="fill-day-projects" value={project} onChange={(e) => setProject(e.target.value)} placeholder="Project" />
            <datalist id="fill-day-projects">{projects.map((p) => <option key={p} value={p} />)}</datalist>
            <button className="primary" disabled={!project.trim()} onClick={() => { setPicking(false); run(async () => `Filled ${minutes(await api.fillToSchedule(d.day, project.trim()))} on ${d.day}.`); }}>
              Fill {minutes(d.unaccounted_min)}
            </button>
            <button onClick={() => setPicking(false)}>Cancel</button>
          </span>
        ) : (
          <button onClick={() => setPicking(true)}>Fill to schedule…</button>
        ))}
    </div>
  );
}
