import { useCallback, useEffect, useMemo, useState, type ReactElement } from "react";
import {
  api,
  blocks,
  clock,
  dayKey,
  hhmm,
  longDay,
  minutes,
  shiftDay,
  stratum,
  type ContextFilter,
  type DaySchedule,
  type FindResult,
  type Grouping,
  type Report,
  type ReportEntry,
  type ReportPrefs,
  type ReportQuery,
  type Status,
  type Suggestion,
  type Ticket,
  type TicketInfo,
  type TicketMode,
  type TicketSyncStatus,
  type TrackerStatus,
} from "./api";
import type { SettingsTab } from "./Settings";

const PRESETS: { id: string; label: string }[] = [
  { id: "today", label: "Today" },
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
  { id: "epic_ticket", label: "Epic and ticket" },
  { id: "prefix", label: "Ticket first" },
  { id: "tag", label: "Ticket as tag" },
  { id: "off", label: "No tickets" },
];
const DRIFTS = [0, 15, 30, 45, 60];

type Period = { from: string; to: string; label: string };

/**
 * The days a preset covers, moved `offset` days, weeks or months from it. Weeks start on Monday;
 * a period reaching past today ends today.
 */
function period(preset: string, offset: number): Period {
  const now = new Date();
  const today = dayKey(now);
  const clampToday = (d: string) => (d > today ? today : d);
  if (preset === "today") {
    const d = shiftDay(today, offset);
    return { from: d, to: d, label: offset === 0 ? "Today" : offset === -1 ? "Yesterday" : shortDay(d) };
  }
  if (preset.endsWith("_month")) {
    const n = offset + (preset === "last_month" ? -1 : 0);
    const first = new Date(now.getFullYear(), now.getMonth() + n, 1);
    const last = new Date(now.getFullYear(), now.getMonth() + n + 1, 0);
    const label = n === 0 ? "This month" : n === -1 ? "Last month" : first.toLocaleDateString([], { month: "long", year: "numeric" });
    return { from: dayKey(first), to: clampToday(dayKey(last)), label };
  }
  const n = offset + (preset === "last_week" ? -1 : 0);
  const monday = shiftDay(shiftDay(today, -((now.getDay() + 6) % 7)), n * 7);
  const label = n === 0 ? "This week" : n === -1 ? "Last week" : `Week of ${shortDay(monday)}`;
  return { from: monday, to: clampToday(shiftDay(monday, 6)), label };
}

function shortDay(day: string): string {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], { day: "numeric", month: "short" });
}

/** Work days (0 = Monday) between `from` and `to`, inclusive. */
function workDaysIn(from: string, to: string, days: number[]): number {
  let n = 0;
  for (let d = from; d <= to; d = shiftDay(d, 1)) {
    const [y, m, dd] = d.split("-").map(Number);
    if (days.includes((new Date(y, m - 1, dd).getDay() + 6) % 7)) n++;
  }
  return n;
}

type Props = {
  status: Status;
  side: ContextFilter;
  onSaved: () => void;
  onOpenDay: (day: string) => void;
  goToSettings: (tab: SettingsTab) => void;
  onReviewCount: (n: number) => void;
};

type Run = (fn: () => Promise<string | void>) => Promise<void>;

/**
 * The timesheet: a period's entries as they will go to Toggl, day by day, with what still needs
 * the user on top and export on the toolbar.
 */
