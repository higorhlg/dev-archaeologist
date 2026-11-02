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
  context: Context;
};

export type Achievement = { id: string; day: string; rule: string; title: string; detail: string; evidence_id: string };
export type JournalLine = { text: string; cites: string[] };
export type Journal = { day: string; lines: JournalLine[]; state: "inferred" | "confirmed"; model: string | null };
export type StatusKind = "daily" | "weekly";
/** A status update for a meeting on `period`; `covers` are the days its done lines cover. */
export type StatusView = {
  kind: StatusKind;
  period: string;
  covers: string[];
  done: JournalLine[];
  now: JournalLine[];
  blockers: JournalLine[];
  model: string | null;
  written_ms: number;
  /** The update as plain text, ready to paste into a chat. */
  text: string;
};

export type Gap = {
  id: string;
  day: string;
  start_ts: number;
  end_ts: number;
  kind: "at_computer" | "bridged" | "unaccounted";
  state: "open" | "approved" | "assigned" | "not_work";
  targets: [string, number][];
  project: string | null;
  label: string | null;
};

export type DaySchedule = {
  day: string;
  schedule_min: number;
  observed_min: number;
  filled_min: number;
  pending_bridge_min: number;
  unaccounted_min: number;
};

export type DayView = {
  day: string;
  active_min: number;
  gaps: Gap[];
  schedule: DaySchedule | null;
  work_windows: [number, number][];
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

export type Grouping = "session" | "ticket_day" | "project_day";
export type TicketMode = "prefix" | "tag" | "off" | "epic_ticket";
export type Context = "work" | "personal";
export type ContextFilter = Context | "all";
export type ReportPrefs = {
  range: string;
  grouping: Grouping;
  tickets: TicketMode;
  context: ContextFilter;
  fit_day: boolean;
  fit_hours: number;
  /** How far fitted days may drift from `fit_hours`, in minutes; 0 fits every day exactly. */
  fit_drift_min: number;
};
export type ContextRules = {
  work_paths: string[];
  personal_paths: string[];
  work_emails: string[];
  overrides: Record<string, Context>;
};
export type ProjectContext = { project: string; context: Context; chosen: boolean };
export type TogglProject = { name: string; id: number | null; client: string; billable: boolean; skip: boolean };
export type MeetingDescription = { title_contains: string; description: string };
/** How the "Epic and ticket" mode words entries. */
export type DescriptionRules = {
  fallback_epic: string;
  no_ticket: string;
  meetings: MeetingDescription[];
  other_meeting: string;
  jira_projects: string[];
  mine_only_projects: string[];
  use_suggestions: boolean;
  fold_no_ticket: boolean;
};
/** An entry added to every worked day on `weekday` (0 = Monday) at `start` (`HH:MM`). */
export type RecurringEntry = { weekday: number; start: string; minutes: number; description: string; project: string; client: string; billable: boolean };
export type TogglSettings = {
  email: string;
  workspace_id: number | null;
  default_billable: boolean;
  project_map: Record<string, TogglProject>;
  descriptions: DescriptionRules;
  recurring: RecurringEntry[];
};

export type WorkSchedule = {
  days: number[];
  start: string;
  end: string;
  breaks: [string, string][];
  bridge_max_min: number;
  fill_project: string | null;
};

export type Settings = {
  sources: { app: boolean; window_titles: boolean; fs: boolean; git: boolean; agents: boolean; shell: boolean; calendar: boolean };
  repo_roots: string[];
  excluded_paths: string[];
  cloud_opt_out: string[];
  model: string;
  backfill_done: boolean;
  toggl: TogglSettings;
  report_prefs: ReportPrefs;
  schedule: WorkSchedule | null;
  schedule_asked: boolean;
  contexts: ContextRules;
  calendar_excluded: string[];
  trackers: TrackerSettings;
  automation: Automation;
};

export type Automation = { journals: boolean; weekly_report: boolean };
export type TrackerSettings = { jira_site: string; jira_email: string; github: boolean; auto_sync_min: number };
export type TrackerStatus = {
  jira_site: string;
  jira_email: string;
  jira_connected: boolean;
  jira_site_guess: string | null;
  jira_email_guess: string | null;
  github_enabled: boolean;
  github_accounts: string[];
  auto_sync_min: number;
};
export type TicketSyncStatus = { at_ms: number; from: string; to: string; checked: number; suggested: number; applied: number; errors: string[] };
export type JiraAccount = { display_name: string; email: string };
export type FindResult = { checked: number; suggested: number; by_ai: number; applied: number; looked_in: string[]; errors: string[] };
export type Epic = { key: string; title: string };
export type TicketInfo = { key: string; title: string; url: string; tracker: string; epic: Epic | null };
/** `auto`: the evidence names the ticket, so the report already files the entry under it. */
export type Suggestion = { info: TicketInfo; reason: string; by_ai: boolean; auto: boolean };

export type Status = {
  event_counts: [string, number][];
  days: string[];
  has_api_key: boolean;
  has_toggl_token: boolean;
  settings: Settings;
};

export type Ticket = { key: string; source: string };
/**
 * `fit_day_min` scales each work day's entries to that many minutes; null reports measured time.
 * `fit_drift_min` lets fitted days move about that far from it, keeping the total.
 */
export type ReportQuery = {
  from: string;
  to: string;
  grouping: Grouping;
  tickets: TicketMode;
  context: ContextFilter;
  fit_day_min?: number | null;
  fit_drift_min?: number | null;
};
/** One block of an entry's time when a fitted day splits it around meetings. */
export type Piece = { start_ts: number; duration_min: number };
export type ReportEntry = {
  key: string;
  day: string;
  start_ts: number;
  duration_min: number;
  /** The measured minutes before the day was fitted; equal to `duration_min` otherwise. */
  measured_min: number;
  project: string | null;
  toggl_project: string;
  client: string;
  billable: boolean;
  skip: boolean;
  context: Context;
  label: string;
  description: string;
  tags: string[];
  /** The Jira ticket; the entry is filed under it in Toggl. */
  jira: Ticket | null;
  jira_info: TicketInfo | null;
  /** The GitHub issue or pull request; filed under only when there is no Jira ticket. */
  github: Ticket | null;
  github_info: TicketInfo | null;
  suggestion: Suggestion | null;
  session_ids: string[];
  approved: boolean;
  reasons: string[];
  sent: boolean;
  parallel_min: number;
  gap_min: number;
  pending_bridge_min: number;
  /** Empty when the entry runs in one block from `start_ts`. */
  pieces: Piece[];
};
export type Report = {
  query: ReportQuery;
  entries: ReportEntry[];
  total_min: number;
  approved_min: number;
  billable_min: number;
  review_count: number;
  by_project: { project: string; min: number }[];
  known_prefixes: string[];
  parallel_min: number;
};
export type ModelInfo = { id: string; display_name: string; note: string };
export type CalendarStatus = {
  access: "not_asked" | "granted" | "denied" | "unsupported";
  calendars: { title: string; account: string }[];
  excluded: string[];
};
export type TogglAccount = { email: string; workspace_id: number };
export type TogglProjectInfo = { id: number; name: string; active: boolean };
export type SendResult = { sent: number; already_sent: number; failed: string[] };

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
  day: (day: string, side: ContextFilter = "all") => invoke<DayView>("day", { day, side }),
  sessionEvents: (id: string) => invoke<WorkEvent[]>("session_events", { id }),
  projects: (from: string, to: string, side: ContextFilter = "all") => invoke<ProjectRow[]>("projects", { from, to, side }),
  confirmSession: (id: string) => invoke<void>("confirm_session", { id }),
  excludeSession: (id: string) => invoke<void>("exclude_session", { id }),
  editSession: (id: string, edit: SessionEdit) => invoke<void>("edit_session", { id, edit }),
  generateJournal: (day: string) => invoke<Journal>("generate_journal", { day }),
  confirmJournal: (day: string) => invoke<void>("confirm_journal", { day }),
  statusUpdate: (kind: StatusKind, day: string) => invoke<StatusView | null>("status_update", { kind, day }),
  generateStatusUpdate: (kind: StatusKind, day: string) => invoke<StatusView>("generate_status_update", { kind, day }),
  rebuildDay: (day: string) => invoke<number>("rebuild_day", { day }),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  /** Saves only the given sections, merged into the stored settings. */
  patchSettings: (patch: Partial<Settings>) => invoke<Settings>("patch_settings", { patch }),
  detectContexts: () => invoke<ContextRules>("detect_contexts"),
  saveApiKey: (key: string) => invoke<void>("save_api_key", { key }),
  installShellHook: () => invoke<string>("install_shell_hook"),
  report: (query: ReportQuery) => invoke<Report>("report", { query }),
  approveSessions: (ids: string[]) => invoke<void>("approve_sessions", { ids }),
  approveConfident: (query: ReportQuery) => invoke<number>("approve_confident", { query }),
  setEntryLabel: (sessionId: string, label: string) => invoke<void>("set_entry_label", { sessionId, label }),
  /** Sets the Jira ticket; a `repo#N` key sets the GitHub ref. `null` goes back to detection. */
  setTicket: (sessionIds: string[], ticket: string | null) => invoke<void>("set_ticket", { sessionIds, ticket }),
  setGithubRef: (sessionIds: string[], githubRef: string | null) => invoke<void>("set_github_ref", { sessionIds, githubRef }),
  exportTogglCsv: (query: ReportQuery) => invoke<string>("export_toggl_csv", { query }),
  connectToggl: (token: string) => invoke<TogglAccount>("connect_toggl", { token }),
  togglProjects: () => invoke<TogglProjectInfo[]>("toggl_projects"),
  sendToToggl: (query: ReportQuery) => invoke<SendResult>("send_to_toggl", { query }),
  reportDays: (query: ReportQuery) => invoke<DaySchedule[]>("report_days", { query }),
  resolveGap: (id: string, resolution: { project?: string; label?: string; not_work?: boolean }) =>
    invoke<void>("resolve_gap", { id, resolution }),
  approveBridges: (from: string, to: string) => invoke<number>("approve_bridges", { from, to }),
  fillToSchedule: (day: string, project: string) => invoke<number>("fill_to_schedule", { day, project }),
  saveSchedule: (schedule: WorkSchedule | null) => invoke<void>("save_schedule", { schedule }),
  projectContexts: () => invoke<ProjectContext[]>("project_contexts"),
  setProjectContext: (project: string, context: Context | null) => invoke<void>("set_project_context", { project, context }),
  calendarStatus: () => invoke<CalendarStatus>("calendar_status"),
  connectCalendar: () => invoke<CalendarStatus>("connect_calendar"),
  reimportMeetings: () => invoke<number>("reimport_meetings"),
  aiModels: () => invoke<ModelInfo[]>("ai_models"),
  trackerStatus: () => invoke<TrackerStatus>("tracker_status"),
  connectJira: (site: string, email: string, token: string) => invoke<JiraAccount>("connect_jira", { site, email, token }),
  openJiraTokenPage: () => invoke<void>("open_jira_token_page"),
  openTicket: (key: string) => invoke<void>("open_ticket", { key }),
  findTickets: (query: ReportQuery) => invoke<FindResult>("find_tickets", { query }),
  dismissSuggestion: (sessionIds: string[]) => invoke<void>("dismiss_suggestion", { sessionIds }),
  ticketSyncStatus: () => invoke<TicketSyncStatus | null>("ticket_sync_status"),
  syncTickets: () => invoke<TicketSyncStatus>("sync_tickets"),
  openWeeklyReports: () => invoke<string>("open_weekly_reports"),
  daysNeedingLabels: (query: ReportQuery) => invoke<string[]>("days_needing_labels", { query }),
  /** Writes the timesheet page to Downloads and opens it in the browser. */
  openTimesheet: (query: ReportQuery) => invoke<string>("open_timesheet", { query }),
  /** Learns ticket epics from a Toggl detailed-report CSV; returns how many tickets. */
  importTogglHistory: (path: string) => invoke<number>("import_toggl_history", { path }),
  /** Day → `[start, end]` local `HH:MM` spans the user was away. */
  awayDays: (from: string, to: string) => invoke<Record<string, [string, string][]>>("away_days", { from, to }),
  addAway: (day: string, start: string, end: string) => invoke<void>("add_away", { day, start, end }),
  clearAway: (day: string) => invoke<void>("clear_away", { day }),
};

/** An entry's time as blocks: its pieces, or one block from its start. */
export function blocks(e: ReportEntry): Piece[] {
  return e.pieces.length ? e.pieces : [{ start_ts: e.start_ts, duration_min: e.duration_min }];
}

/** `125` → `2:05:00`, the way Toggl writes durations. */
export function clock(min: number): string {
  return `${Math.floor(min / 60)}:${String(min % 60).padStart(2, "0")}:00`;
}

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

export const DEFAULT_SCHEDULE: WorkSchedule = {
  days: [0, 1, 2, 3, 4],
  start: "08:00",
  end: "17:00",
  breaks: [["12:00", "13:00"]],
  bridge_max_min: 60,
  fill_project: null,
};
