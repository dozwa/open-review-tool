import React, { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ask, save } from "@tauri-apps/plugin-dialog";

import { api, canManage, canVote, Conflict, CONFLICT_LABEL, Criteria, errorText, Estimate, gateText, Progress, RecordItem, RULE_LABEL, safeFileName, StageDetail, Summary, term } from "./api";
import { stageState } from "./ReviewPage";
import { Avatar, Banner, DECISION_PARTS, DecisionBar, Empty, Group, Icon, Kbd, PageHeader, Pill, Row, Stat } from "./ui";

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

function RecordView({ record, criteria, large = false }: { record: RecordItem; criteria: Criteria; large?: boolean }) {
  const meta = [record.authors, record.journal, record.year].filter(Boolean).join(" · ");
  return (
    <article className={large ? "record record-large" : "record"}>
      <h2><Highlighted text={record.title || "(ohne Titel)"} criteria={criteria} /></h2>
      <p className="record-meta">
        {meta}
        {record.doi && (
          <> · <button className="link" onClick={() => api.openUrl(`https://doi.org/${record.doi}`)}>doi:{record.doi}</button></>
        )}
      </p>
      {record.abstract ? (
        <p className="abstract"><Highlighted text={record.abstract} criteria={criteria} /></p>
      ) : (
        <p className="abstract missing">Kein Abstract vorhanden.</p>
      )}
      {record.keywords && <div className="chips">{record.keywords.split(";").map((k) => k.trim()).filter(Boolean).map((k) => <span className="chip" key={k}>{k}</span>)}</div>}
    </article>
  );
}

// ---------------------------------------------------------------- Screening

