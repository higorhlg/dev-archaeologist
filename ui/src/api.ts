import { invoke } from "@tauri-apps/api/core";

export type SessionState = "observed" | "inferred" | "edited" | "confirmed" | "excluded";

export type Session = {
  id: string;
  day: string;
  start_ts: number;
  end_ts: number;
  project: string | null;
  branch: string | null;
  label: string | null;
  summary: string | null;
  active_min: number;
  agent_only_min: number;
  state: SessionState;
  confidence: "low" | "medium" | "high" | null;
  chips: { label: string; n: number }[];
};

export type Achievement = { id: string; day: string; rule: string; title: string; detail: string; evidence_id: string };
export type JournalLine = { text: string; cites: string[] };
export type Journal = { day: string; lines: JournalLine[]; state: "inferred" | "confirmed"; model: string | null };

export type DayView = {
  day: string;
  active_min: number;
  sessions: Session[];
  achievements: Achievement[];
  journal: Journal | null;
};

export type WorkEvent = {
  id: string;
  source: string;
  ts: string;
  kind: string;
  project: string | null;
  path: string | null;
  branch: string | null;
  detail: string | null;
  meta: Record<string, unknown> | null;
};

export type Settings = {
  sources: { app: boolean; window_titles: boolean; fs: boolean; git: boolean; agents: boolean; shell: boolean };
  repo_roots: string[];
  excluded_paths: string[];
  cloud_opt_out: string[];
  model: string;
  backfill_done: boolean;
};

export type Status = { event_counts: [string, number][]; days: string[]; has_api_key: boolean; settings: Settings };

export type Discovery = {
  claude_profiles: number;
  claude_sessions: number;
  codex_sessions: number;
  repos: string[];
  shell_history_has_times: boolean;
};

export type BackfillReport = { claude_code: number; codex: number; git: number; sessions: number };
export type ProjectRow = { project: string; total_min: number; days: Record<string, number> };
export type SessionEdit = { label?: string; project?: string; active_min?: number; apply_rule: boolean };

export const api = {
  status: () => invoke<Status>("status"),
  discover: (days: number) => invoke<Discovery>("discover", { days }),
  backfill: (days: number) => invoke<BackfillReport>("backfill", { days }),
  day: (day: string) => invoke<DayView>("day", { day }),
  sessionEvents: (id: string) => invoke<WorkEvent[]>("session_events", { id }),
  projects: (from: string, to: string) => invoke<ProjectRow[]>("projects", { from, to }),
  confirmSession: (id: string) => invoke<void>("confirm_session", { id }),
  excludeSession: (id: string) => invoke<void>("exclude_session", { id }),
  editSession: (id: string, edit: SessionEdit) => invoke<void>("edit_session", { id, edit }),
  generateJournal: (day: string) => invoke<Journal>("generate_journal", { day }),
  confirmJournal: (day: string) => invoke<void>("confirm_journal", { day }),
  rebuildDay: (day: string) => invoke<number>("rebuild_day", { day }),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  saveApiKey: (key: string) => invoke<void>("save_api_key", { key }),
  installShellHook: () => invoke<string>("install_shell_hook"),
};

/** Local calendar day as `YYYY-MM-DD`. */
export function dayKey(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function shiftDay(day: string, delta: number): string {
  const [y, m, d] = day.split("-").map(Number);
  return dayKey(new Date(y, m - 1, d + delta));
}

export function hhmm(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
}

/** `95` → `1 h 35 min`. Inputs are already rounded to 5 minutes by the core. */
export function minutes(n: number): string {
  if (n < 60) return `${n} min`;
  const h = Math.floor(n / 60);
  const m = n % 60;
  return m ? `${h} h ${m} min` : `${h} h`;
}

export function longDay(day: string): string {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" });
}

/** Stable earth-tone stratum for a project name. */
const STRATA = ["#b8862f", "#9b5b3d", "#5f7a4a", "#4f6a80", "#c2a77a", "#7d5f8a", "#8a7f3d", "#5d7f7a"];
export function stratum(project: string | null): string {
  if (!project) return "#b9b3a6";
  let h = 0;
  for (const c of project) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return STRATA[h % STRATA.length];
}
