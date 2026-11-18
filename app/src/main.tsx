import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter, Link, Route, Routes, useNavigate } from "react-router-dom";
import { open, save } from "@tauri-apps/plugin-dialog";

import { api, BUNDLE_FILTER, errorText, importSummary, Person, ReviewListItem, safeFileName } from "./api";
import { ReviewPage } from "./ReviewPage";
import { ScreeningPage, StagePage } from "./StagePages";
import "./styles.css";

const ROLE_LABEL: Record<string, string> = { owner: "Owner", admin: "Admin", reviewer: "Reviewer", viewer: "Viewer" };

function Setup({ onDone }: { onDone: (me: Person) => void }) {
  const [name, setName] = useState("");
  const [error, setError] = useState("");

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setError("");
    try {
      onDone(await api.createIdentity(name));
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <form className="card narrow" onSubmit={submit}>
      <h1>Willkommen</h1>
      <p>
        Unter diesem Namen sehen dich die anderen im Team. Die App erzeugt dazu einen Schlüssel, mit dem sie
        jede deiner Entscheidungen signiert. Der private Schlüssel bleibt im Schlüsselbund dieses Macs.
      </p>
      <label>
        Dein Name
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus required />
      </label>
      {error && <p className="error">{error}</p>}
      <button className="primary" type="submit">Weiter</button>
    </form>
  );
}

function ReviewsPage({ me }: { me: Person }) {
  const navigate = useNavigate();
  const [reviews, setReviews] = useState<ReviewListItem[] | null>(null);
  const [title, setTitle] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  async function load() {
    try {
      setReviews(await api.reviews());
    } catch (e) {
      setError(errorText(e));
    }
  }
  useEffect(() => {
    load();
  }, []);

  async function create(event: React.FormEvent) {
    event.preventDefault();
    setError("");
    try {
      const review = await api.createReview(title);
      setTitle("");
      navigate(`/reviews/${review.id}`);
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function importBundle() {
    setError("");
    setNotice("");
    const path = await open({ multiple: false, directory: false, filters: BUNDLE_FILTER });
    if (typeof path !== "string") return;
    try {
      const result = await api.importBundle(path);
      setNotice(importSummary(result));
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function exportIdentity() {
    setError("");
    const path = await save({
      defaultPath: `${safeFileName(me.name)}.ortid`,
      filters: [{ name: "Identitätsdatei", extensions: ["ortid"] }],
    });
    if (!path) return;
    try {
      await api.exportIdentity(path);
      setNotice("Identitätsdatei gespeichert. Schick sie der Person, die das Review leitet.");
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <>
      <h1>Reviews</h1>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}
      {reviews === null && !error && <p>Lade …</p>}
      {reviews !== null && reviews.length === 0 && (
        <p className="muted">Noch kein Review auf diesem Mac. Lege eines an oder lies ein Paket ein, das du bekommen hast.</p>
      )}
      <ul className="list">
        {(reviews ?? []).map((r) => (
          <li key={r.id}>
            <Link to={`/reviews/${r.id}`}>{r.title}</Link>
            <span className="badge">{r.role ? ROLE_LABEL[r.role] ?? r.role : "kein Mitglied"}</span>
          </li>
        ))}
      </ul>

      <section className="card">
        <h2>Mitmachen</h2>
        <p className="muted">
          Wer ein Review leitet, braucht zuerst deine Identitätsdatei. Danach bekommst du ein Paket (.ortpkg), das du
          hier einliest.
        </p>
        <div className="actions">
          <button onClick={exportIdentity}>Identitätsdatei speichern</button>
          <button className="primary" onClick={importBundle}>Paket oder Rückmeldung einlesen</button>
        </div>
      </section>

      <form className="card" onSubmit={create}>
        <h2>Neues Review</h2>
        <label>
          Titel
          <input value={title} onChange={(e) => setTitle(e.target.value)} required />
        </label>
        <button className="primary" type="submit">Anlegen</button>
      </form>
    </>
  );
}

function App() {
  const [me, setMe] = useState<Person | null | undefined>(undefined);
  const [error, setError] = useState("");

  useEffect(() => {
    api.identity().then(setMe).catch((e) => setError(errorText(e)));
  }, []);

  if (error) return <main><p className="error">{error}</p></main>;
  if (me === undefined) return <main><p>Lade …</p></main>;
  if (me === null) {
    return (
      <main>
        <Setup onDone={setMe} />
      </main>
    );
  }
  return (
    <HashRouter>
      <header className="topbar">
        <Link to="/" className="brand">Open Review Tool</Link>
        <span title={`ID ${me.actor_id}`}>{me.name}</span>
      </header>
      <main>
        <Routes>
          <Route path="/" element={<ReviewsPage me={me} />} />
          <Route path="/reviews/:reviewId" element={<ReviewPage />} />
          <Route path="/reviews/:reviewId/stages/:stageId" element={<StagePage />} />
          <Route path="/reviews/:reviewId/stages/:stageId/screen" element={<ScreeningPage />} />
          <Route path="*" element={<p>Seite nicht gefunden. <Link to="/">Zur Übersicht</Link></p>} />
        </Routes>
      </main>
    </HashRouter>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
