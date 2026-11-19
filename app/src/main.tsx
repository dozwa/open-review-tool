import React, { useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter, Link, NavLink, Route, Routes, useLocation, useMatch, useNavigate } from "react-router-dom";
import { open, save } from "@tauri-apps/plugin-dialog";

import { api, BUNDLE_FILTER, errorText, importSummary, Person, ReviewListItem, safeFileName } from "./api";
import { NewReviewPage } from "./NewReview";
import { ReviewPage } from "./ReviewPage";
import { ScreeningPage } from "./Screening";
import { StagePage } from "./StagePages";
import { isReviewTab, RefreshContext, REVIEW_TABS, ROLE_LABEL, TermsContext } from "./shared";
import { Avatar, Banner, Icon } from "./ui";
import "./styles.css";

/** Aktionen, die von überall erreichbar sind. */
function useGlobalActions(me: Person, onDone: (message: string, reviewId?: string) => void, onError: (e: string) => void) {
  const importBundle = async () => {
    const path = await open({ multiple: false, directory: false, filters: BUNDLE_FILTER });
    if (typeof path !== "string") return;
    try {
      const result = await api.importBundle(path);
      onDone(importSummary(result), result.review_id);
    } catch (e) {
      onError(errorText(e));
    }
  };
  const exportIdentity = async () => {
    const path = await save({
      defaultPath: `${safeFileName(me.name)}.ortid`,
      filters: [{ name: "Identitätsdatei", extensions: ["ortid"] }],
    });
    if (!path) return;
    try {
      await api.exportIdentity(path);
      onDone("Identitätsdatei gespeichert. Schick sie der Person, die das Review leitet.");
    } catch (e) {
      onError(errorText(e));
    }
  };
  return { importBundle, exportIdentity };
}

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
    <div className="setup" data-tauri-drag-region>
      <form className="setup-card" onSubmit={submit}>
        <img className="setup-logo" src="/logo.svg" alt="" />
        <h1>Open Review Tool</h1>
        <p className="subtitle">Screening für Scoping und Systematic Reviews. Ohne Server, ohne Konto.</p>
        <label className="field">
          <span>Wie heißt du?</span>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Vor- und Nachname" autoFocus required />
        </label>
        <p className="hint">
          <Icon name="key" size={14} /> Die App erzeugt einen Schlüssel, mit dem sie jede deiner Entscheidungen signiert.
          Er bleibt im Schlüsselbund dieses Macs.
        </p>
        {error && <Banner kind="error">{error}</Banner>}
        <button className="primary large" type="submit">Los geht’s</button>
      </form>
    </div>
  );
}

function Sidebar({ me, reviews, terms, onImport, onIdentity, onNew }: {
  me: Person;
  reviews: ReviewListItem[] | null;
  terms: Record<string, string>;
  onImport: () => void;
  onIdentity: () => void;
  onNew: () => void;
}) {
  // In einem Review zeigt die Leiste dessen Bereiche; die Liste der Reviews liegt hinter „Alle Reviews“.
  const reviewId = useMatch("/reviews/:reviewId/*")?.params.reviewId;
  const onReviewPage = useMatch("/reviews/:reviewId") !== null;
  const location = useLocation();
  const [showList, setShowList] = useState(false);
  useEffect(() => setShowList(false), [reviewId]);
  const current = reviewId ? reviews?.find((r) => r.id === reviewId) : undefined;
  const wanted = new URLSearchParams(location.search).get("tab");
  // Auf den Seiten einer Stufe bleibt „Ablauf“ markiert, denn von dort kommt man.
  const activeTab = onReviewPage && isReviewTab(wanted) ? wanted : "overview";

  return (
    <aside className="sidebar">
      <div className="sidebar-top" data-tauri-drag-region />
      {reviewId && !showList ? (
        <nav className="sidebar-nav" aria-label="Bereiche des Reviews">
          <button className="nav-item nav-back" onClick={() => setShowList(true)}>
            <Icon name="back" size={14} /> Alle Reviews
          </button>
          <div className="nav-title" title={current?.title}>
            <strong>{current?.title ?? "Review"}</strong>
            {current?.role && <small>{ROLE_LABEL[current.role] ?? current.role}</small>}
          </div>
          {REVIEW_TABS.map((t) => (
            <Link
              key={t.value}
              to={t.value === "overview" ? `/reviews/${reviewId}` : `/reviews/${reviewId}?tab=${t.value}`}
              className={`nav-item${activeTab === t.value ? " active" : ""}`}
              aria-current={activeTab === t.value ? "page" : undefined}
            >
              <Icon name={t.icon} />
              <span className="nav-label">{t.value === "sources" ? terms.sources || t.label : t.label}</span>
            </Link>
          ))}
        </nav>
      ) : (
        <nav className="sidebar-nav">
          <NavLink to="/" end className="nav-item">
            <Icon name="inbox" /> Übersicht
          </NavLink>
          <div className="nav-section">
            <span>Reviews</span>
            <button className="plain" onClick={onNew} title="Neues Review" aria-label="Neues Review">
              <Icon name="plus" size={14} />
            </button>
          </div>
          {reviews?.length === 0 && <p className="nav-empty">Noch keine Reviews</p>}
          {(reviews ?? []).map((r) => (
            <NavLink key={r.id} to={`/reviews/${r.id}`} className="nav-item" title={r.title} onClick={() => setShowList(false)}>
              <Icon name="file" />
              <span className="nav-label">{r.title}</span>
              {r.role && <span className="nav-meta">{ROLE_LABEL[r.role]?.[0]}</span>}
            </NavLink>
          ))}
        </nav>
      )}
      <div className="sidebar-bottom">
        <button className="nav-item" onClick={onImport}>
          <Icon name="download" /> Paket einlesen
        </button>
        <button className="me" onClick={onIdentity} title="Identitätsdatei speichern">
          <Avatar name={me.name} size={26} />
          <span>
            <strong>{me.name}</strong>
            <small>Identitätsdatei speichern</small>
          </span>
        </button>
      </div>
    </aside>
  );
}

