import React, { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";

import { api, canManage, canVote, Conflict, Criteria, errorText, Progress, RecordItem, StageDetail, Summary } from "./api";

const DECISION_LABEL: Record<string, string> = { include: "Include", exclude: "Exclude", maybe: "Maybe" };

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Hebt Include-Keywords grün und Exclude-Keywords rot hervor. */
function Highlighted({ text, criteria }: { text: string; criteria: Criteria }) {
  const include = criteria.include_keywords.filter(Boolean);
  const exclude = criteria.exclude_keywords.filter(Boolean);
  const all = [...include, ...exclude];
  if (!text || all.length === 0) return <>{text}</>;
  const pattern = new RegExp(`(${all.map(escapeRegExp).join("|")})`, "gi");
  const excludeSet = new Set(exclude.map((k) => k.toLowerCase()));
  const includeSet = new Set(include.map((k) => k.toLowerCase()));
  return (
    <>
      {text.split(pattern).map((part, index) => {
        const lower = part.toLowerCase();
        if (excludeSet.has(lower)) return <mark key={index} className="exc">{part}</mark>;
        if (includeSet.has(lower)) return <mark key={index} className="inc">{part}</mark>;
        return <React.Fragment key={index}>{part}</React.Fragment>;
      })}
    </>
  );
}

function RecordView({ record, criteria }: { record: RecordItem; criteria: Criteria }) {
  return (
    <article>
      <h2><Highlighted text={record.title || "(ohne Titel)"} criteria={criteria} /></h2>
      <p className="muted">
        {[record.authors, record.year, record.journal].filter(Boolean).join(" · ")}
        {record.doi && <> · <a href={`https://doi.org/${record.doi}`} target="_blank" rel="noreferrer">DOI</a></>}
      </p>
      <p className="abstract">
        {record.abstract ? <Highlighted text={record.abstract} criteria={criteria} /> : <em>Kein Abstract vorhanden.</em>}
      </p>
    </article>
  );
}

export function ScreeningPage() {
  const { stageId } = useParams();
  const [detail, setDetail] = useState<StageDetail | null>(null);
  const [record, setRecord] = useState<RecordItem | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [reasons, setReasons] = useState<string[]>([]);
  const [note, setNote] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const loadNext = useCallback(async () => {
    const next = await api<{ record: RecordItem | null; progress: Progress }>(`/stages/${stageId}/next`);
    setRecord(next.record);
    setProgress(next.progress);
    setReasons([]);
    setNote("");
  }, [stageId]);

  useEffect(() => {
    (async () => {
      try {
        setDetail(await api<StageDetail>(`/stages/${stageId}`));
        await loadNext();
      } catch (e) {
        setError(errorText(e));
      }
    })();
  }, [stageId, loadNext]);

  const send = useCallback(
    async (path: string, body: Record<string, unknown>) => {
      if (!record || busy) return;
      setBusy(true);
      setError("");
      try {
        await api(`/stages/${stageId}/${path}`, { method: "POST", body: { record_id: record.id, ...body } });
        await loadNext();
      } catch (e) {
        setError(errorText(e));
      } finally {
        setBusy(false);
      }
    },
    [record, busy, stageId, loadNext],
  );

  const vote = useCallback((decision: string) => send("votes", { decision, reasons, note }), [send, reasons, note]);

  // Tastatur: i = Include, e = Exclude, m = Maybe. Nicht, solange in einem Feld getippt wird.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      const decision = ({ i: "include", e: "exclude", m: "maybe" } as Record<string, string>)[event.key.toLowerCase()];
      if (decision) vote(decision);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [vote]);

  if (!detail) return <p>{error || "Lade …"}</p>;
  const { stage, criteria } = detail;

  return (
    <>
      <p><Link to={`/reviews/${stage.review_id}`}>← Review</Link> · <Link to={`/stages/${stage.id}`}>Auswertung</Link></p>
      <h1>{stage.name}</h1>
      {progress && (
        <>
          <progress value={progress.done} max={Math.max(progress.total, 1)} />
          <p className="muted">{progress.done} von {progress.total} bewertet{stage.blind_active && " · verblindet"}</p>
        </>
      )}
      {error && <p className="error">{error}</p>}
      {stage.closed && <p className="notice">Diese Stufe ist geschlossen.</p>}
      {!record && !stage.closed && <p className="notice">Du hast alle Records dieser Stufe bewertet.</p>}
      {record && !stage.closed && (
        <div className="card">
          <RecordView record={record} criteria={criteria} />
          {criteria.exclusion_reasons.length > 0 && (
            <fieldset>
              <legend>Ausschlussgründe</legend>
              {criteria.exclusion_reasons.map((reason) => (
                <label className="check" key={reason}>
                  <input
                    type="checkbox"
                    checked={reasons.includes(reason)}
                    onChange={(e) => setReasons(e.target.checked ? [...reasons, reason] : reasons.filter((r) => r !== reason))}
                  />
                  {reason}
                </label>
              ))}
            </fieldset>
          )}
          <label>
            Notiz
            <input value={note} onChange={(e) => setNote(e.target.value)} />
          </label>
          <div className="decision-bar">
            <button className="include" disabled={busy} onClick={() => vote("include")}>Include (i)</button>
            <button className="maybe" disabled={busy} onClick={() => vote("maybe")}>Maybe (m)</button>
            <button className="exclude" disabled={busy} onClick={() => vote("exclude")}>Exclude (e)</button>
          </div>
          <button className="link" disabled={busy} onClick={() => send("flags", { note })}>
            Nicht entscheidbar, zur Klärung geben
          </button>
        </div>
      )}
    </>
  );
}

