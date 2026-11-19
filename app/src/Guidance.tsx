// Leitlinien zum Nachschlagen: das „?“ oben rechts zeigt die Empfehlung zum offenen Bildschirm, der
// Bereich „Leitlinie“ die ganze Zusammenfassung. Die Texte liefert der Kern; maßgeblich bleibt das Original.

import React, { useEffect, useState } from "react";
import { Link } from "react-router-dom";

import { api, errorText, Guideline, GuidelineEntry, ReviewDetail, Station } from "./api";
import { capital, formatDate, t } from "./i18n";
import { Banner, Group, Icon, Pill, Row } from "./ui";

let cache: Promise<Guideline[]> | null = null;
/** Die Leitlinien ändern sich nur mit der App-Version; einmal laden genügt. */
const loadGuidelines = () => (cache ??= api.guidelines().catch((e) => { cache = null; throw e; }));

const AREA_LABEL: Record<string, string> = {
  criteria: t("Kriterien"), registration: t("Registrierung"), search: t("Suche"), imports: t("Import"), fulltext: t("Volltexte"),
  sources: t("Quellen"), forms: t("Formular"), report: t("Bericht"), members: t("Mitglieder"),
};

function EntryCard({ guideline, entry, where }: { guideline: Guideline; entry: GuidelineEntry; where?: string }) {
  const source = guideline.sources.find((s) => s.key === entry.source);
  return (
    <article className="guide-entry" id={`guide-${guideline.key}-${entry.id}`}>
      <header>
        <h3>{entry.title}</h3>
        {entry.section && <Pill>{entry.section}</Pill>}
      </header>
      {entry.text.map((paragraph, i) => <p key={i}>{paragraph}</p>)}
      {entry.points.length > 0 && <ul>{entry.points.map((p, i) => <li key={i}>{p}</li>)}</ul>}
      <footer>
        <button className="link" onClick={() => api.openUrl(entry.url)} title={entry.url}>{t("Original öffnen")}</button>
        {source && <span className="muted">{source.citation}</span>}
        {where && <span className="muted">{t("Gehört zu: {where}", { where })}</span>}
      </footer>
    </article>
  );
}

/** Einträge zum offenen Bildschirm: erst die der Station, dann die des Bereichs, jeder nur einmal. */
function matching(guideline: Guideline, station?: string | null, area?: string): GuidelineEntry[] {
  const byStation = station ? guideline.entries.filter((e) => e.stations.includes(station)) : [];
  const byArea = area ? guideline.entries.filter((e) => e.areas.includes(area) && !byStation.includes(e)) : [];
  return [...byStation, ...byArea];
}

function HelpPanel({ reviewId, area, station, context, onClose }: {
  reviewId: string;
  area?: string;
  station?: string | null;
  context: string;
  onClose: () => void;
}) {
  const [data, setData] = useState<{ guidelines: Guideline[]; templateKey: string; templateName: string } | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    Promise.all([loadGuidelines(), api.review(reviewId)])
      .then(([guidelines, review]) => setData({ guidelines, templateKey: review.workflow.template_key, templateName: review.workflow.template_name }))
      .catch((e) => setError(errorText(e)));
  }, [reviewId]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const guideline = data?.guidelines.find((g) => g.key === data.templateKey);
  const entries = guideline ? matching(guideline, station, area) : [];
  const wiki = `/reviews/${reviewId}?tab=guidance`;

  return (
    <div className="help-layer" onClick={onClose}>
      <aside className="help-panel" role="dialog" aria-label={t("Leitlinie zu diesem Schritt")} onClick={(e) => e.stopPropagation()}>
        <header>
          <div>
            <strong>{guideline ? t("Leitlinie: {title}", { title: guideline.title }) : t("Leitlinie")}</strong>
            <span className="muted">{t("zu „{context}“", { context })}</span>
          </div>
          <button className="plain" onClick={onClose} aria-label={t("Schließen")}><Icon name="x" size={14} /></button>
        </header>
        <div className="help-body">
          {error && <Banner kind="error">{error}</Banner>}
          {data && !guideline && (
            <p className="muted">
              {t("Der Ablauf „{name}“ folgt keiner hinterlegten Leitlinie. Die Leitlinien der anderen Review-Arten stehen im Bereich „Leitlinie“.", { name: data.templateName })}
            </p>
          )}
          {guideline && entries.length === 0 && <p className="muted">{t("Zu diesem Schritt nennt die Leitlinie nichts Eigenes.")}</p>}
          {guideline && entries.map((e) => <EntryCard key={e.id} guideline={guideline} entry={e} />)}
        </div>
        <footer>
          <span className="muted">{t("Zusammenfassung in eigenen Worten. Maßgeblich ist das Original.")}</span>
          <Link to={wiki} onClick={onClose}>{t("Ganze Leitlinie")}</Link>
        </footer>
      </aside>
    </div>
  );
}

