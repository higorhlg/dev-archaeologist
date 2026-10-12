import { useCallback, useEffect, useState } from "react";
import { api, dayKey, longDay, minutes, shiftDay, type DayView } from "./api";

type Props = { day: string; setDay: (d: string) => void; hasKey: boolean; goToSources: () => void };

export default function Journal({ day, setDay, hasKey, goToSources }: Props) {
  const [view, setView] = useState<DayView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    api.day(day).then(setView).catch((e) => setError(String(e)));
  }, [day]);
  useEffect(load, [load]);

  const write = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.generateJournal(day);
      load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const confirm = async () => {
    await api.confirmJournal(day);
    load();
  };

  const journal = view?.journal;
  const sessionCount = view?.sessions.filter((s) => s.state !== "excluded").length ?? 0;
  const citeIndex = new Map<string, number>();
  journal?.lines.forEach((l) => l.cites.forEach((c) => citeIndex.has(c) || citeIndex.set(c, citeIndex.size + 1)));

  return (
    <article className="journal">
      <header className="day-head">
        <div className="day-nav">
          <button aria-label="Previous day" onClick={() => setDay(shiftDay(day, -1))}>‹</button>
          <button aria-label="Next day" disabled={day >= dayKey(new Date())} onClick={() => setDay(shiftDay(day, 1))}>›</button>
        </div>
        <h1>{longDay(day)}</h1>
        <p className="day-total">
          {journal ? (journal.state === "confirmed" ? "Confirmed entry" : "Draft entry") : "No entry yet"}
          {view && <>, <span className="num">{minutes(view.active_min)}</span> active</>}
        </p>
      </header>

      {journal && journal.lines.length > 0 ? (
        <div className="entry">
          {journal.lines.map((l, i) => (
            <p key={i}>
              {l.text}
              {l.cites.map((c) => (
                <sup key={c} title={`Evidence ${c}`}>{citeIndex.get(c)}</sup>
              ))}
            </p>
          ))}
          <footer>
            {journal.state !== "confirmed" && <button className="primary" onClick={confirm}>Confirm entry</button>}
            <button onClick={write} disabled={busy || !hasKey}>{busy ? "Writing…" : "Write again"}</button>
            <span className="muted">Written by {journal.model}. Every line cites the events behind it.</span>
          </footer>
        </div>
      ) : (
        <div className="empty">
          {sessionCount === 0 ? (
            <p>No sessions on this day, so there is nothing to write about.</p>
          ) : hasKey ? (
            <>
              <p>Write this day's entry from its {sessionCount} sessions. One call sends a short summary of the day, never raw logs.</p>
              <button className="primary" onClick={write} disabled={busy}>{busy ? "Writing…" : "Write the entry"}</button>
            </>
          ) : (
            <>
              <p>Add a Claude API key to write journal entries. Your timeline works without one.</p>
              <button onClick={goToSources}>Add a key in Sources</button>
            </>
          )}
        </div>
      )}

      {view && view.achievements.length > 0 && (
        <section className="finds">
          <h2>Finds</h2>
          <ul className="badges">
            {view.achievements.map((a) => (
              <li key={a.id}>
                <b>{a.title}</b>
                <span>{a.detail}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {error && <p className="error">{error}</p>}
    </article>
  );
}
