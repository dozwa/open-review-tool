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
import { PdfView } from "./PdfView";
import { stageState } from "./ReviewPage";
import { Banner, Empty, Group, Icon, PageHeader, Pill, Row, Segmented, Stat } from "./ui";

const MINE_LABEL: Record<MineStatus, string> = {
  open: "offen",
  draft: "angefangen",
  completed: "fertig",
  todo: "nachzutragen",
  to_verify: "zu prüfen",
  locked: "extrahiert jemand anderes",
};
const MINE_TONE: Record<MineStatus, string | undefined> = { open: undefined, draft: "yellow", completed: "green", todo: "red", to_verify: "blue", locked: undefined };
const STATUS_LABEL: Record<EntryStatus, string> = {
  open: "wartet auf Extraktion",
  extracted: "wartet auf Prüfung",
  conflict: "Abweichung",
  incomplete: "Pflichtfeld leer",
  done: "abgeglichen",
};
const STATUS_TONE: Record<EntryStatus, string | undefined> = { open: undefined, extracted: "blue", conflict: "red", incomplete: "yellow", done: "green" };
const MODE_LABEL: Record<string, string> = { unanimous: "doppelt", single_verified: "einfach mit Prüfung" };

const typeLabel = (types: SchemaElement[], key: string) => types.find((t) => t.key === key)?.label ?? key;

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
  const sources = term(view.terms, "sources", "Quellen");
  const mineOpen = rows.find((r) => r.mine !== "completed" && r.mine !== "locked");
  const entry = (recordId: string) => `/reviews/${reviewId}/forms/${stageId}/${recordId}`;

  async function unblind() {
    const ok = await ask("Danach sehen alle, die ein neues Paket bekommen, die Werte der anderen. Das lässt sich nicht zurücknehmen.", {
      title: "Stufe entblinden?", kind: "warning", okLabel: "Entblinden", cancelLabel: "Abbrechen",
    });
    if (ok) await run(() => api.unblind(reviewId, stageId), "Stufe entblindet. Schick den Mitgliedern ein neues Paket.");
  }

  async function closeStage() {
    const gate = await api.gate(reviewId, stageId).catch(() => null);
    let text = "Danach sind keine Werte mehr möglich, auch keine nachgereichten. Der Stand wird als Snapshot festgehalten.";
    if (gate) text += gate.passed ? `\n\nDas Gate der Station ist erfüllt: ${gateText(gate)}.` : `\n\nDas Gate der Station ist nicht erfüllt: ${gateText(gate)}.`;
    else if (stage.blind_active) text += "\n\nDie Stufe ist noch verblindet. Abweichungen lassen sich erst nach dem Entblinden klären.";
    const ok = await ask(text, { title: "Stufe schließen?", kind: "warning", okLabel: "Schließen", cancelLabel: "Abbrechen" });
    if (ok) await run(() => api.close(reviewId, stageId), "Stufe geschlossen, Snapshot erstellt.");
  }

  async function exportFile(kind: "table" | "frequencies") {
    const name = kind === "table" ? "extraktion-tabelle.csv" : "extraktion-haeufigkeiten.csv";
    const path = await save({ defaultPath: name, filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (path) await run(() => api.exportForm(reviewId, stageId, kind, path), "CSV gespeichert.");
  }

  return (
    <>
      <PageHeader
        back={<Link to={`/reviews/${reviewId}`} className="back-link"><Icon name="back" size={14} /> Review</Link>}
        title={stage.name}
        subtitle={
          <>
            <Pill>{stageState(stage)}</Pill>
            <span>{rows.length} {sources}</span>
            <span>{MODE_LABEL[stage.decision_rule] ?? stage.decision_rule}</span>
            {stage.decision_rule === "unanimous" && <span>mind. {stage.min_reviewers} Personen</span>}
            {stage.seed !== null && <span>Stichprobe je Quellenart, Seed {stage.seed}</span>}
            <span>{form.name}, Version {form.version}{form.confirmed ? "" : " (nicht bestätigt)"}</span>
          </>
        }
      >
        {!stage.blind_active && <button onClick={() => exportFile("table")}><Icon name="csv" size={14} /> Tabelle</button>}
        {!stage.blind_active && frequencies.length > 0 && <button onClick={() => exportFile("frequencies")}><Icon name="csv" size={14} /> Häufigkeiten</button>}
        {manage && stage.blind_active && <button onClick={unblind}><Icon name="eye" size={14} /> Entblinden</button>}
        {manage && !stage.closed && <button onClick={closeStage}><Icon name="lock" size={14} /> Schließen</button>}
        {canVote(view.role) && !stage.closed && mineOpen && (
          <button className="primary" onClick={() => navigate(entry(mineOpen.record_id))}><Icon name="play" size={14} /> Extrahieren</button>
        )}
      </PageHeader>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      {notice && <Banner kind="notice" onClose={() => setNotice("")}>{notice}</Banner>}
      {view.todo > 0 && (
        <Banner kind="notice">
          Das Formular hat neue Pflichtfelder. Bei {view.todo} fertigen {sources} ist etwas nachzutragen.
        </Banner>
      )}

      <div className="stats">
        <Stat label="Dein Fortschritt" value={`${view.mine_done} / ${rows.length}`} detail={view.todo > 0 ? `${view.todo} nachzutragen` : "fertig gemeldet"} />
        {counts ? (
          <>
            <Stat label="Abgeglichen" value={`${counts.done} / ${rows.length}`} detail={[counts.open && `${counts.open} offen`, counts.extracted && `${counts.extracted} zu prüfen`, counts.incomplete && `${counts.incomplete} unvollständig`].filter(Boolean).join(" · ") || "alle fertig"} />
            <Stat label="Abweichungen" value={counts.conflict} tone={counts.conflict > 0 ? "exc" : undefined} detail="Quellen mit ungeklärtem Feld" />
          </>
        ) : (
          <Stat label="Abgleich" value={<Icon name="lock" size={20} />} detail="sichtbar nach dem Entblinden" />
        )}
        {view.times.length > 0 && (
          <Stat
            label="Zeit je Quelle"
            value={minutes(Math.round(view.times.reduce((sum, t) => sum + t.seconds * t.n, 0) / view.times.reduce((sum, t) => sum + t.n, 0)))}
            detail={view.times.map((t) => `${typeLabel(view.source_types, t.source_type) || "ohne Quellenart"} ${minutes(t.seconds)}`).join(" · ")}
          />
        )}
      </div>

      <Group title={sources} footer={stage.blind_active ? "Verblindet: Die Tabelle zeigt nur deine eigenen Werte." : "Gültige Werte nach dem Abgleich. Rot markierte Felder weichen ab und sind zu klären."}>
        <div className="form-table-wrap">
          <table className="table form-table">
            <thead>
              <tr>
                <th>Quelle</th>
                <th>Dein Stand</th>
                {!stage.blind_active && <th>Abgleich</th>}
                {form.fields.map((f) => <th key={f.key}>{f.label}</th>)}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.record_id}>
                  <td className="form-source">
                    <Link to={entry(row.record_id)}>{row.title || "(ohne Titel)"}</Link>
                    <div className="muted small-text">
                      {[row.authors.split(";")[0], row.year, row.label, typeLabel(view.source_types, row.source_type), row.n_records > 1 ? `${row.n_records} Einträge` : null].filter(Boolean).join(" · ")}
                    </div>
                  </td>
                  <td><Pill tone={MINE_TONE[row.mine]}>{MINE_LABEL[row.mine]}</Pill></td>
                  {row.status && <td><Pill tone={STATUS_TONE[row.status]}>{STATUS_LABEL[row.status]}</Pill>{row.n_completed !== null && <span className="muted small-text"> {row.n_completed}×</span>}</td>}
                  {row.cells.map((cell) => (
                    <td key={cell.field} className={`form-cell c-${cell.status}`} title={cell.status === "differs" ? cell.values.map((g) => `${g.name}: ${showValue(g.value) || "leer"}`).join("\n") : undefined}>
                      {cell.status === "differs" ? "weicht ab" : showValue(cell.value)}
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
          <Group key={f.field} title={`Häufigkeiten: ${f.label}`} footer={[f.n_empty > 0 && `${f.n_empty} ohne Angabe`, f.n_open > 0 && `${f.n_open} mit offener Abweichung`].filter(Boolean).join(" · ") || undefined}>
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
      return <input id={id} className="narrow-input" inputMode="decimal" placeholder="z. B. 12,5" value={text} disabled={disabled} onChange={(e) => setText(e.target.value)} onBlur={() => onCommit(text.trim() || null)} />;
    default:
      return <textarea id={id} rows={field.type === "quote" ? 3 : 2} placeholder={field.type === "quote" ? "Wörtliche Stelle mit Seitenangabe" : undefined} value={text} disabled={disabled} onChange={(e) => setText(e.target.value)} onBlur={() => onCommit(text.trim() || null)} />;
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
      {cell.status === "differs" && <p className="cell-line"><Icon name="flag" size={11} /> Die Werte weichen ab.</p>}
      {cell.status === "consensus" && <p className="cell-line"><Icon name="check" size={11} /> Geklärt: {showValue(cell.value) || "leer"}</p>}
      {(cell.status === "differs" || cell.status === "consensus") && cell.values.map((g) => (
        <p className="cell-line" key={g.actor_id}>
          <span><strong>{g.name}:</strong> {showValue(g.value) || "leer"}</span>
          {mayResolve && cell.status === "differs" && <button className="link" onClick={() => onResolve(g.value)}>übernehmen</button>}
        </p>
      ))}
      {mayResolve && !editing && (
        <button className="link" onClick={() => { setDraft(cell.value); setEditing(true); }}>
          {cell.status === "differs" ? "Anderen Wert festlegen …" : "Gültigen Wert ändern …"}
        </button>
      )}
      {editing && (
        <div className="cell-resolve">
          <FieldInput field={{ ...field, key: `${field.key}-resolve` }} value={draft} disabled={false} onCommit={setDraft} />
          <span className="inline-actions">
            <button onClick={() => setEditing(false)}>Abbrechen</button>
            <button className="primary" onClick={() => { onResolve(draft); setEditing(false); }}>Als gültigen Wert festlegen</button>
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
  const started = useRef(Date.now());

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
    started.current = Date.now();
    load().then((loaded) => setReading(loaded?.fulltexts[0] ?? "abstract"));
  }, [load]);

  const loadFulltext = useCallback(() => api.readFulltext(reviewId, reading), [reviewId, reading]);

  if (!entry) return error ? <Banner kind="error">{error}</Banner> : null;
  const { stage, form, record, mode, compared } = entry;
  const source = term(entry.terms, "source", "Quelle");
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

  async function complete() {
    setError("");
    try {
      // Die Zeit zählt nur beim ersten Abschluss; späteres Nachtragen würde sie verfälschen.
      const seconds = entry!.completed ? null : Math.round((Date.now() - started.current) / 1000);
      await api.completeEntry(reviewId, stageId, recordId, seconds);
      if (entry!.next_open) navigate(`${base}/${entry!.next_open}`);
      else {
        setNotice(`Fertig gemeldet. Das war die letzte offene ${source}; speichere im Review unter „Austausch“ eine Rückmeldung.`);
        await load();
      }
    } catch (e) {
      setError(errorText(e));
    }
  }

  const tabs = [{ value: "abstract", label: "Titel und Abstract" }, ...entry.fulltexts.map((id, i) => ({ value: id, label: entry.fulltexts.length > 1 ? `Volltext ${i + 1}` : "Volltext" }))];
  let group = "";

  return (
    <div className="screening form-entry">
      <div className="screening-top">
        <Link to={base} className="back-link"><Icon name="back" size={14} /> {stage.name}</Link>
        <div className="screening-title">
          <strong>{source} {entry.position} von {entry.total}</strong>
          {stage.blind_active && <Pill tone="violet"><Icon name="lock" size={11} /> verblindet</Pill>}
          {mode === "verify" && <Pill tone="blue">Prüfung</Pill>}
          {compared && <Pill tone={STATUS_TONE[compared.status]}>{STATUS_LABEL[compared.status]}</Pill>}
        </div>
        <span className="screening-tools">
          <button className="ghost" disabled={!entry.previous} onClick={() => navigate(`${base}/${entry.previous}`)}>Zurück</button>
          <button className="ghost" disabled={!entry.next} onClick={() => navigate(`${base}/${entry.next}`)}>Weiter</button>
        </span>
      </div>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      {notice && <Banner kind="notice" onClose={() => setNotice("")}>{notice}</Banner>}

      <div className="screening-columns">
        <section className="screening-main">
          {tabs.length > 1 && (
            <div className="reading-switch">
              <Segmented label="Ansicht" value={reading} onChange={setReading} options={tabs} />
              <span className="reading-title" title={record.title}>{record.title}</span>
            </div>
          )}
          {reading !== "abstract" ? (
            <PdfView load={loadFulltext} onExternal={() => api.openFulltext(reviewId, reading).catch((e) => setError(errorText(e)))} />
          ) : (
            <div className="reading">
              {all.map((r, i) => (
                <article className={i === 0 ? "record record-large" : "record"} key={r.id}>
                  {i > 0 && <p className="muted small-text">Weiterer Eintrag dieser {source}</p>}
                  <h2>{r.title || "(ohne Titel)"}</h2>
                  <p className="record-meta">{[r.authors, r.journal, r.year].filter(Boolean).join(" · ")}</p>
                  {r.abstract ? <p className="abstract">{r.abstract}</p> : <p className="abstract missing">Kein Abstract vorhanden.</p>}
                </article>
              ))}
              {entry.fulltexts.length === 0 && <p className="reading-note">Zu dieser {source} ist kein Volltext beschafft.</p>}
            </div>
          )}
        </section>

        <aside className="screening-criteria form-panel" aria-label="Formular">
          <h2>{form.name} <small>Version {form.version}</small></h2>
          <p className="muted small-text">
            {[entry.label, typeLabel(entry.source_types, entry.source_type)].filter(Boolean).join(" · ")}
          </p>
          {mode === "verify" && <p className="reading-note">Diese {source} hat jemand anderes extrahiert. Prüfe die Werte am Volltext, lege Korrekturen als gültigen Wert fest und schließe die Prüfung ab.</p>}
          {entry.completed && mode === "extract" && missing.length > 0 && <p className="reading-note">Nachzutragen: {missing.map((f) => f.label).join(", ")}</p>}
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
                    {field.label}{field.required && <span className="required" title="Pflichtfeld"> *</span>}
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
              {mode === "extract" && missing.length > 0 && <p className="muted small-text">Es fehlen: {missing.map((f) => f.label).join(", ")}</p>}
              {entry.completed && <p className="muted small-text"><Icon name="check" size={11} /> {entry.completed.verify ? "Prüfung abgeschlossen" : "Fertig gemeldet"}. Änderungen werden weiter gespeichert.</p>}
              <button className="primary" disabled={mode === "extract" && missing.length > 0} onClick={complete}>
                {mode === "verify" ? "Prüfung abschließen" : entry.completed ? "Erneut fertig melden" : "Fertig melden"}{entry.next_open ? " und weiter" : ""}
              </button>
            </div>
          )}
          {mode === "read" && <Empty icon="lock" title={stage.closed ? "Die Stufe ist geschlossen" : "Nur Ansicht"} />}
        </aside>
      </div>
    </div>
  );
}
