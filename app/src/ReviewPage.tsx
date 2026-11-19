import React, { useEffect, useState } from "react";
import { Link, useLocation, useParams } from "react-router-dom";
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
  ReviewDetail,
  RULE_LABEL,
  safeFileName,
  Stage,
  StageDefaults,
  StageParams,
  Station,
  StationCheck,
  term,
} from "./api";
import { ROLE_LABEL, useRefreshSidebar } from "./shared";
import { Avatar, Banner, Empty, Group, Icon, PageHeader, Pill, Row, Segmented, Stat } from "./ui";

const IMPORT_STATUS: Record<string, string> = { running: "läuft", done: "fertig", failed: "fehlgeschlagen" };

type Tab = "overview" | "exchange" | "members" | "imports" | "criteria" | "proof";

export function stageState(stage: Stage): string {
  if (stage.closed) return "Geschlossen";
  if (stage.blind_active) return "Verblindet";
  return stage.blind ? "Entblindet" : "Offen";
}

export function ReviewPage() {
  const { reviewId = "" } = useParams();
  const location = useLocation();
  const refreshSidebar = useRefreshSidebar();
  const [review, setReview] = useState<ReviewDetail | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  async function load() {
    try {
      setReview(await api.review(reviewId));
    } catch (e) {
      setError(errorText(e));
    }
  }
  useEffect(() => {
    setReview(null);
    setTab("overview");
    // Der Assistent zum Anlegen reicht mit, was beim Import oder beim Team nicht geklappt hat.
    const problems = (location.state as { problems?: string[] } | null)?.problems;
    setError(problems?.length ? `Das Review ist angelegt, aber nicht alles hat geklappt: ${problems.join(" · ")}` : "");
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

  const tabs: { value: Tab; label: string; icon: string }[] = [
    { value: "overview", label: "Ablauf", icon: "stack" },
    { value: "exchange", label: "Austausch", icon: "exchange" },
    { value: "members", label: "Mitglieder", icon: "people" },
    { value: "imports", label: "Import", icon: "upload" },
    { value: "criteria", label: "Kriterien", icon: "list" },
    { value: "proof", label: "Nachweis", icon: "shield" },
  ];

  return (
    <>
      <PageHeader
        title={review.title}
        subtitle={
          <>
            <Pill>{ROLE_LABEL[review.role] ?? review.role}</Pill>
            <span>{review.workflow.template_name}</span>
            <span>{unique.toLocaleString("de-DE")} {term(review.workflow.terms, "records", "Einträge")}</span>
            {review.n_duplicates > 0 && <span>{review.n_duplicates} Dubletten</span>}
            <span>{review.members.length} Mitglieder</span>
            <span>Kriterien v{review.criteria.version}</span>
          </>
        }
      />
      <Segmented label="Bereiche" value={tab} options={tabs} onChange={setTab} />
      <div className="tab-body">
        {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
        {notice && <Banner kind="notice" onClose={() => setNotice("")}>{notice}</Banner>}
        {tab === "overview" && <OverviewTab review={review} run={run} goTo={setTab} />}
        {tab === "exchange" && <ExchangeTab review={review} run={run} />}
        {tab === "members" && <MembersTab review={review} run={run} />}
        {tab === "imports" && <ImportsTab review={review} run={run} />}
        {tab === "criteria" && <CriteriaTab review={review} run={run} />}
        {tab === "proof" && <ProofTab review={review} />}
      </div>
    </>
  );
}

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

const STATE_LABEL: Record<Station["state"], string> = { locked: "Gesperrt", open: "Offen", running: "Läuft", done: "Abgeschlossen" };
const STATE_TONE: Record<Station["state"], string | undefined> = { locked: undefined, open: "blue", running: "yellow", done: "green" };
const TOOL_TABS: Record<string, { tab: Tab; label: string }[]> = {
  protocol: [{ tab: "criteria", label: "Kriterien" }, { tab: "members", label: "Mitglieder" }],
  import: [{ tab: "imports", label: "Import" }],
};

/** Stationen der Vorlage als Leiste: Wo steht das Review? */
function StationBar({ stations }: { stations: Station[] }) {
  return (
    <ol className="station-bar" aria-label="Ablauf">
      {stations.map((s, i) => (
        <li key={s.key} className={`station-step s-${s.state}${s.available ? "" : " s-later"}`} title={`${s.label}: ${s.available ? STATE_LABEL[s.state] : "folgt in einer späteren Version"}`}>
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
  const records = term(workflow.terms, "records", "Einträge");
  const stages = new Map(review.stages.map((s) => [s.id, s]));
  const unique = review.n_records - review.n_duplicates;
  const blocking = workflow.stations.filter((s) => s.blocking);
  // Stationen ohne Abschlussregel werden nie „fertig“; sie taugen nicht als nächster Schritt.
  const next = workflow.stations.find((s) => s.available && s.state !== "done" && s.state !== "locked" && (s.blocking || s.kind === "screening"));
  const openStages = review.stages.filter((s) => !s.closed).length;

  return (
    <>
      <StationBar stations={workflow.stations} />
      <div className="stats compact">
        <Stat label={records} value={unique.toLocaleString("de-DE")} detail={review.n_duplicates > 0 ? `${review.n_duplicates} Dubletten erkannt` : "keine Dubletten"} />
        <Stat label="Team" value={review.members.length} detail={`${review.members.filter((m) => canVote(m.role)).length} mit Schreibrecht`} />
        <Stat label="Stufen" value={review.stages.length} detail={openStages ? `${openStages} offen` : "keine offen"} />
        {blocking.length > 0 && (
          <Stat label="Abschlussregeln" value={`${blocking.filter((s) => s.state === "done").length} / ${blocking.length}`} detail="Stationen mit erfülltem Gate" />
        )}
      </div>
      {next && (
        <p className="next-step">
          <Icon name="play" size={12} /> Als Nächstes: <strong>{next.label}</strong>
          {next.checks.some((c) => c.passed === false) && <> – offen: {next.checks.filter((c) => c.passed === false).map((c) => c.label).join(", ")}</>}
        </p>
      )}

      <div className="stations">
        {workflow.stations.map((station, i) => {
          const canOpen = manage && station.kind === "screening" && station.state !== "locked" && (station.free_rounds || station.rounds.every((r) => r.closed));
          const canOverride = manage && station.blocking && station.state !== "done" && station.state !== "locked" && station.rounds.every((r) => r.closed);
          const failed = station.kind === "screening" && station.checks.some((c) => c.passed === false);
          return (
            <section key={station.key} id={`station-${station.key}`} className={`station s-${station.state}${station.available ? "" : " s-later"}`}>
              <header>
                <span className="station-dot">{station.state === "done" ? <Icon name="check" size={12} /> : i + 1}</span>
                <h3>{station.label}</h3>
                {station.available ? <Pill tone={STATE_TONE[station.state]}>{STATE_LABEL[station.state]}</Pill> : <Pill>folgt</Pill>}
              </header>
              <p className="muted">{station.description}</p>
              {station.state === "locked" && station.available && (
                <p className="station-note"><Icon name="lock" size={12} /> Wartet auf: {station.blocked_by.map((b) => b.label).join(", ")}</p>
              )}
              <CheckList checks={station.checks} />
              {station.overridden && <p className="station-note"><Icon name="flag" size={12} /> Übersteuert: {station.overridden}</p>}
              {failed && !station.overridden && station.state !== "done" && (
                <p className="station-note">Das Gate der letzten Runde ist nicht erfüllt. {manage ? "Öffne eine neue Runde oder übersteuere mit Begründung." : "Die Leitung öffnet eine neue Runde."}</p>
              )}

              {station.rounds.length > 0 && (
                <div className="stage-grid">
                  {station.rounds.map((round) => {
                    const stage = stages.get(round.stage_id);
                    if (!stage) return null;
                    return (
                      <article key={stage.id} className="stage-card">
                        <div className="stage-card-head">
                          <span className="stage-kind">{station.free_rounds ? "Stufe" : `Runde ${round.round}`}</span>
                          <Pill>{stageState(stage)}</Pill>
                        </div>
                        <h3>{stage.name}</h3>
                        <p className="muted">
                          {stage.n_records.toLocaleString("de-DE")} {records} · {RULE_LABEL[stage.decision_rule] ?? stage.decision_rule}
                          {stage.decision_rule === "unanimous" && ` · mind. ${stage.min_reviewers} Stimmen`}
                          {stage.maybe_as && ` · Maybe zählt als ${stage.maybe_as === "include" ? "Include" : "Exclude"}`}
                        </p>
                        {round.gate && (
                          <p className={round.gate.passed ? "gate-line ok" : "gate-line bad"}>
                            <Icon name={round.gate.passed ? "check" : "x"} size={12} /> Gate {round.gate.passed ? "erfüllt" : "nicht erfüllt"}: {gateText(round.gate)}
                          </p>
                        )}
                        <div className="stage-card-actions">
                          {canVote(review.role) && !stage.closed && (
                            <Link className="button primary" to={`/reviews/${review.id}/stages/${stage.id}/screen`}>
                              <Icon name="play" size={14} /> Screenen
                            </Link>
                          )}
                          <Link className="button" to={`/reviews/${review.id}/stages/${stage.id}`}>
                            <Icon name="chart" size={14} /> Auswertung
                          </Link>
                        </div>
                      </article>
                    );
                  })}
                </div>
              )}

              <div className="station-actions">
                {(TOOL_TABS[station.tool ?? ""] ?? []).map((t) => (
                  <button key={t.tab} onClick={() => goTo(t.tab)}>{t.label} öffnen</button>
                ))}
                {canOpen && creating?.key !== station.key && (
                  <button className={station.state === "open" ? "primary" : ""} onClick={() => setCreating(station)}>
                    <Icon name="plus" size={14} /> {station.free_rounds || station.rounds.length === 0 ? "Stufe öffnen" : "Neue Runde öffnen"}
                  </button>
                )}
                {canOverride && overriding !== station.key && (
                  <button className="ghost" onClick={() => { setOverriding(station.key); setReason(""); }}>Übersteuern …</button>
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
                    }, `„${station.label}“ übersteuert. Die Begründung steht im Log.`);
                  }}
                >
                  <label htmlFor={`override-${station.key}`}>Warum gilt die Station ohne erfülltes Gate als abgeschlossen?</label>
                  <div className="inline-form">
                    <input id={`override-${station.key}`} value={reason} onChange={(e) => setReason(e.target.value)} placeholder="Begründung, erscheint später im Bericht" required autoFocus />
                    <button type="button" onClick={() => setOverriding("")}>Abbrechen</button>
                    <button className="primary" type="submit">Übersteuern</button>
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
                    }, "Stufe geöffnet. Schick den Mitgliedern unter „Austausch“ ein neues Paket.")
                  }
                />
              )}
            </section>
          );
        })}
      </div>
      {!manage && review.stages.length === 0 && (
        <p className="muted center">Sobald die Leitung eine Stufe öffnet, bekommst du ein neues Paket.</p>
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
  const records = term(review.workflow.terms, "records", "Einträge");
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
  const single = decisionRule === "single_verified";
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
      verify: single ? { scope: verifyScope, percent: verifyScope === "sample" ? verifyPercent : null } : null,
    });
  }

  return (
    <form className="sheet" onSubmit={submit}>
      <h2>{station.label}: {station.free_rounds || defaults.round === 1 ? "neue Stufe" : `Runde ${defaults.round}`}</h2>
      <Group>
        <Row>
          <label htmlFor="stage-name">Name</label>
          <input id="stage-name" value={name} onChange={(e) => setName(e.target.value)} required />
        </Row>
        <Row>
          <label htmlFor="stage-scope">Umfang</label>
          <select id="stage-scope" value={scope} onChange={(e) => setScope(e.target.value)} disabled={scopeFixed}>
            <option value="sample">Zufallsstichprobe</option>
            <option value="all">Alle {records} ohne Dubletten</option>
            <option value="from_stage">Eingeschlossene einer früheren Stufe</option>
          </select>
        </Row>
        {scope === "sample" && (
          <Row>
            <label htmlFor="stage-sample">Größe der Stichprobe</label>
            <input id="stage-sample" className="narrow-input" type="number" min={1} value={sampleSize} onChange={(e) => setSampleSize(Number(e.target.value))} />
          </Row>
        )}
        {scope === "from_stage" && (
          <Row>
            <label htmlFor="stage-source">Quellstufe</label>
            <select id="stage-source" value={sourceStage} onChange={(e) => setSourceStage(e.target.value)} required disabled={scopeFixed && !!defaults.source_stage}>
              <option value="">Bitte wählen</option>
              {review.stages.map((s) => (
                <option key={s.id} value={s.id}>{s.name}</option>
              ))}
            </select>
          </Row>
        )}
        <Row>
          <label htmlFor="stage-rule">
            Auswahlregel
            {!rules.decision_rule.editable && <small>von der Vorlage festgelegt</small>}
          </label>
          <select id="stage-rule" value={decisionRule} onChange={(e) => setDecisionRule(e.target.value)} disabled={!rules.decision_rule.editable}>
            {Object.entries(RULE_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </Row>
        {single ? (
          <Row>
            <label htmlFor="stage-verify">
              Zweite Stimme für
              <small>Die zweite Person sieht die Ausschlüsse; die Stufe ist nicht verblindet</small>
            </label>
            <span className="inline-actions">
              <select id="stage-verify" value={verifyScope} onChange={(e) => setVerifyScope(e.target.value as "all" | "sample")}>
                <option value="all">alle Ausschlüsse</option>
                <option value="sample">eine Stichprobe der Ausschlüsse</option>
              </select>
              {verifyScope === "sample" && (
                <>
                  <input className="narrow-input" type="number" min={1} max={100} value={verifyPercent} onChange={(e) => setVerifyPercent(Number(e.target.value))} aria-label="Anteil in Prozent" /> %
                </>
              )}
            </span>
          </Row>
        ) : (
          <Row>
            <label htmlFor="stage-min">
              Stimmen je Eintrag
              {decisionRule === "liberal" && <small>Ein Ausschluss braucht mindestens zwei</small>}
            </label>
            <input id="stage-min" className="narrow-input" type="number" min={1} value={minReviewers} onChange={(e) => setMinReviewers(Number(e.target.value))} disabled={!rules.min_reviewers.editable} />
          </Row>
        )}
        <Row>
          <label htmlFor="stage-maybe">„Maybe“ zählt als</label>
          <select id="stage-maybe" value={maybeAs} onChange={(e) => setMaybeAs(e.target.value)} disabled={!rules.maybe_as.editable}>
            <option value="">eigene Kategorie</option>
            <option value="include">Include</option>
            <option value="exclude">Exclude</option>
          </select>
        </Row>
        <Row>
          <label htmlFor="stage-conflict">
            Klärung
            <small>Bei Dritt-Entscheid darf niemand mit eigener Stimme den Konsens festhalten</small>
          </label>
          <select id="stage-conflict" value={conflictRule} onChange={(e) => setConflictRule(e.target.value)} disabled={!rules.conflict_rule.editable}>
            {Object.entries(CONFLICT_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </Row>
        <Row>
          <label htmlFor="stage-blind">
            Verblindet
            <small>Stimmen anderer bleiben bis zum Entblinden unsichtbar</small>
          </label>
          <input id="stage-blind" type="checkbox" className="switch" checked={!single && blind} onChange={(e) => setBlind(e.target.checked)} disabled={single || !rules.blind.editable} />
        </Row>
      </Group>
      <p className="hint">Die Regeln sind nach dem Öffnen fest. Für andere Regeln öffnest du eine neue Stufe.</p>
      <div className="sheet-actions">
        <button type="button" onClick={onCancel}>Abbrechen</button>
        <button className="primary" type="submit">Stufe öffnen</button>
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
      if (result.review_id !== review.id) return `${importSummary(result)} Das war ein anderes Review.`;
      const rejected = result.report.rejected;
      return importSummary(result) + (rejected.length ? ` Abgewiesen: ${rejected.join(" · ")}` : "");
    });
  }

  async function exportReturn() {
    const path = await save({
      defaultPath: `${safeFileName(review.title)} – Rückmeldung ${safeFileName(me?.name ?? "")}.ortvotes`,
      filters: [{ name: "Rückmeldung", extensions: ["ortvotes"] }],
    });
    if (!path) return;
    await run(async () => {
      const n = await api.exportReturn(review.id, path);
      return `Rückmeldung mit ${n} eigenen Einträgen gespeichert. Schick sie an die Leitung.`;
    });
  }

  async function exportPackage(member: Member) {
    const path = await save({
      defaultPath: `${safeFileName(review.title)} – Paket für ${safeFileName(member.name)}.ortpkg`,
      filters: [{ name: "Paket", extensions: ["ortpkg"] }],
    });
    if (!path) return;
    await run(() => api.exportPackage(review.id, member.actor_id, path), `Paket für ${member.name} gespeichert.`);
  }

  const others = review.members.filter((m) => !m.is_me);

  return (
    <>
      <div className="exchange-cards">
        <button className="exchange-card" onClick={importBundle}>
          <Icon name="download" size={20} />
          <strong>Datei einlesen</strong>
          <span>{manage ? "Rückmeldungen deines Teams" : "Neues Paket der Leitung"}</span>
        </button>
        {canVote(review.role) && (
          <button className="exchange-card" onClick={exportReturn}>
            <Icon name="upload" size={20} />
            <strong>Rückmeldung speichern</strong>
            <span>Deine signierten Entscheidungen</span>
          </button>
        )}
      </div>
      {manage && (
        <Group
          title="Pakete verteilen"
          footer="In verblindeten Stufen enthält jedes Paket nur die eigenen Stimmen der Person. Was nicht in der Datei steht, kann niemand sehen."
        >
          {others.length === 0 && <Row><span className="muted">Füge unter „Mitglieder“ zuerst Personen hinzu.</span></Row>}
          {others.map((m) => (
            <Row key={m.actor_id}>
              <span className="person">
                <Avatar name={m.name} />
                <span>
                  {m.name}
                  <small>{ROLE_LABEL[m.role]}</small>
                </span>
              </span>
              <button onClick={() => exportPackage(m)}><Icon name="upload" size={14} /> Paket speichern</button>
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
    const path = await open({ multiple: false, directory: false, filters: [{ name: "Identitätsdatei", extensions: ["ortid"] }] });
    if (typeof path !== "string") return;
    await run(async () => `${await api.addMember(review.id, path, role)} ist jetzt Mitglied. Schick unter „Austausch“ ein Paket.`);
  }

  return (
    <>
      <Group title={`${review.members.length} Mitglieder`}>
        {review.members.map((m) => (
          <Row key={m.actor_id}>
            <span className="person">
              <Avatar name={m.name} />
              <span>
                {m.name}{m.is_me && <span className="muted"> (du)</span>}
                <small className="mono">{m.actor_id.slice(0, 8)}</small>
              </span>
            </span>
            {manage ? (
              <select
                aria-label={`Rolle von ${m.name}`}
                value={m.role}
                onChange={(e) => run(() => api.setRole(review.id, m.actor_id, e.target.value), "Rolle geändert.")}
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
        <Group title="Mitglied hinzufügen" footer="Die Person speichert in ihrer App eine Identitätsdatei (.ortid) und schickt sie dir.">
          <Row>
            <label htmlFor="new-role">Rolle</label>
            <select id="new-role" value={role} onChange={(e) => setRole(e.target.value)}>
              <option value="reviewer">Reviewer</option>
              <option value="admin">Admin</option>
              <option value="viewer">Viewer</option>
              <option value="owner">Owner</option>
            </select>
          </Row>
          <Row>
            <span className="muted">Identitätsdatei</span>
            <button onClick={add}><Icon name="file" size={14} /> Wählen …</button>
          </Row>
        </Group>
      )}
    </>
  );
}

function ImportsTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const manage = canManage(review.role);
  const [source, setSource] = useState("");

  async function choose() {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "RIS oder BibTeX", extensions: ["ris", "bib", "bibtex", "txt", "nbib"] }],
    });
    if (typeof path !== "string") return;
    await run(async () => {
      const batch = await api.importRecords(review.id, path, source);
      setSource("");
      return batch.status === "failed"
        ? `Import fehlgeschlagen: ${batch.error}`
        : `${batch.n_records} Einträge importiert, davon ${batch.n_duplicates} Dubletten.`;
    });
  }

  return (
    <>
      {review.imports.length === 0 ? (
        <Empty icon="upload" title="Noch nichts importiert">Exportiere die Treffer deiner Suche aus PubMed, Scopus oder Zotero als RIS oder BibTeX.</Empty>
      ) : (
        <Group title="Importe">
          {review.imports.map((b) => (
            <Row key={b.id}>
              <span className="person">
                <span className="file-icon"><Icon name="file" /></span>
                <span>
                  {b.filename}
                  <small>
                    {b.status === "failed" ? b.error : `${b.n_records} Einträge · ${b.n_duplicates} Dubletten`}
                    {b.source && ` · ${b.source}`}
                  </small>
                </span>
              </span>
              <Pill>{IMPORT_STATUS[b.status] ?? b.status}</Pill>
            </Row>
          ))}
        </Group>
      )}
      {manage && (
        <Group title="Datei importieren" footer="Dubletten werden über die DOI oder den Titel mit Jahr erkannt und nicht gescreent.">
          <Row>
            <label htmlFor="import-source">Quelle</label>
            <input id="import-source" value={source} onChange={(e) => setSource(e.target.value)} placeholder="z. B. PubMed" />
          </Row>
          <Row>
            <span className="muted">RIS oder BibTeX</span>
            <button className="primary" onClick={choose}><Icon name="upload" size={14} /> Datei wählen …</button>
          </Row>
        </Group>
      )}
    </>
  );
}

function CriteriaTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const c = review.criteria;
  const [text, setText] = useState(c.text);
  const [include, setInclude] = useState(c.include_keywords.join(", "));
  const [exclude, setExclude] = useState(c.exclude_keywords.join(", "));
  const [reasons, setReasons] = useState(c.exclusion_reasons.join("\n"));
  const [changeNote, setChangeNote] = useState("");

  if (!canManage(review.role)) {
    return (
      <Group title={`Version ${c.version}`}>
        <div className="prose">{c.text || <span className="muted">Noch keine Kriterien hinterlegt.</span>}</div>
        {c.exclusion_reasons.length > 0 && (
          <div className="chips">{c.exclusion_reasons.map((r) => <span key={r} className="chip">{r}</span>)}</div>
        )}
      </Group>
    );
  }

  function submit(event: React.FormEvent) {
    event.preventDefault();
    run(async () => {
      await api.publishCriteria(review.id, { text, include_keywords: include, exclude_keywords: exclude, exclusion_reasons: reasons, change_note: changeNote });
      setChangeNote("");
    }, "Neue Kriterienversion veröffentlicht.");
  }

  return (
    <form onSubmit={submit}>
      <Group
        title={c.version ? `Version ${c.version}` : "Noch keine Version"}
        footer={c.change_note ? `Letzte Änderung: ${c.change_note}` : "Jede Änderung wird als neue Version protokolliert. Stufen merken sich, mit welcher Version sie geöffnet wurden."}
      >
        <div className="stack-field">
          <label htmlFor="crit-text">Ein- und Ausschlusskriterien</label>
          <textarea id="crit-text" rows={5} value={text} onChange={(e) => setText(e.target.value)} placeholder="Population, Konzept, Kontext …" />
        </div>
        <div className="stack-field">
          <label htmlFor="crit-inc">Hervorheben als Include <small>durch Komma getrennt</small></label>
          <input id="crit-inc" value={include} onChange={(e) => setInclude(e.target.value)} />
        </div>
        <div className="stack-field">
          <label htmlFor="crit-exc">Hervorheben als Exclude <small>durch Komma getrennt</small></label>
          <input id="crit-exc" value={exclude} onChange={(e) => setExclude(e.target.value)} />
        </div>
        <div className="stack-field">
          <label htmlFor="crit-reasons">Ausschlussgründe <small>einer je Zeile</small></label>
          <textarea id="crit-reasons" rows={4} value={reasons} onChange={(e) => setReasons(e.target.value)} />
        </div>
        <div className="stack-field">
          <label htmlFor="crit-note">Was hat sich geändert?</label>
          <input id="crit-note" value={changeNote} onChange={(e) => setChangeNote(e.target.value)} />
        </div>
      </Group>
      <div className="sheet-actions">
        <button className="primary" type="submit">Als Version {c.version + 1} veröffentlichen</button>
      </div>
    </form>
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
        title="Hash-Ketten"
        footer="Prüft Prüfsumme, Signatur und Lückenlosigkeit jedes Ereignisses. Eine nachträgliche Änderung an der Datei fällt hier auf."
      >
        <Row>
          <span className="person">
            <span className={`proof-icon ${chain ? (chain.ok ? "ok" : "bad") : ""}`}><Icon name="shield" size={18} /></span>
            <span>
              {chain ? (chain.ok ? "Alles unversehrt" : `${chain.n_bad} fehlerhafte Ereignisse`) : "Noch nicht geprüft"}
              <small>{chain ? `${chain.n_events} Ereignisse geprüft` : "Dauert wenige Sekunden"}</small>
            </span>
          </span>
          <button onClick={verify}>Jetzt prüfen</button>
        </Row>
        {chain && !chain.ok && chain.problems.map((p) => <Row key={p}><span className="error-text">{p}</span></Row>)}
      </Group>
      {error && <Banner kind="error">{error}</Banner>}
    </>
  );
}
