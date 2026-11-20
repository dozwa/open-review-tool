// Formular-Stufe: Tabelle der Quellen mit Stand und Werten, und die Eingabe einer Quelle neben dem Volltext.
// Was verblindet ist, was als Abweichung gilt und wann eine Quelle fertig ist, entscheidet der Kern.

import React, { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ask, save } from "@tauri-apps/plugin-dialog";

import {
  api,
  canManage,
  canVote,
  EntryStatus,
  errorText,
  Extraction,
  FormCell,
  FormEntryView,
  FormField,
  FormStageView,
  FormValue,
  Frequency,
  gateText,
  MineStatus,
  SchemaElement,
  showValue,
  term,
} from "./api";
import { ExtractionDialog, ExtractionLine, ExtractionState } from "./AiExtraction";
import { useHighlights } from "./highlights";
import { pdfPages, PdfView } from "./PdfView";
import { stageState } from "./ReviewPage";
import { HelpButton } from "./Guidance";
import { FocusButton, useFocusMode } from "./focus";
import { t, tn } from "./i18n";
import { inFolder, useSettings } from "./settings";
import { Banner, Empty, Group, Icon, PageHeader, Pill, Row, Segmented, Stat } from "./ui";

const MINE_LABEL: Record<MineStatus, string> = {
  open: t("offen"),
  draft: t("angefangen"),
  completed: t("fertig"),
  todo: t("nachzutragen"),
  to_verify: t("zu prüfen"),
  locked: t("extrahiert jemand anderes"),
};
const MINE_TONE: Record<MineStatus, string | undefined> = { open: undefined, draft: "yellow", completed: "green", todo: "red", to_verify: "blue", locked: undefined };
const STATUS_LABEL: Record<EntryStatus, string> = {
  open: t("wartet auf Extraktion"),
  extracted: t("wartet auf Prüfung"),
  conflict: t("Abweichung"),
  incomplete: t("Pflichtfeld leer"),
  done: t("abgeglichen"),
};
const STATUS_TONE: Record<EntryStatus, string | undefined> = { open: undefined, extracted: "blue", conflict: "red", incomplete: "yellow", done: "green" };
const MODE_LABEL: Record<string, string> = { unanimous: t("doppelt"), single_verified: t("einfach mit Prüfung") };

const typeLabel = (types: SchemaElement[], key: string) => types.find((st) => st.key === key)?.label ?? key;

function minutes(seconds: number): string {
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")} min`;
}

// ---------------------------------------------------------------- Tabelle

