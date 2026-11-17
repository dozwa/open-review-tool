import React, { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";

import { api, canManage, canVote, errorText, ReviewDetail, Stage } from "./api";

const ROLE_LABEL: Record<string, string> = { owner: "Owner", admin: "Admin", reviewer: "Reviewer", viewer: "Viewer" };

function stageState(stage: Stage): string {
  if (stage.closed) return "geschlossen";
  if (stage.blind_active) return "verblindet";
  return stage.blind ? "entblindet" : "offen";
}

export function ReviewPage() {
  const { reviewId } = useParams();
  const [review, setReview] = useState<ReviewDetail | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  async function load() {
    try {
      setReview(await api<ReviewDetail>(`/reviews/${reviewId}`));
    } catch (e) {
      setError(errorText(e));
    }
  }
  useEffect(() => {
    load();
  }, [reviewId]);

  // Solange ein Import läuft, regelmäßig nachladen.
  const importing = review?.imports.some((b) => b.status === "pending" || b.status === "running") ?? false;
  useEffect(() => {
    if (!importing) return;
    const timer = window.setInterval(load, 2000);
    return () => window.clearInterval(timer);
  }, [importing]);

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

  if (!review) return <p>{error || "Lade …"}</p>;
  const manage = canManage(review.role);

  return (
    <>
      <p><Link to="/">← Reviews</Link></p>
      <h1>{review.title}</h1>
      <p className="muted">
        Deine Rolle: {ROLE_LABEL[review.role] ?? review.role} · {review.n_records - review.n_duplicates} Records
        {review.n_duplicates > 0 && <> ({review.n_duplicates} Dubletten erkannt)</>}
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
                {canVote(review.role) && !stage.closed && <Link className="button primary" to={`/stages/${stage.id}/screen`}>Screenen</Link>}
                <Link className="button" to={`/stages/${stage.id}`}>Auswertung</Link>
              </span>
            </li>
          ))}
        </ul>
        {manage && <StageForm review={review} onSubmit={(body) => run(() => api(`/reviews/${review.id}/stages`, { method: "POST", body }), "Stufe geöffnet.")} />}
      </section>

      <section className="card">
        <h2>Import</h2>
        {review.imports.length === 0 && <p className="muted">Noch nichts importiert.</p>}
        <ul className="list">
          {review.imports.map((b) => (
            <li key={b.id}>
              <span>
                {b.filename} <span className="badge">{b.status}</span>
                <br />
                <span className="muted">
                  {b.status === "failed" ? b.error : `${b.n_records} Einträge, ${b.n_duplicates} Dubletten`}
                  {b.source && ` · ${b.source}`}
                </span>
              </span>
            </li>
          ))}
        </ul>
        {manage && <ImportForm onSubmit={(form) => run(() => api(`/reviews/${review.id}/imports`, { method: "POST", form }), "Import gestartet.")} />}
      </section>

      <CriteriaSection review={review} onSubmit={(body) => run(() => api(`/reviews/${review.id}/criteria`, { method: "PUT", body }), "Neue Kriterienversion veröffentlicht.")} />

      <section className="card">
        <h2>Mitglieder</h2>
        <ul className="list">
          {review.members.map((m) => (
            <li key={m.username}>
              {m.username} <span className="badge">{ROLE_LABEL[m.role] ?? m.role}</span>
            </li>
          ))}
        </ul>
        {manage && <MemberForm onSubmit={(body) => run(() => api(`/reviews/${review.id}/members`, { method: "POST", body }), "Mitglied gespeichert.")} />}
      </section>
    </>
  );
}

function StageForm({ review, onSubmit }: { review: ReviewDetail; onSubmit: (body: Record<string, unknown>) => void }) {
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

function ImportForm({ onSubmit }: { onSubmit: (form: FormData) => void }) {
  const [file, setFile] = useState<File | null>(null);
  const [source, setSource] = useState("");

  function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!file) return;
    const form = new FormData();
    form.append("file", file);
    form.append("source", source);
    onSubmit(form);
  }

  return (
    <form onSubmit={submit}>
      <h3>Datei importieren</h3>
      <label>
        RIS oder BibTeX
        <input type="file" accept=".ris,.bib,.txt" onChange={(e) => setFile(e.target.files?.[0] ?? null)} required />
      </label>
      <label>
        Quelle, z. B. PubMed
        <input value={source} onChange={(e) => setSource(e.target.value)} />
      </label>
      <button className="primary" type="submit">Importieren</button>
    </form>
  );
}

function CriteriaSection({ review, onSubmit }: { review: ReviewDetail; onSubmit: (body: Record<string, unknown>) => void }) {
  const c = review.criteria;
  const [text, setText] = useState(c.text);
  const [include, setInclude] = useState(c.include_keywords.join(", "));
  const [exclude, setExclude] = useState(c.exclude_keywords.join(", "));
  const [reasons, setReasons] = useState(c.exclusion_reasons.join("\n"));

  function submit(event: React.FormEvent) {
    event.preventDefault();
    onSubmit({ text, include_keywords: include, exclude_keywords: exclude, exclusion_reasons: reasons });
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
        <button type="submit">Als neue Version veröffentlichen</button>
      </form>
    </section>
  );
}

function MemberForm({ onSubmit }: { onSubmit: (body: Record<string, unknown>) => void }) {
  const [username, setUsername] = useState("");
  const [role, setRole] = useState("reviewer");

  function submit(event: React.FormEvent) {
    event.preventDefault();
    onSubmit({ username, role });
    setUsername("");
  }

  return (
    <form onSubmit={submit}>
      <h3>Mitglied hinzufügen oder Rolle ändern</h3>
      <label>
        Benutzername
        <input value={username} onChange={(e) => setUsername(e.target.value)} required />
      </label>
      <label>
        Rolle
        <select value={role} onChange={(e) => setRole(e.target.value)}>
          <option value="reviewer">Reviewer</option>
          <option value="admin">Admin</option>
          <option value="viewer">Viewer</option>
          <option value="owner">Owner</option>
        </select>
      </label>
      <button type="submit">Speichern</button>
    </form>
  );
}
