import React, { useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter, Link, NavLink, Route, Routes, useLocation, useMatch, useNavigate } from "react-router-dom";
import { open, save } from "@tauri-apps/plugin-dialog";

import { api, BUNDLE_FILTER, canVote, errorText, importSummary, Person, ReviewListItem, safeFileName, Stage } from "./api";
import { FormEntryPage, FormStagePage } from "./FormStage";
import { capital, language, t } from "./i18n";
import { NewReviewPage } from "./NewReview";
import { ReviewPage } from "./ReviewPage";
import { ScreeningPage } from "./Screening";
import { folder, inFolder } from "./settings";
import { SettingsPage } from "./SettingsPage";
import { StagePage } from "./StagePages";
import { isReviewTab, RefreshContext, REVIEW_TABS, ROLE_LABEL, TermsContext } from "./shared";
import { Avatar, Banner, Icon } from "./ui";

/** Aktionen, die von überall erreichbar sind. */
function useGlobalActions(me: Person, onDone: (message: string, reviewId?: string) => void, onError: (e: string) => void) {
  const importBundle = async () => {
    const path = await open({ multiple: false, directory: false, defaultPath: await folder("package"), filters: BUNDLE_FILTER });
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
      defaultPath: await inFolder("package", `${safeFileName(me.name)}.ortid`),
      filters: [{ name: t("Identitätsdatei"), extensions: ["ortid"] }],
    });
    if (!path) return;
    try {
      await api.exportIdentity(path);
      onDone(t("Identitätsdatei gespeichert. Schick sie der Person, die das Review leitet."));
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
        <p className="subtitle">{t("Screening für Scoping und Systematic Reviews. Ohne Server, ohne Konto.")}</p>
        <label className="field">
          <span>{t("Wie heißt du?")}</span>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("Vor- und Nachname")} autoFocus required />
        </label>
        <p className="hint">
          <Icon name="key" size={14} /> {t("Die App erzeugt einen Schlüssel, mit dem sie jede deiner Entscheidungen signiert. Er bleibt im Schlüsselbund dieses Macs.")}
        </p>
        {error && <Banner kind="error">{error}</Banner>}
        <button className="primary large" type="submit">{t("Los geht’s")}</button>
      </form>
    </div>
  );
}

