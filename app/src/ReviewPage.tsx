import React, { useEffect, useState } from "react";
import { Link, useLocation, useParams, useSearchParams } from "react-router-dom";
import { open, save } from "@tauri-apps/plugin-dialog";

import {
  api,
  BUNDLE_FILTER,
  canManage,
  canVote,
  ChainReport,
  CONFLICT_LABEL,
  errorText,
  gateText,
  importSummary,
  Member,
  ORIGIN_LABEL,
  reasonsOf,
  ReviewDetail,
  RULE_LABEL,
  safeFileName,
  Search,
  Stage,
  StageDefaults,
  StageParams,
  Station,
  StationCheck,
  term,
} from "./api";
import { CriteriaEditor, CriteriaView } from "./Criteria";
import { Duplicates } from "./Duplicates";
import { FormsTab } from "./Forms";
import { FullTextTab } from "./FullTexts";
import { GuidanceTab, HelpButton } from "./Guidance";
import { capital, formatNumber, t, tn, tx } from "./i18n";
import { RegistrationTab } from "./Registration";
import { ReportTab } from "./Report";
import { SourcesTab } from "./Sources";
import { searchName, SearchTab } from "./Search";
import { isReviewTab, REVIEW_TABS, ReviewTab, ROLE_LABEL, useRefreshSidebar, useShareTerms } from "./shared";
import { Avatar, Banner, Empty, Group, Icon, PageHeader, Pill, Row, Stat } from "./ui";

const IMPORT_STATUS: Record<string, string> = { running: t("läuft"), done: t("fertig"), failed: t("fehlgeschlagen") };

type Tab = ReviewTab;

export function stageState(stage: Stage): string {
  if (stage.closed) return t("Geschlossen");
  if (stage.blind_active) return t("Verblindet");
  return stage.blind ? t("Entblindet") : t("Offen");
}

