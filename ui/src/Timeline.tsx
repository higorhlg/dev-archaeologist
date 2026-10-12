import { useCallback, useEffect, useMemo, useState } from "react";
import { api, dayKey, hhmm, longDay, minutes, shiftDay, stratum, type DaySchedule, type DayView, type Gap, type Session, type WorkEvent } from "./api";

type Props = { day: string; setDay: (d: string) => void; days: string[] };

export default function Timeline({ day, setDay, days }: Props) {
  const [view, setView] = useState<DayView | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api.day(day).then(setView).catch((e) => setError(String(e)));
  }, [day]);

  useEffect(() => {
    setSelected(null);
    load();
  }, [load]);

  useEffect(() => {
    if (day !== dayKey(new Date())) return;
    const t = setInterval(load, 60_000);
    return () => clearInterval(t);
  }, [day, load]);

  const [openGap, setOpenGap] = useState<string | null>(null);
  const visible = view?.sessions.filter((s) => s.state !== "excluded") ?? [];
  const current = view?.sessions.find((s) => s.id === selected) ?? null;
  const gap = view?.gaps.find((g) => g.id === openGap) ?? null;

  return (
    <div className={`day ${current ? "with-drawer" : ""}`}>
      <DayHeader day={day} setDay={setDay} days={days} total={view?.active_min ?? 0} count={visible.length} schedule={view?.schedule ?? null} />
      {error && <p className="error">{error}</p>}
      {view && visible.length === 0 && <EmptyDay day={day} days={days} setDay={setDay} />}
      {visible.length > 0 && view && (
        <Strata
          sessions={visible}
          gaps={view.gaps}
          windows={view.work_windows}
          selected={selected}
          onSelect={setSelected}
          openGap={openGap}
          onOpenGap={setOpenGap}
        />
      )}
      {gap && view && (
        <GapPanel gap={gap} projects={[...new Set(visible.map((s) => s.project).filter((p): p is string => !!p))]}
          onDone={() => { setOpenGap(null); load(); }} />
      )}
      {view && view.achievements.length > 0 && (
        <ul className="badges">
          {view.achievements.map((a) => (
            <li key={a.id} title={a.detail}>
              <b>{a.title}</b>
              <span>{a.detail}</span>
            </li>
          ))}
        </ul>
      )}
      <ol className="sessions">
        {view?.sessions.map((s) => (
          <SessionRow key={s.id} s={s} active={s.id === selected} onSelect={() => setSelected(s.id === selected ? null : s.id)} />
        ))}
      </ol>
      {current && <EvidenceDrawer session={current} onClose={() => setSelected(null)} onChanged={load} />}
    </div>
  );
}

function DayHeader({ day, setDay, days, total, count, schedule }: { day: string; setDay: (d: string) => void; days: string[]; total: number; count: number; schedule: DaySchedule | null }) {
  const today = dayKey(new Date());
  return (
    <header className="day-head">
      <div className="day-nav">
        <button aria-label="Previous day" onClick={() => setDay(shiftDay(day, -1))}>‹</button>
        <button aria-label="Next day" disabled={day >= today} onClick={() => setDay(shiftDay(day, 1))}>›</button>
        <select aria-label="Jump to a day" value={days.includes(day) ? day : ""} onChange={(e) => e.target.value && setDay(e.target.value)}>
          <option value="">Jump to…</option>
          {days.map((d) => (
            <option key={d} value={d}>{d}</option>
          ))}
        </select>
      </div>
      <h1>{day === today ? "Today" : longDay(day)}</h1>
      <p className="day-total">
        <span className="num">{minutes(total)}</span> active across {count} {count === 1 ? "session" : "sessions"}
        {schedule && schedule.filled_min > 0 && <>, <span className="num">{minutes(schedule.filled_min)}</span> filled</>}
        {schedule && schedule.pending_bridge_min > 0 && <>, {minutes(schedule.pending_bridge_min)} bridged to approve</>}
        {schedule && schedule.unaccounted_min > 0 && <>, <span className="warn">{minutes(schedule.unaccounted_min)} unaccounted</span></>}
        {schedule && <span className="muted"> of {minutes(schedule.schedule_min)} scheduled</span>}
      </p>
    </header>
  );
}

