import { useCallback, useEffect, useState, type ReactElement } from "react";
import { api, dayKey, type ContextFilter, type Status } from "./api";
import Onboarding from "./Onboarding";
import Timeline from "./Timeline";
import Journal from "./Journal";
import ProjectTimeline from "./ProjectTimeline";
import Settings, { type SettingsTab } from "./Settings";
import Timesheet from "./Timesheet";
import ScheduleStep from "./ScheduleStep";
import Scrum from "./Scrum";

type View = "timesheet" | "standup" | "weekly" | "day" | "journal" | "projects" | "settings";

const SIDES: { id: ContextFilter; label: string }[] = [
  { id: "work", label: "Work" },
  { id: "personal", label: "Personal" },
  { id: "all", label: "All" },
];

/** 16px line icons for the sidebar, drawn on a 16-unit grid. */
const ICONS: Record<View, ReactElement> = {
  timesheet: <path d="M3 3.5h10M3 8h10M3 12.5h6M11.5 10.5v4M9.5 12.5h4" />,
  standup: <path d="M2.5 3.5h11v7H8l-3 2.5v-2.5H2.5z" />,
  weekly: <><rect x="3.5" y="2.5" width="9" height="11.5" rx="1.5" /><path d="M6 6h4M6 8.5h4M6 11h2.5" /></>,
  day: <><rect x="2.5" y="3" width="11" height="10.5" rx="1.5" /><path d="M2.5 6.5h11M5.5 1.5v3M10.5 1.5v3" /></>,
  journal: <><path d="M4 2.5h7.5a1 1 0 0 1 1 1v10H4a1.5 1.5 0 0 1 0-3h8.5" /><path d="M6 5.5h4" /></>,
  projects: <path d="M2.5 13.5h11M4 11V8M7 11V4M10 11V6.5M13 11V9" />,
  settings: <><circle cx="8" cy="8" r="2" /><path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" /></>,
};

const GROUPS: { title: string; items: { id: View; label: string }[] }[] = [
  { title: "Timesheet", items: [{ id: "timesheet", label: "Hours for Toggl" }] },
  { title: "Scrum", items: [{ id: "standup", label: "Daily standup" }, { id: "weekly", label: "Weekly status" }] },
  { title: "Activity", items: [{ id: "day", label: "Day" }, { id: "journal", label: "Journal" }, { id: "projects", label: "Projects" }] },
];

export default function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [view, setView] = useState<View>("timesheet");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("connections");
  const [day, setDay] = useState(dayKey(new Date()));
  const [reviewCount, setReviewCount] = useState(0);
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
  const openDay = (d: string) => {
    setDay(d);
    setView("day");
  };
  const goToSettings = (tab: SettingsTab) => {
    setSettingsTab(tab);
    setView("settings");
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

  const navItem = (id: View, label: string, badge?: number) => (
    <button key={id} className={view === id ? "on" : ""} aria-current={view === id ? "page" : undefined} onClick={() => setView(id)}>
      <svg viewBox="0 0 16 16" aria-hidden>{ICONS[id]}</svg>
      <span>{label}</span>
      {!!badge && <span className="badge num" aria-label={`${badge} to review`}>{badge}</span>}
    </button>
  );

  return (
    <div className="shell">
      <nav className="sidebar" aria-label="Views">
        <div className="mark" aria-hidden>
          <span />
          <span />
          <span />
        </div>
        {GROUPS.map((g) => (
          <div key={g.title} className="nav-group">
            <h2>{g.title}</h2>
            {g.items.map((i) => navItem(i.id, i.label, i.id === "timesheet" ? reviewCount : undefined))}
          </div>
        ))}
        <div className="sidebar-foot">
          <div className="side-switch" role="radiogroup" aria-label="Show work, personal or all">
            {SIDES.map((x) => (
              <button key={x.id} role="radio" aria-checked={side === x.id} className={side === x.id ? "on" : ""} onClick={() => setSide(x.id)}>
                {x.label}
              </button>
            ))}
          </div>
          {navItem("settings", "Settings")}
        </div>
      </nav>
      <main className="main">
        {view === "timesheet" && (
          <Timesheet status={status} side={side} onSaved={refresh} onOpenDay={openDay} goToSettings={goToSettings} onReviewCount={setReviewCount} />
        )}
        {(view === "standup" || view === "weekly") && (
          <Scrum key={view} kind={view === "standup" ? "daily" : "weekly"} hasKey={status.has_api_key} automation={status.settings.automation} goToAi={() => goToSettings("ai")} />
        )}
        {view === "day" && <Timeline day={day} setDay={setDay} days={status.days} side={side} />}
        {view === "journal" && <Journal day={day} setDay={setDay} hasKey={status.has_api_key} goToSources={() => goToSettings("ai")} />}
        {view === "projects" && <ProjectTimeline side={side} onPickDay={openDay} />}
        {view === "settings" && <Settings status={status} tab={settingsTab} onTab={setSettingsTab} onSaved={refresh} />}
      </main>
    </div>
  );
}
