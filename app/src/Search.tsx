// Reiter „Suche“: je Quelle eine Suche mit Versionen, Suchstring, Datum, Grenzen und Peer-Review-Vermerk.

import React, { useEffect, useState } from "react";

import { api, canManage, errorText, ORIGIN_LABEL, ReviewDetail, Search, SearchVersion } from "./api";
import { Banner, Empty, Group, Icon, Pill, Row } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

export const today = () => new Date().toISOString().slice(0, 10);

/** JJJJ-MM-TT als TT.MM.JJJJ. */
export function dateText(iso: string): string {
  const [year, month, day] = iso.split("-");
  return day ? `${day}.${month}.${year}` : iso;
}

/** Name einer Suche in Listen: Quelle und Plattform der neuesten Version. */
export function searchName(search: Search): string {
  const latest = search.versions[search.versions.length - 1];
  return latest.platform ? `${latest.source} · ${latest.platform}` : latest.source;
}

/** Gemeldete gegen importierte Treffer; eine Abweichung fällt auf. */
function HitCount({ version }: { version: SearchVersion }) {
  if (version.n_imports === 0) {
    return <span className="muted">{version.hits != null ? `${version.hits} Treffer gemeldet · ` : ""}noch nichts importiert</span>;
  }
  const imported = `${version.n_records} importiert${version.n_imports > 1 ? ` (${version.n_imports} Dateien)` : ""}`;
  if (version.hits == null) return <span className="muted">{imported}</span>;
  const differs = version.hits !== version.n_records;
  return (
    <span className={differs ? "hits-differ" : "muted"}>
      {version.hits} gemeldet · {imported}
      {differs && " · weicht ab"}
    </span>
  );
}

