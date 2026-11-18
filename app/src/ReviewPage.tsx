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

const ROLE_LABEL: Record<string, string> = { owner: "Owner", admin: "Admin", reviewer: "Reviewer", viewer: "Viewer" };
const IMPORT_STATUS: Record<string, string> = { running: "läuft", done: "fertig", failed: "fehlgeschlagen" };

function stageState(stage: Stage): string {
  if (stage.closed) return "geschlossen";
  if (stage.blind_active) return "verblindet";
  return stage.blind ? "entblindet" : "offen";
}

export function ReviewPage() {
  const { reviewId = "" } = useParams();
  const [review, setReview] = useState<ReviewDetail | null>(null);
  const [chain, setChain] = useState<ChainReport | null>(null);
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
    load();
  }, [reviewId]);

  async function run(action: () => Promise<string | void>, done: string) {
    setError("");
    setNotice("");
    try {
      const message = await action();
      setNotice(message || done);
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  }

  if (!review) return <p>{error || "Lade …"}</p>;
  const manage = canManage(review.role);

  async function importBundle() {
    const path = await open({ multiple: false, directory: false, filters: BUNDLE_FILTER });
    if (typeof path !== "string") return;
    await run(async () => {
      const result = await api.importBundle(path);
      if (result.review_id !== reviewId) return `${importSummary(result)} Das war ein anderes Review.`;
      const rejected = result.report.rejected;
      return importSummary(result) + (rejected.length ? ` Abgewiesen: ${rejected.join(" · ")}` : "");
    }, "");
  }

  async function exportReturn() {
    const me = review!.members.find((m) => m.is_me);
    const path = await save({
      defaultPath: `${safeFileName(review!.title)} – Rückmeldung ${safeFileName(me?.name ?? "")}.ortvotes`,
      filters: [{ name: "Rückmeldung", extensions: ["ortvotes"] }],
    });
    if (!path) return;
    await run(async () => {
      const n = await api.exportReturn(reviewId, path);
      return `Rückmeldung mit ${n} eigenen Einträgen gespeichert. Schick sie an die Person, die das Review leitet.`;
    }, "");
  }

  async function exportPackage(member: Member) {
    const path = await save({
      defaultPath: `${safeFileName(review!.title)} – Paket für ${safeFileName(member.name)}.ortpkg`,
      filters: [{ name: "Paket", extensions: ["ortpkg"] }],
    });
    if (!path) return;
    await run(() => api.exportPackage(reviewId, member.actor_id, path), `Paket für ${member.name} gespeichert.`);
  }

  async function verify() {
    setError("");
    try {
      setChain(await api.verifyChain(reviewId));
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <>
      <p><Link to="/">← Reviews</Link></p>
      <h1>{review.title}</h1>
      <p className="muted">
        Deine Rolle: {ROLE_LABEL[review.role] ?? review.role} · {review.n_records - review.n_duplicates} Records
        {review.n_duplicates > 0 && <> · erkannte Dubletten: {review.n_duplicates}</>}
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <section className="card">
        <h2>Stufen</h2>
        {review.stages.length === 0 && <p className="muted">Noch keine Stufe geöffnet.</p>}
        <ul className="list">
          {review.stages.map((stage) => (
            <li key={stage.id}>
              <span>
                <strong>{stage.name}</strong> <span className="badge">{stageState(stage)}</span>
                <br />
                <span className="muted">{stage.n_records} Records · mindestens {stage.min_reviewers} Stimmen</span>
              </span>
              <span className="actions">
                {canVote(review.role) && !stage.closed && (
                  <Link className="button primary" to={`/reviews/${reviewId}/stages/${stage.id}/screen`}>Screenen</Link>
                )}
                <Link className="button" to={`/reviews/${reviewId}/stages/${stage.id}`}>Auswertung</Link>
              </span>
            </li>
          ))}
        </ul>
        {manage && <StageForm review={review} onSubmit={(params) => run(() => api.openStage(reviewId, params).then(() => {}), "Stufe geöffnet. Schick den Mitgliedern ein neues Paket.")} />}
      </section>

      <section className="card">
        <h2>Austausch</h2>
        <p className="muted">
          Es gibt keinen Server. {manage
            ? "Schick jedem Mitglied ein Paket. Bei verblindeten Stufen enthält es nur dessen eigene Stimmen. Rückmeldungen liest du hier ein."
            : "Nach dem Screenen speicherst du eine Rückmeldung und schickst sie an die Person, die das Review leitet. Neue Pakete liest du hier ein."}
        </p>
        <div className="actions">
          <button className="primary" onClick={importBundle}>Paket oder Rückmeldung einlesen</button>
          {canVote(review.role) && <button onClick={exportReturn}>Rückmeldung speichern</button>}
        </div>
      </section>

      <section className="card">
        <h2>Mitglieder</h2>
        <ul className="list">
          {review.members.map((m) => (
            <li key={m.actor_id}>
              <span>
                {m.name}{m.is_me && " (du)"} <span className="badge">{ROLE_LABEL[m.role] ?? m.role}</span>
              </span>
              {manage && (
                <span className="actions">
                  {!m.is_me && <button onClick={() => exportPackage(m)}>Paket speichern</button>}
                  <select
                    aria-label={`Rolle von ${m.name}`}
                    value={m.role}
                    onChange={(e) => run(() => api.setRole(reviewId, m.actor_id, e.target.value), "Rolle geändert.")}
                  >
                    {Object.entries(ROLE_LABEL).map(([value, label]) => (
                      <option key={value} value={value}>{label}</option>
                    ))}
                  </select>
                </span>
              )}
            </li>
          ))}
        </ul>
        {manage && <MemberForm onSubmit={(path, role) => run(async () => `${await api.addMember(reviewId, path, role)} ist jetzt Mitglied. Schick ein Paket.`, "")} />}
      </section>

      <section className="card">
        <h2>Import</h2>
        {review.imports.length === 0 && <p className="muted">Noch nichts importiert.</p>}
        <ul className="list">
          {review.imports.map((b) => (
            <li key={b.id}>
              <span>
                {b.filename} <span className="badge">{IMPORT_STATUS[b.status] ?? b.status}</span>
                <br />
                <span className="muted">
                  {b.status === "failed" ? b.error : `Einträge: ${b.n_records}, davon Dubletten: ${b.n_duplicates}`}
                  {b.source && ` · ${b.source}`}
                </span>
              </span>
            </li>
          ))}
        </ul>
        {manage && <ImportForm onSubmit={(path, source) => run(async () => {
          const batch = await api.importRecords(reviewId, path, source);
          return batch.status === "failed" ? `Import fehlgeschlagen: ${batch.error}` : `${batch.n_records} Einträge importiert, davon ${batch.n_duplicates} Dubletten.`;
        }, "")} />}
      </section>

      <CriteriaSection review={review} onSubmit={(body) => run(() => api.publishCriteria(reviewId, body).then(() => {}), "Neue Kriterienversion veröffentlicht.")} />

      <section className="card">
        <h2>Nachweis</h2>
        <p className="muted">Prüft Prüfsummen, Signaturen und Lückenlosigkeit aller Ereignisketten in diesem Review.</p>
        <button onClick={verify}>Hash-Ketten prüfen</button>
        {chain && (
          <p className={chain.ok ? "notice" : "error"}>
            {chain.ok ? `Alle ${chain.n_events} Ereignisse sind unversehrt.` : `${chain.n_bad} von ${chain.n_events} Ereignissen fehlerhaft: ${chain.problems.join(" · ")}`}
          </p>
        )}
      </section>
    </>
  );
}

function StageForm({ review, onSubmit }: { review: ReviewDetail; onSubmit: (params: StageParams) => void }) {
  const [name, setName] = useState("Titel/Abstract");
  const [scope, setScope] = useState("all");
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
    <form onSubmit={submit}>
      <h3>Neue Stufe öffnen</h3>
      <label>
        Name
        <input value={name} onChange={(e) => setName(e.target.value)} required />
      </label>
      <label>
        Umfang
        <select value={scope} onChange={(e) => setScope(e.target.value)}>
          <option value="all">Alle Records ohne Dubletten</option>
          <option value="sample">Zufallsstichprobe (Pilot)</option>
          <option value="from_stage">Eingeschlossene einer früheren Stufe</option>
        </select>
      </label>
      {scope === "sample" && (
        <label>
          Größe der Stichprobe
          <input type="number" min={1} value={sampleSize} onChange={(e) => setSampleSize(Number(e.target.value))} />
        </label>
      )}
      {scope === "from_stage" && (
        <label>
          Quellstufe
          <select value={sourceStage} onChange={(e) => setSourceStage(e.target.value)} required>
            <option value="">Bitte wählen</option>
            {review.stages.map((s) => (
              <option key={s.id} value={s.id}>{s.name}</option>
            ))}
          </select>
        </label>
      )}
      <label>
        Mindestzahl der Stimmen je Record
        <input type="number" min={1} value={minReviewers} onChange={(e) => setMinReviewers(Number(e.target.value))} />
      </label>
      <label>
        „Maybe“ zählt als
        <select value={maybeAs} onChange={(e) => setMaybeAs(e.target.value)}>
          <option value="">eigene Kategorie</option>
          <option value="include">Include</option>
          <option value="exclude">Exclude</option>
        </select>
      </label>
      <label className="check">
        <input type="checkbox" checked={blind} onChange={(e) => setBlind(e.target.checked)} />
        Verblindet: Stimmen anderer bleiben bis zum Entblinden unsichtbar
      </label>
      <button className="primary" type="submit">Stufe öffnen</button>
    </form>
  );
}

