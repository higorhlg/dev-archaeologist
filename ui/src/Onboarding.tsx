import { useEffect, useState } from "react";
import { api, type BackfillReport, type Discovery } from "./api";

const DAYS = 90;

export default function Onboarding({ onDone }: { onDone: () => void }) {
  const [found, setFound] = useState<Discovery | null>(null);
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<BackfillReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.discover(DAYS).then(setFound).catch((e) => setError(String(e)));
  }, []);

  const rebuild = async () => {
    setRunning(true);
    setError(null);
    try {
      setReport(await api.backfill(DAYS));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="onboard">
      <h1>Your computer already knows what you worked on.</h1>
      <p className="lede">
        Dev Archaeologist reads the logs your tools already keep on this Mac and rebuilds your work history from
        them. Nothing leaves this machine while it does.
      </p>

      {!found && !error && <p className="muted">Looking for local history…</p>}
      {found && (
        <div className="found">
          <h2>Found in the last {DAYS} days</h2>
          <dl>
            <dt>Claude Code sessions</dt>
            <dd>
              {found.claude_sessions}
              <small>{found.claude_profiles} profiles</small>
            </dd>
            <dt>Codex sessions</dt>
            <dd>{found.codex_sessions}</dd>
            <dt>Git repositories</dt>
            <dd>
              {found.repos.length}
              <small>read from .git folders, no GitHub login</small>
            </dd>
            <dt>Terminal history</dt>
            <dd className={found.shell_history_has_times ? "" : "warn"}>
              {found.shell_history_has_times ? "has times" : "no timestamps"}
              {!found.shell_history_has_times && <small>Terminal capture starts once you add the shell hook.</small>}
            </dd>
          </dl>
          {!report && (
            <button className="primary" disabled={running} onClick={rebuild}>
              {running ? "Rebuilding your history…" : "Rebuild my history"}
            </button>
          )}
        </div>
      )}

      {report && (
        <div className="found done">
          <h2>History rebuilt</h2>
          <p>
            {report.sessions} work sessions from {report.claude_code + report.codex} agent events and {report.git}{" "}
            commits.
          </p>
          <button className="primary" onClick={onDone}>
            Open my timeline
          </button>
        </div>
      )}
      {error && <p className="error">{error}</p>}
    </div>
  );
}