export function FormStagePage() {
  const { reviewId = "", stageId = "" } = useParams();
  const navigate = useNavigate();
  const [view, setView] = useState<FormStageView | null>(null);
  const [frequencies, setFrequencies] = useState<Frequency[]>([]);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    try {
      const loaded = await api.formStage(reviewId, stageId);
      setView(loaded);
      setFrequencies(loaded.stage.blind_active ? [] : await api.formFrequencies(reviewId, stageId));
    } catch (e) {
      setError(errorText(e));
    }
  }, [reviewId, stageId]);
  useEffect(() => {
    load();
  }, [load]);

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

  if (!view) return error ? <Banner kind="error">{error}</Banner> : null;
  const { stage, form, rows, counts } = view;
  const manage = canManage(view.role);
  const sources = term(view.terms, "sources", t("Quellen"));
  const mineOpen = rows.find((r) => r.mine !== "completed" && r.mine !== "locked");
  const entry = (recordId: string) => `/reviews/${reviewId}/forms/${stageId}/${recordId}`;

  async function unblind() {
    const ok = await ask(t("Danach sehen alle, die ein neues Paket bekommen, die Werte der anderen. Das lässt sich nicht zurücknehmen."), {
      title: t("Stufe entblinden?"), kind: "warning", okLabel: t("Entblinden"), cancelLabel: t("Abbrechen"),
    });
    if (ok) await run(() => api.unblind(reviewId, stageId), t("Stufe entblindet. Schick den Mitgliedern ein neues Paket."));
  }

  async function closeStage() {
    const gate = await api.gate(reviewId, stageId).catch(() => null);
    let text = t("Danach sind keine Werte mehr möglich, auch keine nachgereichten. Der Stand wird als Snapshot festgehalten.");
    if (gate) text += "\n\n" + (gate.passed ? t("Das Gate der Station ist erfüllt: {gate}.", { gate: gateText(gate) }) : t("Das Gate der Station ist nicht erfüllt: {gate}.", { gate: gateText(gate) }));
    else if (stage.blind_active) text += "\n\n" + t("Die Stufe ist noch verblindet. Abweichungen lassen sich erst nach dem Entblinden klären.");
    const ok = await ask(text, { title: t("Stufe schließen?"), kind: "warning", okLabel: t("Schließen"), cancelLabel: t("Abbrechen") });
    if (ok) await run(() => api.close(reviewId, stageId), t("Stufe geschlossen, Snapshot erstellt."));
  }

  async function exportFile(kind: "table" | "frequencies" | "json") {
    const name = kind === "table" ? t("extraktion-tabelle.csv") : kind === "frequencies" ? t("extraktion-haeufigkeiten.csv") : t("extraktion.json");
    const format = kind === "json" ? "json" : "csv";
    const path = await save({ defaultPath: await inFolder("export", name, reviewId), filters: [{ name: format.toUpperCase(), extensions: [format] }] });
    if (path) await run(() => api.exportForm(reviewId, stageId, kind, path), kind === "json" ? t("JSON gespeichert.") : t("CSV gespeichert."));
  }

  return (
    <>
      <PageHeader
        back={<Link to={`/reviews/${reviewId}`} className="back-link"><Icon name="back" size={14} /> {t("Review")}</Link>}
        title={stage.name}
        subtitle={
          <>
            <Pill>{stageState(stage)}</Pill>
            <span>{t("{n} {sources}", { n: rows.length, sources })}</span>
            <span>{MODE_LABEL[stage.decision_rule] ?? stage.decision_rule}</span>
            {stage.decision_rule === "unanimous" && <span>{t("mind. {n} Personen", { n: stage.min_reviewers })}</span>}
            {stage.seed !== null && <span>{t("Stichprobe je Quellenart, Seed {seed}", { seed: stage.seed })}</span>}
            <span>{form.confirmed ? t("{name}, Version {version}", { name: form.name, version: form.version }) : t("{name}, Version {version} (nicht bestätigt)", { name: form.name, version: form.version })}</span>
          </>
        }
        help={<HelpButton reviewId={reviewId} station={stage.station} context={stage.name} />}
      >
        {!stage.blind_active && <button onClick={() => exportFile("table")}><Icon name="csv" size={14} /> {t("Tabelle")}</button>}
        {!stage.blind_active && frequencies.length > 0 && <button onClick={() => exportFile("frequencies")}><Icon name="csv" size={14} /> {t("Häufigkeiten")}</button>}
        {!stage.blind_active && <button onClick={() => exportFile("json")} title={t("Formular, Werte je Quelle und Häufigkeiten in einer Datei")}><Icon name="csv" size={14} /> JSON</button>}
        {manage && stage.blind_active && <button onClick={unblind}><Icon name="eye" size={14} /> {t("Entblinden")}</button>}
        {manage && !stage.closed && <button onClick={closeStage}><Icon name="lock" size={14} /> {t("Schließen")}</button>}
        {canVote(view.role) && !stage.closed && mineOpen && (
          <button className="primary" onClick={() => navigate(entry(mineOpen.record_id))}><Icon name="play" size={14} /> {t("Extrahieren")}</button>
        )}
      </PageHeader>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      {notice && <Banner kind="notice" onClose={() => setNotice("")}>{notice}</Banner>}
      {view.todo > 0 && (
        <Banner kind="notice">
          {t("Das Formular hat neue Pflichtfelder. Bei {n} fertigen {sources} ist etwas nachzutragen.", { n: view.todo, sources })}
        </Banner>
      )}

      <div className="stats">
        <Stat label={t("Dein Fortschritt")} value={`${view.mine_done} / ${rows.length}`} detail={view.todo > 0 ? t("{n} nachzutragen", { n: view.todo }) : t("fertig gemeldet")} />
        {counts ? (
          <>
            <Stat label={t("Abgeglichen")} value={`${counts.done} / ${rows.length}`} detail={[counts.open && t("{n} offen", { n: counts.open }), counts.extracted && t("{n} zu prüfen", { n: counts.extracted }), counts.incomplete && t("{n} unvollständig", { n: counts.incomplete })].filter(Boolean).join(" · ") || t("alle fertig")} />
            <Stat label={t("Abweichungen")} value={counts.conflict} tone={counts.conflict > 0 ? "exc" : undefined} detail={t("Quellen mit ungeklärtem Feld")} />
          </>
        ) : (
          <Stat label={t("Abgleich")} value={<Icon name="lock" size={20} />} detail={t("sichtbar nach dem Entblinden")} />
        )}
        {view.times.length > 0 && (
          <Stat
            label={t("Zeit je Quelle")}
            value={minutes(Math.round(view.times.reduce((sum, x) => sum + x.seconds * x.n, 0) / view.times.reduce((sum, x) => sum + x.n, 0)))}
            detail={view.times.map((x) => `${typeLabel(view.source_types, x.source_type) || t("ohne Quellenart")} ${minutes(x.seconds)}`).join(" · ")}
          />
        )}
      </div>

      <Group title={sources} footer={stage.blind_active ? t("Verblindet: Die Tabelle zeigt nur deine eigenen Werte.") : t("Gültige Werte nach dem Abgleich. Rot markierte Felder weichen ab und sind zu klären.")}>
        <div className="form-table-wrap">
          <table className="table form-table">
            <thead>
              <tr>
                <th>{t("Quelle")}</th>
                <th>{t("Dein Stand")}</th>
                {!stage.blind_active && <th>{t("Abgleich")}</th>}
                {form.fields.map((f) => <th key={f.key}>{f.label}</th>)}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.record_id}>
                  <td className="form-source">
                    <Link to={entry(row.record_id)}>{row.title || t("(ohne Titel)")}</Link>
                    <div className="muted small-text">
                      {[row.authors.split(";")[0], row.year, row.label, typeLabel(view.source_types, row.source_type), row.n_records > 1 ? t("{n} Einträge", { n: row.n_records }) : null].filter(Boolean).join(" · ")}
                    </div>
                  </td>
                  <td><Pill tone={MINE_TONE[row.mine]}>{MINE_LABEL[row.mine]}</Pill></td>
                  {row.status && <td><Pill tone={STATUS_TONE[row.status]}>{STATUS_LABEL[row.status]}</Pill>{row.n_completed !== null && <span className="muted small-text"> {row.n_completed}×</span>}</td>}
                  {row.cells.map((cell) => (
                    <td key={cell.field} className={`form-cell c-${cell.status}`} title={cell.status === "differs" ? cell.values.map((g) => `${g.name}: ${showValue(g.value) || t("leer")}`).join("\n") : undefined}>
                      {cell.status === "differs" ? t("weicht ab") : showValue(cell.value)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Group>

      {frequencies.map((f) => {
        const max = Math.max(1, ...f.counts.map((c) => c.n));
        return (
          <Group key={f.field} title={t("Häufigkeiten: {label}", { label: f.label })} footer={[f.n_empty > 0 && t("{n} ohne Angabe", { n: f.n_empty }), f.n_open > 0 && t("{n} mit offener Abweichung", { n: f.n_open })].filter(Boolean).join(" · ") || undefined}>
            {f.counts.map((c) => (
              <Row key={c.option}>
                <span>{c.option}</span>
                <span className="meter-cell">
                  <span className="meter"><span style={{ width: `${(c.n / max) * 100}%` }} /></span>
                  <span className="num">{c.n}</span>
                </span>
              </Row>
            ))}
          </Group>
        );
      })}
    </>
  );
}

// ---------------------------------------------------------------- Eingabe

/** Eingabe eines Feldes. Geschrieben wird beim Verlassen des Feldes, bei Auswahlen sofort. */
function FieldInput({ field, value, disabled, onCommit }: {
  field: FormField;
  value: FormValue;
  disabled: boolean;
  onCommit: (value: FormValue) => void;
}) {
  const [text, setText] = useState(showValue(value));
  useEffect(() => setText(showValue(value)), [value]);
  const id = `field-${field.key}`;
  const options = field.options ?? [];
  switch (field.type) {
    case "choice":
      return (
        <select id={id} value={typeof value === "string" ? value : ""} disabled={disabled} onChange={(e) => onCommit(e.target.value || null)}>
          <option value="">–</option>
          {typeof value === "string" && !options.includes(value) && <option value={value}>{value}</option>}
          {options.map((o) => <option key={o} value={o}>{o}</option>)}
        </select>
      );
    case "multi_choice": {
      const chosen = Array.isArray(value) ? value : [];
      return (
        <div className="check-list" role="group" aria-labelledby={`${id}-label`}>
          {options.map((o) => (
            <label key={o} className="check">
              <input
                type="checkbox"
                checked={chosen.includes(o)}
                disabled={disabled}
                onChange={(e) => onCommit(e.target.checked ? [...chosen, o] : chosen.filter((c) => c !== o))}
              />{" "}
              {o}
            </label>
          ))}
        </div>
      );
    }
    case "integer":
      return (
        <input
          id={id}
          className="narrow-input"
          type="number"
          step={1}
          value={text}
          disabled={disabled}
          onChange={(e) => setText(e.target.value)}
          onBlur={() => onCommit(text.trim() === "" ? null : Math.trunc(Number(text)))}
        />
      );
    case "decimal":
      return <input id={id} className="narrow-input" inputMode="decimal" placeholder={t("z. B. 12,5")} value={text} disabled={disabled} onChange={(e) => setText(e.target.value)} onBlur={() => onCommit(text.trim() || null)} />;
    default:
      return <textarea id={id} rows={field.type === "quote" ? 3 : 2} placeholder={field.type === "quote" ? t("Wörtliche Stelle mit Seitenangabe") : undefined} value={text} disabled={disabled} onChange={(e) => setText(e.target.value)} onBlur={() => onCommit(text.trim() || null)} />;
  }
}

/** Abgleich eines Feldes nach dem Entblinden: Werte der anderen, Stand und die Klärung. */
function CellCompare({ field, cell, mayResolve, onResolve }: {
  field: FormField;
  cell: FormCell;
  mayResolve: boolean;
  onResolve: (value: FormValue) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<FormValue>(cell.value);
  return (
    <div className={`cell-compare c-${cell.status}`}>
      {cell.status === "differs" && <p className="cell-line"><Icon name="flag" size={11} /> {t("Die Werte weichen ab.")}</p>}
      {cell.status === "consensus" && <p className="cell-line"><Icon name="check" size={11} /> {t("Geklärt: {value}", { value: showValue(cell.value) || t("leer") })}</p>}
      {(cell.status === "differs" || cell.status === "consensus") && cell.values.map((g) => (
        <p className="cell-line" key={g.actor_id}>
          <span><strong>{g.name}:</strong> {showValue(g.value) || t("leer")}</span>
          {mayResolve && cell.status === "differs" && <button className="link" onClick={() => onResolve(g.value)}>{t("übernehmen")}</button>}
        </p>
      ))}
      {mayResolve && !editing && (
        <button className="link" onClick={() => { setDraft(cell.value); setEditing(true); }}>
          {cell.status === "differs" ? t("Anderen Wert festlegen …") : t("Gültigen Wert ändern …")}
        </button>
      )}
      {editing && (
        <div className="cell-resolve">
          <FieldInput field={{ ...field, key: `${field.key}-resolve` }} value={draft} disabled={false} onCommit={setDraft} />
          <span className="inline-actions">
            <button onClick={() => setEditing(false)}>{t("Abbrechen")}</button>
            <button className="primary" onClick={() => { onResolve(draft); setEditing(false); }}>{t("Als gültigen Wert festlegen")}</button>
          </span>
        </div>
      )}
    </div>
  );
}

export function FormEntryPage() {
  const { reviewId = "", stageId = "", recordId = "" } = useParams();
  const navigate = useNavigate();
  const [entry, setEntry] = useState<FormEntryView | null>(null);
  const [values, setValues] = useState<Record<string, FormValue>>({});
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [reading, setReading] = useState("abstract");
  const [focus, setFocus] = useFocusMode();
  const settings = useSettings();
  const [ai, setAi] = useState<ExtractionState | null>(null);
  const [busy, setBusy] = useState(false);
  const started = useRef(Date.now());
  // Die Quelle, die gerade offen ist: Eine Antwort der KI zu einer verlassenen Quelle öffnet keinen Dialog.
  const shown = useRef(recordId);
  shown.current = recordId;

  const load = useCallback(async () => {
    try {
      const loaded = await api.formEntry(reviewId, stageId, recordId);
      setEntry(loaded);
      setValues(loaded.values);
      return loaded;
    } catch (e) {
      setError(errorText(e));
      return null;
    }
  }, [reviewId, stageId, recordId]);

  useEffect(() => {
    setEntry(null);
    setError("");
    setNotice("");
    setAi(null);
    started.current = Date.now();
    load().then((loaded) => setReading(loaded?.fulltexts[0] ?? "abstract"));
  }, [load]);

  const loadFulltext = useCallback(() => api.readFulltext(reviewId, reading), [reviewId, reading]);
  const mayMark = entry !== null && canVote(entry.role) && !entry.stage.closed;
  const marks = useHighlights(reviewId, reading === "abstract" ? null : reading, mayMark ? stageId : null, setError);

  if (!entry) return error ? <Banner kind="error">{error}</Banner> : null;
  const { stage, form, record, mode, compared } = entry;
  const source = term(entry.terms, "source", t("Quelle"));
  const all = [record, ...entry.companions];
  const base = `/reviews/${reviewId}/forms/${stageId}`;
  const missing = form.fields.filter((f) => f.required && (values[f.key] ?? null) === null);
  const mayResolve = compared !== null && canVote(entry.role) && !stage.closed;

  async function commit(field: FormField, value: FormValue) {
    setError("");
    try {
      if (await api.formValue(reviewId, stageId, recordId, field.key, value)) setValues((old) => ({ ...old, [field.key]: value }));
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function resolve(field: FormField, value: FormValue) {
    setError("");
    try {
      await api.formConsensus(reviewId, stageId, recordId, field.key, value, "");
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  }

  /** Liest den Text des Volltexts und holt dazu einen Vorschlag der KI für die Felder des Formulars. */
  async function askAi() {
    const fulltext = reading !== "abstract" ? reading : entry!.fulltexts[0];
    if (!fulltext) return;
    setError("");
    setAi({ step: "loading", recordId });
    try {
      const pages = await pdfPages(await api.readFulltext(reviewId, fulltext));
      const suggestion = await api.aiExtract(reviewId, stageId, recordId, fulltext, pages);
      if (shown.current !== recordId) return;
      setAi((now) => (now ? { step: "done", recordId, suggestion } : now));
      await load();
    } catch (e) {
      if (shown.current === recordId) setAi((now) => (now ? { step: "error", recordId, text: errorText(e) } : now));
    }
  }

  async function applyAi(suggestion: Extraction, keys: string[]) {
    setBusy(true);
    setError("");
    try {
      const changed = await api.aiExtractApply(reviewId, stageId, recordId, suggestion.id, keys);
      setAi(null);
      setNotice(keys.length === 0 ? t("KI-Vorschlag verworfen. Eingetragen wurde nichts.") : tn("{n} Wert aus dem KI-Vorschlag eingetragen. Prüfe ihn am Volltext.", "{n} Werte aus dem KI-Vorschlag eingetragen. Prüfe sie am Volltext.", changed));
      await load();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  async function complete() {
    setError("");
    try {
      // Die Zeit zählt nur beim ersten Abschluss; späteres Nachtragen würde sie verfälschen.
      const seconds = entry!.completed ? null : Math.round((Date.now() - started.current) / 1000);
      await api.completeEntry(reviewId, stageId, recordId, seconds);
      if (entry!.next_open) navigate(`${base}/${entry!.next_open}`);
      else {
        setNotice(t("Fertig gemeldet. Das war die letzte offene {source}; speichere im Review unter „Austausch“ eine Rückmeldung.", { source }));
        await load();
      }
    } catch (e) {
      setError(errorText(e));
    }
  }

  const aiOn = settings.ai.enabled && entry.ai.allowed && mode === "extract" && entry.fulltexts.length > 0;
  const tabs = [{ value: "abstract", label: t("Titel und Abstract") }, ...entry.fulltexts.map((id, i) => ({ value: id, label: entry.fulltexts.length > 1 ? t("Volltext {n}", { n: i + 1 }) : t("Volltext") }))];
  let group = "";

  return (
    <div className="screening form-entry">
      <div className="screening-top">
        <Link to={base} className="back-link"><Icon name="back" size={14} /> {stage.name}</Link>
        <div className="screening-title">
          <strong>{t("{source} {position} von {total}", { source, position: entry.position, total: entry.total })}</strong>
          {stage.blind_active && <Pill tone="violet"><Icon name="lock" size={11} /> {t("verblindet")}</Pill>}
          {mode === "verify" && <Pill tone="blue">{t("Prüfung")}</Pill>}
          {compared && <Pill tone={STATUS_TONE[compared.status]}>{STATUS_LABEL[compared.status]}</Pill>}
        </div>
        <span className="screening-tools">
          <FocusButton on={focus} onChange={setFocus} />
          <button className="ghost" disabled={!entry.previous} onClick={() => navigate(`${base}/${entry.previous}`)}>{t("Zurück")}</button>
          <button className="ghost" disabled={!entry.next} onClick={() => navigate(`${base}/${entry.next}`)}>{t("Weiter")}</button>
          <span className="focus-hide"><HelpButton reviewId={reviewId} station={stage.station} context={stage.name} /></span>
        </span>
      </div>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      {notice && <Banner kind="notice" onClose={() => setNotice("")}>{notice}</Banner>}
      {ai && ai.recordId === recordId && (
        <ExtractionDialog state={ai} model={settings.ai.model} fields={form.fields} values={values} busy={busy} onApply={applyAi} onClose={() => setAi(null)} onRetry={askAi} />
      )}

      <div className="screening-columns">
        <section className="screening-main">
          {tabs.length > 1 && (
            <div className="reading-switch">
              <Segmented label={t("Ansicht")} value={reading} onChange={setReading} options={tabs} />
              <span className="reading-title" title={record.title}>{record.title}</span>
            </div>
          )}
          {reading !== "abstract" ? (
            <PdfView load={loadFulltext} marks={marks} onExternal={() => api.openFulltext(reviewId, reading).catch((e) => setError(errorText(e)))} />
          ) : (
            <div className="reading">
              {all.map((r, i) => (
                <article className={i === 0 ? "record record-large" : "record"} key={r.id}>
                  {i > 0 && <p className="muted small-text">{t("Weiterer Eintrag dieser {source}", { source })}</p>}
                  <h2>{r.title || t("(ohne Titel)")}</h2>
                  <p className="record-meta">{[r.authors, r.journal, r.year].filter(Boolean).join(" · ")}</p>
                  {r.abstract ? <p className="abstract">{r.abstract}</p> : <p className="abstract missing">{t("Kein Abstract vorhanden.")}</p>}
                </article>
              ))}
              {entry.fulltexts.length === 0 && <p className="reading-note">{t("Zu dieser {source} ist kein Volltext beschafft.", { source })}</p>}
            </div>
          )}
        </section>

        <aside className="screening-criteria form-panel" aria-label={t("Formular")}>
          <h2>{form.name} <small>{t("Version {version}", { version: form.version })}</small></h2>
          <p className="muted small-text">
            {[entry.label, typeLabel(entry.source_types, entry.source_type)].filter(Boolean).join(" · ")}
          </p>
          {aiOn && (
            <div className="form-ai">
              <button className="small" onClick={askAi} title={t("Die KI liest den Volltext und schlägt Werte für die Felder vor. Eingetragen wird erst, was du übernimmst.")}>
                <Icon name="spark" size={13} /> {entry.ai.suggestion ? t("Neuen KI-Vorschlag holen") : t("KI-Vorschlag aus dem Volltext")}
              </button>
              {entry.ai.suggestion && <ExtractionLine suggestion={entry.ai.suggestion} onShow={() => setAi({ step: "done", recordId, suggestion: entry.ai.suggestion! })} />}
            </div>
          )}
          {mode === "verify" && <p className="reading-note">{t("Diese {source} hat jemand anderes extrahiert. Prüfe die Werte am Volltext, lege Korrekturen als gültigen Wert fest und schließe die Prüfung ab.", { source })}</p>}
          {entry.completed && mode === "extract" && missing.length > 0 && <p className="reading-note">{t("Nachzutragen: {fields}", { fields: missing.map((f) => f.label).join(", ") })}</p>}
          {form.fields.map((field) => {
            const heading = field.group !== group ? field.group : "";
            group = field.group;
            const cell = compared?.cells.find((c) => c.field === field.key);
            const shown = mode === "verify" ? (cell?.value ?? null) : (values[field.key] ?? null);
            return (
              <React.Fragment key={field.key}>
                {heading && <h3 className="form-group-title">{heading}</h3>}
                <div className="form-field">
                  <label htmlFor={`field-${field.key}`} id={`field-${field.key}-label`}>
                    {field.label}{field.required && <span className="required" title={t("Pflichtfeld")}> *</span>}
                  </label>
                  {field.description && <p className="muted small-text">{field.description}</p>}
                  <FieldInput field={field} value={shown} disabled={mode !== "extract"} onCommit={(value) => commit(field, value)} />
                  {cell && (cell.status === "differs" || cell.status === "consensus" || (mayResolve && mode === "verify")) && (
                    <CellCompare field={field} cell={cell} mayResolve={mayResolve} onResolve={(value) => resolve(field, value)} />
                  )}
                </div>
              </React.Fragment>
            );
          })}
          {mode !== "read" && (
            <div className="form-actions">
              {mode === "extract" && missing.length > 0 && <p className="muted small-text">{t("Es fehlen: {fields}", { fields: missing.map((f) => f.label).join(", ") })}</p>}
              {entry.completed && <p className="muted small-text"><Icon name="check" size={11} /> {entry.completed.verify ? t("Prüfung abgeschlossen. Änderungen werden weiter gespeichert.") : t("Fertig gemeldet. Änderungen werden weiter gespeichert.")}</p>}
              <button className="primary" disabled={mode === "extract" && missing.length > 0} onClick={complete}>
                {entry.next_open
                  ? (mode === "verify" ? t("Prüfung abschließen und weiter") : entry.completed ? t("Erneut fertig melden und weiter") : t("Fertig melden und weiter"))
                  : (mode === "verify" ? t("Prüfung abschließen") : entry.completed ? t("Erneut fertig melden") : t("Fertig melden"))}
              </button>
            </div>
          )}
          {mode === "read" && <Empty icon="lock" title={stage.closed ? t("Die Stufe ist geschlossen") : t("Nur Ansicht")} />}
        </aside>
      </div>
    </div>
  );
}