export function ScreeningPage() {
  const { reviewId = "", stageId = "" } = useParams();
  const [detail, setDetail] = useState<StageDetail | null>(null);
  const [record, setRecord] = useState<RecordItem | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [reasons, setReasons] = useState<string[]>([]);
  const [note, setNote] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [last, setLast] = useState("");

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
    async (action: (recordId: string) => Promise<void>, label: string) => {
      if (!record || busy) return;
      setBusy(true);
      setError("");
      try {
        await action(record.id);
        setLast(label);
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
    (decision: string) => send((id) => api.vote(reviewId, stageId, id, decision, reasons, note), DECISION_LABEL[decision]),
    [send, reviewId, stageId, reasons, note],
  );

  // Tastatur: i = Include, e = Exclude, m = Maybe, 1–9 = Ausschlussgrund. Nicht, solange in einem Feld getippt wird.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      const decision = ({ i: "include", e: "exclude", m: "maybe" } as Record<string, string>)[event.key.toLowerCase()];
      if (decision) {
        vote(decision);
        return;
      }
      const n = Number(event.key);
      const reason = detail?.criteria.exclusion_reasons[n - 1];
      if (n >= 1 && reason) setReasons((r) => (r.includes(reason) ? r.filter((x) => x !== reason) : [...r, reason]));
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [vote, detail]);

  if (!detail) return error ? <Banner kind="error">{error}</Banner> : null;
  const { stage, criteria } = detail;
  const pct = progress ? Math.round((progress.done / Math.max(progress.total, 1)) * 100) : 0;

  return (
    <div className="screening">
      <div className="screening-top">
        <Link to={`/reviews/${reviewId}`} className="back-link"><Icon name="back" size={14} /> Review</Link>
        <div className="screening-title">
          <strong>{stage.name}</strong>
          {stage.blind_active && <Pill tone="violet"><Icon name="lock" size={11} /> verblindet</Pill>}
        </div>
        <span className="screening-count">{progress ? `${progress.done} / ${progress.total}` : ""}</span>
      </div>
      <div className="progress-line"><span style={{ width: `${pct}%` }} /></div>

      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      {stage.closed && <Empty icon="lock" title="Diese Stufe ist geschlossen" />}
      {!record && !stage.closed && (
        <Empty icon="check" title="Alles bewertet">
          Speichere im Review unter „Austausch“ eine Rückmeldung und schick sie an die Leitung.
          <div><Link className="button primary" to={`/reviews/${reviewId}`}>Zum Review</Link></div>
        </Empty>
      )}
      {record && !stage.closed && (
        <>
          <div className="reading">
            <RecordView record={record} criteria={criteria} large />
          </div>
          <div className="decision-dock">
            {criteria.exclusion_reasons.length > 0 && (
              <div className="reasons" role="group" aria-label="Ausschlussgründe">
                {criteria.exclusion_reasons.map((reason, i) => (
                  <button
                    key={reason}
                    className={`chip toggle ${reasons.includes(reason) ? "on" : ""}`}
                    aria-pressed={reasons.includes(reason)}
                    onClick={() => setReasons(reasons.includes(reason) ? reasons.filter((r) => r !== reason) : [...reasons, reason])}
                  >
                    {i < 9 && <Kbd>{i + 1}</Kbd>} {reason}
                  </button>
                ))}
              </div>
            )}
            <div className="dock-row">
              <input className="note" value={note} onChange={(e) => setNote(e.target.value)} placeholder="Notiz (optional)" aria-label="Notiz" />
              <button className="ghost" disabled={busy} onClick={() => send((id) => api.flag(reviewId, stageId, id, note), "Zur Klärung")} title="Nicht entscheidbar, zur Klärung geben">
                <Icon name="flag" size={14} /> Klärung
              </button>
            </div>
            <div className="vote-buttons">
              <button className="vote vote-include" disabled={busy} onClick={() => vote("include")}>
                <Icon name="check" /> Include <Kbd>I</Kbd>
              </button>
              <button className="vote vote-maybe" disabled={busy} onClick={() => vote("maybe")}>
                <Icon name="help" /> Maybe <Kbd>M</Kbd>
              </button>
              <button className="vote vote-exclude" disabled={busy} onClick={() => vote("exclude")}>
                <Icon name="x" /> Exclude <Kbd>E</Kbd>
              </button>
            </div>
            {last && <p className="last-vote">Gespeichert: {last}</p>}
          </div>
        </>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- Auswertung

function fmt(value: number | null | undefined, digits = 2): string {
  return value === null || value === undefined ? "–" : value.toLocaleString("de-DE", { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

/** Konfidenzintervall, z. B. „95-%-KI 0,41–0,63“. */
function ci(e: Estimate, scale = 1, digits = 2, unit = ""): string {
  if (e.ci_low === null || e.ci_high === null) return "kein Intervall";
  return `95-%-KI ${fmt(e.ci_low * scale, digits)}–${fmt(e.ci_high * scale, digits)}${unit}`;
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

  if (!detail || !summary) return error ? <Banner kind="error">{error}</Banner> : null;
  const { stage, role, criteria } = detail;
  const manage = canManage(role);
  const records = term(detail.terms, "records", "Einträge");
  const d = summary.decisions;
  const a = summary.agreement;
  const maxVotes = Math.max(1, ...(summary.reviewers ?? []).map((r) => r.n_votes));

  async function unblind() {
    const ok = await ask("Danach sehen alle, die ein neues Paket bekommen, die Stimmen der anderen. Das lässt sich nicht zurücknehmen.", {
      title: "Stufe entblinden?", kind: "warning", okLabel: "Entblinden", cancelLabel: "Abbrechen",
    });
    if (ok) await run(() => api.unblind(reviewId, stageId), "Stufe entblindet. Schick den Mitgliedern ein neues Paket.");
  }

  async function closeStage() {
    // Vorab zeigen, was das Gate der Station ergäbe. Verblindet gibt der Kern darüber keine Auskunft.
    const gate = await api.gate(reviewId, stageId).catch(() => null);
    let text = "Danach sind keine Stimmen mehr möglich, auch keine nachgereichten. Der Stand wird als Snapshot festgehalten.";
    if (gate) {
      text += gate.passed
        ? `\n\nDas Gate der Station ist erfüllt: ${gateText(gate)}.`
        : `\n\nDas Gate der Station ist nicht erfüllt: ${gateText(gate)}. Die nächste Station bleibt gesperrt, bis eine neue Runde das Gate erfüllt oder die Leitung es mit Begründung übersteuert.`;
    } else if (stage.blind_active) {
      text += "\n\nDie Stufe ist noch verblindet. Konflikte lassen sich erst nach dem Entblinden klären.";
    }
    const ok = await ask(text, { title: "Stufe schließen?", kind: "warning", okLabel: "Schließen", cancelLabel: "Abbrechen" });
    if (!ok) return;
    setError("");
    setNotice("");
    try {
      const closed = await api.close(reviewId, stageId);
      const result = closed.gate ? ` Gate ${closed.gate.passed ? "erfüllt" : "nicht erfüllt"}: ${gateText(closed.gate)}.` : "";
      setNotice(`Stufe geschlossen, Snapshot erstellt.${result}`);
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function exportCsv() {
    const path = await save({ defaultPath: `${safeFileName(stage.name)}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (path) await run(() => api.exportCsv(reviewId, stageId, path), "CSV gespeichert.");
  }

  const counts: Record<string, number> = d ? { ...d } : {};

  return (
    <>
      <PageHeader
        back={<Link to={`/reviews/${reviewId}`} className="back-link"><Icon name="back" size={14} /> Review</Link>}
        title={stage.name}
        subtitle={
          <>
            <Pill>{stageState(stage)}</Pill>
            <span>{stage.n_records.toLocaleString("de-DE")} {records}</span>
            <span>{RULE_LABEL[stage.decision_rule] ?? stage.decision_rule}</span>
            {stage.decision_rule === "unanimous" && <span>mind. {stage.min_reviewers} Stimmen</span>}
            {stage.verify && <span>{stage.verify.scope === "sample" ? `Prüfung von ${stage.verify.percent} % der Ausschlüsse, Seed ${stage.verify.seed}` : "Prüfung aller Ausschlüsse"}</span>}
            {stage.conflict_rule !== "discussion" && <span>Klärung: {CONFLICT_LABEL[stage.conflict_rule] ?? stage.conflict_rule}</span>}
            {stage.seed !== null && <span>Stichprobe, Seed {stage.seed}</span>}
          </>
        }
      >
        {!stage.blind_active && <button onClick={exportCsv}><Icon name="csv" size={14} /> CSV</button>}
        {manage && stage.blind_active && <button onClick={unblind}><Icon name="eye" size={14} /> Entblinden</button>}
        {manage && !stage.closed && <button onClick={closeStage}><Icon name="lock" size={14} /> Schließen</button>}
        {canVote(role) && !stage.closed && (
          <Link className="button primary" to={`/reviews/${reviewId}/stages/${stageId}/screen`}><Icon name="play" size={14} /> Screenen</Link>
        )}
      </PageHeader>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      {notice && <Banner kind="notice" onClose={() => setNotice("")}>{notice}</Banner>}

      <div className="stats">
        <Stat label="Dein Fortschritt" value={`${summary.progress.done} / ${summary.progress.total}`} detail={`${Math.round((summary.progress.done / Math.max(summary.progress.total, 1)) * 100)} % bewertet`} />
        {a && <Stat label="Übereinstimmung" value={a.percent.percent_agreement == null ? "–" : `${fmt(a.percent.percent_agreement * 100, 1)} %`} detail={`${ci(a.percent_ci, 100, 1, " %")} · ${a.percent.n_items} ${records}`} />}
        {a && <Stat label="Krippendorffs Alpha" value={fmt(a.krippendorff.alpha)} detail={ci(a.alpha_ci)} />}
        {!a && <Stat label="Konflikte" value={<Icon name="lock" size={20} />} detail="sichtbar nach dem Entblinden" />}
      </div>

      {d && (
        <Group title="Entscheidungen">
          <div className="decisions">
            <DecisionBar counts={counts} />
            <div className="decision-legend">
              {DECISION_PARTS.map((p) => (
                <Stat key={p.key} tone={p.tone} label={p.label} value={(counts[p.key] ?? 0).toLocaleString("de-DE")} />
              ))}
            </div>
          </div>
        </Group>
      )}

      {summary.reviewers && (
        <Group title="Team" footer={stage.blind_active ? "Verblindet: Hier steht nur, wie viele Stimmen eingegangen sind. Neue Stimmen kommen über Rückmeldungen herein." : undefined}>
          {summary.reviewers.length === 0 && <Row><span className="muted">Noch keine Stimmen eingegangen.</span></Row>}
          {summary.reviewers.map((r) => (
            <Row key={r.name}>
              <span className="person"><Avatar name={r.name} /> {r.name}</span>
              <span className="meter-cell">
                <span className="meter"><span style={{ width: `${(r.n_votes / Math.max(stage.n_records, maxVotes)) * 100}%` }} /></span>
                <span className="num">{r.n_votes}</span>
              </span>
            </Row>
          ))}
        </Group>
      )}

      {a && a.cohen_pairs.length > 0 && (
        <Group title="Paare" footer="Kappa allein kann bei seltenen Einschlüssen täuschen (Kappa-Paradox); deshalb stehen Prozent und PABAK daneben. Intervalle: Perzentil-Bootstrap über die Einträge.">
          <table className="table">
            <thead><tr><th>Paar</th><th className="num">{records}</th><th className="num">Übereinstimmung</th><th className="num">Kappa</th><th className="num">PABAK</th></tr></thead>
            <tbody>
              {a.cohen_pairs.map((p) => (
                <tr key={`${p.rater_a}-${p.rater_b}`}>
                  <td>{p.rater_a} · {p.rater_b}</td>
                  <td className="num">{p.n_items}</td>
                  <td className="num">{p.p_o == null ? "–" : `${fmt(p.p_o * 100, 1)} %`}</td>
                  <td className="num">{fmt(p.kappa)}</td>
                  <td className="num">{fmt(p.pabak)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Group>
      )}

      {!stage.blind_active && (
        <>
          <h2 className="section-title">Klärung <span className="count">{conflicts.length}</span></h2>
          {conflicts.length === 0 && <Empty icon="check" title="Keine offenen Konflikte" />}
          {conflicts.map((c) => (
            <div className="conflict" key={c.record.id}>
              <RecordView record={c.record} criteria={criteria} />
              <div className="votes">
                {c.status === "needs_clarification" && <Pill tone="violet"><Icon name="flag" size={11} /> als nicht entscheidbar markiert</Pill>}
                {c.votes.map((v) => (
                  <div className="vote-chip" key={v.name}>
                    <Avatar name={v.name} size={22} />
                    <span>
                      <strong>{v.name}</strong> <span className={`decision-text d-${v.decision}`}>{DECISION_LABEL[v.decision] ?? v.decision}</span>
                      {(v.reasons.length > 0 || v.note) && <small>{[v.reasons.join(", "), v.note].filter(Boolean).join(" · ")}</small>}
                    </span>
                  </div>
                ))}
              </div>
              {canVote(role) && !stage.closed && (
                <div className="conflict-actions">
                  <span className="muted">Konsens</span>
                  <button className="vote-include small" onClick={() => run(() => api.consensus(reviewId, stageId, c.record.id, "include"), "Konsens gespeichert.")}><Icon name="check" size={14} /> Include</button>
                  <button className="vote-exclude small" onClick={() => run(() => api.consensus(reviewId, stageId, c.record.id, "exclude"), "Konsens gespeichert.")}><Icon name="x" size={14} /> Exclude</button>
                </div>
              )}
            </div>
          ))}
        </>
      )}
    </>
  );
}
