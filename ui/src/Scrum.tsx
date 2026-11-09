import { useEffect, useRef, useState } from "react";
import { api, dayKey, latestWeekday, longDay, shiftDay, type Automation, type JournalLine, type StatusKind, type StatusView } from "./api";

type Props = { kind: StatusKind; hasKey: boolean; automation: Automation; goToAi: () => void };

const COPY: Record<StatusKind, { title: string; noun: string; stepDays: number; step: string; now: string; empty: string }> = {
  daily: {
    title: "Daily standup",
    noun: "standup",
    stepDays: 1,
    step: "day",
    now: "Today",
    empty: "Write a standup for a meeting on this day: the last day worked, what you are on now and what blocks you.",
  },
  weekly: {
    title: "Weekly status",
    noun: "weekly status",
    stepDays: 7,
    step: "week",
    now: "Working on now",
    empty: "Write a weekly status for a meeting on this day: the week before it, what you are on now and what blocks you.",
  },
};

/** The heading over the done lines, matching the copied text. */
function doneHeading(u: StatusView): string {
  return u.text.split("\n", 1)[0].replace(/:$/, "");
}

/** `Mon 08:12` for when an update was written. */
function writtenAt(ms: number): string {
  return new Date(ms).toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit", hour12: false });
}

/** The meeting day a page opens on: today for a standup, the latest weekly meeting day for a weekly status. */
function currentMeetingDay(kind: StatusKind, automation: Automation): string {
  return kind === "daily" ? dayKey(new Date()) : latestWeekday(automation.weekly_status_day);
}

function Lines({ lines, cite }: { lines: JournalLine[]; cite: (id: string) => number }) {
  if (lines.length === 0) return <p className="muted">None</p>;
  return (
    <ul>
      {lines.map((l, i) => (
        <li key={i}>
          {l.text}
          {l.cites.map((c) => (
            <sup key={c} title={`Evidence ${c}`}>{cite(c)}</sup>
          ))}
        </li>
      ))}
    </ul>
  );
}

/**
 * A Scrum page: the status update of `kind` for one meeting day, written from work sessions only
 * (done, now and blockers), with a button that copies it as plain text for a chat. It opens on
 * the current meeting day and steps a day (standup) or a week (weekly status) at a time. With
 * status updates on in the automation settings and an API key saved, the current meeting day's
 * update is written when the page opens without one; any other day waits for the button.
 */
export default function Scrum({ kind, hasKey, automation, goToAi }: Props) {
  const copy = COPY[kind];
  const current = currentMeetingDay(kind, automation);
  const [day, setDay] = useState(current);
  const [update, setUpdate] = useState<StatusView | null>(null);
  const [writingDay, setWritingDay] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const shownDay = useRef(day);
  shownDay.current = day;
  const busy = writingDay === day;
  const writesItself = hasKey && automation.status_updates && day === current;

  /** Runs `write` for the day shown and keeps its result unless the page moved to another day meanwhile. */
  const writeWith = async (write: () => Promise<StatusView>) => {
    setWritingDay(day);
    setError(null);
    try {
      const written = await write();
      if (shownDay.current !== day) return;
      setUpdate(written);
      setCopied(false);
    } catch (e) {
      if (shownDay.current === day) setError(String(e));
    } finally {
      setWritingDay((writing) => (writing === day ? null : writing));
    }
  };

  useEffect(() => {
    setUpdate(null);
    setError(null);
    setCopied(false);
    api
      .statusUpdate(kind, day)
      .then((kept) => {
        if (shownDay.current !== day) return;
        if (kept || !writesItself) setUpdate(kept);
        else void writeWith(() => api.ensureStatusUpdate(kind, day));
      })
      .catch((e) => setError(String(e)));
  }, [kind, day, writesItself]);

  const copyText = async () => {
    if (!update) return;
    try {
      await navigator.clipboard.writeText(update.text);
      setCopied(true);
    } catch (e) {
      setError(`Could not copy: ${e}`);
    }
  };

  const citeIndex = new Map<string, number>();
  const cite = (id: string) => {
    if (!citeIndex.has(id)) citeIndex.set(id, citeIndex.size + 1);
    return citeIndex.get(id)!;
  };

  return (
    <article className="journal status-update">
      <header className="day-head">
        <div className="day-nav">
          <button aria-label={`Previous ${copy.step}`} onClick={() => setDay(shiftDay(day, -copy.stepDays))}>‹</button>
          <button aria-label={`Next ${copy.step}`} disabled={day >= current} onClick={() => setDay(shiftDay(day, copy.stepDays))}>›</button>
        </div>
        <h1>{longDay(day)}</h1>
        <p className="day-total">
          {copy.title}, {update ? <>written <span className="num">{writtenAt(update.written_ms)}</span></> : busy ? "being written" : "not written yet"}
        </p>
      </header>

      {update ? (
        <div className="entry">
          <h3>{doneHeading(update)}</h3>
          <Lines lines={update.done} cite={cite} />
          <h3>{copy.now}</h3>
          <Lines lines={update.now} cite={cite} />
          <h3>Blockers</h3>
          <Lines lines={update.blockers} cite={cite} />
          <footer>
            <button className="primary" onClick={copyText}>{copied ? "Copied" : "Copy as text"}</button>
            <button onClick={() => writeWith(() => api.generateStatusUpdate(kind, day))} disabled={busy || !hasKey}>{busy ? "Writing…" : "Write again"}</button>
            <span className="muted">Written by {update.model}. Blockers come only from evidence; add any it cannot see.</span>
          </footer>
        </div>
      ) : busy ? (
        <div className="empty" role="status">
          <p>Writing the {copy.noun} from your work sessions…</p>
        </div>
      ) : hasKey ? (
        <div className="empty">
          <p>{copy.empty}</p>
          <button className="primary" onClick={() => writeWith(() => api.generateStatusUpdate(kind, day))}>Write the {copy.noun}</button>
        </div>
      ) : (
        <div className="empty">
          <p>Add a Claude API key to write status updates from your work sessions.</p>
          <button onClick={goToAi}>Add a key in Settings</button>
        </div>
      )}
      {error && <p className="error">{error}</p>}
    </article>
  );
}