function ImportForm({ onSubmit }: { onSubmit: (path: string, source: string) => void }) {
  const [path, setPath] = useState<string | null>(null);
  const [source, setSource] = useState("");

  async function choose() {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "RIS oder BibTeX", extensions: ["ris", "bib", "bibtex", "txt", "nbib"] }],
    });
    if (typeof picked === "string") setPath(picked);
  }

  function submit(event: React.FormEvent) {
    event.preventDefault();
    if (path) onSubmit(path, source);
  }

  return (
    <form onSubmit={submit}>
      <h3>Datei importieren</h3>
      <div className="actions">
        <button type="button" onClick={choose}>Datei wählen …</button>
        <span className="muted">{path ? path.split("/").pop() : "RIS oder BibTeX"}</span>
      </div>
      <label>
        Quelle, z. B. PubMed
        <input value={source} onChange={(e) => setSource(e.target.value)} />
      </label>
      <button className="primary" type="submit" disabled={!path}>Importieren</button>
    </form>
  );
}

function CriteriaSection({ review, onSubmit }: { review: ReviewDetail; onSubmit: (body: Record<string, unknown>) => void }) {
  const c = review.criteria;
  const [text, setText] = useState(c.text);
  const [include, setInclude] = useState(c.include_keywords.join(", "));
  const [exclude, setExclude] = useState(c.exclude_keywords.join(", "));
  const [reasons, setReasons] = useState(c.exclusion_reasons.join("\n"));
  const [changeNote, setChangeNote] = useState("");

  function submit(event: React.FormEvent) {
    event.preventDefault();
    onSubmit({ text, include_keywords: include, exclude_keywords: exclude, exclusion_reasons: reasons, change_note: changeNote });
    setChangeNote("");
  }

  if (!canManage(review.role)) {
    return (
      <section className="card">
        <h2>Kriterien (Version {c.version})</h2>
        <p className="pre">{c.text || "Noch keine Kriterien hinterlegt."}</p>
      </section>
    );
  }
  return (
    <section className="card">
      <h2>Kriterien (Version {c.version})</h2>
      {c.change_note && <p className="muted">Letzte Änderung: {c.change_note}</p>}
      <form onSubmit={submit}>
        <label>
          Ein- und Ausschlusskriterien
          <textarea rows={4} value={text} onChange={(e) => setText(e.target.value)} />
        </label>
        <label>
          Keywords für Include, durch Komma getrennt
          <input value={include} onChange={(e) => setInclude(e.target.value)} />
        </label>
        <label>
          Keywords für Exclude, durch Komma getrennt
          <input value={exclude} onChange={(e) => setExclude(e.target.value)} />
        </label>
        <label>
          Ausschlussgründe, einer je Zeile
          <textarea rows={3} value={reasons} onChange={(e) => setReasons(e.target.value)} />
        </label>
        <label>
          Was hat sich geändert?
          <input value={changeNote} onChange={(e) => setChangeNote(e.target.value)} />
        </label>
        <button type="submit">Als neue Version veröffentlichen</button>
      </form>
    </section>
  );
}

function MemberForm({ onSubmit }: { onSubmit: (path: string, role: string) => void }) {
  const [role, setRole] = useState("reviewer");

  async function choose() {
    const path = await open({ multiple: false, directory: false, filters: [{ name: "Identitätsdatei", extensions: ["ortid"] }] });
    if (typeof path === "string") onSubmit(path, role);
  }

  return (
    <div>
      <h3>Mitglied hinzufügen</h3>
      <p className="muted">Die Person speichert in ihrer App eine Identitätsdatei (.ortid) und schickt sie dir.</p>
      <label>
        Rolle
        <select value={role} onChange={(e) => setRole(e.target.value)}>
          <option value="reviewer">Reviewer</option>
          <option value="admin">Admin</option>
          <option value="viewer">Viewer</option>
          <option value="owner">Owner</option>
        </select>
      </label>
      <button onClick={choose}>Identitätsdatei wählen …</button>
    </div>
  );
}