/** Das „?“ oben rechts: öffnet die Empfehlungen der Leitlinie zum offenen Bildschirm. */
export function HelpButton({ reviewId, area, station, context }: {
  reviewId: string;
  /** Bereich des Reviews, z. B. `search`. */
  area?: string;
  /** Station der offenen Stufe. */
  station?: string | null;
  /** Name des Bildschirms für die Überschrift. */
  context: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button className="help-button" onClick={() => setOpen(true)} title={t("Was sagt die Leitlinie zu diesem Schritt?")} aria-label={t("Leitlinie zu diesem Schritt")}>
        <Icon name="help" size={17} />
      </button>
      {open && <HelpPanel reviewId={reviewId} area={area} station={station} context={context} onClose={() => setOpen(false)} />}
    </>
  );
}

/** Der Bereich „Leitlinie“: die ganze Zusammenfassung, für die Vorlage des Reviews und für die anderen. */
export function GuidanceTab({ review }: { review: ReviewDetail }) {
  const [guidelines, setGuidelines] = useState<Guideline[] | null>(null);
  const [chosen, setChosen] = useState(review.workflow.template_key);
  const [error, setError] = useState("");

  useEffect(() => {
    loadGuidelines().then(setGuidelines).catch((e) => setError(errorText(e)));
  }, []);

  if (!guidelines) return error ? <Banner kind="error">{error}</Banner> : null;
  const own = guidelines.find((g) => g.key === review.workflow.template_key);
  const guideline = guidelines.find((g) => g.key === chosen) ?? own ?? guidelines[0];
  // Nur bei der eigenen Vorlage lassen sich die Stationen beim Namen nennen.
  const stations: Station[] = guideline.key === review.workflow.template_key ? review.workflow.stations : [];
  const where = (e: GuidelineEntry) =>
    [
      ...e.stations.map((key) => stations.find((s) => s.key === key)?.label).filter(Boolean),
      ...e.areas.map((a) => (a === "sources" && review.workflow.terms.sources ? capital(review.workflow.terms.sources) : undefined) ?? AREA_LABEL[a] ?? a),
    ].filter((label, i, all) => all.indexOf(label) === i).join(" · ");

  return (
    <>
      {!own && (
        <Banner kind="notice">
          {t("Der Ablauf „{name}“ folgt keiner hinterlegten Leitlinie. Hier stehen die Leitlinien der anderen Review-Arten.", { name: review.workflow.template_name })}
        </Banner>
      )}
      <div className="guide-choice" role="group" aria-label={t("Review-Art")}>
        {guidelines.map((g) => (
          <button key={g.key} className={`chip toggle${g.key === guideline.key ? " active" : ""}`} aria-pressed={g.key === guideline.key} onClick={() => setChosen(g.key)}>
            {g.key === review.workflow.template_key ? t("{title} (dieses Review)", { title: g.title }) : g.title}
          </button>
        ))}
      </div>
      <Group title={guideline.title} footer={t("Die Texte fassen die Leitlinie in eigenen Worten zusammen und liegen in der App, also auch ohne Netz. Sie ersetzen das Original nicht; jeder Eintrag verweist auf seinen Abschnitt.")}>
        <Row><span>{guideline.summary}</span></Row>
        {guideline.sources.map((s) => (
          <div className="report-file" key={s.key}>
            <div>
              <strong>{s.citation}</strong>
              <span className="muted">
                {s.license
                  ? t("Abgerufen am {date} · Lizenz des Originals: {license}", { date: formatDate(s.accessed), license: s.license })
                  : t("Abgerufen am {date}", { date: formatDate(s.accessed) })}
              </span>
            </div>
            <button className="small" onClick={() => api.openUrl(s.url)} title={s.url}>{t("Original öffnen")}</button>
          </div>
        ))}
      </Group>
      <nav className="guide-toc" aria-label={t("Inhalt")}>
        {guideline.entries.map((e) => (
          <a key={e.id} href={`#guide-${guideline.key}-${e.id}`} onClick={(event) => { event.preventDefault(); document.getElementById(`guide-${guideline.key}-${e.id}`)?.scrollIntoView({ behavior: "smooth", block: "start" }); }}>
            {e.title}
          </a>
        ))}
      </nav>
      <div className="guide-list">
        {guideline.entries.map((e) => <EntryCard key={e.id} guideline={guideline} entry={e} where={where(e)} />)}
      </div>
    </>
  );
}