export function ReviewPage() {
  const { reviewId = "" } = useParams();
  const location = useLocation();
  const refreshSidebar = useRefreshSidebar();
  const [review, setReview] = useState<ReviewDetail | null>(null);
  const shareTerms = useShareTerms();
  const [params, setParams] = useSearchParams();
  const wanted = params.get("tab");
  const tab: Tab = isReviewTab(wanted) ? wanted : "overview";
  const setTab = (next: Tab) => setParams(next === "overview" ? {} : { tab: next });
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  // Meldungen gehören zum Bereich, in dem sie entstanden.
  useEffect(() => setNotice(""), [tab]);

  async function load() {
    try {
      const loaded = await api.review(reviewId);
      setReview(loaded);
      shareTerms(reviewId, loaded.workflow.terms);
    } catch (e) {
      setError(errorText(e));
    }
  }
  useEffect(() => {
    setReview(null);
    // Der Assistent zum Anlegen reicht mit, was beim Import oder beim Team nicht geklappt hat.
    const problems = (location.state as { problems?: string[] } | null)?.problems;
    setError(problems?.length ? t("Das Review ist angelegt, aber nicht alles hat geklappt: {problems}", { problems: problems.join(" · ") }) : "");
    setNotice("");
    load();
  }, [reviewId]);

  async function run(action: () => Promise<string | void>, done = "") {
    setError("");
    setNotice("");
    try {
      const message = await action();
      setNotice(message || done);
      await load();
      refreshSidebar();
    } catch (e) {
      setError(errorText(e));
    }
  }

  if (!review) return error ? <Banner kind="error">{error}</Banner> : null;
  const unique = review.n_records - review.n_duplicates;

  return (
    <>
      <PageHeader
        title={review.title}
        subtitle={
          <>
            <Pill>{ROLE_LABEL[review.role] ?? review.role}</Pill>
            <span>{review.workflow.template_name}</span>
            <span>{formatNumber(unique)} {term(review.workflow.terms, "records", t("Einträge"))}</span>
            {review.n_duplicates > 0 && <span>{tn("{n} Dublette", "{n} Dubletten", review.n_duplicates)}</span>}
            <span>{tn("{n} Mitglied", "{n} Mitglieder", review.members.length)}</span>
            <span>{t("Kriterien v{version}", { version: review.criteria.version })}</span>
          </>
        }
        help={tab !== "guidance" && (
          <HelpButton reviewId={review.id} area={tab} context={REVIEW_TABS.find((area) => area.value === tab)?.label ?? t("Review")} />
        )}
      />
      <div className="tab-body">
        {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
        {notice && <Banner kind="notice" onClose={() => setNotice("")}>{notice}</Banner>}
        {tab === "overview" && <OverviewTab review={review} run={run} goTo={setTab} />}
        {tab === "exchange" && <ExchangeTab review={review} run={run} />}
        {tab === "members" && <MembersTab review={review} run={run} />}
        {tab === "search" && <SearchTab review={review} run={run} />}
        {tab === "imports" && <ImportsTab review={review} run={run} />}
        {tab === "fulltext" && <FullTextTab review={review} run={run} />}
        {tab === "sources" && <SourcesTab review={review} run={run} />}
        {tab === "criteria" && <CriteriaTab review={review} run={run} />}
        {tab === "registration" && <RegistrationTab review={review} run={run} />}
        {tab === "guidance" && <GuidanceTab review={review} />}
        {tab === "forms" && <FormsTab review={review} run={run} />}
        {tab === "report" && <ReportTab review={review} run={run} />}
        {tab === "proof" && <ProofTab review={review} />}
      </div>
    </>
  );
}

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

const STATE_LABEL: Record<Station["state"], string> = { locked: t("Gesperrt"), open: t("Offen"), running: t("Läuft"), done: t("Abgeschlossen") };
const STATE_TONE: Record<Station["state"], string | undefined> = { locked: undefined, open: "blue", running: "yellow", done: "green" };
const TOOL_TABS: Record<string, { tab: Tab; label: string }[]> = {
  protocol: [{ tab: "criteria", label: t("Kriterien öffnen") }, { tab: "members", label: t("Mitglieder öffnen") }, { tab: "registration", label: t("Registrierung vorbereiten öffnen") }],
  search: [{ tab: "search", label: t("Suche öffnen") }],
  import: [{ tab: "imports", label: t("Import und Dubletten öffnen") }],
  fulltext: [{ tab: "fulltext", label: t("Volltexte öffnen") }],
  citation: [{ tab: "search", label: t("Suche festhalten öffnen") }, { tab: "imports", label: t("Treffer einlesen öffnen") }],
  bundling: [{ tab: "sources", label: t("Quellen bündeln öffnen") }],
};

/** Auswahlregeln heißen beim Extrahieren anders als beim Screening. */
const FORM_RULE_LABEL: Record<string, string> = { unanimous: t("Doppelt, unabhängig"), single_verified: t("Einfach mit Prüfung") };

/** Stationen der Vorlage als Leiste: Wo steht das Review? */
function StationBar({ stations }: { stations: Station[] }) {
  // Ab neun Stationen zwei Zeilen: Eine Zeile wäre zu eng für die Namen, seitliches Scrollen versteckt Stationen.
  const columns = stations.length > 8 ? Math.ceil(stations.length / 2) : Math.max(stations.length, 1);
  return (
    <ol className="station-bar" aria-label={t("Ablauf")} style={{ "--columns": columns } as React.CSSProperties}>
      {stations.map((s, i) => (
        <li key={s.key} className={`station-step s-${s.state}${s.available ? "" : " s-later"}`} title={`${s.label}: ${s.available ? STATE_LABEL[s.state] : t("folgt in einer späteren Version")}`}>
          <a href={`#station-${s.key}`} onClick={(e) => { e.preventDefault(); document.getElementById(`station-${s.key}`)?.scrollIntoView({ behavior: "smooth", block: "center" }); }}>
            <span className="station-dot">{s.state === "done" ? <Icon name="check" size={12} /> : s.state === "locked" ? <Icon name="lock" size={11} /> : i + 1}</span>
            <span className="station-name">{s.label}</span>
          </a>
        </li>
      ))}
    </ol>
  );
}

function CheckList({ checks }: { checks: StationCheck[] }) {
  if (checks.length === 0) return null;
  return (
    <ul className="checks">
      {checks.map((c) => (
        <li key={c.check} className={c.passed === null ? "check-open" : c.passed ? "check-ok" : "check-bad"}>
          <span className="check-mark">{c.passed === null ? null : <Icon name={c.passed ? "check" : "x"} size={11} />}</span>
          <span>{c.label}{c.detail && <small> · {c.detail}</small>}</span>
        </li>
      ))}
    </ul>
  );
}

function OverviewTab({ review, run, goTo }: { review: ReviewDetail; run: Run; goTo: (tab: Tab) => void }) {
  const [creating, setCreating] = useState<Station | null>(null);
  const [overriding, setOverriding] = useState("");
  const [reason, setReason] = useState("");
  const manage = canManage(review.role);
  const { workflow } = review;
  const records = term(workflow.terms, "records", t("Einträge"));
  const stages = new Map(review.stages.map((s) => [s.id, s]));
  const unique = review.n_records - review.n_duplicates;
  const blocking = workflow.stations.filter((s) => s.blocking);
  // Stationen ohne Abschlussregel werden nie „fertig“; sie taugen nicht als nächster Schritt.
  const next = workflow.stations.find((s) => s.available && s.state !== "done" && s.state !== "locked" && (s.blocking || s.kind === "screening" || s.kind === "form"));
  const openStages = review.stages.filter((s) => !s.closed).length;

  return (
    <>
      <StationBar stations={workflow.stations} />
      <div className="stats compact">
        <Stat label={capital(records)} value={formatNumber(unique)} detail={review.n_duplicates > 0 ? tn("{n} Dublette erkannt", "{n} Dubletten erkannt", review.n_duplicates) : t("keine Dubletten")} />
        <Stat label={t("Team")} value={review.members.length} detail={t("{n} mit Schreibrecht", { n: review.members.filter((m) => canVote(m.role)).length })} />
        <Stat label={t("Stufen")} value={review.stages.length} detail={openStages ? t("{n} offen", { n: openStages }) : t("keine offen")} />
        {blocking.length > 0 && (
          <Stat label={t("Abschlussregeln")} value={`${blocking.filter((s) => s.state === "done").length} / ${blocking.length}`} detail={t("Stationen mit erfülltem Gate")} />
        )}
      </div>
      {next && (
        <p className="next-step">
          <Icon name="play" size={12} />{" "}
          {next.checks.some((c) => c.passed === false)
            ? tx("Als Nächstes: {station} – offen: {checks}", {
                station: <strong>{next.label}</strong>,
                checks: next.checks.filter((c) => c.passed === false).map((c) => c.label).join(", "),
              })
            : tx("Als Nächstes: {station}", { station: <strong>{next.label}</strong> })}
        </p>
      )}

      <div className="stations">
        {workflow.stations.map((station, i) => {
          const staged = station.kind === "screening" || station.kind === "form";
          const canOpen = manage && staged && station.state !== "locked" && (station.free_rounds || station.rounds.every((r) => r.closed));
          const canOverride = manage && station.blocking && station.state !== "done" && station.state !== "locked" && station.rounds.every((r) => r.closed);
          // Die Bestätigung des Formulars hängt nicht an einer Runde; eine neue Runde hilft dort nicht.
          const failed = staged && station.rounds.length > 0 && station.rounds.every((r) => r.closed) && station.checks.some((c) => c.passed === false && c.check !== "form_confirmed");
          return (
            <section key={station.key} id={`station-${station.key}`} className={`station s-${station.state}${station.available ? "" : " s-later"}`}>
              <header>
                <span className="station-dot">{station.state === "done" ? <Icon name="check" size={12} /> : i + 1}</span>
                <h3>{station.label}</h3>
                {station.available ? <Pill tone={STATE_TONE[station.state]}>{STATE_LABEL[station.state]}</Pill> : <Pill>{t("folgt")}</Pill>}
              </header>
              <p className="muted">{station.description}</p>
              {station.state === "locked" && station.available && (
                <p className="station-note"><Icon name="lock" size={12} /> {t("Wartet auf: {stations}", { stations: station.blocked_by.map((b) => b.label).join(", ") })}</p>
              )}
              <CheckList checks={station.checks} />
              {station.overridden && <p className="station-note"><Icon name="flag" size={12} /> {t("Übersteuert: {reason}", { reason: station.overridden })}</p>}
              {failed && !station.overridden && station.state !== "done" && (
                <p className="station-note">{manage ? t("Das Gate der letzten Runde ist nicht erfüllt. Öffne eine neue Runde oder übersteuere mit Begründung.") : t("Das Gate der letzten Runde ist nicht erfüllt. Die Leitung öffnet eine neue Runde.")}</p>
              )}

              {station.rounds.length > 0 && (
                <div className="stage-grid">
                  {station.rounds.map((round) => {
                    const stage = stages.get(round.stage_id);
                    if (!stage) return null;
                    return (
                      <article key={stage.id} className="stage-card">
                        <div className="stage-card-head">
                          <span className="stage-kind">{station.free_rounds ? t("Stufe") : t("Runde {n}", { n: round.round })}</span>
                          <Pill>{stageState(stage)}</Pill>
                        </div>
                        <h3>{stage.name}</h3>
                        <p className="muted">
                          {formatNumber(stage.n_records)} {stage.form_id ? term(workflow.terms, "sources", t("Quellen")) : records} ·{" "}
                          {stage.form_id ? FORM_RULE_LABEL[stage.decision_rule] ?? stage.decision_rule : RULE_LABEL[stage.decision_rule] ?? stage.decision_rule}
                          {stage.decision_rule === "unanimous" && ` · ${stage.form_id ? tn("mind. {n} Person", "mind. {n} Personen", stage.min_reviewers) : tn("mind. {n} Stimme", "mind. {n} Stimmen", stage.min_reviewers)}`}
                          {stage.maybe_as && !stage.form_id && ` · ${stage.maybe_as === "include" ? t("Maybe zählt als Include") : t("Maybe zählt als Exclude")}`}
                        </p>
                        {round.gate && (
                          <p className={round.gate.passed ? "gate-line ok" : "gate-line bad"}>
                            <Icon name={round.gate.passed ? "check" : "x"} size={12} /> {round.gate.passed ? t("Gate beim Schließen erfüllt: {result}", { result: gateText(round.gate) }) : t("Gate beim Schließen nicht erfüllt: {result}", { result: gateText(round.gate) })}
                          </p>
                        )}
                        <div className="stage-card-actions">
                          {stage.form_id && (
                            <Link className={canVote(review.role) && !stage.closed ? "button primary" : "button"} to={`/reviews/${review.id}/forms/${stage.id}`}>
                              <Icon name="list" size={14} /> {canVote(review.role) && !stage.closed ? t("Extrahieren") : t("Tabelle")}
                            </Link>
                          )}
                          {!stage.form_id && canVote(review.role) && !stage.closed && (
                            <Link className="button primary" to={`/reviews/${review.id}/stages/${stage.id}/screen`}>
                              <Icon name="play" size={14} /> {t("Screenen")}
                            </Link>
                          )}
                          {!stage.form_id && (
                            <Link className="button" to={`/reviews/${review.id}/stages/${stage.id}`}>
                              <Icon name="chart" size={14} /> {t("Auswertung")}
                            </Link>
                          )}
                        </div>
                      </article>
                    );
                  })}
                </div>
              )}

              <div className="station-actions">
                {(TOOL_TABS[station.tool ?? ""] ?? []).map((tool) => (
                  <button key={tool.tab} onClick={() => goTo(tool.tab)}>{tool.label}</button>
                ))}
                {station.kind === "form" && <button onClick={() => goTo("forms")}>{t("Formular öffnen")}</button>}
                {station.kind === "report" && station.available && (
                  <button onClick={() => goTo("report")}>{t("Bericht öffnen")}</button>
                )}
                {canOpen && creating?.key !== station.key && (
                  <button className={station.state === "open" ? "primary" : ""} onClick={() => setCreating(station)}>
                    <Icon name="plus" size={14} /> {station.free_rounds || station.rounds.length === 0 ? t("Stufe öffnen") : t("Neue Runde öffnen")}
                  </button>
                )}
                {canOverride && overriding !== station.key && (
                  <button className="ghost" onClick={() => { setOverriding(station.key); setReason(""); }}>{t("Übersteuern …")}</button>
                )}
              </div>

              {overriding === station.key && (
                <form
                  className="override"
                  onSubmit={(e) => {
                    e.preventDefault();
                    run(async () => {
                      await api.overrideGate(review.id, station.key, reason);
                      setOverriding("");
                    }, t("„{station}“ übersteuert. Die Begründung steht im Log.", { station: station.label }));
                  }}
                >
                  <label htmlFor={`override-${station.key}`}>{t("Warum gilt die Station ohne erfülltes Gate als abgeschlossen?")}</label>
                  <div className="inline-form">
                    <input id={`override-${station.key}`} value={reason} onChange={(e) => setReason(e.target.value)} placeholder={t("Begründung, erscheint später im Bericht")} required autoFocus />
                    <button type="button" onClick={() => setOverriding("")}>{t("Abbrechen")}</button>
                    <button className="primary" type="submit">{t("Übersteuern")}</button>
                  </div>
                </form>
              )}

              {creating?.key === station.key && station.defaults && (
                <StageForm
                  review={review}
                  station={station}
                  defaults={station.defaults}
                  onCancel={() => setCreating(null)}
                  onSubmit={(params) =>
                    run(async () => {
                      await api.openStage(review.id, params);
                      setCreating(null);
                    }, t("Stufe geöffnet. Schick den Mitgliedern unter „Austausch“ ein neues Paket."))
                  }
                />
              )}
            </section>
          );
        })}
      </div>
      {!manage && review.stages.length === 0 && (
        <p className="muted center">{t("Sobald die Leitung eine Stufe öffnet, bekommst du ein neues Paket.")}</p>
      )}
    </>
  );
}

function StageForm({ review, station, defaults, onSubmit, onCancel }: {
  review: ReviewDetail;
  station: Station;
  defaults: StageDefaults;
  onSubmit: (params: StageParams) => void;
  onCancel: () => void;
}) {
  const { rules } = defaults;
  const records = term(review.workflow.terms, "records", t("Einträge"));
  const [name, setName] = useState(defaults.name);
  const [scope, setScope] = useState(defaults.scope);
  const [sampleSize, setSampleSize] = useState(defaults.sample_size ?? 25);
  const [sourceStage, setSourceStage] = useState(defaults.source_stage ?? "");
  const [decisionRule, setDecisionRule] = useState(rules.decision_rule.value);
  const [conflictRule, setConflictRule] = useState(rules.conflict_rule.value);
  const [blind, setBlind] = useState(rules.blind.value);
  const [minReviewers, setMinReviewers] = useState(rules.min_reviewers.value);
  const [maybeAs, setMaybeAs] = useState(rules.maybe_as.value ?? "");
  const [verifyScope, setVerifyScope] = useState<"all" | "sample">("all");
  const [verifyPercent, setVerifyPercent] = useState(20);
  const [reasonRequired, setReasonRequired] = useState(defaults.reason_required);
  const hasReasons = reasonsOf(review.criteria).length > 0;
  const single = decisionRule === "single_verified";
  const isForm = station.kind === "form";
  const sources = term(review.workflow.terms, "sources", t("Quellen"));
  // In der Vorlage festgelegte Eingangsmenge bleibt fest; nur der freie Ablauf lässt die Wahl.
  const scopeFixed = review.workflow.published && !station.free_rounds;

  function submit(event: React.FormEvent) {
    event.preventDefault();
    onSubmit({
      name, scope, blind: single ? false : blind, min_reviewers: minReviewers, maybe_as: maybeAs || null,
      kind: station.key,
      sample_size: scope === "sample" ? sampleSize : null,
      seed: null,
      source_stage: scope === "from_stage" ? sourceStage || null : null,
      station: station.key,
      decision_rule: decisionRule,
      conflict_rule: conflictRule,
      verify: single && !isForm ? { scope: verifyScope, percent: verifyScope === "sample" ? verifyPercent : null } : null,
      reason_required: reasonRequired,
    });
  }

  return (
    <form className="sheet" onSubmit={submit}>
      <h2>{station.free_rounds || defaults.round === 1 ? t("{station}: neue Stufe", { station: station.label }) : t("{station}: Runde {n}", { station: station.label, n: defaults.round })}</h2>
      <Group>
        <Row>
          <label htmlFor="stage-name">{t("Name")}</label>
          <input id="stage-name" value={name} onChange={(e) => setName(e.target.value)} required />
        </Row>
        <Row>
          <label htmlFor="stage-scope">{t("Umfang")}</label>
          <select id="stage-scope" value={scope} onChange={(e) => setScope(e.target.value)} disabled={scopeFixed && !isForm}>
            <option value="sample">{isForm ? t("Stichprobe je Quellenart") : t("Zufallsstichprobe")}</option>
            <option value="all">{isForm ? t("Alle eingeschlossenen {sources}", { sources }) : t("Alle {records} ohne Dubletten", { records })}</option>
            {!isForm && <option value="from_stage">{t("Eingeschlossene einer früheren Stufe")}</option>}
          </select>
        </Row>
        {scope === "sample" && (
          <Row>
            <label htmlFor="stage-sample">{isForm ? t("{sources} je Quellenart", { sources }) : t("Größe der Stichprobe")}</label>
            <input id="stage-sample" className="narrow-input" type="number" min={1} value={sampleSize} onChange={(e) => setSampleSize(Number(e.target.value))} />
          </Row>
        )}
        {scope === "from_stage" && (
          <Row>
            <label htmlFor="stage-source">{t("Quellstufe")}</label>
            <select id="stage-source" value={sourceStage} onChange={(e) => setSourceStage(e.target.value)} required disabled={scopeFixed && !!defaults.source_stage}>
              <option value="">{t("Bitte wählen")}</option>
              {review.stages.map((s) => (
                <option key={s.id} value={s.id}>{s.name}</option>
              ))}
            </select>
          </Row>
        )}
        <Row>
          <label htmlFor="stage-rule">
            {isForm ? t("Arbeitsweise") : t("Auswahlregel")}
            {!rules.decision_rule.editable && <small>{t("von der Vorlage festgelegt")}</small>}
          </label>
          <select id="stage-rule" value={decisionRule} onChange={(e) => setDecisionRule(e.target.value)} disabled={!rules.decision_rule.editable}>
            {Object.entries(isForm ? FORM_RULE_LABEL : RULE_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </Row>
        {single && isForm ? (
          <Row>
            <span>{t("Eine Person extrahiert, eine zweite prüft jede {source}. Die Stufe ist nicht verblindet.", { source: term(review.workflow.terms, "source", t("Quelle")) })}</span>
          </Row>
        ) : single ? (
          <Row>
            <label htmlFor="stage-verify">
              {t("Zweite Stimme für")}
              <small>{t("Die zweite Person sieht die Ausschlüsse; die Stufe ist nicht verblindet")}</small>
            </label>
            <span className="inline-actions">
              <select id="stage-verify" value={verifyScope} onChange={(e) => setVerifyScope(e.target.value as "all" | "sample")}>
                <option value="all">{t("alle Ausschlüsse")}</option>
                <option value="sample">{t("eine Stichprobe der Ausschlüsse")}</option>
              </select>
              {verifyScope === "sample" && (
                <>
                  <input className="narrow-input" type="number" min={1} max={100} value={verifyPercent} onChange={(e) => setVerifyPercent(Number(e.target.value))} aria-label={t("Anteil in Prozent")} /> %
                </>
              )}
            </span>
          </Row>
        ) : (
          <Row>
            <label htmlFor="stage-min">
              {isForm ? t("Personen je Quelle") : t("Stimmen je Eintrag")}
              {decisionRule === "liberal" && <small>{t("Ein Ausschluss braucht mindestens zwei")}</small>}
            </label>
            <input id="stage-min" className="narrow-input" type="number" min={1} value={minReviewers} onChange={(e) => setMinReviewers(Number(e.target.value))} disabled={!rules.min_reviewers.editable} />
          </Row>
        )}
        {!isForm && <><Row>
          <label htmlFor="stage-maybe">{t("„Maybe“ zählt als")}</label>
          <select id="stage-maybe" value={maybeAs} onChange={(e) => setMaybeAs(e.target.value)} disabled={!rules.maybe_as.editable}>
            <option value="">{t("eigene Kategorie")}</option>
            <option value="include">Include</option>
            <option value="exclude">Exclude</option>
          </select>
        </Row>
        <Row>
          <label htmlFor="stage-conflict">
            {t("Klärung")}
            <small>{t("Bei Dritt-Entscheid darf niemand mit eigener Stimme den Konsens festhalten")}</small>
          </label>
          <select id="stage-conflict" value={conflictRule} onChange={(e) => setConflictRule(e.target.value)} disabled={!rules.conflict_rule.editable}>
            {Object.entries(CONFLICT_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </Row></>}
        <Row>
          <label htmlFor="stage-blind">
            {t("Verblindet")}
            <small>{isForm ? t("Werte anderer bleiben bis zum Entblinden unsichtbar") : t("Stimmen anderer bleiben bis zum Entblinden unsichtbar")}</small>
          </label>
          <input id="stage-blind" type="checkbox" className="switch" checked={!single && blind} onChange={(e) => setBlind(e.target.checked)} disabled={single || !rules.blind.editable} />
        </Row>
        {!isForm && <Row>
          <label htmlFor="stage-reason">
            {t("Ausschluss nur mit Hauptgrund")}
            <small>
              {defaults.reason_required
                ? hasReasons ? t("von der Vorlage festgelegt; genau ein Kürzel aus den Ausschlusskriterien") : t("von der Vorlage festgelegt; dafür fehlen Ausschlusskriterien mit Kürzel")
                : hasReasons ? t("genau ein Kürzel aus den Ausschlusskriterien") : t("dafür fehlen Ausschlusskriterien mit Kürzel")}
            </small>
          </label>
          <input id="stage-reason" type="checkbox" className="switch" checked={reasonRequired} onChange={(e) => setReasonRequired(e.target.checked)} disabled={defaults.reason_required} />
        </Row>}
      </Group>
      <p className="hint">{t("Die Regeln sind nach dem Öffnen fest. Für andere Regeln öffnest du eine neue Stufe.")}</p>
      <div className="sheet-actions">
        <button type="button" onClick={onCancel}>{t("Abbrechen")}</button>
        <button className="primary" type="submit">{t("Stufe öffnen")}</button>
      </div>
    </form>
  );
}

function ExchangeTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const manage = canManage(review.role);
  const me = review.members.find((m) => m.is_me);

  async function importBundle() {
    const path = await open({ multiple: false, directory: false, filters: BUNDLE_FILTER });
    if (typeof path !== "string") return;
    await run(async () => {
      const result = await api.importBundle(path);
      if (result.review_id !== review.id) return t("{summary} Das war ein anderes Review.", { summary: importSummary(result) });
      const rejected = result.report.rejected;
      return rejected.length ? t("{summary} Abgewiesen: {list}", { summary: importSummary(result), list: rejected.join(" · ") }) : importSummary(result);
    });
  }

  async function exportReturn() {
    const path = await save({
      defaultPath: `${t("{review} – Rückmeldung {name}", { review: safeFileName(review.title), name: safeFileName(me?.name ?? "") })}.ortvotes`,
      filters: [{ name: t("Rückmeldung"), extensions: ["ortvotes"] }],
    });
    if (!path) return;
    await run(async () => {
      const n = await api.exportReturn(review.id, path);
      return tn("Rückmeldung mit {n} eigenen Ereignis gespeichert. Schick sie an die Leitung.", "Rückmeldung mit {n} eigenen Ereignissen gespeichert. Schick sie an die Leitung.", n);
    });
  }

  async function exportPackage(member: Member) {
    const path = await save({
      defaultPath: `${t("{review} – Paket für {name}", { review: safeFileName(review.title), name: safeFileName(member.name) })}.ortpkg`,
      filters: [{ name: t("Paket"), extensions: ["ortpkg"] }],
    });
    if (!path) return;
    await run(() => api.exportPackage(review.id, member.actor_id, path), t("Paket für {name} gespeichert.", { name: member.name }));
  }

  const others = review.members.filter((m) => !m.is_me);

  return (
    <>
      <div className="exchange-cards">
        <button className="exchange-card" onClick={importBundle}>
          <Icon name="download" size={20} />
          <strong>{t("Datei einlesen")}</strong>
          <span>{manage ? t("Rückmeldungen deines Teams") : t("Neues Paket der Leitung")}</span>
        </button>
        {canVote(review.role) && (
          <button className="exchange-card" onClick={exportReturn}>
            <Icon name="upload" size={20} />
            <strong>{t("Rückmeldung speichern")}</strong>
            <span>{t("Deine signierten Entscheidungen")}</span>
          </button>
        )}
      </div>
      {manage && (
        <Group
          title={t("Pakete verteilen")}
          footer={t("In verblindeten Stufen enthält jedes Paket nur die eigenen Stimmen der Person. Was nicht in der Datei steht, kann niemand sehen.")}
        >
          {others.length === 0 && <Row><span className="muted">{t("Füge unter „Mitglieder“ zuerst Personen hinzu.")}</span></Row>}
          {others.map((m) => (
            <Row key={m.actor_id}>
              <span className="person">
                <Avatar name={m.name} />
                <span>
                  {m.name}
                  <small>{ROLE_LABEL[m.role]}</small>
                </span>
              </span>
              <button onClick={() => exportPackage(m)}><Icon name="upload" size={14} /> {t("Paket speichern")}</button>
            </Row>
          ))}
        </Group>
      )}
    </>
  );
}

function MembersTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const manage = canManage(review.role);
  const [role, setRole] = useState("reviewer");

  async function add() {
    const path = await open({ multiple: false, directory: false, filters: [{ name: t("Identitätsdatei"), extensions: ["ortid"] }] });
    if (typeof path !== "string") return;
    await run(async () => t("{name} ist jetzt Mitglied. Schick unter „Austausch“ ein Paket.", { name: await api.addMember(review.id, path, role) }));
  }

  return (
    <>
      <Group title={tn("{n} Mitglied", "{n} Mitglieder", review.members.length)}>
        {review.members.map((m) => (
          <Row key={m.actor_id}>
            <span className="person">
              <Avatar name={m.name} />
              <span>
                {m.name}{m.is_me && <span className="muted"> {t("(du)")}</span>}
                <small className="mono">{m.actor_id.slice(0, 8)}</small>
              </span>
            </span>
            {manage ? (
              <select
                aria-label={t("Rolle von {name}", { name: m.name })}
                value={m.role}
                onChange={(e) => run(() => api.setRole(review.id, m.actor_id, e.target.value), t("Rolle geändert."))}
              >
                {Object.entries(ROLE_LABEL).map(([value, label]) => (
                  <option key={value} value={value}>{label}</option>
                ))}
              </select>
            ) : (
              <Pill>{ROLE_LABEL[m.role] ?? m.role}</Pill>
            )}
          </Row>
        ))}
      </Group>
      {manage && (
        <Group title={t("Mitglied hinzufügen")} footer={t("Die Person speichert in ihrer App eine Identitätsdatei (.ortid) und schickt sie dir.")}>
          <Row>
            <label htmlFor="new-role">{t("Rolle")}</label>
            <select id="new-role" value={role} onChange={(e) => setRole(e.target.value)}>
              <option value="reviewer">Reviewer</option>
              <option value="admin">Admin</option>
              <option value="viewer">Viewer</option>
              <option value="owner">Owner</option>
            </select>
          </Row>
          <Row>
            <span className="muted">{t("Identitätsdatei")}</span>
            <button onClick={add}><Icon name="file" size={14} /> {t("Wählen …")}</button>
          </Row>
        </Group>
      )}
    </>
  );
}

function ImportsTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const manage = canManage(review.role);
  const [searches, setSearches] = useState<Search[]>([]);
  // ID einer Suche oder leer für einen Import ohne Suchversion
  const [searchId, setSearchId] = useState("");
  const [source, setSource] = useState("");
  const [origin, setOrigin] = useState("database");

  useEffect(() => {
    api.searches(review.id).then((list) => {
      setSearches(list);
      setSearchId((current) => (list.some((s) => s.id === current) ? current : list.length === 1 ? list[0].id : ""));
    }).catch(() => setSearches([]));
  }, [review]);

  async function choose() {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: t("RIS, BibTeX oder PubMed"), extensions: ["ris", "bib", "bibtex", "nbib", "txt"] }],
    });
    if (typeof path !== "string") return;
    await run(async () => {
      const batch = await api.importRecords(review.id, path, searchId ? { search_id: searchId } : { source, origin });
      setSource("");
      return batch.status === "failed"
        ? t("Import fehlgeschlagen: {error}", { error: batch.error ?? "" })
        : tn("{n} Eintrag importiert, davon {duplicates} Dubletten.", "{n} Einträge importiert, davon {duplicates} Dubletten.", batch.n_records, { duplicates: batch.n_duplicates });
    });
  }

  const nameOf = (id: string | null) => {
    const search = searches.find((s) => s.id === id);
    return search ? searchName(search) : "";
  };

  return (
    <>
      {review.imports.length === 0 ? (
        <Empty icon="upload" title={t("Noch nichts importiert")}>{t("Exportiere die Treffer jeder Suche als RIS, BibTeX oder im PubMed-Format und lies sie hier ein.")}</Empty>
      ) : (
        <Group title={t("Importe")}>
          {review.imports.map((b) => (
            <Row key={b.id}>
              <span className="person">
                <span className="file-icon"><Icon name="file" /></span>
                <span>
                  {b.filename}
                  <small>
                    {b.status === "failed" ? b.error : `${tn("{n} Eintrag", "{n} Einträge", b.n_records)} · ${tn("{n} Dublette", "{n} Dubletten", b.n_duplicates)}`}
                    {b.source && ` · ${b.source}`}
                    {b.origin && ` · ${ORIGIN_LABEL[b.origin] ?? b.origin}`}
                    {b.status !== "failed" && (b.search_version != null ? ` · ${t("Suchversion {version}", { version: b.search_version })}` : ` · ${t("ohne Suchversion")}`)}
                  </small>
                </span>
              </span>
              <Pill>{IMPORT_STATUS[b.status] ?? b.status}</Pill>
            </Row>
          ))}
        </Group>
      )}
      <Duplicates review={review} run={run} />
      {manage && (
        <Group
          title={t("Datei importieren")}
          footer={t("Gleiche Einträge werden über die DOI oder den Titel mit Jahr erkannt und nicht gescreent. Was nur ähnlich ist, erscheint danach unter „Mögliche Dubletten“.")}
        >
          <Row>
            <label htmlFor="import-search">{t("Suche")} <small>{t("die neueste Version; festgehalten im Reiter „Suche“")}</small></label>
            <select id="import-search" value={searchId} onChange={(e) => setSearchId(e.target.value)}>
              <option value="">{t("ohne Suchversion")}</option>
              {searches.map((s) => (
                <option key={s.id} value={s.id}>{t("{name} (Version {version})", { name: nameOf(s.id), version: s.versions[s.versions.length - 1].version })}</option>
              ))}
            </select>
          </Row>
          {!searchId && (
            <>
              <Row>
                <label htmlFor="import-source">{t("Quelle")}</label>
                <input id="import-source" value={source} onChange={(e) => setSource(e.target.value)} placeholder={t("z. B. Handsuche")} />
              </Row>
              <Row>
                <label htmlFor="import-origin">{t("Herkunft")}</label>
                <select id="import-origin" value={origin} onChange={(e) => setOrigin(e.target.value)}>
                  {Object.entries(ORIGIN_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                </select>
              </Row>
            </>
          )}
          <Row>
            <span className="muted">{t("RIS, BibTeX oder PubMed (nbib)")}</span>
            <button className="primary" onClick={choose}><Icon name="upload" size={14} /> {t("Datei wählen …")}</button>
          </Row>
        </Group>
      )}
    </>
  );
}

function CriteriaTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const c = review.criteria;
  const schema = review.workflow.criteria_schema;
  if (!canManage(review.role)) {
    return (
      <Group title={c.version ? t("Version {version}", { version: c.version }) : t("Kriterien")} footer={c.change_note ? t("Letzte Änderung: {note}", { note: c.change_note }) : undefined}>
        <CriteriaView criteria={c} schema={schema} />
      </Group>
    );
  }
  // Der Schlüssel setzt den Editor nach dem Veröffentlichen auf die neue Version zurück.
  return (
    <CriteriaEditor
      key={c.version}
      criteria={c}
      schema={schema}
      onPublish={(data) => run(async () => { await api.publishCriteria(review.id, data); }, t("Neue Kriterienversion veröffentlicht."))}
    />
  );
}

