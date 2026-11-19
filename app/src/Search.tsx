// Reiter „Suche“: je Quelle eine Suche mit Versionen, Suchstring, Datum, Grenzen und Peer-Review-Vermerk.

import React, { useEffect, useState } from "react";

import { api, canManage, errorText, ORIGIN_LABEL, ReviewDetail, Search, SearchVersion } from "./api";
import { formatDate, t, tn } from "./i18n";
import { Banner, Empty, Group, Icon, Pill, Row } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

export const today = () => new Date().toISOString().slice(0, 10);

/** JJJJ-MM-TT als Datum in der Sprache der Oberfläche. */
export function dateText(iso: string): string {
  return formatDate(iso);
}

/** Name einer Suche in Listen: Quelle und Plattform der neuesten Version. */
export function searchName(search: Search): string {
  const latest = search.versions[search.versions.length - 1];
  return latest.platform ? `${latest.source} · ${latest.platform}` : latest.source;
}

/** Gemeldete gegen importierte Treffer; eine Abweichung fällt auf. */
function HitCount({ version }: { version: SearchVersion }) {
  if (version.n_imports === 0) {
    const parts = [version.hits != null ? tn("{n} Treffer gemeldet", "{n} Treffer gemeldet", version.hits) : null, t("noch nichts importiert")];
    return <span className="muted">{parts.filter(Boolean).join(" · ")}</span>;
  }
  const imported = version.n_imports > 1
    ? t("{records} importiert ({files} Dateien)", { records: version.n_records, files: version.n_imports })
    : t("{records} importiert", { records: version.n_records });
  if (version.hits == null) return <span className="muted">{imported}</span>;
  const differs = version.hits !== version.n_records;
  return (
    <span className={differs ? "hits-differ" : "muted"}>
      {[t("{hits} gemeldet", { hits: version.hits }), imported, differs ? t("weicht ab") : null].filter(Boolean).join(" · ")}
    </span>
  );
}

function VersionBody({ version, review }: { version: SearchVersion; review: ReviewDetail }) {
  const name = (id: string | null) => review.members.find((m) => m.actor_id === id)?.name;
  return (
    <div className="search-version">
      <p className="search-meta">
        <Pill tone="blue">{t("Version {version}", { version: version.version })}</Pill>
        <span>{ORIGIN_LABEL[version.origin] ?? version.origin}</span>
        <span>{t("gesucht am {date}", { date: dateText(version.searched_on) })}</span>
        <HitCount version={version} />
      </p>
      <pre className="query">{version.query}</pre>
      {(version.limits || version.hit_limit != null) && (
        <p className="muted">
          {version.limits && t("Grenzen: {limits}", { limits: version.limits })}
          {version.limits && version.hit_limit != null && " · "}
          {version.hit_limit != null && tn("übernommen höchstens {n} Treffer", "übernommen höchstens {n} Treffer", version.hit_limit)}
        </p>
      )}
      {version.change_note && <p className="muted">{t("Änderung: {note}", { note: version.change_note })}</p>}
      {version.peer_reviews.map((r, i) => (
        <p key={i} className="peer-review">
          <Icon name="check" size={13} />{" "}
          {name(r.reviewer)
            ? t("Geprüft am {date} von {name}: {note}", { date: dateText(r.reviewed_on), name: name(r.reviewer) ?? "", note: r.note })
            : t("Geprüft am {date}: {note}", { date: dateText(r.reviewed_on), note: r.note })}
        </p>
      ))}
    </div>
  );
}