function EmptyDay({ day, days, setDay }: { day: string; days: string[]; setDay: (d: string) => void }) {
  const earlier = days.find((d) => d < day);
  return (
    <div className="empty">
      <p>No work found on this day.</p>
      {earlier && <button onClick={() => setDay(earlier)}>Go to {longDay(earlier)}</button>}
    </div>
  );
}

/** The day as layers: one lane per project, sessions as bands on a shared clock.
 * Work hours are tinted. Filled gaps are hatched in the lane that gets their minutes;
 * open unaccounted gaps sit dashed in their own lane. */
function Strata({ sessions, gaps, windows, selected, onSelect, openGap, onOpenGap }: {
  sessions: Session[];
  gaps: Gap[];
  windows: [number, number][];
  selected: string | null;
  onSelect: (id: string) => void;
  openGap: string | null;
  onOpenGap: (id: string | null) => void;
}) {
  const { lanes, from, to } = useMemo(() => {
    const start = Math.min(...sessions.map((s) => s.start_ts), ...windows.map((w) => w[0]));
    const end = Math.max(...sessions.map((s) => s.end_ts), ...windows.map((w) => w[1]));
    const from = new Date(start).setMinutes(0, 0, 0);
    const to = new Date(end).setMinutes(60, 0, 0);
    const byProject = new Map<string, Session[]>();
    for (const s of sessions) {
      const k = s.project ?? "Other";
      byProject.set(k, [...(byProject.get(k) ?? []), s]);
    }
    const lanes = [...byProject.entries()].sort((a, b) => sum(b[1]) - sum(a[1]));
    return { lanes, from, to };
  }, [sessions, windows]);

  const pct = (t: number) => ((t - from) / (to - from)) * 100;
  const hours: number[] = [];
  for (let t = from; t <= to; t += 3_600_000) hours.push(t);
  const byId = new Map(sessions.map((s) => [s.id, s]));
  const unaccounted = gaps.filter((g) => g.kind === "unaccounted" && g.state === "open");

  /** Pieces of filled gaps for one lane, in time order within each gap. */
  const fills = (project: string) =>
    gaps
      .filter((g) => g.kind !== "unaccounted" && g.state !== "not_work")
      .flatMap((g) => {
        let t = g.start_ts;
        return g.targets.map(([id, min]) => {
          const piece = { g, start: t, end: t + min * 60_000, project: byId.get(id)?.project ?? "Other" };
          t = piece.end;
          return piece;
        });
      })
      .filter((p) => p.project === project);

  const track = (children: React.ReactNode) => (
    <div className="lane-track">
      {windows.map(([a, b]) => <span key={a} className="work-hours" style={{ left: `${pct(a)}%`, width: `${pct(b) - pct(a)}%` }} />)}
      {hours.map((t) => <i key={t} className="tick" style={{ left: `${pct(t)}%` }} />)}
      {children}
    </div>
  );

  return (
    <section className="strata" aria-label="Sessions by project over the day">
      <div className="strata-axis">
        {hours.map((t) => (
          <span key={t} style={{ left: `${pct(t)}%` }}>{new Date(t).getHours()}</span>
        ))}
      </div>
      {lanes.map(([project, list]) => (
        <div className="lane" key={project}>
          <span className="lane-name" title={project}>{project}</span>
          {track(
            <>
              {fills(project).map((f) => (
                <span
                  key={`${f.g.id}${f.start}`}
                  className={`band fill ${f.g.kind} ${f.g.state}`}
                  style={{ left: `${pct(f.start)}%`, width: `${pct(f.end) - pct(f.start)}%`, color: stratum(project) }}
                  title={`${hhmm(f.start)}–${hhmm(f.end)} ${f.g.kind === "at_computer" ? "at the computer" : f.g.state === "approved" ? "bridged, approved" : "bridged, not approved"}`}
                />
              ))}
              {list.map((s) => (
                <button
                  key={s.id}
                  className={`band ${s.id === selected ? "on" : ""} ${s.state}`}
                  style={{ left: `${pct(s.start_ts)}%`, width: `max(6px, ${pct(s.end_ts) - pct(s.start_ts)}%)`, background: stratum(s.project) }}
                  title={`${hhmm(s.start_ts)}–${hhmm(s.end_ts)} ${s.label ?? ""}`}
                  onClick={() => onSelect(s.id)}
                />
              ))}
            </>,
          )}
        </div>
      ))}
      {unaccounted.length > 0 && (
        <div className="lane">
          <span className="lane-name warn">Unaccounted</span>
          {track(
            unaccounted.map((g) => (
              <button
                key={g.id}
                className={`band gap ${g.id === openGap ? "on" : ""}`}
                style={{ left: `${pct(g.start_ts)}%`, width: `${pct(g.end_ts) - pct(g.start_ts)}%` }}
                title={`${hhmm(g.start_ts)}–${hhmm(g.end_ts)}: assign this time`}
                onClick={() => onOpenGap(g.id === openGap ? null : g.id)}
              />
            )),
          )}
        </div>
      )}
    </section>
  );
}

