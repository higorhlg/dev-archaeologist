import { useCallback, useEffect, useState } from "react";
import { api, longDay, type JournalLine, type StatusKind, type StatusView } from "./api";

type Props = { kind: StatusKind; day: string; hasKey: boolean };

const COPY: Record<StatusKind, { title: string; covers: string; now: string; empty: string }> = {
  daily: {
    title: "Daily standup",
    covers: "What you did on the last day worked, what you are on now and what blocks you.",
    now: "Today",
    empty: "Write a standup for a meeting on this day from your work sessions.",
  },
  weekly: {
    title: "Weekly status",
    covers: "What you did in the week before this day, what you are on now and what blocks you.",
    now: "Working on now",
    empty: "Write a weekly status for a meeting on this day from the past week's work sessions.",
  },
};

/** The heading over the done lines, matching the copied text. */
function doneHeading(u: StatusView): string {
  return u.text.split("\n", 1)[0].replace(/:$/, "");
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
 * A status update for a meeting on `day`, written from work sessions only: done, now and
 * blockers, with a button that copies it as plain text for a chat.
 */
export default function StatusUpdate({ kind, day, hasKey }: Props) {
  const [update, setUpdate] = useState<StatusView | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const copy = COPY[kind];

  const load = useCallback(() => {
    setError(null);
    setCopied(false);
    api.statusUpdate(kind, day).then(setUpdate).catch((e) => setError(String(e)));
  }, [kind, day]);
  useEffect(load, [load]);

  const write = async () => {
    setBusy(true);
    setError(null);
    try {
      setUpdate(await api.generateStatusUpdate(kind, day));
      setCopied(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

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
    <section className="status-update" aria-labelledby={`status-${kind}`}>
      <h2 id={`status-${kind}`}>{copy.title}</h2>
      <p className="muted">{copy.covers}</p>
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
            <button onClick={write} disabled={busy || !hasKey}>{busy ? "Writing…" : "Write again"}</button>
            <span className="muted">Written by {update.model} for {longDay(day)}. Blockers come only from evidence; add any it cannot see.</span>
          </footer>
        </div>
      ) : (
        <div className="empty">
          <p>{copy.empty}</p>
          <button className="primary" onClick={write} disabled={busy || !hasKey}>{busy ? "Writing…" : `Write the ${kind === "daily" ? "standup" : "weekly status"}`}</button>
          {!hasKey && <p className="muted">Needs a Claude API key, like the journal.</p>}
        </div>
      )}
      {error && <p className="error">{error}</p>}
    </section>
  );
}