function fmt(value: number | null | undefined, digits = 2): string {
  return value === null || value === undefined ? "–" : value.toFixed(digits);
}

export function StagePage() {
  const { stageId } = useParams();
  const [detail, setDetail] = useState<StageDetail | null>(null);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [conflicts, setConflicts] = useState<Conflict[]>([]);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    try {
      const d = await api<StageDetail>(`/stages/${stageId}`);
      setDetail(d);
      setSummary(await api<Summary>(`/stages/${stageId}/summary`));
      setConflicts(d.stage.blind_active ? [] : await api<Conflict[]>(`/stages/${stageId}/conflicts`));
    } catch (e) {
      setError(errorText(e));
    }
  }, [stageId]);

  useEffect(() => {
    load();
  }, [load]);

  async function run(path: string, body: Record<string, unknown> | undefined, done: string) {
    setError("");
    setNotice("");
    try {
      await api(`/stages/${stageId}/${path}`, { method: "POST", body: body ?? {} });
      setNotice(done);
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  }

  if (!detail || !summary) return <p>{error || "Lade …"}</p>;
  const { stage, role, criteria } = detail;
  const manage = canManage(role);
  const d = summary.decisions;
  const a = summary.agreement;

  return (
    <>
      <p><Link to={`/reviews/${stage.review_id}`}>← Review</Link></p>
      <h1>{stage.name}</h1>
      <p className="muted">
        {stage.n_records} Records · mindestens {stage.min_reviewers} Stimmen ·{" "}
        {stage.closed ? "geschlossen" : stage.blind_active ? "verblindet" : "offen"}
        {stage.seed !== null && ` · Stichprobe mit Seed ${stage.seed}`}
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <div className="actions">
        {canVote(role) && !stage.closed && <Link className="button primary" to={`/stages/${stage.id}/screen`}>Screenen</Link>}
        {manage && stage.blind_active && <button onClick={() => run("unblind", undefined, "Stufe entblindet.")}>Entblinden</button>}
        {manage && !stage.closed && (
          <button onClick={() => window.confirm("Stufe schließen? Danach sind keine Stimmen mehr möglich.") && run("close", undefined, "Stufe geschlossen, Snapshot erstellt.")}>
            Stufe schließen
          </button>
        )}
        {!stage.blind_active && <a className="button" href={`/api/stages/${stage.id}/export.csv`}>CSV exportieren</a>}
      </div>

      <section className="card">
        <h2>Fortschritt</h2>
        <p>Du: {summary.progress.done} von {summary.progress.total} bewertet.</p>
        {summary.reviewers && (
          <table>
            <thead><tr><th>Person</th><th>Stimmen</th></tr></thead>
            <tbody>
              {summary.reviewers.map((r) => (
                <tr key={r.username}><td>{r.username}</td><td>{r.n_votes}</td></tr>
              ))}
            </tbody>
          </table>
        )}
        {stage.blind_active && <p className="muted">Die Stufe ist verblindet. Entscheidungen, Konflikte und Übereinstimmung erscheinen nach dem Entblinden.</p>}
      </section>

      {d && (
        <section className="card">
          <h2>Entscheidungen</h2>
          <table>
            <tbody>
              <tr><td>Include</td><td>{d.include}</td></tr>
              <tr><td>Exclude</td><td>{d.exclude}</td></tr>
              {d.maybe > 0 && <tr><td>Maybe</td><td>{d.maybe}</td></tr>}
              <tr><td>Widerspruch</td><td>{d.disagreement}</td></tr>
              <tr><td>Zur Klärung</td><td>{d.needs_clarification}</td></tr>
              <tr><td>Offen</td><td>{d.pending}</td></tr>
            </tbody>
          </table>
        </section>
      )}

      {a && (
        <section className="card">
          <h2>Übereinstimmung</h2>
          <p>
            Prozentuale Übereinstimmung: {a.percent.percent_agreement == null ? "–" : `${(a.percent.percent_agreement * 100).toFixed(1)} %`}
            {" "}über {a.percent.n_items ?? 0} Records · Krippendorffs Alpha: {fmt(a.krippendorff.alpha)}
          </p>
          {a.cohen_pairs.length > 0 && (
            <table>
              <thead><tr><th>Paar</th><th>n</th><th>Kappa</th><th>PABAK</th></tr></thead>
              <tbody>
                {a.cohen_pairs.map((p) => (
                  <tr key={`${p.rater_a}-${p.rater_b}`}>
                    <td>{p.rater_a} / {p.rater_b}</td><td>{p.n_items}</td><td>{fmt(p.kappa)}</td><td>{fmt(p.pabak)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <p className="muted">Punktschätzer ohne Konfidenzintervalle.</p>
        </section>
      )}

      {!stage.blind_active && (
        <section>
          <h2>Konflikte und Klärung ({conflicts.length})</h2>
          {conflicts.length === 0 && <p className="muted">Keine offenen Konflikte.</p>}
          {conflicts.map((c) => (
            <div className="card" key={c.record.id}>
              <RecordView record={c.record} criteria={criteria} />
              {c.status === "needs_clarification" && <p className="badge">als nicht entscheidbar markiert</p>}
              <ul className="list">
                {c.votes.map((v) => (
                  <li key={v.username}>
                    <span>
                      {v.username}: <strong>{DECISION_LABEL[v.decision] ?? v.decision}</strong>
                      {v.reasons.length > 0 && <span className="muted"> · {v.reasons.join(", ")}</span>}
                      {v.note && <span className="muted"> · {v.note}</span>}
                    </span>
                  </li>
                ))}
              </ul>
              {canVote(role) && !stage.closed && (
                <div className="decision-bar">
                  <button className="include" onClick={() => run("consensus", { record_id: c.record.id, decision: "include" }, "Konsens gespeichert.")}>Konsens: Include</button>
                  <button className="exclude" onClick={() => run("consensus", { record_id: c.record.id, decision: "exclude" }, "Konsens gespeichert.")}>Konsens: Exclude</button>
                </div>
              )}
            </div>
          ))}
        </section>
      )}
    </>
  );
}