function ProofTab({ review }: { review: ReviewDetail }) {
  const [chain, setChain] = useState<ChainReport | null>(null);
  const [error, setError] = useState("");
  async function verify() {
    setError("");
    try {
      setChain(await api.verifyChain(review.id));
    } catch (e) {
      setError(errorText(e));
    }
  }
  return (
    <>
      <Group
        title={t("Hash-Ketten")}
        footer={t("Prüft Prüfsumme, Signatur und Lückenlosigkeit jedes Ereignisses. Eine nachträgliche Änderung an der Datei fällt hier auf.")}
      >
        <Row>
          <span className="person">
            <span className={`proof-icon ${chain ? (chain.ok ? "ok" : "bad") : ""}`}><Icon name="shield" size={18} /></span>
            <span>
              {chain ? (chain.ok ? t("Alles unversehrt") : tn("{n} fehlerhaftes Ereignis", "{n} fehlerhafte Ereignisse", chain.n_bad)) : t("Noch nicht geprüft")}
              <small>{chain ? tn("{n} Ereignis geprüft", "{n} Ereignisse geprüft", chain.n_events) : t("Dauert wenige Sekunden")}</small>
            </span>
          </span>
          <button onClick={verify}>{t("Jetzt prüfen")}</button>
        </Row>
        {chain && !chain.ok && chain.problems.map((p) => <Row key={p}><span className="error-text">{p}</span></Row>)}
      </Group>
      {error && <Banner kind="error">{error}</Banner>}
    </>
  );
}
