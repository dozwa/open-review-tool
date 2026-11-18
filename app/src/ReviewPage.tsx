import React, { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { open, save } from "@tauri-apps/plugin-dialog";

import {
  api,
  BUNDLE_FILTER,
  canManage,
  canVote,
  ChainReport,
  errorText,
  importSummary,
  Member,
  ReviewDetail,
  safeFileName,
  Stage,
  StageParams,
} from "./api";
import { ROLE_LABEL, useRefreshSidebar } from "./shared";
import { Avatar, Banner, Empty, Group, Icon, PageHeader, Pill, Row, Segmented } from "./ui";

const IMPORT_STATUS: Record<string, string> = { running: "läuft", done: "fertig", failed: "fehlgeschlagen" };
const KIND_LABEL: Record<string, string> = { pilot: "Pilot", title_abstract: "Titel/Abstract", fulltext: "Volltext" };

type Tab = "stages" | "exchange" | "members" | "imports" | "criteria" | "proof";

export function stageState(stage: Stage): string {
  if (stage.closed) return "Geschlossen";
  if (stage.blind_active) return "Verblindet";
  return stage.blind ? "Entblindet" : "Offen";
}

export function ReviewPage() {
  const { reviewId = "" } = useParams();
  const refreshSidebar = useRefreshSidebar();
  const [review, setReview] = useState<ReviewDetail | null>(null);
  const [tab, setTab] = useState<Tab>("stages");
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
    setTab("stages");
    setError("");
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
  const manage = canManage(review.role);
  const unique = review.n_records - review.n_duplicates;

  const tabs: { value: Tab; label: string; icon: string }[] = [
    { value: "stages", label: "Stufen", icon: "stack" },
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
            <span>{unique.toLocaleString("de-DE")} Records</span>
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
        {tab === "stages" && <StagesTab review={review} run={run} />}
        {tab === "exchange" && <ExchangeTab review={review} run={run} />}
        {tab === "members" && <MembersTab review={review} run={run} />}
        {tab === "imports" && <ImportsTab review={review} run={run} />}
        {tab === "criteria" && <CriteriaTab review={review} run={run} />}
        {tab === "proof" && <ProofTab review={review} />}
      </div>
      {!manage && tab === "stages" && review.stages.length === 0 && (
        <p className="muted center">Sobald die Leitung eine Stufe öffnet, bekommst du ein neues Paket.</p>
      )}
    </>
  );
}

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

function StagesTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const [creating, setCreating] = useState(false);
  const manage = canManage(review.role);
  return (
    <>
      {review.stages.length === 0 && !creating && (
        <Empty icon="stack" title="Noch keine Stufe">
          {manage ? (
            <>
              Starte mit einer Pilotstufe: Das ganze Team screent eine Zufallsstichprobe und gleicht die Kriterien ab.
              <div><button className="primary" onClick={() => setCreating(true)}><Icon name="plus" /> Stufe öffnen</button></div>
            </>
          ) : "Die Leitung hat noch keine Stufe geöffnet."}
        </Empty>
      )}
      <div className="stage-grid">
        {review.stages.map((stage) => (
          <article key={stage.id} className="stage-card">
            <div className="stage-card-head">
              <span className="stage-kind">{KIND_LABEL[stage.kind] ?? stage.kind}</span>
              <Pill>{stageState(stage)}</Pill>
            </div>
            <h3>{stage.name}</h3>
            <p className="muted">
              {stage.n_records.toLocaleString("de-DE")} Records · mind. {stage.min_reviewers} Stimmen
              {stage.maybe_as && ` · Maybe zählt als ${stage.maybe_as === "include" ? "Include" : "Exclude"}`}
            </p>
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
        ))}
        {manage && review.stages.length > 0 && !creating && (
          <button className="stage-card add" onClick={() => setCreating(true)}>
            <Icon name="plus" size={20} />
            Neue Stufe
          </button>
        )}
      </div>
      {creating && (
        <StageForm
          review={review}
          onCancel={() => setCreating(false)}
          onSubmit={(params) =>
            run(async () => {
              await api.openStage(review.id, params);
              setCreating(false);
            }, "Stufe geöffnet. Schick den Mitgliedern unter „Austausch“ ein neues Paket.")
          }
        />
      )}
    </>
  );
}

function StageForm({ review, onSubmit, onCancel }: {
  review: ReviewDetail;
  onSubmit: (params: StageParams) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(review.stages.length === 0 ? "Pilot Titel/Abstract" : "Titel/Abstract");
  const [scope, setScope] = useState(review.stages.length === 0 ? "sample" : "all");
  const [sampleSize, setSampleSize] = useState(25);
  const [sourceStage, setSourceStage] = useState("");
  const [blind, setBlind] = useState(true);
  const [minReviewers, setMinReviewers] = useState(2);
  const [maybeAs, setMaybeAs] = useState("");

  function submit(event: React.FormEvent) {
    event.preventDefault();
    onSubmit({
      name, scope, blind, min_reviewers: minReviewers, maybe_as: maybeAs || null,
      kind: scope === "sample" ? "pilot" : scope === "from_stage" ? "fulltext" : "title_abstract",
      sample_size: scope === "sample" ? sampleSize : null,
      seed: null,
      source_stage: scope === "from_stage" ? sourceStage : null,
    });
  }

  return (
    <form className="sheet" onSubmit={submit}>
      <h2>Neue Stufe</h2>
      <Group>
        <Row>
          <label htmlFor="stage-name">Name</label>
          <input id="stage-name" value={name} onChange={(e) => setName(e.target.value)} required />
        </Row>
        <Row>
          <label htmlFor="stage-scope">Umfang</label>
          <select id="stage-scope" value={scope} onChange={(e) => setScope(e.target.value)}>
            <option value="sample">Zufallsstichprobe (Pilot)</option>
            <option value="all">Alle Records ohne Dubletten</option>
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
            <select id="stage-source" value={sourceStage} onChange={(e) => setSourceStage(e.target.value)} required>
              <option value="">Bitte wählen</option>
              {review.stages.map((s) => (
                <option key={s.id} value={s.id}>{s.name}</option>
              ))}
            </select>
          </Row>
        )}
        <Row>
          <label htmlFor="stage-min">Stimmen je Record</label>
          <input id="stage-min" className="narrow-input" type="number" min={1} value={minReviewers} onChange={(e) => setMinReviewers(Number(e.target.value))} />
        </Row>
        <Row>
          <label htmlFor="stage-maybe">„Maybe“ zählt als</label>
          <select id="stage-maybe" value={maybeAs} onChange={(e) => setMaybeAs(e.target.value)}>
            <option value="">eigene Kategorie</option>
            <option value="include">Include</option>
            <option value="exclude">Exclude</option>
          </select>
        </Row>
        <Row>
          <label htmlFor="stage-blind">
            Verblindet
            <small>Stimmen anderer bleiben bis zum Entblinden unsichtbar</small>
          </label>
          <input id="stage-blind" type="checkbox" className="switch" checked={blind} onChange={(e) => setBlind(e.target.checked)} />
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
