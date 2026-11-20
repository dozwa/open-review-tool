// Screening in drei Spalten: Liste mit Filtern, Eintrag mit Entscheidung, Kriterien mit Erläuterung.
// Welche Filter in einer verblindeten Stufe gesperrt sind, sagt der Kern (`locked`); er weist sie auch ab.

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";

import { AiDialog, AiLine, AiState, suggestionLabel } from "./AiSuggestion";
import { api, canManage, canVote, errorText, FullText, MyVote, Progress, reasonsOf, RecordFilter, RecordItem, RecordList, StageDetail, Suggestion, term } from "./api";
import { CriteriaView } from "./Criteria";
import { PdfView } from "./PdfView";
import { DECISION_LABEL, RecordView } from "./StagePages";
import { HelpButton } from "./Guidance";
import { FocusButton, useFocusMode } from "./focus";
import { formatNumber, t, tn, tx } from "./i18n";
import { useScreeningLayout, useSettings } from "./settings";
import { hits, shortcutLabel, typing } from "./shortcuts";
import { Banner, Empty, Icon, Kbd, Pill, Segmented } from "./ui";

const PAGE = 200;
const DECISION_FILTERS: [string, string][] = [
  ["", t("Alle")],
  ["open", t("Offen")],
  ["include", "Include"],
  ["maybe", "Maybe"],
  ["exclude", "Exclude"],
  ["conflict", t("Konflikt")],
];
const VOTE_FILTERS: Record<string, { label: string; min?: number; max?: number }> = {
  "": { label: t("beliebig viele Stimmen") },
  none: { label: t("keine Stimme"), max: 0 },
  min1: { label: t("mindestens 1 Stimme"), min: 1 },
  min2: { label: t("mindestens 2 Stimmen"), min: 2 },
  max1: { label: t("höchstens 1 Stimme"), max: 1 },
};
const STATUS_LABEL: Record<string, string> = { disagreement: t("Konflikt"), needs_clarification: t("Zur Klärung"), consensus: t("Konsens") };

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
  const [focus, setFocus] = useFocusMode();
  // Aufbau der Seite aus den Einstellungen (Projekt vor Programm); die Schalter hier ändern sie.
  const [layout, setLayout] = useScreeningLayout(reviewId);
  const settings = useSettings();
  const keys = settings.shortcuts;
  // Eingeschaltet auf diesem Rechner und von der Leitung im Review nicht abgeschaltet.
  const aiOn = settings.ai.enabled && (detail?.ai_allowed ?? false);
  // Der letzte eigene KI-Vorschlag zum offenen Eintrag und der Dialog dazu.
  const [suggestion, setSuggestion] = useState<Suggestion | null>(null);
  const [ai, setAi] = useState<AiState | null>(null);
  const showSource = layout.show_source;
  const showCriteria = layout.show_criteria;
  const setShowSource = (show_source: boolean) => setLayout({ show_source });
  const setShowCriteria = (show_criteria: boolean) => setLayout({ show_criteria });

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

  // Ob es zum Eintrag einen Volltext oder schon einen KI-Vorschlag gibt, weiß nur der Kern.
  const recordId = record?.id;
  useEffect(() => {
    let stale = false;
    setFulltext(null);
    setSuggestion(null);
    setAi(null);
    if (recordId) {
      api.record(reviewId, stageId, recordId).then((found) => {
        if (stale) return;
        setFulltext(found.fulltext);
        setSuggestion(found.my_suggestion);
      }).catch(() => undefined);
    }
    return () => {
      stale = true;
    };
  }, [reviewId, stageId, recordId]);
  const openRecord = useRef(recordId);
  openRecord.current = recordId;
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

  /** Nach einer Entscheidung: nächster Eintrag der Liste hinter dem aktuellen, der meine Stimme noch braucht,
   *  sonst der nächste der Stufe. Was die Stufe braucht, entscheidet der Kern nach ihrer Auswahlregel. */
  const advance = useCallback(async (votedId: string) => {
    const [loaded, next] = await Promise.all([api.records(reviewId, stageId, filter()), api.next(reviewId, stageId)]);
    const rows = loaded.items;
    const at = rows.findIndex((r) => r.id === votedId);
    const following = at >= 0 ? rows.slice(at + 1).find((r) => r.wants_my_vote) : undefined;
    setList(loaded);
    setProgress(next.progress);
    if (following) await select(following.id);
    else show(next.record, null);
  }, [reviewId, stageId, filter, select, show]);

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
      setError(t("In dieser Stufe braucht jeder Ausschluss genau einen Hauptgrund. Wähle ihn mit den Tasten 1 bis 9."));
      return;
    }
    const excluded = choice === "exclude";
    const label = excluded && primary ? `${DECISION_LABEL[choice]} · ${primary}` : DECISION_LABEL[choice];
    send((id) => api.vote(reviewId, stageId, id, choice, excluded ? primary : null, excluded ? reasons : [], note), label);
  }, [send, reviewId, stageId, primary, reasons, note, required]);

  // KI-Vorschlag einholen. Kommt die Antwort, steht sie schon im Log; der Dialog zeigt sie nur.
  const askAi = useCallback(async () => {
    if (!recordId || busy) return;
    setError("");
    setAi({ step: "loading", recordId });
    // Die Antwort gilt dem Eintrag, zu dem gefragt wurde; ist inzwischen ein anderer offen, verfällt sie hier.
    const still = (current: AiState | null) => current?.step === "loading" && current.recordId === recordId;
    try {
      const found = await api.aiSuggest(reviewId, stageId, recordId);
      if (openRecord.current === recordId) setSuggestion(found.suggestion);
      setAi((current) => (still(current) ? { step: "done", recordId, suggestion: found.suggestion, prompt: found.prompt } : current));
    } catch (e) {
      setAi((current) => (still(current) ? { step: "error", recordId, text: errorText(e) } : current));
    }
  }, [reviewId, stageId, recordId, busy]);

  /** Übernehmen: vermerken und mit dem Vorschlag abstimmen. Der Grund wird Hauptgrund, wenn die Stufe einen verlangt. */
  const acceptAi = useCallback((found: Suggestion) => {
    const excluded = found.suggested === "exclude";
    setAi(null);
    send(async (id) => {
      await api.aiResolve(reviewId, stageId, id, found.id, "accepted");
      await api.vote(reviewId, stageId, id, found.suggested, excluded && required ? found.reason : null, excluded && !required && found.reason ? [found.reason] : [], note);
    }, t("{label} (KI-Vorschlag übernommen)", { label: suggestionLabel(found) }));
  }, [send, reviewId, stageId, required, note]);

  /** Ablehnen: vermerken, der Eintrag bleibt offen. */
  const declineAi = useCallback((found: Suggestion) => {
    setAi(null);
    if (!recordId) return;
    api.aiResolve(reviewId, stageId, recordId, found.id, "declined")
      .then(() => setSuggestion((current) => (current?.id === found.id ? { ...current, outcome: "declined" } : current)))
      .catch((e) => setError(errorText(e)));
  }, [reviewId, stageId, recordId]);

  const toggleReason = useCallback((code: string) => {
    setError("");
    if (required) setPrimary((current) => (current === code ? null : code));
    else setReasons((current) => (current.includes(code) ? current.filter((r) => r !== code) : [...current, code]));
  }, [required]);

  // Tastatur: Entscheiden und Blättern mit den Kürzeln aus den Einstellungen (anfangs i, e, m und j, k);
  // 1–9 wählen den Grund. Beim Tippen in einem Feld gelten nur Kürzel mit Strg oder Befehlstaste.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (!detail) return;
      const choice = hits(event, keys.include) ? "include" : hits(event, keys.exclude) ? "exclude" : hits(event, keys.maybe) ? "maybe" : null;
      if (choice) {
        event.preventDefault();
        if (canVote(detail.role) && !detail.stage.closed) vote(choice);
        return;
      }
      if (hits(event, keys.ai)) {
        event.preventDefault();
        if (aiOn && canVote(detail.role) && !detail.stage.closed) askAi();
        return;
      }
      const forward = hits(event, keys.next);
      if (forward || hits(event, keys.previous)) {
        event.preventDefault();
        const rows = list?.items ?? [];
        const at = rows.findIndex((r) => r.id === record?.id);
        const row = rows[forward ? at + 1 : Math.max(at - 1, 0)];
        if (row) select(row.id);
        return;
      }
      if (typing(event) || event.ctrlKey || event.metaKey || event.altKey) return;
      const reason = reasonList[Number(event.key) - 1];
      if (reason) toggleReason(reason.code);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [vote, toggleReason, detail, reasonList, list, record, select, keys, aiOn, askAi]);

  if (!detail) return error ? <Banner kind="error">{error}</Banner> : null;
  const { stage, criteria } = detail;
  const records = term(detail.terms, "records", t("Einträge"));
  const locked = new Set(list?.locked ?? []);
  const voters = detail.members.filter((m) => canVote(m.role));
  const mayVote = canVote(detail.role) && !stage.closed;
  const hasFulltext = fulltext?.status === "retrieved";
  // In Stufen mit Pflichtgrund (Volltext) steht die PDF vorn, sonst der Abstract.
  const showFulltext = hasFulltext && (reading ?? (stage.reason_required ? "fulltext" : "abstract")) === "fulltext";
  const pct = progress ? Math.round((progress.done / Math.max(progress.total, 1)) * 100) : 0;

  return (
    <div className={`screening${showCriteria ? "" : " no-criteria"}${layout.show_list ? "" : " no-list"}${layout.highlight ? "" : " no-highlight"}`}>
      <div className="screening-top">
        <Link to={`/reviews/${reviewId}`} className="back-link"><Icon name="back" size={14} /> {t("Review")}</Link>
        <div className="screening-title">
          <strong>{stage.name}</strong>
          {stage.blind_active && <Pill tone="violet"><Icon name="lock" size={11} /> {t("verblindet")}</Pill>}
          {stage.closed && <Pill>{t("geschlossen")}</Pill>}
        </div>
        <span className="screening-tools">
          <span className="screening-count">{progress ? `${progress.done} / ${progress.total}` : ""}</span>
          <FocusButton on={focus} onChange={setFocus} />
          <button className="ghost focus-hide" aria-pressed={layout.show_list} onClick={() => setLayout({ show_list: !layout.show_list })} title={t("Liste ein- oder ausblenden")}><Icon name="stack" size={14} /> {t("Liste")}</button>
          <button className="ghost focus-hide" aria-pressed={showCriteria} onClick={() => setShowCriteria(!showCriteria)} title={t("Kriterien ein- oder ausblenden")}><Icon name="list" size={14} /> {t("Kriterien")}</button>
          <span className="focus-hide"><HelpButton reviewId={reviewId} station={stage.station} context={stage.name} /></span>
        </span>
      </div>
      <div className="progress-line"><span style={{ width: `${pct}%` }} /></div>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      {ai && (
        <AiDialog
          state={ai}
          model={settings.ai.model}
          reasons={reasonList}
          required={required}
          busy={busy}
          onAccept={acceptAi}
          onDecline={declineAi}
          onClose={() => setAi(null)}
          onRetry={askAi}
        />
      )}

      <div className="screening-columns">
        <aside className="screening-list" aria-label={t("Liste der {records}", { records })}>
          <div className="filters">
            <input type="search" value={query} onChange={(e) => { setQuery(e.target.value); setLimit(PAGE); }} placeholder={t("In Titel und Abstract suchen")} aria-label={t("Suche")} />
            <div className="filter-chips" role="group" aria-label={t("Entscheidung")}>
              {DECISION_FILTERS.map(([value, label]) => {
                const off = value === "conflict" && locked.has("conflict");
                return (
                  <button key={value} className={`chip toggle${decision === value ? " active" : ""}`} aria-pressed={decision === value} disabled={off} title={off ? t("Erst nach dem Entblinden") : undefined} onClick={() => { setDecision(value); setLimit(PAGE); }}>
                    {off && <Icon name="lock" size={10} />} {label}
                  </button>
                );
              })}
            </div>
            <select value={person} onChange={(e) => { setPerson(e.target.value); setLimit(PAGE); }} disabled={locked.has("person")} aria-label={t("Person")}>
              <option value="">{t("Meine Stimmen")}</option>
              {voters.filter((m) => !m.is_me).map((m) => <option key={m.actor_id} value={m.actor_id}>{t("Stimmen von {name}", { name: m.name })}</option>)}
            </select>
            <select value={voteCount} onChange={(e) => { setVoteCount(e.target.value); setLimit(PAGE); }} disabled={locked.has("votes")} aria-label={t("Zahl der Stimmen")}>
              {Object.entries(VOTE_FILTERS).map(([value, f]) => <option key={value} value={value}>{f.label}</option>)}
            </select>
            {locked.size > 0 && <small className="locked-hint"><Icon name="lock" size={11} /> {t("Verblindet: Konflikt, Person und Zahl der Stimmen sind gesperrt.")}</small>}
            {list && list.keywords.length > 0 && (
              <div className="filter-chips" role="group" aria-label={t("Schlagwörter")}>
                {list.keywords.map((k) => (
                  <button key={`${k.kind}-${k.keyword}`} className={`chip toggle kw-${k.kind}${keyword === k.keyword ? " active" : ""}`} aria-pressed={keyword === k.keyword} onClick={() => { setKeyword(keyword === k.keyword ? "" : k.keyword); setLimit(PAGE); }}>
                    {k.keyword} <span className="count">{k.n}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
          <p className="list-count">{list ? `${formatNumber(list.total)} ${records}` : ""}</p>
          <ol className="record-list">
            {list?.items.map((r) => (
              <li key={r.id}>
                <button className={`record-row${record?.id === r.id ? " current" : ""}`} aria-current={record?.id === r.id} onClick={() => select(r.id)}>
                  <span className={`dot d-${r.my_decision ?? "open"}`} title={r.my_decision ? t("Deine Stimme: {decision}", { decision: DECISION_LABEL[r.my_decision] ?? r.my_decision }) : t("Noch nicht bewertet")} />
                  <span className="record-row-text">
                    <span className="record-row-title">{r.title || t("(ohne Titel)")}</span>
                    <small>
                      {[r.year, r.n_votes !== null ? tn("{n} Stimme", "{n} Stimmen", r.n_votes) : null, r.status ? STATUS_LABEL[r.status] : null].filter(Boolean).join(" · ")}
                    </small>
                  </span>
                </button>
              </li>
            ))}
          </ol>
          {list && list.total > list.items.length && <button className="ghost more" onClick={() => setLimit(limit + PAGE)}>{t("Weitere laden")}</button>}
          {list && list.total === 0 && <p className="muted list-empty">{t("Kein Eintrag passt zu den Filtern.")}</p>}
        </aside>

        <section className="screening-main">
          {!record && (
            <Empty icon="check" title={stage.closed ? t("Diese Stufe ist geschlossen") : t("Alles bewertet")}>
              {stage.closed
                ? t("Einträge lassen sich über die Liste weiter ansehen.")
                : canManage(detail.role)
                  ? t("Liegen die Rückmeldungen des Teams vor, liest du sie unter „Austausch“ ein und klärst die Abweichungen in der Auswertung der Stufe.")
                  : t("Speichere im Review unter „Austausch“ eine Rückmeldung und schick sie an die Leitung.")}
              <div>
                <Link className="button primary" to={canManage(detail.role) ? `/reviews/${reviewId}/stages/${stageId}` : `/reviews/${reviewId}?tab=exchange`}>
                  {canManage(detail.role) ? t("Zur Auswertung") : t("Zum Austausch")}
                </Link>
              </div>
            </Empty>
          )}
          {record && (
            <>
              {hasFulltext && (
                <div className="reading-switch">
                  <Segmented<"abstract" | "fulltext">
                    label={t("Ansicht")}
                    value={showFulltext ? "fulltext" : "abstract"}
                    onChange={setReading}
                    options={[{ value: "abstract", label: t("Titel und Abstract") }, { value: "fulltext", label: t("Volltext") }]}
                  />
                  {showFulltext && <span className="reading-title" title={record.title}>{record.title}</span>}
                </div>
              )}
              {fulltext?.status === "unavailable" && <p className="reading-note">{t("Volltext nicht beschaffbar: {reason}", { reason: fulltext.reason ?? "" })}</p>}
              {showFulltext ? (
                <PdfView load={loadFulltext} onExternal={() => api.openFulltext(reviewId, record.id).catch((e) => setError(errorText(e)))} />
              ) : (
                <div className="reading">
                  <RecordView record={record} criteria={criteria} large showSource={showSource} />
                  <button className="link source-toggle" onClick={() => setShowSource(!showSource)}>
                    {showSource ? t("Autoren und Zeitschrift ausblenden") : t("Autoren und Zeitschrift einblenden")}
                  </button>
                </div>
              )}
              {mayVote && (
                <div className="decision-dock">
                  {myVote && (
                    <p className="my-vote">
                      {myVote.primary_reason
                        ? tx("Deine Stimme: {decision} · Hauptgrund {reason}. Eine neue Entscheidung ersetzt sie; die alte bleibt im Log.", {
                            decision: <strong>{DECISION_LABEL[myVote.decision] ?? myVote.decision}</strong>,
                            reason: myVote.primary_reason,
                          })
                        : tx("Deine Stimme: {decision}. Eine neue Entscheidung ersetzt sie; die alte bleibt im Log.", {
                            decision: <strong>{DECISION_LABEL[myVote.decision] ?? myVote.decision}</strong>,
                          })}
                    </p>
                  )}
                  {reasonList.length > 0 && (
                    <div className="reasons" role="group" aria-label={required ? t("Hauptgrund für den Ausschluss") : t("Ausschlussgründe")}>
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
                  {required && <p className="reason-hint">{primary ? t("Hauptgrund für den Ausschluss: {reason}", { reason: primary }) : t("Ein Ausschluss braucht in dieser Stufe genau einen Hauptgrund.")}</p>}
                  <div className="dock-row">
                    <input className="note" value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("Notiz (optional)")} aria-label={t("Notiz")} />
                    <button className="ghost" disabled={busy} onClick={() => send((id) => api.flag(reviewId, stageId, id, note), t("Zur Klärung"))} title={t("Nicht entscheidbar, zur Klärung geben")}>
                      <Icon name="flag" size={14} /> {t("Zur Klärung")}
                    </button>
                    {aiOn && (
                      <button className="ghost" disabled={busy} onClick={askAi} title={t("Vorschlag der KI zu diesem Eintrag einholen")}>
                        <Icon name="spark" size={14} /> {t("KI-Vorschlag")} {keys.ai && <Kbd>{shortcutLabel(keys.ai)}</Kbd>}
                      </button>
                    )}
                  </div>
                  <div className="vote-buttons">
                    <button className="vote vote-include" disabled={busy} onClick={() => vote("include")}>
                      <Icon name="check" /> Include {keys.include && <Kbd>{shortcutLabel(keys.include)}</Kbd>}
                    </button>
                    <button className="vote vote-maybe" disabled={busy} onClick={() => vote("maybe")}>
                      <Icon name="help" /> Maybe {keys.maybe && <Kbd>{shortcutLabel(keys.maybe)}</Kbd>}
                    </button>
                    <button className="vote vote-exclude" disabled={busy || (required && !primary)} onClick={() => vote("exclude")}>
                      <Icon name="x" /> Exclude {keys.exclude && <Kbd>{shortcutLabel(keys.exclude)}</Kbd>}
                    </button>
                  </div>
                  {suggestion && <AiLine suggestion={suggestion} onShow={() => setAi({ step: "done", recordId: record.id, suggestion })} />}
                  {last && <p className="last-vote">{t("Gespeichert: {label}", { label: last })}</p>}
                </div>
              )}
            </>
          )}
        </section>

        {showCriteria && (
          <aside className="screening-criteria" aria-label={t("Kriterien")}>
            <h2>{t("Kriterien")} <small>{t("Version {version}", { version: criteria.version })}</small></h2>
            {detail.newer_criteria && <p className="criteria-newer">{t("Es gibt eine neuere Version. Sie gilt ab der nächsten Stufe; diese Stufe bleibt bei Version {version}.", { version: criteria.version })}</p>}
            <CriteriaView criteria={criteria} schema={detail.criteria_schema} />
          </aside>
        )}
      </div>
    </div>
  );
}