function VersionBody({ version, review }: { version: SearchVersion; review: ReviewDetail }) {
  const name = (id: string | null) => review.members.find((m) => m.actor_id === id)?.name;
  return (
    <div className="search-version">
      <p className="search-meta">
        <Pill tone="blue">Version {version.version}</Pill>
        <span>{ORIGIN_LABEL[version.origin] ?? version.origin}</span>
        <span>gesucht am {dateText(version.searched_on)}</span>
        <HitCount version={version} />
      </p>
      <pre className="query">{version.query}</pre>
      {(version.limits || version.hit_limit != null) && (
        <p className="muted">
          {version.limits && <>Grenzen: {version.limits}</>}
          {version.limits && version.hit_limit != null && " · "}
          {version.hit_limit != null && <>übernommen höchstens {version.hit_limit} Treffer</>}
        </p>
      )}
      {version.change_note && <p className="muted">Änderung: {version.change_note}</p>}
      {version.peer_reviews.map((r, i) => (
        <p key={i} className="peer-review">
          <Icon name="check" size={13} /> Geprüft am {dateText(r.reviewed_on)}
          {name(r.reviewer) && ` von ${name(r.reviewer)}`}: {r.note}
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
    }, "Peer-Review vermerkt.");
  }
  return (
    <form className="search-form" onSubmit={submit}>
      <div className="stack-field">
        <label htmlFor="pr-note">Vermerk <small>wer geprüft hat (Rolle, kein Name nötig), womit und mit welchem Ergebnis, z. B. „Bibliothekarin, PRESS: ohne Befund“</small></label>
        <input id="pr-note" value={note} onChange={(e) => setNote(e.target.value)} required />
      </div>
      <Row>
        <label htmlFor="pr-date">Geprüft am</label>
        <input id="pr-date" type="date" value={date} onChange={(e) => setDate(e.target.value)} required />
      </Row>
      <Row>
        <label htmlFor="pr-reviewer">Geprüft von <small>nur wenn die Person zum Team gehört</small></label>
        <select id="pr-reviewer" value={reviewer} onChange={(e) => setReviewer(e.target.value)}>
          <option value="">nicht im Team</option>
          {review.members.map((m) => <option key={m.actor_id} value={m.actor_id}>{m.name}</option>)}
        </select>
      </Row>
      <div className="sheet-actions">
        <button type="button" onClick={onDone}>Abbrechen</button>
        <button className="primary" type="submit">Vermerken</button>
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
    }, base ? "Neue Version der Suche festgehalten." : "Suche festgehalten.");
  }

  return (
    <form onSubmit={submit}>
      <Group
        title={last ? `${searchName(base!)}: Version ${last.version + 1}` : "Neue Suche"}
        footer="Eine Suche steht für eine Quelle. Wird der Suchstring geändert oder die Suche wiederholt, entsteht eine neue Version; Importe nennen die Version, aus der ihre Treffer stammen."
      >
        <Row>
          <label htmlFor="s-source">Quelle <small>Datenbank, Register oder Webseite</small></label>
          <input id="s-source" value={source} onChange={(e) => setSource(e.target.value)} placeholder="z. B. MEDLINE" required />
        </Row>
        <Row>
          <label htmlFor="s-platform">Plattform <small>worüber gesucht wurde</small></label>
          <input id="s-platform" value={platform} onChange={(e) => setPlatform(e.target.value)} placeholder="z. B. Ovid oder PubMed" />
        </Row>
        <Row>
          <label htmlFor="s-origin">Herkunft <small>der Bericht zählt Datenbanken und Register getrennt von anderen Wegen</small></label>
          <select id="s-origin" value={origin} onChange={(e) => setOrigin(e.target.value)}>
            {Object.entries(ORIGIN_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </Row>
        <div className="stack-field">
          <label htmlFor="s-query">Suchstring <small>vollständig, so wie er ausgeführt wurde</small></label>
          <textarea id="s-query" className="query-input" rows={6} value={query} onChange={(e) => setQuery(e.target.value)} required />
        </div>
        <Row>
          <label htmlFor="s-date">Gesucht am</label>
          <input id="s-date" type="date" value={searchedOn} onChange={(e) => setSearchedOn(e.target.value)} required />
        </Row>
        <Row>
          <label htmlFor="s-limits">Grenzen <small>Zeitraum, Sprachen, Filter</small></label>
          <input id="s-limits" value={limits} onChange={(e) => setLimits(e.target.value)} placeholder="z. B. ab 2015, Englisch und Deutsch" />
        </Row>
        <Row>
          <label htmlFor="s-hits">Treffer <small>wie die Quelle sie meldet; die App vergleicht mit dem Import</small></label>
          <input id="s-hits" className="narrow-input" type="number" min={0} value={hits} onChange={(e) => setHits(e.target.value)} />
        </Row>
        <Row>
          <label htmlFor="s-limit">Trefferlimit <small>falls nur die ersten Treffer übernommen werden, z. B. bei einer Websuche</small></label>
          <input id="s-limit" className="narrow-input" type="number" min={1} value={hitLimit} onChange={(e) => setHitLimit(e.target.value)} />
        </Row>
        {last && (
          <div className="stack-field">
            <label htmlFor="s-change">Was hat sich geändert?</label>
            <input id="s-change" value={changeNote} onChange={(e) => setChangeNote(e.target.value)} required />
          </div>
        )}
      </Group>
      <div className="sheet-actions">
        <button type="button" onClick={onDone}>Abbrechen</button>
        <button className="primary" type="submit">{last ? `Als Version ${last.version + 1} festhalten` : "Suche festhalten"}</button>
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
        <Empty icon="list" title="Noch keine Suche festgehalten">
          Je Quelle eine Suche mit Suchstring, Datum und Grenzen. Der Bericht braucht sie für den Suchanhang und das Flussdiagramm.
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
                <button className="small" onClick={() => setReviewing(reviewing === search.id ? null : search.id)}>Peer-Review vermerken</button>
                <button className="small" onClick={() => setEditing(search)}><Icon name="plus" size={13} /> Neue Version</button>
              </span>
            )}
          >
            <VersionBody version={latest} review={review} />
            {reviewing === search.id && (
              <PeerReviewForm review={review} search={search} version={latest.version} run={run} onDone={() => setReviewing(null)} />
            )}
            {older.length > 0 && (
              <details className="older-versions">
                <summary>{older.length === 1 ? "1 frühere Version" : `${older.length} frühere Versionen`}</summary>
                {older.map((v) => <VersionBody key={v.version} version={v} review={review} />)}
              </details>
            )}
          </Group>
        );
      })}
      {manage && (
        <div className="sheet-actions">
          <button className="primary" onClick={() => setEditing("new")}><Icon name="plus" size={14} /> Neue Suche</button>
        </div>
      )}
    </>
  );
}