function PeerReviewForm({ review, search, version, run, onDone }: {
  review: ReviewDetail;
  search: Search;
  version: number;
  run: Run;
  onDone: () => void;
}) {
  const [date, setDate] = useState(today());
  const [note, setNote] = useState("");
  const [reviewer, setReviewer] = useState("");
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    await run(async () => {
      await api.reviewSearch(review.id, search.id, version, date, note, reviewer || null);
      onDone();
    }, t("Peer-Review vermerkt."));
  }
  return (
    <form className="search-form" onSubmit={submit}>
      <div className="stack-field">
        <label htmlFor="pr-note">{t("Vermerk")} <small>{t("wer geprüft hat (Rolle, kein Name nötig), womit und mit welchem Ergebnis, z. B. „Bibliothekarin, PRESS: ohne Befund“")}</small></label>
        <input id="pr-note" value={note} onChange={(e) => setNote(e.target.value)} required />
      </div>
      <Row>
        <label htmlFor="pr-date">{t("Geprüft am")}</label>
        <input id="pr-date" type="date" value={date} onChange={(e) => setDate(e.target.value)} required />
      </Row>
      <Row>
        <label htmlFor="pr-reviewer">{t("Geprüft von")} <small>{t("nur wenn die Person zum Team gehört")}</small></label>
        <select id="pr-reviewer" value={reviewer} onChange={(e) => setReviewer(e.target.value)}>
          <option value="">{t("nicht im Team")}</option>
          {review.members.map((m) => <option key={m.actor_id} value={m.actor_id}>{m.name}</option>)}
        </select>
      </Row>
      <div className="sheet-actions">
        <button type="button" onClick={onDone}>{t("Abbrechen")}</button>
        <button className="primary" type="submit">{t("Vermerken")}</button>
      </div>
    </form>
  );
}

