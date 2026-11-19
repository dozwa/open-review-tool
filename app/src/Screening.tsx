// Screening in drei Spalten: Liste mit Filtern, Eintrag mit Entscheidung, Kriterien mit Erläuterung.
// Welche Filter in einer verblindeten Stufe gesperrt sind, sagt der Kern (`locked`); er weist sie auch ab.

import React, { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";

import { api, canVote, errorText, FullText, MyVote, Progress, reasonsOf, RecordFilter, RecordItem, RecordList, StageDetail, term } from "./api";
import { CriteriaView } from "./Criteria";
import { PdfView } from "./PdfView";
import { DECISION_LABEL, RecordView } from "./StagePages";
import { Banner, Empty, Icon, Kbd, Pill, Segmented } from "./ui";

const PAGE = 200;
const DECISION_FILTERS: [string, string][] = [
  ["", "Alle"],
  ["open", "Offen"],
  ["include", "Include"],
  ["maybe", "Maybe"],
  ["exclude", "Exclude"],
  ["conflict", "Konflikt"],
];
const VOTE_FILTERS: Record<string, { label: string; min?: number; max?: number }> = {
  "": { label: "beliebig viele Stimmen" },
  none: { label: "keine Stimme", max: 0 },
  min1: { label: "mindestens 1 Stimme", min: 1 },
  min2: { label: "mindestens 2 Stimmen", min: 2 },
  max1: { label: "höchstens 1 Stimme", max: 1 },
};
const STATUS_LABEL: Record<string, string> = { disagreement: "Konflikt", needs_clarification: "Klärung", consensus: "Konsens" };

/** Merkt sich eine Einstellung der Ansicht auf diesem Rechner. Ohne Speicher gilt der Standard. */
function useStored(key: string, fallback: boolean): [boolean, (value: boolean) => void] {
  const [value, setValue] = useState(() => {
    try {
      const stored = localStorage.getItem(key);
      return stored === null ? fallback : stored === "1";
    } catch {
      return fallback;
    }
  });
  const set = (next: boolean) => {
    setValue(next);
    try {
      localStorage.setItem(key, next ? "1" : "0");
    } catch {
      // Einstellung gilt dann nur bis zum Schließen
    }
  };
  return [value, set];
}

/** Je Stufe eine eigene Instanz, damit Filter und Auswahl nicht von einer Stufe in die nächste wandern. */
export function ScreeningPage() {
  const { stageId = "" } = useParams();
  return <Screening key={stageId} />;
}

function Screening() {
  const { reviewId = "", stageId = "" } = useParams();
  const [detail, setDetail] = useState<StageDetail | null>(null);
  const [list, setList] = useState<RecordList | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [record, setRecord] = useState<RecordItem | null>(null);
  const [myVote, setMyVote] = useState<MyVote | null>(null);
  const [fulltext, setFulltext] = useState<FullText | null>(null);
  const [reading, setReading] = useState<"abstract" | "fulltext" | null>(null);
  const [primary, setPrimary] = useState<string | null>(null);
  const [reasons, setReasons] = useState<string[]>([]);
  const [note, setNote] = useState("");
  const [decision, setDecision] = useState("");
  const [person, setPerson] = useState("");
  const [voteCount, setVoteCount] = useState("");
  const [keyword, setKeyword] = useState("");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(PAGE);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [last, setLast] = useState("");
  const [showSource, setShowSource] = useStored("ort.screening.source", false);
  const [showCriteria, setShowCriteria] = useStored("ort.screening.criteria", true);

  const filter = useCallback((): RecordFilter => {
    const votes = VOTE_FILTERS[voteCount] ?? {};
    return { decision: decision || null, person: person || null, votes_min: votes.min ?? null, votes_max: votes.max ?? null, keyword: keyword || null, query: query || null, limit };
  }, [decision, person, voteCount, keyword, query, limit]);

  const show = useCallback((item: RecordItem | null, vote: MyVote | null) => {
    setRecord(item);
    setMyVote(vote);
    setPrimary(vote?.primary_reason ?? null);
    setReasons(vote?.reasons ?? []);
    setNote(vote?.note ?? "");
  }, []);

  // Ob es zum Eintrag einen Volltext gibt, weiß nur der Kern.
  const recordId = record?.id;
  useEffect(() => {
    let stale = false;
    setFulltext(null);
    if (recordId) api.record(reviewId, stageId, recordId).then((found) => !stale && setFulltext(found.fulltext)).catch(() => undefined);
    return () => {
      stale = true;
    };
  }, [reviewId, stageId, recordId]);
  const loadFulltext = useCallback(() => api.readFulltext(reviewId, recordId ?? ""), [reviewId, recordId]);

  const select = useCallback(async (recordId: string) => {
    try {
      const found = await api.record(reviewId, stageId, recordId);
      show(found.record, found.my_vote);
    } catch (e) {
      setError(errorText(e));
    }
  }, [reviewId, stageId, show]);

  // Liste neu laden, wenn sich ein Filter ändert. Die Suche wartet kurz, bis nicht mehr getippt wird.
  useEffect(() => {
    let stale = false;
    const timer = setTimeout(async () => {
      try {
        const loaded = await api.records(reviewId, stageId, filter());
        if (!stale) setList(loaded);
      } catch (e) {
        if (!stale) setError(errorText(e));
      }
    }, query ? 200 : 0);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [reviewId, stageId, filter, query]);

  // Beim Öffnen: Stufe laden und mit dem nächsten offenen Eintrag beginnen.
  useEffect(() => {
    let stale = false;
    (async () => {
      try {
        const [loaded, next] = await Promise.all([api.stage(reviewId, stageId), api.next(reviewId, stageId)]);
        if (stale) return;
        setDetail(loaded);
        setProgress(next.progress);
        show(next.record, null);
      } catch (e) {
        if (!stale) setError(errorText(e));
      }
    })();
    return () => {
      stale = true;
    };
  }, [reviewId, stageId, show]);

  /** Nach einer Entscheidung: nächster offener Eintrag der Liste hinter dem aktuellen, sonst der nächste der Stufe. */
  const advance = useCallback(async (votedId: string) => {
    const rows = list?.items ?? [];
    const at = rows.findIndex((r) => r.id === votedId);
    const following = at >= 0 ? rows.slice(at + 1).find((r) => !r.my_decision) : undefined;
    const [loaded, next] = await Promise.all([api.records(reviewId, stageId, filter()), api.next(reviewId, stageId)]);
    setList(loaded);
    setProgress(next.progress);
    if (following) await select(following.id);
    else show(next.record, null);
  }, [list, reviewId, stageId, filter, select, show]);

  const send = useCallback(async (action: (recordId: string) => Promise<void>, label: string) => {
    if (!record || busy) return;
    setBusy(true);
    setError("");
    try {
      await action(record.id);
      setLast(label);
      await advance(record.id);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }, [record, busy, advance]);

  const required = detail?.stage.reason_required ?? false;
  const reasonList = useMemo(() => (detail ? reasonsOf(detail.criteria) : []), [detail]);

  const vote = useCallback((choice: string) => {
    if (choice === "exclude" && required && !primary) {
      setError("In dieser Stufe braucht jeder Ausschluss genau einen Hauptgrund. Wähle ihn mit den Tasten 1 bis 9.");
      return;
    }
    const excluded = choice === "exclude";
    const label = excluded && primary ? `${DECISION_LABEL[choice]} · ${primary}` : DECISION_LABEL[choice];
    send((id) => api.vote(reviewId, stageId, id, choice, excluded ? primary : null, excluded ? reasons : [], note), label);
  }, [send, reviewId, stageId, primary, reasons, note, required]);

  const toggleReason = useCallback((code: string) => {
    setError("");
    if (required) setPrimary((current) => (current === code ? null : code));
    else setReasons((current) => (current.includes(code) ? current.filter((r) => r !== code) : [...current, code]));
  }, [required]);

  // Tastatur: i, e, m entscheiden; 1–9 wählen den Grund; j und k blättern in der Liste. Nicht beim Tippen in einem Feld.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;
      if (event.ctrlKey || event.metaKey || event.altKey || !detail) return;
      const key = event.key.toLowerCase();
      const choice = ({ i: "include", e: "exclude", m: "maybe" } as Record<string, string>)[key];
      if (choice && canVote(detail.role) && !detail.stage.closed) {
        vote(choice);
        return;
      }
      const reason = reasonList[Number(event.key) - 1];
      if (reason) {
        toggleReason(reason.code);
        return;
      }
      if (key === "j" || key === "k") {
        const rows = list?.items ?? [];
        const at = rows.findIndex((r) => r.id === record?.id);
        const row = rows[key === "j" ? at + 1 : Math.max(at - 1, 0)];
        if (row) select(row.id);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [vote, toggleReason, detail, reasonList, list, record, select]);

  if (!detail) return error ? <Banner kind="error">{error}</Banner> : null;
  const { stage, criteria } = detail;
  const records = term(detail.terms, "records", "Einträge");
  const locked = new Set(list?.locked ?? []);
  const voters = detail.members.filter((m) => canVote(m.role));
  const mayVote = canVote(detail.role) && !stage.closed;
  const hasFulltext = fulltext?.status === "retrieved";
  // In Stufen mit Pflichtgrund (Volltext) steht die PDF vorn, sonst der Abstract.
  const showFulltext = hasFulltext && (reading ?? (stage.reason_required ? "fulltext" : "abstract")) === "fulltext";
  const pct = progress ? Math.round((progress.done / Math.max(progress.total, 1)) * 100) : 0;

  return (
    <div className={`screening${showCriteria ? "" : " no-criteria"}`}>
      <div className="screening-top">
        <Link to={`/reviews/${reviewId}`} className="back-link"><Icon name="back" size={14} /> Review</Link>
        <div className="screening-title">
          <strong>{stage.name}</strong>
          {stage.blind_active && <Pill tone="violet"><Icon name="lock" size={11} /> verblindet</Pill>}
          {stage.closed && <Pill>geschlossen</Pill>}
        </div>
        <span className="screening-tools">
          <span className="screening-count">{progress ? `${progress.done} / ${progress.total}` : ""}</span>
          <button className="ghost" aria-pressed={showCriteria} onClick={() => setShowCriteria(!showCriteria)} title="Kriterien ein- oder ausblenden"><Icon name="list" size={14} /> Kriterien</button>
        </span>
      </div>
      <div className="progress-line"><span style={{ width: `${pct}%` }} /></div>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}

      <div className="screening-columns">
        <aside className="screening-list" aria-label={`Liste der ${records}`}>
          <div className="filters">
            <input type="search" value={query} onChange={(e) => { setQuery(e.target.value); setLimit(PAGE); }} placeholder="In Titel und Abstract suchen" aria-label="Suche" />
            <div className="filter-chips" role="group" aria-label="Entscheidung">
              {DECISION_FILTERS.map(([value, label]) => {
                const off = value === "conflict" && locked.has("conflict");
                return (
                  <button key={value} className={`chip toggle${decision === value ? " active" : ""}`} aria-pressed={decision === value} disabled={off} title={off ? "Erst nach dem Entblinden" : undefined} onClick={() => { setDecision(value); setLimit(PAGE); }}>
                    {off && <Icon name="lock" size={10} />} {label}
                  </button>
                );
              })}
            </div>
            <select value={person} onChange={(e) => { setPerson(e.target.value); setLimit(PAGE); }} disabled={locked.has("person")} aria-label="Person">
              <option value="">Meine Stimmen</option>
              {voters.filter((m) => !m.is_me).map((m) => <option key={m.actor_id} value={m.actor_id}>Stimmen von {m.name}</option>)}
            </select>
            <select value={voteCount} onChange={(e) => { setVoteCount(e.target.value); setLimit(PAGE); }} disabled={locked.has("votes")} aria-label="Zahl der Stimmen">
              {Object.entries(VOTE_FILTERS).map(([value, f]) => <option key={value} value={value}>{f.label}</option>)}
            </select>
            {locked.size > 0 && <small className="locked-hint"><Icon name="lock" size={11} /> Verblindet: Konflikt, Person und Zahl der Stimmen sind gesperrt.</small>}
            {list && list.keywords.length > 0 && (
              <div className="filter-chips" role="group" aria-label="Schlagwörter">
                {list.keywords.map((k) => (
                  <button key={`${k.kind}-${k.keyword}`} className={`chip toggle kw-${k.kind}${keyword === k.keyword ? " active" : ""}`} aria-pressed={keyword === k.keyword} onClick={() => { setKeyword(keyword === k.keyword ? "" : k.keyword); setLimit(PAGE); }}>
                    {k.keyword} <span className="count">{k.n}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
          <p className="list-count">{list ? `${list.total.toLocaleString("de-DE")} ${records}` : ""}</p>
          <ol className="record-list">
            {list?.items.map((r) => (
              <li key={r.id}>
                <button className={`record-row${record?.id === r.id ? " current" : ""}`} aria-current={record?.id === r.id} onClick={() => select(r.id)}>
                  <span className={`dot d-${r.my_decision ?? "open"}`} title={r.my_decision ? `Deine Stimme: ${DECISION_LABEL[r.my_decision]}` : "Noch nicht bewertet"} />
                  <span className="record-row-text">
                    <span className="record-row-title">{r.title || "(ohne Titel)"}</span>
                    <small>
                      {[r.year, r.n_votes !== null ? `${r.n_votes} ${r.n_votes === 1 ? "Stimme" : "Stimmen"}` : null, r.status ? STATUS_LABEL[r.status] : null].filter(Boolean).join(" · ")}
                    </small>
                  </span>
                </button>
              </li>
            ))}
          </ol>
          {list && list.total > list.items.length && <button className="ghost more" onClick={() => setLimit(limit + PAGE)}>Weitere laden</button>}
          {list && list.total === 0 && <p className="muted list-empty">Kein Eintrag passt zu den Filtern.</p>}
        </aside>

        <section className="screening-main">
          {!record && (
            <Empty icon="check" title={stage.closed ? "Diese Stufe ist geschlossen" : "Alles bewertet"}>
              {stage.closed ? "Einträge lassen sich über die Liste weiter ansehen." : "Speichere im Review unter „Austausch“ eine Rückmeldung und schick sie an die Leitung."}
              <div><Link className="button primary" to={`/reviews/${reviewId}`}>Zum Review</Link></div>
            </Empty>
          )}
          {record && (
            <>
              {hasFulltext && (
                <div className="reading-switch">
                  <Segmented<"abstract" | "fulltext">
                    label="Ansicht"
                    value={showFulltext ? "fulltext" : "abstract"}
                    onChange={setReading}
                    options={[{ value: "abstract", label: "Titel und Abstract" }, { value: "fulltext", label: "Volltext" }]}
                  />
                  {showFulltext && <span className="reading-title" title={record.title}>{record.title}</span>}
                </div>
              )}
              {fulltext?.status === "unavailable" && <p className="reading-note">Volltext nicht beschaffbar: {fulltext.reason}</p>}
              {showFulltext ? (
                <PdfView load={loadFulltext} onExternal={() => api.openFulltext(reviewId, record.id).catch((e) => setError(errorText(e)))} />
              ) : (
                <div className="reading">
                  <RecordView record={record} criteria={criteria} large showSource={showSource} />
                  <button className="link source-toggle" onClick={() => setShowSource(!showSource)}>
                    {showSource ? "Autoren und Zeitschrift ausblenden" : "Autoren und Zeitschrift einblenden"}
                  </button>
                </div>
              )}
              {mayVote && (
                <div className="decision-dock">
                  {myVote && (
                    <p className="my-vote">
                      Deine Stimme: <strong>{DECISION_LABEL[myVote.decision] ?? myVote.decision}</strong>
                      {myVote.primary_reason && ` · Hauptgrund ${myVote.primary_reason}`}. Eine neue Entscheidung ersetzt sie; die alte bleibt im Log.
                    </p>
                  )}
                  {reasonList.length > 0 && (
                    <div className="reasons" role="group" aria-label={required ? "Hauptgrund für den Ausschluss" : "Ausschlussgründe"}>
                      {reasonList.map((reason, i) => {
                        const on = required ? primary === reason.code : reasons.includes(reason.code);
                        return (
                          <button key={reason.code} className={`chip toggle ${on ? "on" : ""}`} aria-pressed={on} onClick={() => toggleReason(reason.code)} title={reason.note || undefined}>
                            {i < 9 && <Kbd>{i + 1}</Kbd>} <strong>{reason.code}</strong> {reason.text}
                          </button>
                        );
                      })}
                    </div>
                  )}
                  {required && <p className="reason-hint">{primary ? `Hauptgrund für den Ausschluss: ${primary}` : "Ein Ausschluss braucht in dieser Stufe genau einen Hauptgrund."}</p>}
                  <div className="dock-row">
                    <input className="note" value={note} onChange={(e) => setNote(e.target.value)} placeholder="Notiz (optional)" aria-label="Notiz" />
                    <button className="ghost" disabled={busy} onClick={() => send((id) => api.flag(reviewId, stageId, id, note), "Zur Klärung")} title="Nicht entscheidbar, zur Klärung geben">
                      <Icon name="flag" size={14} /> Klärung
                    </button>
                  </div>
                  <div className="vote-buttons">
                    <button className="vote vote-include" disabled={busy} onClick={() => vote("include")}>
                      <Icon name="check" /> Include <Kbd>I</Kbd>
                    </button>
                    <button className="vote vote-maybe" disabled={busy} onClick={() => vote("maybe")}>
                      <Icon name="help" /> Maybe <Kbd>M</Kbd>
                    </button>
                    <button className="vote vote-exclude" disabled={busy || (required && !primary)} onClick={() => vote("exclude")}>
                      <Icon name="x" /> Exclude <Kbd>E</Kbd>
                    </button>
                  </div>
                  {last && <p className="last-vote">Gespeichert: {last}</p>}
                </div>
              )}
            </>
          )}
        </section>

        {showCriteria && (
          <aside className="screening-criteria" aria-label="Kriterien">
            <h2>Kriterien <small>Version {criteria.version}</small></h2>
            {detail.newer_criteria && <p className="criteria-newer">Es gibt eine neuere Version. Sie gilt ab der nächsten Stufe; diese Stufe bleibt bei Version {criteria.version}.</p>}
            <CriteriaView criteria={criteria} schema={detail.criteria_schema} />
          </aside>
        )}
      </div>
    </div>
  );
}
