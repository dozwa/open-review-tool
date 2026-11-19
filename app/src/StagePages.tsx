import React, { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ask, save } from "@tauri-apps/plugin-dialog";

import { api, canManage, canVote, Conflict, CONFLICT_LABEL, Criteria, errorText, Estimate, gateText, reasonsOf, RecordItem, RULE_LABEL, safeFileName, StageDetail, Summary, term } from "./api";
import { stageState } from "./ReviewPage";
import { HelpButton } from "./Guidance";
import { formatNumber, formatPercent, t } from "./i18n";
import { inFolder } from "./settings";
import { Avatar, Banner, DECISION_PARTS, DecisionBar, Empty, Group, Icon, PageHeader, Pill, Row, Stat } from "./ui";

export const DECISION_LABEL: Record<string, string> = { include: "Include", exclude: "Exclude", maybe: "Maybe" };

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

/** Ein Eintrag. Ohne `showSource` bleiben Autoren und Zeitschrift ausgeblendet, damit sie das Urteil nicht färben. */
export function RecordView({ record, criteria, large = false, showSource = true }: {
  record: RecordItem;
  criteria: Criteria;
  large?: boolean;
  showSource?: boolean;
}) {
  const meta = (showSource ? [record.authors, record.journal, record.year] : [record.year]).filter(Boolean).join(" · ");
  return (
    <article className={large ? "record record-large" : "record"}>
      <h2><Highlighted text={record.title || t("(ohne Titel)")} criteria={criteria} /></h2>
      <p className="record-meta">
        {meta}
        {record.doi && showSource && (
          <> · <button className="link" onClick={() => api.openUrl(`https://doi.org/${record.doi}`)}>doi:{record.doi}</button></>
        )}
      </p>
      {record.abstract ? (
        <p className="abstract"><Highlighted text={record.abstract} criteria={criteria} /></p>
      ) : (
        <p className="abstract missing">{t("Kein Abstract vorhanden.")}</p>
      )}
      {record.keywords && <div className="chips">{record.keywords.split(";").map((k) => k.trim()).filter(Boolean).map((k) => <span className="chip" key={k}>{k}</span>)}</div>}
    </article>
  );
}

// ---------------------------------------------------------------- Auswertung

function fmt(value: number | null | undefined, digits = 2): string {
  return value === null || value === undefined ? "–" : formatNumber(value, digits);
}

/** Konfidenzintervall, z. B. „95-%-KI 0,41–0,63“. */
function ci(e: Estimate, percent = false): string {
  if (e.ci_low === null || e.ci_high === null) return t("kein Intervall");
  return percent
    ? t("95-%-KI {low}–{high} %", { low: fmt(e.ci_low * 100, 1), high: fmt(e.ci_high * 100, 1) })
    : t("95-%-KI {low}–{high}", { low: fmt(e.ci_low), high: fmt(e.ci_high) });
}