/** Formular für eine neue Suche oder eine neue Version; die Felder der Vorversion sind vorbelegt. */
function SearchForm({ review, base, run, onDone }: { review: ReviewDetail; base: Search | null; run: Run; onDone: () => void }) {
  const last = base ? base.versions[base.versions.length - 1] : null;
  const [source, setSource] = useState(last?.source ?? "");
  const [platform, setPlatform] = useState(last?.platform ?? "");
  const [origin, setOrigin] = useState(last?.origin ?? "database");
  const [query, setQuery] = useState(last?.query ?? "");
  const [searchedOn, setSearchedOn] = useState(today());
  const [limits, setLimits] = useState(last?.limits ?? "");
  const [hits, setHits] = useState("");
  const [hitLimit, setHitLimit] = useState(last?.hit_limit != null ? String(last.hit_limit) : "");
  const [changeNote, setChangeNote] = useState("");
  const number = (text: string) => (text.trim() === "" ? null : Number(text));

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    await run(async () => {
      await api.publishSearch(review.id, {
        search_id: base?.id ?? "", source, platform, origin, query, searched_on: searchedOn, limits,
        hits: number(hits), hit_limit: number(hitLimit), change_note: changeNote,
      });
      onDone();
    }, base ? t("Neue Version der Suche festgehalten.") : t("Suche festgehalten."));
  }

  return (
    <form onSubmit={submit}>
      <Group
        title={last ? t("{name}: Version {version}", { name: searchName(base!), version: last.version + 1 }) : t("Neue Suche")}
        footer={t("Eine Suche steht für eine Quelle. Wird der Suchstring geändert oder die Suche wiederholt, entsteht eine neue Version; Importe nennen die Version, aus der ihre Treffer stammen.")}
      >
        <Row>
          <label htmlFor="s-source">{t("Quelle")} <small>{t("Datenbank, Register oder Webseite")}</small></label>
          <input id="s-source" value={source} onChange={(e) => setSource(e.target.value)} placeholder={t("z. B. MEDLINE")} required />
        </Row>
        <Row>
          <label htmlFor="s-platform">{t("Plattform")} <small>{t("worüber gesucht wurde")}</small></label>
          <input id="s-platform" value={platform} onChange={(e) => setPlatform(e.target.value)} placeholder={t("z. B. Ovid oder PubMed")} />
        </Row>
        <Row>
          <label htmlFor="s-origin">{t("Herkunft")} <small>{t("der Bericht zählt Datenbanken und Register getrennt von anderen Wegen")}</small></label>
          <select id="s-origin" value={origin} onChange={(e) => setOrigin(e.target.value)}>
            {Object.entries(ORIGIN_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </Row>
        <div className="stack-field">
          <label htmlFor="s-query">{t("Suchstring")} <small>{t("vollständig, so wie er ausgeführt wurde")}</small></label>
          <textarea id="s-query" className="query-input" rows={6} value={query} onChange={(e) => setQuery(e.target.value)} required />
        </div>
        <Row>
          <label htmlFor="s-date">{t("Gesucht am")}</label>
          <input id="s-date" type="date" value={searchedOn} onChange={(e) => setSearchedOn(e.target.value)} required />
        </Row>
        <Row>
          <label htmlFor="s-limits">{t("Grenzen")} <small>{t("Zeitraum, Sprachen, Filter")}</small></label>
          <input id="s-limits" value={limits} onChange={(e) => setLimits(e.target.value)} placeholder={t("z. B. ab 2015, Englisch und Deutsch")} />
        </Row>
        <Row>
          <label htmlFor="s-hits">{t("Treffer")} <small>{t("wie die Quelle sie meldet; die App vergleicht mit dem Import")}</small></label>
          <input id="s-hits" className="narrow-input" type="number" min={0} value={hits} onChange={(e) => setHits(e.target.value)} />
        </Row>
        <Row>
          <label htmlFor="s-limit">{t("Trefferlimit")} <small>{t("falls nur die ersten Treffer übernommen werden, z. B. bei einer Websuche")}</small></label>
          <input id="s-limit" className="narrow-input" type="number" min={1} value={hitLimit} onChange={(e) => setHitLimit(e.target.value)} />
        </Row>
        {last && (
          <div className="stack-field">
            <label htmlFor="s-change">{t("Was hat sich geändert?")}</label>
            <input id="s-change" value={changeNote} onChange={(e) => setChangeNote(e.target.value)} required />
          </div>
        )}
      </Group>
      <div className="sheet-actions">
        <button type="button" onClick={onDone}>{t("Abbrechen")}</button>
        <button className="primary" type="submit">{last ? t("Als Version {version} festhalten", { version: last.version + 1 }) : t("Suche festhalten")}</button>
      </div>
    </form>
  );
}

export function SearchTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const manage = canManage(review.role);
  const [searches, setSearches] = useState<Search[] | null>(null);
  const [error, setError] = useState("");
  // null: kein Formular; "new": neue Suche; sonst die Suche, die eine neue Version bekommt
  const [editing, setEditing] = useState<Search | "new" | null>(null);
  const [reviewing, setReviewing] = useState<string | null>(null);

  useEffect(() => {
    api.searches(review.id).then(setSearches).catch((e) => setError(errorText(e)));
  }, [review]);

  if (error) return <Banner kind="error">{error}</Banner>;
  if (!searches) return null;
  if (editing) {
    return <SearchForm key={editing === "new" ? "new" : editing.id} review={review} base={editing === "new" ? null : editing} run={run} onDone={() => setEditing(null)} />;
  }

  return (
    <>
      {searches.length === 0 && (
        <Empty icon="list" title={t("Noch keine Suche festgehalten")}>
          {t("Je Quelle eine Suche mit Suchstring, Datum und Grenzen. Der Bericht braucht sie für den Suchanhang und das Flussdiagramm.")}
        </Empty>
      )}
      {searches.map((search) => {
        const latest = search.versions[search.versions.length - 1];
        const older = search.versions.slice(0, -1).reverse();
        return (
          <Group
            key={search.id}
            title={searchName(search)}
            action={manage && (
              <span className="group-actions">
                <button className="small" onClick={() => setReviewing(reviewing === search.id ? null : search.id)}>{t("Peer-Review vermerken")}</button>
                <button className="small" onClick={() => setEditing(search)}><Icon name="plus" size={13} /> {t("Neue Version")}</button>
              </span>
            )}
          >
            <VersionBody version={latest} review={review} />
            {reviewing === search.id && (
              <PeerReviewForm review={review} search={search} version={latest.version} run={run} onDone={() => setReviewing(null)} />
            )}
            {older.length > 0 && (
              <details className="older-versions">
                <summary>{tn("{n} frühere Version", "{n} frühere Versionen", older.length)}</summary>
                {older.map((v) => <VersionBody key={v.version} version={v} review={review} />)}
              </details>
            )}
          </Group>
        );
      })}
      {manage && (
        <div className="sheet-actions">
          <button className="primary" onClick={() => setEditing("new")}><Icon name="plus" size={14} /> {t("Neue Suche")}</button>
        </div>
      )}
    </>
  );
}
