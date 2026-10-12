import { useCallback, useEffect, useState } from "react";
import { api, dayKey, type Status } from "./api";
import Onboarding from "./Onboarding";
import Timeline from "./Timeline";
import Journal from "./Journal";
import ProjectTimeline from "./ProjectTimeline";
import Sources from "./Sources";
import Reports from "./Reports";
import ScheduleStep from "./ScheduleStep";

type View = "day" | "journal" | "projects" | "reports" | "sources";

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
      </nav>
      <main className="main">
        {view === "day" && <Timeline day={day} setDay={setDay} days={status.days} />}
        {view === "journal" && (
          <Journal day={day} setDay={setDay} hasKey={status.has_api_key} goToSources={() => setView("sources")} />
        )}
        {view === "projects" && (
          <ProjectTimeline
            onPickDay={(d) => {
              setDay(d);
              setView("day");
            }}
          />
        )}
        {view === "reports" && (
          <Reports
            status={status}
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
