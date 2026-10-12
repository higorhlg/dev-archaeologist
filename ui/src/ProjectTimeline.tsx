import { useEffect, useMemo, useState } from "react";
import { api, dayKey, minutes, shiftDay, stratum, type ProjectRow } from "./api";

const RANGES = [30, 60, 90];

/** Projects across weeks: one row per project, one cell per day, shaded by active time. */
export default function ProjectTimeline({ onPickDay }: { onPickDay: (day: string) => void }) {
  const [range, setRange] = useState(60);
  const [rows, setRows] = useState<ProjectRow[]>([]);
  const to = dayKey(new Date());
  const from = shiftDay(to, -(range - 1));

  useEffect(() => {
    api.projects(from, to).then(setRows);
  }, [from, to]);

  const days = useMemo(() => Array.from({ length: range }, (_, i) => shiftDay(from, i)), [from, range]);
  const max = Math.max(60, ...rows.flatMap((r) => Object.values(r.days)));

  return (
    <div className="projects">
      <header className="day-head">
        <h1>Projects</h1>
        <div className="seg" role="group" aria-label="Range">
          {RANGES.map((r) => (
            <button key={r} className={r === range ? "on" : ""} onClick={() => setRange(r)}>{r} days</button>
          ))}
        </div>
      </header>
      {rows.length === 0 && <p className="empty">No sessions in this range.</p>}
      <div className="grid" style={{ ["--days" as string]: range }}>
        <div className="grid-head">
          <span />
          <div className="cells">
            {days.map((d) => (
              <span key={d} className={new Date(d).getUTCDate() === 1 ? "month" : ""}>
                {new Date(d).getUTCDate() === 1 ? new Date(d).toLocaleDateString([], { month: "short", timeZone: "UTC" }) : ""}
              </span>
            ))}
          </div>
          <span />
        </div>
        {rows.map((r) => (
          <div className="grid-row" key={r.project}>
            <span className="lane-name" title={r.project}>{r.project}</span>
            <div className="cells">
              {days.map((d) => {
                const m = r.days[d] ?? 0;
                return (
                  <button
                    key={d}
                    className="cell"
                    disabled={!m}
                    title={m ? `${d}: ${minutes(m)}` : d}
                    onClick={() => onPickDay(d)}
                    style={m ? { background: stratum(r.project), opacity: 0.25 + 0.75 * Math.min(1, m / max) } : undefined}
                  />
                );
              })}
            </div>
            <span className="num total">{minutes(r.total_min)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
