import React, { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ask, save } from "@tauri-apps/plugin-dialog";

import { api, canManage, canVote, Conflict, Criteria, errorText, Estimate, Progress, RecordItem, safeFileName, StageDetail, Summary } from "./api";

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
        {record.doi && <> · <button className="link" onClick={() => api.openUrl(`https://doi.org/${record.doi}`)}>DOI</button></>}
      </p>
      <p className="abstract">
        {record.abstract ? <Highlighted text={record.abstract} criteria={criteria} /> : <em>Kein Abstract vorhanden.</em>}
      </p>
    </article>
  );
}

export function ScreeningPage() {
  const { reviewId = "", stageId = "" } = useParams();
  const [detail, setDetail] = useState<StageDetail | null>(null);
  const [record, setRecord] = useState<RecordItem | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [reasons, setReasons] = useState<string[]>([]);
  const [note, setNote] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const loadNext = useCallback(async () => {
    const next = await api.next(reviewId, stageId);
    setRecord(next.record);
    setProgress(next.progress);
    setReasons([]);
    setNote("");
  }, [reviewId, stageId]);

  useEffect(() => {
    (async () => {
      try {
        setDetail(await api.stage(reviewId, stageId));
        await loadNext();
      } catch (e) {
        setError(errorText(e));
      }
    })();
  }, [reviewId, stageId, loadNext]);

  const send = useCallback(
    async (action: (recordId: string) => Promise<void>) => {
      if (!record || busy) return;
      setBusy(true);
      setError("");
      try {
        await action(record.id);
        await loadNext();
      } catch (e) {
        setError(errorText(e));
      } finally {
        setBusy(false);
      }
    },
    [record, busy, loadNext],
  );

  const vote = useCallback(
    (decision: string) => send((id) => api.vote(reviewId, stageId, id, decision, reasons, note)),
    [send, reviewId, stageId, reasons, note],
  );

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
      <p><Link to={`/reviews/${reviewId}`}>← Review</Link> · <Link to={`/reviews/${reviewId}/stages/${stageId}`}>Auswertung</Link></p>
      <h1>{stage.name}</h1>
      {progress && (
        <>
          <progress value={progress.done} max={Math.max(progress.total, 1)} />
          <p className="muted">{progress.done} von {progress.total} bewertet{stage.blind_active && " · verblindet"}</p>
        </>
      )}
      {error && <p className="error">{error}</p>}
      {stage.closed && <p className="notice">Diese Stufe ist geschlossen.</p>}
      {!record && !stage.closed && <p className="notice">Du hast alle Records dieser Stufe bewertet. Speichere auf der Review-Seite eine Rückmeldung und schick sie an die Person, die das Review leitet.</p>}
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
          <button className="link" disabled={busy} onClick={() => send((id) => api.flag(reviewId, stageId, id, note))}>
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

/** Konfidenzintervall in Klammern, z. B. „(0,41–0,63)“. */
function ci(e: Estimate, scale = 1, digits = 2, unit = ""): string {
  if (e.ci_low === null || e.ci_high === null) return "";
  return `(${(e.ci_low * scale).toFixed(digits)}–${(e.ci_high * scale).toFixed(digits)}${unit})`;
}

export function StagePage() {
  const { reviewId = "", stageId = "" } = useParams();
  const [detail, setDetail] = useState<StageDetail | null>(null);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [conflicts, setConflicts] = useState<Conflict[]>([]);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    try {
      const d = await api.stage(reviewId, stageId);
      setDetail(d);
      setSummary(await api.summary(reviewId, stageId));
      setConflicts(d.stage.blind_active ? [] : await api.conflicts(reviewId, stageId));
    } catch (e) {
      setError(errorText(e));
    }
  }, [reviewId, stageId]);

  useEffect(() => {
    load();
  }, [load]);

  async function run(action: () => Promise<unknown>, done: string) {
    setError("");
    setNotice("");
    try {
      await action();
      setNotice(done);
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function unblind() {
    const ok = await ask("Stufe entblinden? Danach sehen alle, die ein neues Paket bekommen, die Stimmen der anderen.", {
      title: "Entblinden", kind: "warning", okLabel: "Entblinden", cancelLabel: "Abbrechen",
    });
    if (ok) await run(() => api.unblind(reviewId, stageId), "Stufe entblindet. Schick den Mitgliedern ein neues Paket.");
  }

  async function closeStage() {
    const ok = await ask("Stufe schließen? Danach sind keine Stimmen mehr möglich, auch keine nachgereichten.", {
      title: "Stufe schließen", kind: "warning", okLabel: "Schließen", cancelLabel: "Abbrechen",
    });
    if (ok) await run(() => api.close(reviewId, stageId), "Stufe geschlossen, Snapshot erstellt.");
  }

  async function exportCsv() {
    const path = await save({
      defaultPath: `${safeFileName(detail!.stage.name)}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (path) await run(() => api.exportCsv(reviewId, stageId, path), "CSV gespeichert.");
  }

  if (!detail || !summary) return <p>{error || "Lade …"}</p>;
  const { stage, role, criteria } = detail;
  const manage = canManage(role);
  const d = summary.decisions;
  const a = summary.agreement;

  return (
    <>
      <p><Link to={`/reviews/${reviewId}`}>← Review</Link></p>
      <h1>{stage.name}</h1>
      <p className="muted">
        {stage.n_records} Records · mindestens {stage.min_reviewers} Stimmen ·{" "}
        {stage.closed ? "geschlossen" : stage.blind_active ? "verblindet" : "offen"}
        {stage.seed !== null && ` · Stichprobe mit Seed ${stage.seed}`}
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <div className="actions">
        {canVote(role) && !stage.closed && <Link className="button primary" to={`/reviews/${reviewId}/stages/${stageId}/screen`}>Screenen</Link>}
        {manage && stage.blind_active && <button onClick={unblind}>Entblinden</button>}
        {manage && !stage.closed && <button onClick={closeStage}>Stufe schließen</button>}
        {!stage.blind_active && <button onClick={exportCsv}>CSV exportieren</button>}
      </div>

      <section className="card">
        <h2>Fortschritt</h2>
        <p>Du: {summary.progress.done} von {summary.progress.total} bewertet.</p>
        {summary.reviewers && (
          <table>
            <thead><tr><th>Person</th><th>Stimmen</th></tr></thead>
            <tbody>
              {summary.reviewers.map((r) => (
                <tr key={r.name}><td>{r.name}</td><td>{r.n_votes}</td></tr>
              ))}
            </tbody>
          </table>
        )}
        {stage.blind_active && <p className="muted">Die Stufe ist verblindet. Entscheidungen, Konflikte und Übereinstimmung erscheinen nach dem Entblinden. Stimmen anderer kommen über deren Rückmeldungen herein.</p>}
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
            {" "}{ci(a.percent_ci, 100, 1, " %")} über {a.percent.n_items} Records
            <br />
            Krippendorffs Alpha: {fmt(a.krippendorff.alpha)} {ci(a.alpha_ci)}
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
          <p className="muted">
            In Klammern: 95-%-Konfidenzintervall, Perzentil-Bootstrap über Records ({a.alpha_ci.n_boot} Ziehungen, Seed {a.alpha_ci.seed}).
            Kappa allein kann bei seltenen Einschlüssen täuschen; deshalb stehen Prozent und PABAK daneben.
          </p>
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
                  <li key={v.name}>
                    <span>
                      {v.name}: <strong>{DECISION_LABEL[v.decision] ?? v.decision}</strong>
                      {v.reasons.length > 0 && <span className="muted"> · {v.reasons.join(", ")}</span>}
                      {v.note && <span className="muted"> · {v.note}</span>}
                    </span>
                  </li>
                ))}
              </ul>
              {canVote(role) && !stage.closed && (
                <div className="decision-bar">
                  <button className="include" onClick={() => run(() => api.consensus(reviewId, stageId, c.record.id, "include"), "Konsens gespeichert.")}>Konsens: Include</button>
                  <button className="exclude" onClick={() => run(() => api.consensus(reviewId, stageId, c.record.id, "exclude"), "Konsens gespeichert.")}>Konsens: Exclude</button>
                </div>
              )}
            </div>
          ))}
        </section>
      )}
    </>
  );
}