export function StagePage() {
  const { reviewId = "", stageId = "" } = useParams();
  const [detail, setDetail] = useState<StageDetail | null>(null);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [conflicts, setConflicts] = useState<Conflict[]>([]);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  // Gewählter Hauptgrund je Eintrag für den Konsens „Exclude“
  const [consensusReason, setConsensusReason] = useState<Record<string, string>>({});

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
  const records = term(detail.terms, "records", t("Einträge"));
  const d = summary.decisions;
  const a = summary.agreement;
  const maxVotes = Math.max(1, ...(summary.reviewers ?? []).map((r) => r.n_votes));

  async function unblind() {
    const ok = await ask(t("Danach sehen alle, die ein neues Paket bekommen, die Stimmen der anderen. Das lässt sich nicht zurücknehmen."), {
      title: t("Stufe entblinden?"), kind: "warning", okLabel: t("Entblinden"), cancelLabel: t("Abbrechen"),
    });
    if (ok) await run(() => api.unblind(reviewId, stageId), t("Stufe entblindet. Schick den Mitgliedern ein neues Paket."));
  }

  async function closeStage() {
    // Vorab zeigen, was das Gate der Station ergäbe. Verblindet gibt der Kern darüber keine Auskunft.
    const gate = await api.gate(reviewId, stageId).catch(() => null);
    let text = t("Danach sind keine Stimmen mehr möglich, auch keine nachgereichten. Der Stand wird als Snapshot festgehalten.");
    if (gate) {
      text += "\n\n";
      text += gate.passed
        ? t("Das Gate der Station ist erfüllt: {gate}.", { gate: gateText(gate) })
        : t("Das Gate der Station ist nicht erfüllt: {gate}. Die nächste Station bleibt gesperrt, bis eine neue Runde das Gate erfüllt oder die Leitung es mit Begründung übersteuert.", { gate: gateText(gate) });
    } else if (stage.blind_active) {
      text += "\n\n";
      text += t("Die Stufe ist noch verblindet. Konflikte lassen sich erst nach dem Entblinden klären.");
    }
    const ok = await ask(text, { title: t("Stufe schließen?"), kind: "warning", okLabel: t("Schließen"), cancelLabel: t("Abbrechen") });
    if (!ok) return;
    setError("");
    setNotice("");
    try {
      const closed = await api.close(reviewId, stageId);
      if (!closed.gate) setNotice(t("Stufe geschlossen, Snapshot erstellt."));
      else if (closed.gate.passed) setNotice(t("Stufe geschlossen, Snapshot erstellt. Gate erfüllt: {gate}.", { gate: gateText(closed.gate) }));
      else setNotice(t("Stufe geschlossen, Snapshot erstellt. Gate nicht erfüllt: {gate}.", { gate: gateText(closed.gate) }));
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function exportCsv() {
    const path = await save({ defaultPath: await inFolder("export", `${safeFileName(stage.name)}.csv`, reviewId), filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (path) await run(() => api.exportCsv(reviewId, stageId, path), t("CSV gespeichert."));
  }

  const counts: Record<string, number> = d ? { ...d } : {};

  return (
    <>
      <PageHeader
        back={<Link to={`/reviews/${reviewId}`} className="back-link"><Icon name="back" size={14} /> {t("Review")}</Link>}
        title={stage.name}
        subtitle={
          <>
            <Pill>{stageState(stage)}</Pill>
            <span>{formatNumber(stage.n_records)} {records}</span>
            <span>{RULE_LABEL[stage.decision_rule] ?? stage.decision_rule}</span>
            {stage.decision_rule === "unanimous" && <span>{t("mind. {n} Stimmen", { n: stage.min_reviewers })}</span>}
            {stage.verify && <span>{stage.verify.scope === "sample" ? t("Prüfung von {percent} % der Ausschlüsse, Seed {seed}", { percent: stage.verify.percent ?? "", seed: stage.verify.seed ?? "" }) : t("Prüfung aller Ausschlüsse")}</span>}
            {stage.conflict_rule !== "discussion" && <span>{t("Klärung: {rule}", { rule: CONFLICT_LABEL[stage.conflict_rule] ?? stage.conflict_rule })}</span>}
            {stage.seed !== null && <span>{t("Stichprobe, Seed {seed}", { seed: stage.seed })}</span>}
            {stage.reason_required && <span>{t("Ausschluss nur mit Hauptgrund")}</span>}
            {criteria.version > 0 && <span>{t("Kriterien Version {version}", { version: criteria.version })}</span>}
          </>
        }
        help={<HelpButton reviewId={reviewId} station={stage.station} context={stage.name} />}
      >
        {!stage.blind_active && <button onClick={exportCsv}><Icon name="csv" size={14} /> CSV</button>}
        {manage && stage.blind_active && <button onClick={unblind}><Icon name="eye" size={14} /> {t("Entblinden")}</button>}
        {manage && !stage.closed && <button onClick={closeStage}><Icon name="lock" size={14} /> {t("Schließen")}</button>}
        {canVote(role) && !stage.closed && (
          <Link className="button primary" to={`/reviews/${reviewId}/stages/${stageId}/screen`}><Icon name="play" size={14} /> {t("Screenen")}</Link>
        )}
      </PageHeader>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      {notice && <Banner kind="notice" onClose={() => setNotice("")}>{notice}</Banner>}

      <div className="stats">
        <Stat label={t("Dein Fortschritt")} value={`${summary.progress.done} / ${summary.progress.total}`} detail={t("{percent} % bewertet", { percent: Math.round((summary.progress.done / Math.max(summary.progress.total, 1)) * 100) })} />
        {a && <Stat label={t("Übereinstimmung")} value={a.percent.percent_agreement == null ? "–" : formatPercent(a.percent.percent_agreement * 100, 1)} detail={`${ci(a.percent_ci, true)} · ${a.percent.n_items} ${records}`} />}
        {a && <Stat label={t("Krippendorffs Alpha")} value={fmt(a.krippendorff.alpha)} detail={ci(a.alpha_ci)} />}
        {!a && <Stat label={t("Konflikte")} value={<Icon name="lock" size={20} />} detail={t("sichtbar nach dem Entblinden")} />}
      </div>

      {d && (
        <Group title={t("Entscheidungen")}>
          <div className="decisions">
            <DecisionBar counts={counts} />
            <div className="decision-legend">
              {DECISION_PARTS.map((p) => (
                <Stat key={p.key} tone={p.tone} label={p.label} value={formatNumber(counts[p.key] ?? 0)} />
              ))}
            </div>
          </div>
        </Group>
      )}

      {summary.reviewers && (
        <Group title={t("Team")} footer={stage.blind_active ? t("Verblindet: Hier steht nur, wie viele Stimmen eingegangen sind. Neue Stimmen kommen über Rückmeldungen herein.") : undefined}>
          {summary.reviewers.length === 0 && <Row><span className="muted">{t("Noch keine Stimmen eingegangen.")}</span></Row>}
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
        <Group title={t("Paare")} footer={t("Kappa allein kann bei seltenen Einschlüssen täuschen (Kappa-Paradox); deshalb stehen Prozent und PABAK daneben. Intervalle: Perzentil-Bootstrap über die Einträge.")}>
          <table className="table">
            <thead><tr><th>{t("Paar")}</th><th className="num">{records}</th><th className="num">{t("Übereinstimmung")}</th><th className="num">Kappa</th><th className="num">PABAK</th></tr></thead>
            <tbody>
              {a.cohen_pairs.map((p) => (
                <tr key={`${p.rater_a}-${p.rater_b}`}>
                  <td>{p.rater_a} · {p.rater_b}</td>
                  <td className="num">{p.n_items}</td>
                  <td className="num">{p.p_o == null ? "–" : formatPercent(p.p_o * 100, 1)}</td>
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
          <h2 className="section-title">{t("Klärung")} <span className="count">{conflicts.length}</span></h2>
          {conflicts.length === 0 && <Empty icon="check" title={t("Keine offenen Konflikte")} />}
          {conflicts.map((c) => (
            <div className="conflict" key={c.record.id}>
              <RecordView record={c.record} criteria={criteria} />
              <div className="votes">
                {c.status === "needs_clarification" && <Pill tone="violet"><Icon name="flag" size={11} /> {t("als nicht entscheidbar markiert")}</Pill>}
                {c.votes.map((v) => (
                  <div className="vote-chip" key={v.name}>
                    <Avatar name={v.name} size={22} />
                    <span>
                      <strong>{v.name}</strong> <span className={`decision-text d-${v.decision}`}>{DECISION_LABEL[v.decision] ?? v.decision}</span>
                      {(v.primary_reason || v.reasons.length > 0 || v.note) && (
                        <small>{[v.primary_reason && t("Hauptgrund {reason}", { reason: v.primary_reason }), v.reasons.join(", "), v.note].filter(Boolean).join(" · ")}</small>
                      )}
                    </span>
                  </div>
                ))}
              </div>
              {canVote(role) && !stage.closed && (
                <div className="conflict-actions">
                  <span className="muted">{t("Konsens")}</span>
                  <button className="vote-include small" onClick={() => run(() => api.consensus(reviewId, stageId, c.record.id, "include", null), t("Konsens gespeichert."))}><Icon name="check" size={14} /> Include</button>
                  {stage.reason_required && (
                    <select value={consensusReason[c.record.id] ?? ""} onChange={(e) => setConsensusReason({ ...consensusReason, [c.record.id]: e.target.value })} aria-label={t("Hauptgrund für den Ausschluss")}>
                      <option value="">{t("Hauptgrund …")}</option>
                      {reasonsOf(criteria).map((r) => <option key={r.code} value={r.code}>{r.code} {r.text}</option>)}
                    </select>
                  )}
                  <button
                    className="vote-exclude small"
                    disabled={stage.reason_required && !consensusReason[c.record.id]}
                    onClick={() => run(() => api.consensus(reviewId, stageId, c.record.id, "exclude", consensusReason[c.record.id] || null), t("Konsens gespeichert."))}
                  >
                    <Icon name="x" size={14} /> Exclude
                  </button>
                </div>
              )}
            </div>
          ))}
        </>
      )}
    </>
  );
}