function Sidebar({ me, reviews, terms, stages, onImport, onNew }: {
  me: Person;
  reviews: ReviewListItem[] | null;
  terms: Record<string, string>;
  stages: Stage[];
  onImport: () => void;
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
  // Auf den Seiten einer Stufe ist die Stufe markiert, kein Bereich.
  const stageId = useMatch("/reviews/:reviewId/:kind/:stageId/*")?.params.stageId;
  const activeTab = onReviewPage ? (isReviewTab(wanted) ? wanted : "overview") : null;
  // Wer abstimmen darf, landet in einer offenen Stufe gleich bei der Arbeit, sonst bei der Auswertung.
  const stageLink = (stage: Stage) => {
    if (stage.form_id) return `/reviews/${reviewId}/forms/${stage.id}`;
    const work = !stage.closed && current?.role && canVote(current.role);
    return `/reviews/${reviewId}/stages/${stage.id}${work ? "/screen" : ""}`;
  };

  return (
    <aside className="sidebar">
      <div className="sidebar-top" data-tauri-drag-region />
      {reviewId && !showList ? (
        <nav className="sidebar-nav" aria-label={t("Bereiche des Reviews")}>
          <button className="nav-item nav-back" onClick={() => setShowList(true)}>
            <Icon name="back" size={14} /> {t("Alle Reviews")}
          </button>
          <div className="nav-title" title={current?.title}>
            <strong>{current?.color && <span className={`tag-dot tag-${current.color}`} />}{current?.title ?? t("Review")}</strong>
            {current?.role && <small>{ROLE_LABEL[current.role] ?? current.role}</small>}
          </div>
          {REVIEW_TABS.map((tab) => (
            <React.Fragment key={tab.value}>
            <Link
              to={tab.value === "overview" ? `/reviews/${reviewId}` : `/reviews/${reviewId}?tab=${tab.value}`}
              className={`nav-item${activeTab === tab.value ? " active" : ""}`}
              aria-current={activeTab === tab.value ? "page" : undefined}
            >
              <Icon name={tab.icon} />
              <span className="nav-label">{tab.value === "sources" ? (terms.sources && capital(terms.sources)) || tab.label : tab.label}</span>
            </Link>
            {/* Die Stufen gehören zum Ablauf und stehen deshalb eingerückt darunter. */}
            {tab.value === "overview" && stages.map((stage) => (
                <Link
                  key={stage.id}
                  to={stageLink(stage)}
                  className={`nav-item nav-sub${stageId === stage.id ? " active" : ""}`}
                  aria-current={stageId === stage.id ? "page" : undefined}
                  title={stage.closed ? `${stage.name} (${t("geschlossen")})` : stage.name}
                >
                  <Icon name={stage.closed ? "check" : stage.form_id ? "list" : "play"} size={13} />
                  <span className="nav-label">{stage.name}</span>
                </Link>
              ))}
            </React.Fragment>
          ))}
        </nav>
      ) : (
        <nav className="sidebar-nav">
          <NavLink to="/" end className="nav-item">
            <Icon name="inbox" /> {t("Übersicht")}
          </NavLink>
          <div className="nav-section">
            <span>{t("Reviews")}</span>
            <button className="plain" onClick={onNew} title={t("Neues Review")} aria-label={t("Neues Review")}>
              <Icon name="plus" size={14} />
            </button>
          </div>
          {reviews?.length === 0 && <p className="nav-empty">{t("Noch keine Reviews")}</p>}
          {(reviews ?? []).map((r) => (
            <NavLink key={r.id} to={`/reviews/${r.id}`} className="nav-item" title={r.title} onClick={() => setShowList(false)}>
              {r.color ? <span className={`tag-dot tag-${r.color}`} /> : <Icon name="file" />}
              <span className="nav-label">{r.title}</span>
              {r.role && <span className="nav-meta">{ROLE_LABEL[r.role]?.[0]}</span>}
            </NavLink>
          ))}
        </nav>
      )}
      <div className="sidebar-bottom">
        <button className="nav-item" onClick={onImport}>
          <Icon name="download" /> {t("Paket einlesen")}
        </button>
        <NavLink className="me" to="/settings" title={t("Einstellungen")}>
          <Avatar name={me.name} size={26} />
          <span>
            <strong>{me.name}</strong>
            <small>{t("Einstellungen")}</small>
          </span>
          <Icon name="gear" size={15} />
        </NavLink>
      </div>
    </aside>
  );
}

function Home({ me, onImport, onIdentity }: { me: Person; onImport: () => void; onIdentity: () => void }) {
  const steps = [
    [t("Identität teilen"), t("Jede Person schickt ihre Identitätsdatei an die Leitung.")],
    [t("Review anlegen"), t("Die Leitung wählt den Review-Typ, importiert die Treffer und öffnet eine Stufe.")],
    [t("Pakete verteilen"), t("Jedes Mitglied bekommt ein Paket – verblindet nur mit den eigenen Stimmen.")],
    [t("Screenen"), t("Mit i, e und m geht es schnell. Danach eine Rückmeldung speichern.")],
    [t("Zusammenführen"), t("Die Leitung liest Rückmeldungen ein, klärt Konflikte und schließt ab.")],
  ];

  return (
    <div className="home">
      <div className="hero">
        <h1>{t("Hallo {name}", { name: me.name.split(" ")[0] })}</h1>
        <p className="subtitle">{t("Alle Entscheidungen werden signiert und lückenlos protokolliert. Zusammengearbeitet wird über Dateien.")}</p>
      </div>
      <div className="action-cards">
        <div className="action-card">
          <div className="action-icon"><Icon name="plus" size={20} /></div>
          <h2>{t("Neues Review")}</h2>
          <p>{t("Du leitest das Review: Typ wählen, Treffer importieren, Team einladen.")}</p>
          <div className="inline-actions">
            <NavLink className="button primary" to="/neu">{t("Review anlegen …")}</NavLink>
          </div>
        </div>
        <div className="action-card">
          <div className="action-icon"><Icon name="download" size={20} /></div>
          <h2>{t("Bei einem Review mitmachen")}</h2>
          <p>{t("Schick zuerst deine Identitätsdatei an die Leitung. Das Paket, das du dann bekommst, liest du hier ein.")}</p>
          <div className="inline-actions">
            <button onClick={onIdentity}>{t("Identitätsdatei speichern")}</button>
            <button className="primary" onClick={onImport}>{t("Paket einlesen")}</button>
          </div>
        </div>
      </div>
      <h2 className="section-title">{t("So funktioniert’s")}</h2>
      <ol className="steps">
        {steps.map(([title, text], i) => (
          <li key={title}>
            <span className="step-num">{i + 1}</span>
            <strong>{title}</strong>
            <span>{text}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}

function Shell({ me }: { me: Person }) {
  const navigate = useNavigate();
  const [reviews, setReviews] = useState<ReviewListItem[] | null>(null);
  // Eine Meldung gilt für die Seite, auf der sie entstand; beim Wechsel der Seite oder des Bereichs verschwindet sie.
  const location = useLocation();
  const here = location.pathname + location.search;
  const [notice, setNoticeAt] = useState({ text: "", at: "" });
  const setNotice = (text: string, at = here) => setNoticeAt({ text, at });
  const [error, setError] = useState("");
  // Begriffe und Stufen des offenen Reviews für die Seitenleiste. Die Seite des Reviews meldet sie nach jedem
  // Laden; auf den Seiten einer Stufe lädt die Hülle sie selbst.
  const [shared, setShared] = useState<{ reviewId: string; terms: Record<string, string>; stages: Stage[] }>({ reviewId: "", terms: {}, stages: [] });
  const shareTerms = useCallback((reviewId: string, terms: Record<string, string>, stages: Stage[]) => setShared({ reviewId, terms, stages }), []);
  const openReview = useMatch("/reviews/:reviewId/*")?.params.reviewId;
  const onReviewPage = useMatch("/reviews/:reviewId") !== null;
  useEffect(() => {
    if (!openReview || onReviewPage) return;
    let live = true;
    api.review(openReview).then((detail) => live && setShared({ reviewId: openReview, terms: detail.workflow.terms, stages: detail.stages })).catch(() => undefined);
    return () => {
      live = false;
    };
  }, [openReview, onReviewPage, location.pathname]);
  const known = shared.reviewId === openReview;

  const refresh = useCallback(() => {
    api.reviews().then(setReviews).catch((e) => setError(errorText(e)));
  }, []);
  useEffect(refresh, [refresh]);

  const actions = useGlobalActions(
    me,
    (message, reviewId) => {
      setError("");
      setNotice(message, reviewId ? `/reviews/${reviewId}` : here);
      refresh();
      if (reviewId) navigate(`/reviews/${reviewId}`);
    },
    (e) => {
      setNotice("");
      setError(e);
    },
  );

  // Der Menüeintrag „Einstellungen …“ (⌘,) der App meldet sich als Ereignis.
  useEffect(() => {
    if (!document.documentElement.classList.contains("native")) return;
    const stop = import("@tauri-apps/api/event").then(({ listen }) => listen("open-settings", () => navigate("/settings")));
    return () => void stop.then((off) => off()).catch(() => undefined);
  }, [navigate]);

  function newReview() {
    navigate("/neu");
  }

  return (
    <RefreshContext.Provider value={refresh}>
      <TermsContext.Provider value={shareTerms}>
      <div className="shell">
        <Sidebar me={me} reviews={reviews} terms={known ? shared.terms : {}} stages={known ? shared.stages : []} onImport={actions.importBundle} onNew={newReview} />
        <div className="content">
          <div className="content-drag" data-tauri-drag-region />
          <main>
            {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
            {notice.text && notice.at === here && <Banner kind="notice" onClose={() => setNotice("")}>{notice.text}</Banner>}
            <Routes>
              <Route path="/" element={<Home me={me} onImport={actions.importBundle} onIdentity={actions.exportIdentity} />} />
              <Route path="/neu" element={<NewReviewPage />} />
              <Route path="/settings" element={<SettingsPage me={me} onIdentity={actions.exportIdentity} />} />
              <Route path="/reviews/:reviewId" element={<ReviewPage />} />
              <Route path="/reviews/:reviewId/stages/:stageId" element={<StagePage />} />
              <Route path="/reviews/:reviewId/stages/:stageId/screen" element={<ScreeningPage />} />
              <Route path="/reviews/:reviewId/forms/:stageId" element={<FormStagePage />} />
              <Route path="/reviews/:reviewId/forms/:stageId/:recordId" element={<FormEntryPage />} />
              <Route path="*" element={<p>{t("Seite nicht gefunden.")}</p>} />
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

/** Zeichnet die App. Die Sprache steht beim Laden dieses Moduls schon fest (siehe `main.tsx`). */
export async function render() {
  // Der Kern spricht dieselbe Sprache wie die Oberfläche: Fehlermeldungen, Vorlagentexte, Leitlinien.
  await api.setLanguage(language).catch(() => undefined);
  createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}