export default function Timesheet({ status, side, onSaved, onOpenDay, goToSettings, onReviewCount }: Props) {
  const [prefs, setPrefs] = useState<ReportPrefs>(status.settings.report_prefs);
  const [offset, setOffset] = useState(0);
  const [report, setReport] = useState<Report | null>(null);
  const [unlabelled, setUnlabelled] = useState<string[]>([]);
  const [schedule, setSchedule] = useState<DaySchedule[]>([]);
  const [away, setAway] = useState<Record<string, [string, string][]>>({});
  const [trackers, setTrackers] = useState<TrackerStatus | null>(null);
  const [lastSync, setLastSync] = useState<TicketSyncStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.trackerStatus().then(setTrackers).catch(() => setTrackers(null));
  }, []);

  const { from, to, label } = period(prefs.range, offset);
  const query: ReportQuery = useMemo(
    () => ({
      from,
      to,
      grouping: prefs.grouping,
      tickets: prefs.tickets,
      context: side,
      fit_day_min: prefs.fit_day ? Math.round(prefs.fit_hours * 60) : null,
      fit_drift_min: prefs.fit_day ? prefs.fit_drift_min : null,
    }),
    [from, to, prefs.grouping, prefs.tickets, side, prefs.fit_day, prefs.fit_hours, prefs.fit_drift_min],
  );

  const load = useCallback(() => {
    setError(null);
    api.report(query).then((r) => {
      setReport(r);
      onReviewCount(r.review_count);
    }).catch((e) => setError(String(e)));
    api.daysNeedingLabels(query).then(setUnlabelled);
    api.reportDays(query).then(setSchedule);
    api.awayDays(query.from, query.to).then(setAway);
  }, [query, onReviewCount]);
  useEffect(load, [load]);
  useSyncWatch(setLastSync, load);

  /** The last applied options become the saved preference. */
  const applyPrefs = async (next: ReportPrefs) => {
    setPrefs(next);
    await api.patchSettings({ report_prefs: { ...next, context: side } });
    onSaved();
  };

  const run: Run = async (fn) => {
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
  const runBusy = (what: string, fn: () => Promise<string | void>) => {
    setBusy(what);
    return run(fn).finally(() => setBusy(null));
  };

  const byDay = useMemo(() => {
    const m = new Map<string, ReportEntry[]>();
    for (const e of report?.entries ?? []) m.set(e.day, [...(m.get(e.day) ?? []), e]);
    return [...m.entries()].sort(([a], [b]) => b.localeCompare(a));
  }, [report]);

  const workDays = status.settings.schedule?.days ?? [0, 1, 2, 3, 4];
  const targetMin = prefs.fit_day ? Math.round(prefs.fit_hours * 60) * workDaysIn(from, to, workDays) : null;

  return (
    <div className="page timesheet">
      <header className="toolbar">
        <div className="range-nav">
          <button className="icon" aria-label="Earlier" onClick={() => setOffset(offset - 1)}>‹</button>
          <button className="icon" aria-label="Later" disabled={to >= dayKey(new Date())} onClick={() => setOffset(offset + 1)}>›</button>
          <h1>{label}</h1>
          <span className="muted num">{from === to ? longDay(from) : `${shortDay(from)} – ${shortDay(to)}`}</span>
          <select aria-label="Period" value={prefs.range} onChange={(e) => { setOffset(0); applyPrefs({ ...prefs, range: e.target.value }); }}>
            {PRESETS.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
          </select>
        </div>
        {report && (
          <ExportButtons report={report} query={query} hasToken={status.has_toggl_token} run={run} goToSettings={goToSettings} />
        )}
      </header>

      {report && <PeriodSummary report={report} days={byDay} targetMin={targetMin} />}

      <ViewOptions prefs={prefs} onChange={applyPrefs} />

      {report && (
        <NeedsYou
          report={report}
          query={query}
          unlabelled={unlabelled}
          schedule={schedule}
          hasKey={status.has_api_key}
          canFind={!!trackers && (trackers.jira_connected || (trackers.github_enabled && trackers.github_accounts.length > 0))}
          busy={busy}
          runBusy={runBusy}
          goToSettings={goToSettings}
        />
      )}
      {lastSync && <SyncLine s={lastSync} />}
      {notice && <p className="notice" role="status">{notice}</p>}
      {error && <p className="error" role="alert">{error}</p>}

      {report && report.entries.length === 0 && (
        <div className="empty">
          <b>Nothing recorded in this period.</b>
          <span>Pick another period, or check that the app's sources are on in Settings › Collection.</span>
        </div>
      )}
      {byDay.map(([day, entries]) => (
        <DaySection
          key={day}
          day={day}
          entries={entries}
          grouping={prefs.grouping}
          away={away[day] ?? []}
          schedule={schedule.find((d) => d.day === day) ?? null}
          fillDefault={status.settings.schedule?.fill_project ?? null}
          run={run}
          onOpenDay={onOpenDay}
        />
      ))}
    </div>
  );
}

/** Time reported against the target, a bar per day that jumps to it, and time per project. */
function PeriodSummary({ report, days, targetMin }: { report: Report; days: [string, ReportEntry[]][]; targetMin: number | null }) {
  const total = report.total_min;
  const perDay = days.map(([day, entries]) => [day, entries.filter((e) => !e.skip).reduce((n, e) => n + e.duration_min, 0)] as const).reverse();
  const tallest = Math.max(1, ...perDay.map(([, m]) => m));
  const measured = report.entries.filter((e) => !e.skip).reduce((n, e) => n + e.measured_min, 0);
  return (
    <section className="period">
      <div className="period-total">
        <span className="big num">{clock(total)}</span>
        <span className="muted">
          {targetMin ? (total >= targetMin ? `${minutes(total - targetMin)} over ${minutes(targetMin)}` : `${minutes(targetMin - total)} to go of ${minutes(targetMin)}`) : "reported"}
          {measured !== total && ` · measured ${minutes(measured)}`}
          {report.parallel_min > 0 && ` · ${minutes(report.parallel_min)} of parallel agents counted once`}
        </span>
      </div>
      {perDay.length > 1 && (
        <ol className="day-bars" aria-label="Time per day">
          {perDay.map(([day, min]) => (
            <li key={day}>
              <a href={`#day-${day}`} title={`${longDay(day)}: ${minutes(min)}`}>
                <span className="num">{(min / 60).toFixed(min % 60 ? 2 : 0)}h</span>
                <i style={{ height: `${Math.max(4, (min / tallest) * 48)}px` }} />
                <small>{new Date(`${day}T12:00`).toLocaleDateString([], { weekday: "short" })}</small>
              </a>
            </li>
          ))}
        </ol>
      )}
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

/**
 * Fit, drift, grouping and ticket wording, folded into one line that says what is in effect.
 * Fitting scales each work day to N hours in the timesheet, the CSV and Toggl; drift lets days
 * move up or down around N while the period keeps its total.
 */
function ViewOptions({ prefs, onChange }: { prefs: ReportPrefs; onChange: (next: ReportPrefs) => void }) {
  const [hours, setHours] = useState(String(prefs.fit_hours));
  const commit = () => {
    const n = Number(hours);
    if (n >= 0.5 && n <= 24 && n !== prefs.fit_hours) onChange({ ...prefs, fit_hours: n });
    else setHours(String(prefs.fit_hours));
  };
  const fit = prefs.fit_day ? `Days fitted to ${prefs.fit_hours} h${prefs.fit_drift_min ? ` ± ${prefs.fit_drift_min} min` : ""}` : "Measured time";
  const summary = [fit, GROUPINGS.find((g) => g.id === prefs.grouping)?.label, TICKET_MODES.find((t) => t.id === prefs.tickets)?.label].join(" · ");
  return (
    <details className="view-options">
      <summary>{summary}</summary>
      <div className="options-grid">
        <label className="option">
          <input type="checkbox" role="switch" checked={prefs.fit_day} onChange={(e) => onChange({ ...prefs, fit_day: e.target.checked })} />
          <span>Fit work days to</span>
          <input type="number" min={0.5} max={24} step={0.25} value={hours} aria-label="Hours per day"
            onChange={(e) => setHours(e.target.value)} onBlur={commit} onKeyDown={(e) => e.key === "Enter" && commit()} />
          <span>h</span>
        </label>
        <label className="option">
          <span>Let days drift</span>
          <select value={prefs.fit_drift_min} disabled={!prefs.fit_day} onChange={(e) => onChange({ ...prefs, fit_drift_min: Number(e.target.value) })}>
            {DRIFTS.map((d) => <option key={d} value={d}>{d ? `± ${d} min` : "No drift"}</option>)}
          </select>
        </label>
        <Seg label="Entries" options={GROUPINGS} value={prefs.grouping} onChange={(grouping) => onChange({ ...prefs, grouping })} />
        <Seg label="Descriptions" options={TICKET_MODES} value={prefs.tickets} onChange={(tickets) => onChange({ ...prefs, tickets })} />
      </div>
    </details>
  );
}

/** What stands between the period and Toggl, each with the one action that clears it. */
function NeedsYou({ report, query, unlabelled, schedule, hasKey, canFind, busy, runBusy, goToSettings }: {
  report: Report;
  query: ReportQuery;
  unlabelled: string[];
  schedule: DaySchedule[];
  hasKey: boolean;
  canFind: boolean;
  busy: string | null;
  runBusy: (what: string, fn: () => Promise<string | void>) => Promise<void>;
  goToSettings: (tab: SettingsTab) => void;
}) {
  const unapproved = report.entries.filter((e) => !e.approved && !e.skip && e.context === "work");
  const suggested = report.entries.filter((e) => e.suggestion && !e.jira);
  const bridgeMin = schedule.reduce((n, d) => n + d.pending_bridge_min, 0);
  const items: ReactElement[] = [];

  if (unlabelled.length > 0)
    items.push(
      <li key="labels">
        <span><b>{unlabelled.length}</b> {unlabelled.length === 1 ? "day has" : "days have"} sessions without AI labels</span>
        {hasKey ? (
          <button disabled={!!busy} onClick={() => runBusy("labels", async () => {
            for (const d of unlabelled) await api.generateJournal(d);
            return `Labelled ${unlabelled.length} ${unlabelled.length === 1 ? "day" : "days"} and wrote their journal entries.`;
          })}>{busy === "labels" ? "Labelling…" : "Label with AI"}</button>
        ) : (
          <button onClick={() => goToSettings("ai")}>Add an API key</button>
        )}
      </li>,
    );
  if (suggested.length > 0)
    items.push(
      <li key="suggested">
        <span><b>{suggested.length}</b> suggested {suggested.length === 1 ? "ticket" : "tickets"} to check</span>
        <button disabled={!!busy} onClick={() => runBusy("suggested", async () => {
          for (const e of suggested) await api.setTicket(e.session_ids, e.suggestion!.info.key);
          return `Set ${suggested.length} suggested ${suggested.length === 1 ? "ticket" : "tickets"}.`;
        })}>Use all</button>
      </li>,
    );
  if (bridgeMin > 0)
    items.push(
      <li key="bridges">
        <span><b>{minutes(bridgeMin)}</b> of bridged breaks to approve</span>
        <button disabled={!!busy} onClick={() => runBusy("bridges", async () => `Approved ${await api.approveBridges(query.from, query.to)} bridged gaps.`)}>Approve</button>
      </li>,
    );
  if (unapproved.length > 0)
    items.push(
      <li key="approve">
        <span><b>{unapproved.length}</b> {unapproved.length === 1 ? "entry" : "entries"} to approve before export</span>
        <button disabled={!!busy} onClick={() => runBusy("confident", async () => `Approved ${await api.approveConfident(query)} high-confidence sessions.`)}>Approve confident</button>
        <button disabled={!!busy} onClick={() => runBusy("all", async () => {
          await api.approveSessions(unapproved.flatMap((e) => e.session_ids));
          return `Approved ${unapproved.length} ${unapproved.length === 1 ? "entry" : "entries"}.`;
        })}>Approve all</button>
      </li>,
    );

  return (
    <section className={`needs ${items.length ? "" : "clear"}`} aria-label="Needs you">
      {items.length ? <ul>{items}</ul> : <p>Everything here is approved and ready for Toggl.</p>}
      {canFind ? (
        <button className="link" disabled={!!busy} onClick={() => runBusy("find", async () => findSummary(await api.findTickets(query)))}>
          {busy === "find" ? "Looking for tickets…" : "Find tickets"}
        </button>
      ) : (
        <button className="link" onClick={() => goToSettings("connections")}>Connect Jira or GitHub to find tickets</button>
      )}
    </section>
  );
}

/** One line on what "Find tickets" did, with where it looked and what failed. */
function findSummary(r: FindResult): string {
  const where = r.looked_in.length ? ` Looked in ${r.looked_in.join("; ")}.` : "";
  const failed = r.errors.length ? ` ${r.errors.join(" ")}` : "";
  if (r.checked === 0) return "Every entry already has a ticket.";
  const ai = r.by_ai ? `, ${r.by_ai} picked by AI between close matches` : "";
  const applied = r.applied ? ` ${r.applied} named outright were applied; "Not this" undoes one.` : "";
  return `Suggested tickets for ${r.suggested} of ${r.checked} entries without one${ai}.${applied}${where}${failed}`;
}

/**
 * Reads the last automatic ticket sync every minute and reloads the report when a new one lands.
 * @param onStatus receives each status read.
 * @param reload runs after a sync newer than the last one seen.
 */
function useSyncWatch(onStatus: (s: TicketSyncStatus | null) => void, reload: () => void) {
  useEffect(() => {
    let seen: number | null = null;
    const check = () =>
      api.ticketSyncStatus().then((s) => {
        onStatus(s);
        if (s && seen !== null && s.at_ms !== seen) reload();
        seen = s?.at_ms ?? 0;
      }).catch(() => onStatus(null));
    check();
    const timer = setInterval(check, 60_000);
    return () => clearInterval(timer);
  }, [onStatus, reload]);
}

/** When tickets last synced on their own, what changed, and what failed. */
function SyncLine({ s }: { s: TicketSyncStatus }) {
  const fresh = s.suggested - s.applied;
  const found = [s.applied > 0 && `${s.applied} applied`, fresh > 0 && `${fresh} new ${fresh === 1 ? "suggestion" : "suggestions"}`].filter(Boolean).join(", ");
  const what = s.checked === 0 ? "every entry had a ticket" : `${found || "nothing new"} for ${s.checked} without a ticket`;
  return (
    <p className="muted sync-line">
      Tickets synced at {hhmm(s.at_ms)}: {what}.
      {s.errors.length > 0 && <span className="error"> {s.errors.join(" ")}</span>}
    </p>
  );
}

function Seg<T extends string>({ label, options, value, onChange }: { label: string; options: { id: T; label: string }[]; value: T; onChange: (v: T) => void }) {
  return (
    <div className="option">
      <span>{label}</span>
      <div className="seg" role="group" aria-label={label}>
        {options.map((o) => (
          <button key={o.id} className={o.id === value ? "on" : ""} aria-pressed={o.id === value} onClick={() => onChange(o.id)}>{o.label}</button>
        ))}
      </div>
    </div>
  );
}

/** Open the timesheet page, save the CSV, or send approved entries to Toggl after a confirm. */
function ExportButtons({ report, query, hasToken, run, goToSettings }: { report: Report; query: ReportQuery; hasToken: boolean; run: Run; goToSettings: (tab: SettingsTab) => void }) {
  const [confirming, setConfirming] = useState(false);
  const ready = report.entries.filter((e) => e.approved && !e.skip);
  const unsent = ready.filter((e) => !e.sent);
  const unsentMin = unsent.reduce((n, e) => n + e.duration_min, 0);

  return (
    <div className="toolbar-actions">
      <button disabled={!report.entries.length} onClick={() => run(async () => `Opened ${await api.openTimesheet(query)}`)}>Open page</button>
      <button disabled={!ready.length} title={ready.length ? "Approved entries, as a Toggl CSV in Downloads" : "Approve entries first"}
        onClick={() => run(async () => `Saved ${await api.exportTogglCsv(query)}`)}>
        Export CSV
      </button>
      {!hasToken ? (
        <button onClick={() => goToSettings("connections")}>Connect Toggl</button>
      ) : confirming ? (
        <>
          <button onClick={() => setConfirming(false)}>Cancel</button>
          <button className="primary" onClick={() => { setConfirming(false); run(async () => {
            const r = await api.sendToToggl(query);
            return `Sent ${r.sent} ${r.sent === 1 ? "entry" : "entries"} to Toggl.${r.failed.length ? ` ${r.failed.length} failed: ${r.failed[0]}` : ""}`;
          }); }}>
            Send {unsent.length} ({minutes(unsentMin)})
          </button>
        </>
      ) : (
        <button className="primary" disabled={!unsent.length} onClick={() => setConfirming(true)}>Send to Toggl</button>
      )}
    </div>
  );
}

/** One day: its total, a timeline of its entries and away time, and its rows. */
function DaySection({ day, entries, grouping, away, schedule, fillDefault, run, onOpenDay }: {
  day: string;
  entries: ReportEntry[];
  grouping: Grouping;
  away: [string, string][];
  schedule: DaySchedule | null;
  fillDefault: string | null;
  run: Run;
  onOpenDay: (day: string) => void;
}) {
  const counted = entries.filter((e) => !e.skip);
  const reported = counted.reduce((n, e) => n + e.duration_min, 0);
  const measured = counted.reduce((n, e) => n + e.measured_min, 0);
  const rows = [...entries].sort((a, b) => a.start_ts - b.start_ts);
  return (
    <section className="ts-day" id={`day-${day}`}>
      <header>
        <h2>{longDay(day)}</h2>
        <button className="link" onClick={() => onOpenDay(day)}>Open day</button>
        <AwayControl day={day} spans={away} run={run} />
        <span className="spacer" />
        {measured !== reported && <span className="muted num" title="Before the day was fitted">measured {minutes(measured)}</span>}
        <b className="num">{clock(reported)}</b>
      </header>
      <DayTimeline day={day} entries={counted} away={away} />
      {schedule && schedule.unaccounted_min > 0 && (
        <FillToSchedule d={schedule} fillDefault={fillDefault} projects={[...new Set(entries.map((e) => e.project).filter((p): p is string => !!p))]} run={run} />
      )}
      <ol className="ts-rows">
        {rows.map((e) => <EntryRow key={e.key} e={e} grouping={grouping} run={run} />)}
      </ol>
    </section>
  );
}

/** Minutes since local midnight of `day` for `HH:MM`. */
function clockMin(t: string): number {
  const [h, m] = t.split(":").map(Number);
  return h * 60 + m;
}

/** The day's blocks on one strip, hours marked, with away time hatched. */
function DayTimeline({ day, entries, away }: { day: string; entries: ReportEntry[]; away: [string, string][] }) {
  const [y, m, d] = day.split("-").map(Number);
  const midnight = new Date(y, m - 1, d).getTime();
  const spans = entries.flatMap((e) => blocks(e).map((p) => ({ e, s: (p.start_ts - midnight) / 60_000, len: p.duration_min })));
  const awayMin = away.map(([s, t]) => ({ s: clockMin(s), len: clockMin(t) - clockMin(s) }));
  const all = [...spans, ...awayMin];
  if (!all.length) return null;
  const start = Math.floor(Math.min(...all.map((x) => x.s)) / 60) * 60;
  const end = Math.ceil(Math.max(...all.map((x) => x.s + x.len)) / 60) * 60;
  const width = Math.max(60, end - start);
  const pct = (min: number) => `${((min - start) / width) * 100}%`;
  const hours = Array.from({ length: width / 60 + 1 }, (_, i) => start + i * 60);
  return (
    <div className="ts-timeline" aria-hidden>
      {hours.map((h) => <span key={h} className="hour" style={{ left: pct(h) }}>{String(h / 60).padStart(2, "0")}</span>)}
      {awayMin.map((a, i) => <i key={`a${i}`} className="away" style={{ left: pct(a.s), width: `${(a.len / width) * 100}%` }} title="Away" />)}
      {spans.map(({ e, s, len }, i) => (
        <i key={i} className="block" style={{ left: pct(s), width: `${(len / width) * 100}%`, background: stratum(e.description) }} title={`${e.description} · ${minutes(len)}`} />
      ))}
    </div>
  );
}

/** The day's away time, and a small form to add a span or clear the day's. */
function AwayControl({ day, spans, run }: { day: string; spans: [string, string][]; run: Run }) {
  const [open, setOpen] = useState(false);
  const [start, setStart] = useState("12:00");
  const [end, setEnd] = useState("13:00");
  return (
    <span className="away-control">
      {spans.map(([s, e]) => <span key={s} className="tag">Away {s}–{e}</span>)}
      {spans.length > 0 && <button className="link small" onClick={() => run(async () => { await api.clearAway(day); return "Cleared the day's away time."; })}>Clear</button>}
      {open ? (
        <span className="inline-form">
          <input type="time" aria-label="Away from" value={start} onChange={(e) => setStart(e.target.value)} />
          <input type="time" aria-label="Away until" value={end} onChange={(e) => setEnd(e.target.value)} />
          <button className="small" onClick={() => { setOpen(false); run(async () => { await api.addAway(day, start, end); return `Marked ${start}–${end} away on ${longDay(day)}.`; }); }}>Add</button>
          <button className="small" onClick={() => setOpen(false)}>Cancel</button>
        </span>
      ) : (
        <button className="link small" onClick={() => setOpen(true)}>Add away time</button>
      )}
    </span>
  );
}

/** "Fill to schedule" for a day with unaccounted time inside work hours. */
function FillToSchedule({ d, fillDefault, projects, run }: { d: DaySchedule; fillDefault: string | null; projects: string[]; run: Run }) {
  const [picking, setPicking] = useState(false);
  const [project, setProject] = useState(fillDefault ?? projects[0] ?? "");
  return (
    <p className="fill-line">
      <span className="warn">{minutes(d.unaccounted_min)} unaccounted</span> inside your work hours.
      {picking ? (
        <span className="inline-form">
          <input list={`fill-${d.day}`} value={project} onChange={(e) => setProject(e.target.value)} placeholder="Project" />
          <datalist id={`fill-${d.day}`}>{projects.map((p) => <option key={p} value={p} />)}</datalist>
          <button className="small primary" disabled={!project.trim()} onClick={() => { setPicking(false); run(async () => `Filled ${minutes(await api.fillToSchedule(d.day, project.trim()))} on ${longDay(d.day)}.`); }}>
            Fill {minutes(d.unaccounted_min)}
          </button>
          <button className="small" onClick={() => setPicking(false)}>Cancel</button>
        </span>
      ) : (
        <button className="link small" onClick={() => setPicking(true)}>Fill to schedule…</button>
      )}
    </p>
  );
}

/**
 * An entry as a Toggl row: approve, description over its work label, project and tickets, time
 * range and duration. Clicking the row opens its details: tickets, label, and why it needs review.
 */
function EntryRow({ e, grouping, run }: { e: ReportEntry; grouping: Grouping; run: Run }) {
  const [open, setOpen] = useState(false);
  const pieces = blocks(e);
  const range = pieces.map((p) => `${hhmm(p.start_ts)}–${hhmm(p.start_ts + p.duration_min * 60_000)}`).join(", ");
  const flagged = e.reasons.some((r) => r === "Low confidence" || r === "No project") || (!!e.suggestion && !e.jira);
  return (
    <li className={`ts-row ${e.approved ? "approved" : ""} ${e.skip ? "skipped" : ""} ${open ? "open" : ""}`}>
      <div className="row-main">
        <button
          className="approve"
          aria-label={e.approved ? "Approved" : "Approve this entry"}
          aria-pressed={e.approved}
          disabled={e.approved || e.skip}
          onClick={() => run(() => api.approveSessions(e.session_ids))}
        >
          {e.approved ? "✓" : ""}
        </button>
        <button className="row-body" aria-expanded={open} onClick={() => setOpen(!open)}>
          <span className="desc-line">
            <i className="dot" style={{ background: stratum(e.description) }} />
            <b>{e.description || e.label}</b>
            {flagged && <span className="tag low" title={[...e.reasons, e.suggestion && !e.jira ? "A ticket is suggested" : ""].filter(Boolean).join(", ")}>Check</span>}
            {e.sent && <span className="tag ok">In Toggl</span>}
            {e.context === "personal" && <span className="tag">Personal</span>}
            {e.skip && e.context === "work" && <span className="tag">Never sent</span>}
          </span>
          <span className="sub-line">
            <span className="proj">{e.toggl_project || "No project"}</span>
            {e.project && e.project !== e.toggl_project && <span> · {e.project}</span>}
            {e.label && e.label !== e.description && <span className="label"> · {e.label}</span>}
          </span>
        </button>
        <span className="row-time num">
          <span className="range">{range}</span>
          {e.measured_min !== e.duration_min && <small className="muted" title="Measured before the day was fitted">from {minutes(e.measured_min)}</small>}
        </span>
        <b className="row-dur num">{clock(e.duration_min)}</b>
      </div>
      {open && <EntryDetails e={e} grouping={grouping} run={run} />}
    </li>
  );
}

/** The editable parts of an entry: its Jira ticket and GitHub ref, label, and review notes. */
function EntryDetails({ e, grouping, run }: { e: ReportEntry; grouping: Grouping; run: Run }) {
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
    <div className="row-details">
      <dl>
        <dt>Jira</dt>
        <dd>
          {editing === "jira" ? (
            <TicketEditor initial={e.jira?.key ?? ""} placeholder="SHOP-1261 or repo#12" onSave={saveTicket} onCancel={() => setEditing(null)} />
          ) : e.jira ? (
            <TicketChip ticket={e.jira} info={e.jira_info} run={run} onEdit={() => setEditing("jira")}
              auto={e.jira.source === "tracker" ? e.suggestion : null} onReject={() => run(() => api.dismissSuggestion(e.session_ids))} />
          ) : e.suggestion ? (
            <SuggestionChip e={e} run={run} onOther={() => setEditing("jira")} />
          ) : (
            <button className="ticket add" onClick={() => setEditing("jira")}>Add ticket</button>
          )}
        </dd>
        {(e.github || editing === "github") && (
          <>
            <dt>GitHub</dt>
            <dd>
              {editing === "github" ? (
                <TicketEditor initial={e.github?.key ?? ""} placeholder="repo#12" onSave={saveTicket} onCancel={() => setEditing(null)} />
              ) : (
                <TicketChip ticket={e.github!} info={e.github_info} run={run} onEdit={() => setEditing("github")} />
              )}
            </dd>
          </>
        )}
        <dt>Work</dt>
        <dd>
          {single ? (
            <input className="desc" value={label} aria-label="Session label" onChange={(ev) => setLabel(ev.target.value)} onBlur={saveLabel}
              onKeyDown={(ev) => ev.key === "Enter" && (ev.target as HTMLInputElement).blur()} />
          ) : (
            <span>{e.label}{e.session_ids.length > 1 && <span className="muted"> ({e.session_ids.length} sessions; show per session to rename one)</span>}</span>
          )}
        </dd>
        <dt>Toggl</dt>
        <dd>{e.toggl_project || "No project"}{e.client && ` · ${e.client}`} · {e.billable ? "billable" : "not billable"}</dd>
        {(e.reasons.length > 0 || e.parallel_min > 0 || e.gap_min > 0) && (
          <>
            <dt>Notes</dt>
            <dd className="chips">
              {e.reasons.map((r) => <span key={r} className={`tag ${r === "Low confidence" || r === "No project" ? "low" : ""}`}>{r}</span>)}
              {e.parallel_min > 0 && <span className="chip">{minutes(e.parallel_min)} shared with parallel work</span>}
              {e.gap_min > 0 && <span className="chip">includes {minutes(e.gap_min)} filled</span>}
            </dd>
          </>
        )}
      </dl>
    </div>
  );
}

/**
 * A ticket chip: the key, with the title and epic on hover. Click to change it; "Open" opens it.
 * @param ticket the Jira ticket or the GitHub ref of an entry.
 * @param info the stored title, link and epic, when a tracker returned them.
 * @param auto the suggestion the report applied without a click, which "Not this" takes back.
 */
function TicketChip({ ticket, info, run, onEdit, auto, onReject }: {
  ticket: Ticket;
  info: TicketInfo | null;
  run: Run;
  onEdit: () => void;
  auto?: Suggestion | null;
  onReject?: () => void;
}) {
  const github = ticket.key.includes("#");
  const epic = info?.epic;
  const found = auto ? `Linked on its own: ${auto.reason}.` : `Found in ${ticket.source}.`;
  const title = [info?.title, epic && `Epic ${epic.key}: ${epic.title}`, `${found} Click to change.`].filter(Boolean).join("\n");
  return (
    <span className="chip-group">
      <button className={`ticket ${github ? "github" : ""}`} title={title} onClick={onEdit}>{ticket.key}</button>
      {info?.title && <span className="ticket-title">{info.title}</span>}
      {auto && <span className="tag" title={auto.reason}>auto</span>}
      {auto && onReject && <button className="small" onClick={onReject}>Not this</button>}
      {epic && <span className="tag epic" title={`Epic ${epic.key}`}>{epic.key} {epic.title}</span>}
      {info?.url && <button className="link small" onClick={() => run(() => api.openTicket(ticket.key))}>Open</button>}
    </span>
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
    <span className="inline-form">
      <input autoFocus value={value} placeholder={placeholder} onChange={(ev) => setValue(ev.target.value)}
        onKeyDown={(ev) => { if (ev.key === "Enter") onSave(value.trim()); if (ev.key === "Escape") onCancel(); }} />
      <button className="small" onClick={() => onSave(value.trim())}>Set</button>
      <button className="small" onClick={() => onSave(null)} title="Find it from evidence again">Auto</button>
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
    <span className="chip-group">
      <button className="ticket suggested" title={`Suggested: ${s.reason}.${epic}\nClick to open.`} onClick={() => run(() => api.openTicket(s.info.key))}>
        {s.info.key}
      </button>
      {s.by_ai && <span className="tag">AI</span>}
      {s.info.title && <span className="ticket-title" title={s.reason}>{s.info.title}</span>}
      <button className="small" onClick={() => run(() => api.setTicket(e.session_ids, s.info.key))}>Use</button>
      <button className="small" onClick={() => run(() => api.dismissSuggestion(e.session_ids))}>Not this</button>
      <button className="link small" onClick={onOther}>Other</button>
    </span>
  );
}
