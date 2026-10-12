import { useCallback, useEffect, useState } from "react";
import { api, dayKey, type ContextFilter, type Status } from "./api";
import Onboarding from "./Onboarding";
import Timeline from "./Timeline";
import Journal from "./Journal";
import ProjectTimeline from "./ProjectTimeline";
import Sources from "./Sources";
import Reports from "./Reports";
import ScheduleStep from "./ScheduleStep";

type View = "day" | "journal" | "projects" | "reports" | "sources";

const SIDES: { id: ContextFilter; label: string }[] = [
  { id: "work", label: "Work" },
  { id: "personal", label: "Personal" },
  { id: "all", label: "All" },
];

const NAV: { id: View; label: string }[] = [
  { id: "day", label: "Day" },
  { id: "journal", label: "Journal" },
  { id: "projects", label: "Projects" },
  { id: "reports", label: "Reports" },
  { id: "sources", label: "Sources" },
];

export default function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [view, setView] = useState<View>("day");
  const [day, setDay] = useState(dayKey(new Date()));
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api.status().then(setStatus).catch((e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  const side: ContextFilter = status?.settings.report_prefs.context ?? "work";
  const setSide = async (context: ContextFilter) => {
    if (!status) return;
    await api.patchSettings({ report_prefs: { ...status.settings.report_prefs, context } });
    refresh();
  };

  if (error) return <p className="fatal">The app could not read its local database: {error}</p>;
  if (!status) return null;
  if (!status.settings.backfill_done) return <Onboarding onDone={refresh} />;
  if (!status.settings.schedule_asked)
    return (
      <div className="onboard">
        <ScheduleStep initial={null} onDone={refresh} />
      </div>
    );

  return (
    <div className="shell">
      <nav className="rail" aria-label="Views">
        <div className="mark" aria-hidden>
          <span />
          <span />
          <span />
        </div>
        {NAV.map((n) => (
          <button key={n.id} className={view === n.id ? "on" : ""} onClick={() => setView(n.id)}>
            {n.label}
          </button>
        ))}
        <div className="side-switch" role="radiogroup" aria-label="Show work, personal or all">
          {SIDES.map((x) => (
            <button key={x.id} role="radio" aria-checked={side === x.id} className={side === x.id ? "on" : ""} onClick={() => setSide(x.id)}>
              <i className={`dot ${x.id}`} />
              {x.label}
            </button>
          ))}
        </div>
      </nav>
      <main className="main">
        {view === "day" && <Timeline day={day} setDay={setDay} days={status.days} side={side} />}
        {view === "journal" && (
          <Journal day={day} setDay={setDay} hasKey={status.has_api_key} goToSources={() => setView("sources")} />
        )}
        {view === "projects" && (
          <ProjectTimeline
            side={side}
            onPickDay={(d) => {
              setDay(d);
              setView("day");
            }}
          />
        )}
        {view === "reports" && (
          <Reports
            status={status}
            side={side}
            onSaved={refresh}
            onOpenDay={(d) => {
              setDay(d);
              setView("day");
            }}
            goToSources={() => setView("sources")}
          />
        )}
        {view === "sources" && <Sources status={status} onSaved={refresh} />}
      </main>
    </div>
  );
}