function Home({ me, onImport, onIdentity }: { me: Person; onImport: () => void; onIdentity: () => void }) {
  const steps = [
    ["Identität teilen", "Jede Person schickt ihre Identitätsdatei an die Leitung."],
    ["Review anlegen", "Die Leitung wählt den Review-Typ, importiert die Treffer und öffnet eine Stufe."],
    ["Pakete verteilen", "Jedes Mitglied bekommt ein Paket – verblindet nur mit den eigenen Stimmen."],
    ["Screenen", "Mit i, e und m geht es schnell. Danach eine Rückmeldung speichern."],
    ["Zusammenführen", "Die Leitung liest Rückmeldungen ein, klärt Konflikte und schließt ab."],
  ];

  return (
    <div className="home">
      <div className="hero">
        <h1>Hallo {me.name.split(" ")[0]}</h1>
        <p className="subtitle">Alle Entscheidungen werden signiert und lückenlos protokolliert. Zusammengearbeitet wird über Dateien.</p>
      </div>
      <div className="action-cards">
        <div className="action-card">
          <div className="action-icon"><Icon name="plus" size={20} /></div>
          <h2>Neues Review</h2>
          <p>Du leitest das Review: Typ wählen, Treffer importieren, Team einladen.</p>
          <div className="inline-actions">
            <NavLink className="button primary" to="/neu">Review anlegen …</NavLink>
          </div>
        </div>
        <div className="action-card">
          <div className="action-icon"><Icon name="download" size={20} /></div>
          <h2>Bei einem Review mitmachen</h2>
          <p>Schick zuerst deine Identitätsdatei an die Leitung. Das Paket, das du dann bekommst, liest du hier ein.</p>
          <div className="inline-actions">
            <button onClick={onIdentity}>Identitätsdatei speichern</button>
            <button className="primary" onClick={onImport}>Paket einlesen</button>
          </div>
        </div>
      </div>
      <h2 className="section-title">So funktioniert’s</h2>
      <ol className="steps">
        {steps.map(([t, d], i) => (
          <li key={t}>
            <span className="step-num">{i + 1}</span>
            <strong>{t}</strong>
            <span>{d}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}

function Shell({ me }: { me: Person }) {
  const navigate = useNavigate();
  const [reviews, setReviews] = useState<ReviewListItem[] | null>(null);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const [terms, setTerms] = useState<{ reviewId: string; terms: Record<string, string> }>({ reviewId: "", terms: {} });
  const shareTerms = useCallback((reviewId: string, next: Record<string, string>) => setTerms({ reviewId, terms: next }), []);
  const openReview = useMatch("/reviews/:reviewId/*")?.params.reviewId;

  const refresh = useCallback(() => {
    api.reviews().then(setReviews).catch((e) => setError(errorText(e)));
  }, []);
  useEffect(refresh, [refresh]);

  const actions = useGlobalActions(
    me,
    (message, reviewId) => {
      setError("");
      setNotice(message);
      refresh();
      if (reviewId) navigate(`/reviews/${reviewId}`);
    },
    (e) => {
      setNotice("");
      setError(e);
    },
  );

  function newReview() {
    navigate("/neu");
  }

  return (
    <RefreshContext.Provider value={refresh}>
      <TermsContext.Provider value={shareTerms}>
      <div className="shell">
        <Sidebar me={me} reviews={reviews} terms={terms.reviewId === openReview ? terms.terms : {}} onImport={actions.importBundle} onIdentity={actions.exportIdentity} onNew={newReview} />
        <div className="content">
          <div className="content-drag" data-tauri-drag-region />
          <main>
            {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
            {notice && <Banner kind="notice" onClose={() => setNotice("")}>{notice}</Banner>}
            <Routes>
              <Route path="/" element={<Home me={me} onImport={actions.importBundle} onIdentity={actions.exportIdentity} />} />
              <Route path="/neu" element={<NewReviewPage />} />
              <Route path="/reviews/:reviewId" element={<ReviewPage />} />
              <Route path="/reviews/:reviewId/stages/:stageId" element={<StagePage />} />
              <Route path="/reviews/:reviewId/stages/:stageId/screen" element={<ScreeningPage />} />
              <Route path="*" element={<p>Seite nicht gefunden.</p>} />
            </Routes>
          </main>
        </div>
      </div>
      </TermsContext.Provider>
    </RefreshContext.Provider>
  );
}

function App() {
  const [me, setMe] = useState<Person | null | undefined>(undefined);
  const [error, setError] = useState("");

  useEffect(() => {
    api.identity().then(setMe).catch((e) => setError(errorText(e)));
  }, []);

  if (error) return <div className="setup"><Banner kind="error">{error}</Banner></div>;
  if (me === undefined) return <div className="setup" />;
  if (me === null) return <Setup onDone={setMe} />;
  return (
    <HashRouter>
      <Shell me={me} />
    </HashRouter>
  );
}

async function start() {
  const native = "__TAURI_INTERNALS__" in window;
  document.documentElement.classList.toggle("native", native);
  // Vorschau im Browser (nur Entwicklung): Rust-Befehle durch Beispieldaten ersetzen.
  if (import.meta.env.DEV && !native) {
    await import("./dev/mock").then((m) => m.installMock());
  }
  createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

start();
