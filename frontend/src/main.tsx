import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Link, Route, Routes } from "react-router-dom";

import { api, errorText, Me, ReviewListItem } from "./api";
import { ReviewPage } from "./ReviewPage";
import { ScreeningPage, StagePage } from "./StagePages";
import "./styles.css";

function Login({ onLogin }: { onLogin: (me: Me) => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setError("");
    try {
      onLogin(await api<Me>("/auth/login", { method: "POST", body: { username, password } }));
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <form className="card narrow" onSubmit={submit}>
      <h1>Anmelden</h1>
      <label>
        Benutzername
        <input value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" autoFocus />
      </label>
      <label>
        Passwort
        <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" />
      </label>
      {error && <p className="error">{error}</p>}
      <button className="primary" type="submit">Anmelden</button>
    </form>
  );
}

function ReviewsPage() {
  const [reviews, setReviews] = useState<ReviewListItem[] | null>(null);
  const [title, setTitle] = useState("");
  const [error, setError] = useState("");

  async function load() {
    try {
      setReviews(await api<ReviewListItem[]>("/reviews"));
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
      await api("/reviews", { method: "POST", body: { title } });
      setTitle("");
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <>
      <h1>Reviews</h1>
      {reviews === null && !error && <p>Lade …</p>}
      {reviews !== null && reviews.length === 0 && <p>Du bist noch in keinem Review Mitglied.</p>}
      <ul className="list">
        {(reviews ?? []).map((r) => (
          <li key={r.id}>
            <Link to={`/reviews/${r.id}`}>{r.title}</Link>
            <span className="badge">{r.role}</span>
          </li>
        ))}
      </ul>
      <form className="card" onSubmit={create}>
        <h2>Neues Review</h2>
        <label>
          Titel
          <input value={title} onChange={(e) => setTitle(e.target.value)} required />
        </label>
        {error && <p className="error">{error}</p>}
        <button className="primary" type="submit">Anlegen</button>
      </form>
    </>
  );
}

function App() {
  const [me, setMe] = useState<Me | null>(null);

  useEffect(() => {
    api<Me>("/auth/me")
      .then(setMe)
      .catch(() => setMe({ authenticated: false }));
  }, []);

  async function logout() {
    await api("/auth/logout", { method: "POST" });
    setMe({ authenticated: false });
  }

  if (me === null) return <main><p>Lade …</p></main>;
  if (!me.authenticated) {
    return (
      <main>
        <Login onLogin={setMe} />
      </main>
    );
  }
  return (
    <BrowserRouter>
      <header className="topbar">
        <Link to="/" className="brand">Open Review Tool</Link>
        <span>
          {me.username}
          {me.is_staff && <> · <a href="/admin/">Verwaltung</a></>}
          {" · "}
          <button className="link" onClick={logout}>Abmelden</button>
        </span>
      </header>
      <main>
        <Routes>
          <Route path="/" element={<ReviewsPage />} />
          <Route path="/reviews/:reviewId" element={<ReviewPage />} />
          <Route path="/stages/:stageId" element={<StagePage />} />
          <Route path="/stages/:stageId/screen" element={<ScreeningPage />} />
          <Route path="*" element={<p>Seite nicht gefunden. <Link to="/">Zur Übersicht</Link></p>} />
        </Routes>
      </main>
    </BrowserRouter>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