/** One tap gives an unaccounted block a project, a label, or marks it as not work. */
function GapPanel({ gap, projects, onDone }: { gap: Gap; projects: string[]; onDone: () => void }) {
  const [other, setOther] = useState("");
  const resolve = async (r: { project?: string; label?: string; not_work?: boolean }) => {
    await api.resolveGap(gap.id, r);
    onDone();
  };
  const mins = Math.round((gap.end_ts - gap.start_ts) / 60_000);
  return (
    <div className="gap-panel">
      <b>{hhmm(gap.start_ts)}–{hhmm(gap.end_ts)}, {minutes(mins)} with no activity</b>
      <span className="opts">
        {projects.map((p) => (
          <button key={p} onClick={() => resolve({ project: p })}>
            <i style={{ background: stratum(p) }} />{p}
          </button>
        ))}
        <button onClick={() => resolve({ label: "Meetings", project: "Meetings" })}>Meetings</button>
        <span className="other">
          <input value={other} placeholder="Other project or label" onChange={(e) => setOther(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && other.trim() && resolve({ project: other.trim() })} />
          <button disabled={!other.trim()} onClick={() => resolve({ project: other.trim() })}>Assign</button>
        </span>
        <button className="link" onClick={() => resolve({ not_work: true })}>Not work</button>
      </span>
    </div>
  );
}

function sum(list: Session[]) {
  return list.reduce((n, s) => n + s.active_min, 0);
}

function SessionRow({ s, active, onSelect }: { s: Session; active: boolean; onSelect: () => void }) {
  return (
    <li className={`session ${active ? "on" : ""} ${s.state}`}>
      <button onClick={onSelect}>
        <span className="when num">
          {hhmm(s.start_ts)}–{hhmm(s.end_ts)}
          <small>{minutes(s.active_min)}</small>
        </span>
        <span className="what">
          <span className="proj" style={{ color: stratum(s.project) }}>{s.project ?? "No project"}</span>
          <b>{s.label ?? "Untitled session"}</b>
          {s.summary && s.state !== "observed" && <span className="summary">{s.summary}</span>}
          <span className="chips">
            {s.chips.map((c) => (
              <span key={c.label} className="chip">{c.n} {c.label}</span>
            ))}
            <StateTag s={s} />
            {s.context === "personal" && <span className="tag personal">Personal</span>}
          </span>
        </span>
      </button>
    </li>
  );
}

function StateTag({ s }: { s: Session }) {
  if (s.state === "confirmed") return <span className="tag ok">Confirmed</span>;
  if (s.state === "edited") return <span className="tag">Edited</span>;
  if (s.state === "excluded") return <span className="tag">Hidden</span>;
  if (s.confidence === "low") return <span className="tag low">Low confidence</span>;
  return null;
}

const KIND_LABEL: Record<string, string> = {
  prompt: "Prompt",
  tool_use: "Agent tool",
  ai_title: "Session title",
  agent_turn: "Agent turn",
  test_passed: "Tests passed",
  test_failed: "Tests failed",
  commit_created: "Commit",
  shell_command: "Command",
  file_changed: "File",
  app_active: "App",
  idle_start: "Away",
  idle_end: "Back",
};

/** Lists the events behind a session and the actions that correct it. */
function EvidenceDrawer({ session, onClose, onChanged }: { session: Session; onClose: () => void; onChanged: () => void }) {
  const [events, setEvents] = useState<WorkEvent[]>([]);
  const [editing, setEditing] = useState(false);
  const [showTools, setShowTools] = useState(false);

  useEffect(() => {
    setEditing(false);
    api.sessionEvents(session.id).then(setEvents);
  }, [session.id]);

  const act = async (fn: () => Promise<void>) => {
    await fn();
    onChanged();
  };
  const shown = events.filter((e) => showTools || (e.kind !== "tool_use" && e.kind !== "agent_turn"));
  const hidden = events.length - shown.length;

  return (
    <aside className="drawer" aria-label="Session evidence">
      <header>
        <button className="close" aria-label="Close" onClick={onClose}>×</button>
        <span className="proj" style={{ color: stratum(session.project) }}>{session.project ?? "No project"}</span>
        <h2>{session.label ?? "Untitled session"}</h2>
        <p className="muted num">
          {hhmm(session.start_ts)}–{hhmm(session.end_ts)}, {minutes(session.active_min)} active
          {session.agent_only_min > 0 && `, ${minutes(session.agent_only_min)} agent alone`}
          {session.branch && <>, on <code>{session.branch}</code></>}
        </p>
      </header>

      {editing ? (
        <EditForm session={session} onDone={() => { setEditing(false); onChanged(); }} />
      ) : (
        <div className="actions">
          {session.state !== "confirmed" && <button className="primary" onClick={() => act(() => api.confirmSession(session.id))}>Confirm</button>}
          <button onClick={() => setEditing(true)}>Edit</button>
          {session.state !== "excluded" && <button onClick={() => act(() => api.excludeSession(session.id))}>Hide</button>}
        </div>
      )}

      <h3>Evidence <span className="muted">{events.length} events</span></h3>
      <ol className="events">
        {shown.map((e) => (
          <li key={e.id} className={e.kind}>
            <span className="t num">{hhmm(Date.parse(e.ts))}</span>
            <span className="k">{KIND_LABEL[e.kind] ?? e.kind}</span>
            <span className="d">{eventText(e)}</span>
          </li>
        ))}
      </ol>
      {hidden > 0 && (
        <button className="link" onClick={() => setShowTools(true)}>Show {hidden} agent tool calls and turns</button>
      )}
    </aside>
  );
}

function eventText(e: WorkEvent): string {
  const m = e.meta ?? {};
  switch (e.kind) {
    case "commit_created":
      return `${e.detail ?? ""}  +${m.plus ?? 0} −${m.minus ?? 0} in ${m.files ?? 0} files`;
    case "test_passed":
    case "test_failed":
      return m.pass || m.fail ? `${m.pass ?? 0} passed, ${m.fail ?? 0} failed` : (e.detail ?? "");
    case "file_changed":
      return e.path?.split("/").slice(-2).join("/") ?? "";
    case "shell_command":
      return `${e.detail ?? ""} (exit ${m.exit ?? "?"})`;
    case "agent_turn":
      return minutes(Math.max(1, Math.round(Number(m.dur_ms ?? 0) / 60000)));
    default:
      return `${e.source === "codex" ? "Codex: " : ""}${e.detail ?? ""}`;
  }
}

function EditForm({ session, onDone }: { session: Session; onDone: () => void }) {
  const [label, setLabel] = useState(session.label ?? "");
  const [project, setProject] = useState(session.project ?? "");
  const [active, setActive] = useState(session.active_min);
  const [applyRule, setApplyRule] = useState(false);

  const save = async () => {
    await api.editSession(session.id, {
      label: label !== session.label ? label : undefined,
      project: project !== session.project ? project : undefined,
      active_min: active !== session.active_min ? active : undefined,
      apply_rule: applyRule,
    });
    onDone();
  };

  return (
    <form className="edit" onSubmit={(e) => { e.preventDefault(); save(); }}>
      <label>Label<input value={label} onChange={(e) => setLabel(e.target.value)} /></label>
      <label>Project<input value={project} onChange={(e) => setProject(e.target.value)} /></label>
      <label>Active minutes<input type="number" min={0} step={5} value={active} onChange={(e) => setActive(Number(e.target.value))} /></label>
      <label className="check">
        <input type="checkbox" checked={applyRule} onChange={(e) => setApplyRule(e.target.checked)} />
        Use this for future sessions{session.branch ? ` on ${session.branch}` : " in this folder"}
      </label>
      <div className="actions">
        <button className="primary" type="submit">Save</button>
        <button type="button" onClick={onDone}>Cancel</button>
      </div>
    </form>
  );
}
