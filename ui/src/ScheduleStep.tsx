import { useState } from "react";
import { api, DEFAULT_SCHEDULE, type WorkSchedule } from "./api";

const DAY_LETTERS = ["M", "T", "W", "T", "F", "S", "S"];
const DAY_NAMES = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

type Props = {
  initial: WorkSchedule | null;
  /** Sources shows the full form; onboarding shows the short question. */
  compact?: boolean;
  projects?: string[];
  onDone: () => void;
};

/** Asks for regular work hours. Saving rebuilds gaps for the last 90 days. */
export default function ScheduleStep({ initial, compact, projects = [], onDone }: Props) {
  const [s, setS] = useState<WorkSchedule>(initial ?? DEFAULT_SCHEDULE);
  const [lunch, setLunch] = useState((initial ?? DEFAULT_SCHEDULE).breaks.length > 0);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  const toggleDay = (d: number) =>
    setS({ ...s, days: s.days.includes(d) ? s.days.filter((x) => x !== d) : [...s.days, d].sort() });

  const save = async (schedule: WorkSchedule | null) => {
    setSaving(true);
    await api.saveSchedule(schedule && { ...schedule, breaks: lunch ? schedule.breaks : [] });
    setSaving(false);
    setSaved(true);
    onDone();
  };

  return (
    <div className="schedule">
      {!compact && (
        <>
          <h2>Do you keep regular hours?</h2>
          <p className="muted">
            Inside your hours, the app can bridge short quiet gaps between work and show you longer ones to sort out.
            Outside them, it only counts work it saw.
          </p>
        </>
      )}
      <div className="sched-row">
        <span>Work days</span>
        <span className="days" role="group" aria-label="Work days">
          {DAY_LETTERS.map((l, i) => (
            <button key={i} aria-label={DAY_NAMES[i]} aria-pressed={s.days.includes(i)} className={s.days.includes(i) ? "on" : ""} onClick={() => toggleDay(i)}>
              {l}
            </button>
          ))}
        </span>
      </div>
      <div className="sched-row">
        <span>Hours</span>
        <span>
          <input type="time" value={s.start} onChange={(e) => setS({ ...s, start: e.target.value })} /> to{" "}
          <input type="time" value={s.end} onChange={(e) => setS({ ...s, end: e.target.value })} />
        </span>
      </div>
      <div className="sched-row">
        <label className="check-line">
          <input type="checkbox" checked={lunch} onChange={(e) => setLunch(e.target.checked)} /> Lunch
        </label>
        {lunch && (
          <span>
            <input type="time" value={s.breaks[0]?.[0] ?? "12:00"} onChange={(e) => setS({ ...s, breaks: [[e.target.value, s.breaks[0]?.[1] ?? "13:00"]] })} /> to{" "}
            <input type="time" value={s.breaks[0]?.[1] ?? "13:00"} onChange={(e) => setS({ ...s, breaks: [[s.breaks[0]?.[0] ?? "12:00", e.target.value]] })} />
          </span>
        )}
      </div>
      {compact && (
        <>
          <div className="sched-row">
            <span>Bridge quiet gaps up to</span>
            <span>
              <input type="number" min={15} max={120} step={15} value={s.bridge_max_min} onChange={(e) => setS({ ...s, bridge_max_min: Number(e.target.value) })} /> min
            </span>
          </div>
          <div className="sched-row">
            <span>Project that “Fill to schedule” offers first</span>
            <input list="fill-projects" value={s.fill_project ?? ""} placeholder="None" onChange={(e) => setS({ ...s, fill_project: e.target.value || null })} />
            <datalist id="fill-projects">{projects.map((p) => <option key={p} value={p} />)}</datalist>
          </div>
        </>
      )}
      <div className="actions">
        <button className="primary" disabled={saving || s.days.length === 0} onClick={() => save(s)}>
          {saving ? "Saving…" : compact ? "Save hours" : "Use these hours"}
        </button>
        <button disabled={saving} onClick={() => save(null)}>{compact ? "Turn off" : "My hours change"}</button>
        {saved && compact && <span className="muted">Saved. Gaps rebuilt for the last 90 days.</span>}
      </div>
    </div>
  );
}
