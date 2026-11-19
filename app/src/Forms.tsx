// Formular: Felder für das Extrahieren ansehen, bearbeiten, veröffentlichen und bestätigen.
// Versionen, Begründungen und die Regel „ein Feld behält seinen Typ“ prüft der Kern.

import React, { useEffect, useState } from "react";

import { api, canManage, errorText, FIELD_TYPE_LABEL, FieldType, Form, FormField, FormsView, ReviewDetail } from "./api";
import { formatDate, t, tn } from "./i18n";
import { Banner, Empty, Group, Icon, Pill, Row } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

/** Feld im Editor. `fixed` heißt: schon veröffentlicht, Schlüssel und Typ bleiben. */
interface Draft extends FormField {
  fixed: boolean;
  optionsText: string;
}

const latest = (form: Form) => form.versions[form.versions.length - 1];
const hasOptions = (type: FieldType) => type === "choice" || type === "multi_choice";

/** Schlüssel aus dem Namen: Kleinbuchstaben, Ziffern, Unterstriche. */
function keyFrom(label: string, taken: string[]): string {
  const base =
    label
      .toLowerCase()
      .replace(/ä/g, "ae").replace(/ö/g, "oe").replace(/ü/g, "ue").replace(/ß/g, "ss")
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "")
      .slice(0, 36) || "feld";
  let key = base;
  for (let n = 2; taken.includes(key); n++) key = `${base}_${n}`;
  return key;
}

const toDraft = (field: FormField, fixed: boolean): Draft => ({ ...field, fixed, optionsText: (field.options ?? []).join("\n") });

function FormEditor({ name: startName, fields: startFields, form, purpose, onCancel, onSave }: {
  name: string;
  fields: Draft[];
  form: Form | null;
  purpose: string;
  onCancel: () => void;
  onSave: (data: Record<string, unknown>) => void;
}) {
  const [name, setName] = useState(startName);
  const [fields, setFields] = useState<Draft[]>(startFields);
  const [note, setNote] = useState("");

  const change = (index: number, patch: Partial<Draft>) => setFields(fields.map((f, i) => (i === index ? { ...f, ...patch } : f)));
  const move = (index: number, by: number) => {
    const next = [...fields];
    const [field] = next.splice(index, 1);
    next.splice(index + by, 0, field);
    setFields(next);
  };
  const add = () =>
    setFields([...fields, { key: "", label: "", description: "", type: "text", required: false, group: fields[fields.length - 1]?.group ?? "", analysis: "", fixed: false, optionsText: "" }]);

  function submit(event: React.FormEvent) {
    event.preventDefault();
    const taken = fields.filter((f) => f.fixed).map((f) => f.key);
    const out = fields.map((f) => {
      const key = f.fixed ? f.key : keyFrom(f.label, taken);
      taken.push(key);
      const options = hasOptions(f.type) ? f.optionsText.split("\n").map((o) => o.trim()).filter(Boolean) : [];
      return { key, label: f.label, description: f.description, type: f.type, required: f.required, group: f.group, analysis: f.analysis, options };
    });
    onSave({ form_id: form?.id, purpose, name, fields: out, change_note: note });
  }

  return (
    <form className="sheet form-editor" onSubmit={submit}>
      <h2>{form ? t("Formular bearbeiten: Version {version}", { version: latest(form).version + 1 }) : t("Neues Formular")}</h2>
      <Group>
        <Row>
          <label htmlFor="form-name">{t("Name")}</label>
          <input id="form-name" value={name} onChange={(e) => setName(e.target.value)} required />
        </Row>
      </Group>
      {fields.map((f, i) => (
        <fieldset className="field-draft" key={i}>
          <legend>{t("Feld {n}", { n: i + 1 })}{f.fixed && <small> · {t("Schlüssel {key}", { key: f.key })}</small>}</legend>
          <div className="field-draft-grid">
            <label>
              {t("Name")}
              <input value={f.label} onChange={(e) => change(i, { label: e.target.value })} required />
            </label>
            <label>
              {t("Typ")}
              <select value={f.type} onChange={(e) => change(i, { type: e.target.value as FieldType })} disabled={f.fixed} title={f.fixed ? t("Ein veröffentlichtes Feld behält seinen Typ") : undefined}>
                {Object.entries(FIELD_TYPE_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
              </select>
            </label>
            <label>
              {t("Abschnitt")}
              <input value={f.group} onChange={(e) => change(i, { group: e.target.value })} placeholder={t("z. B. Methodik")} />
            </label>
            <label className="check">
              <input type="checkbox" checked={f.required} onChange={(e) => change(i, { required: e.target.checked })} /> {t("Pflichtfeld")}
            </label>
            <label className="wide">
              {t("Hinweis zum Ausfüllen")}
              <input value={f.description} onChange={(e) => change(i, { description: e.target.value })} />
            </label>
            <label className="wide">
              {t("Auswertung")}
              <input value={f.analysis} onChange={(e) => change(i, { analysis: e.target.value })} placeholder={t("z. B. Häufigkeit je Kategorie")} />
            </label>
            {hasOptions(f.type) && (
              <label className="wide">
                {t("Auswahlwerte, einer je Zeile")}
                <textarea rows={4} value={f.optionsText} onChange={(e) => change(i, { optionsText: e.target.value })} required />
              </label>
            )}
          </div>
          <div className="field-draft-actions">
            <button type="button" className="ghost" disabled={i === 0} onClick={() => move(i, -1)}>{t("Nach oben")}</button>
            <button type="button" className="ghost" disabled={i === fields.length - 1} onClick={() => move(i, 1)}>{t("Nach unten")}</button>
            <button type="button" className="ghost" onClick={() => setFields(fields.filter((_, j) => j !== i))}><Icon name="x" size={12} /> {t("Entfernen")}</button>
          </div>
        </fieldset>
      ))}
      <button type="button" onClick={add}><Icon name="plus" size={14} /> {t("Feld hinzufügen")}</button>
      {form && (
        <Group>
          <Row>
            <label htmlFor="form-note">
              {t("Was hat sich geändert?")}
              {form.confirmed && <small>{t("Pflicht, weil das Formular bestätigt ist. Die Begründung steht im Log.")}</small>}
            </label>
            <input id="form-note" value={note} onChange={(e) => setNote(e.target.value)} required={form.confirmed} />
          </Row>
        </Group>
      )}
      <p className="hint">
        {t("Entfernte Felder verschwinden aus Tabelle und Eingabe; ihre Werte bleiben im Log. Neue Pflichtfelder sind bei fertigen Quellen nachzutragen.")}
      </p>
      <div className="sheet-actions">
        <button type="button" onClick={onCancel}>{t("Abbrechen")}</button>
        <button className="primary" type="submit" disabled={fields.length === 0}>{t("Veröffentlichen")}</button>
      </div>
    </form>
  );
}

function FormCard({ form, manage, onEdit, onConfirm }: { form: Form; manage: boolean; onEdit: () => void; onConfirm: () => void }) {
  const current = latest(form);
  const [history, setHistory] = useState(false);
  return (
    <Group
      title={t("{name} · Version {version}", { name: current.name, version: current.version })}
      action={
        manage && (
          <span className="inline-actions">
            {!current.confirmed && <button onClick={onConfirm}><Icon name="check" size={14} /> {t("Bestätigen")}</button>}
            <button onClick={onEdit}>{t("Bearbeiten")}</button>
          </span>
        )
      }
      footer={
        current.confirmed
          ? t("Diese Version ist bestätigt. Jede weitere Änderung braucht eine Begründung.")
          : form.confirmed
            ? t("Geändert nach der Bestätigung. Die Begründung steht in den Versionen.")
            : t("Noch nicht bestätigt. Die Leitung bestätigt das Formular nach dem Pilot.")
      }
    >
      <table className="table">
        <thead><tr><th>{t("Feld")}</th><th>{t("Typ")}</th><th>{t("Abschnitt")}</th><th>{t("Auswertung")}</th></tr></thead>
        <tbody>
          {current.fields.map((f) => (
            <tr key={f.key}>
              <td>
                {f.label} {f.required && <Pill tone="blue">{t("Pflicht")}</Pill>}
                {f.description && <div className="muted small-text">{f.description}</div>}
                {f.options && f.options.length > 0 && <div className="muted small-text">{f.options.join(" · ")}</div>}
              </td>
              <td>{FIELD_TYPE_LABEL[f.type] ?? f.type}</td>
              <td>{f.group}</td>
              <td>{f.analysis}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {form.versions.length > 1 && (
        <Row>
          <button className="link" onClick={() => setHistory(!history)}>{history ? t("Versionen ausblenden") : t("{n} Versionen anzeigen", { n: form.versions.length })}</button>
        </Row>
      )}
      {history && form.versions.map((v) => (
        <Row key={v.version}>
          <span>
            {tn("Version {version} · {date} · {n} Feld", "Version {version} · {date} · {n} Felder", v.fields.length, { version: v.version, date: formatDate(v.published_at.slice(0, 10)) })}
            {v.confirmed && <> <Pill tone="green">{t("bestätigt")}</Pill></>}
            {v.after_confirmation && <> <Pill tone="yellow">{t("nach der Bestätigung")}</Pill></>}
          </span>
          <span className="muted">{v.change_note}</span>
        </Row>
      ))}
    </Group>
  );
}

export function FormsTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const manage = canManage(review.role);
  const [view, setView] = useState<FormsView | null>(null);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState<{ form: Form | null; name: string; fields: Draft[]; purpose: string } | null>(null);

  const load = () => api.forms(review.id).then(setView).catch((e) => setError(errorText(e)));
  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [review]);

  if (!view) return error ? <Banner kind="error">{error}</Banner> : null;

  if (editing) {
    return (
      <FormEditor
        {...editing}
        onCancel={() => setEditing(null)}
        onSave={(data) =>
          run(async () => {
            await api.publishForm(review.id, data);
            setEditing(null);
          }, t("Formular veröffentlicht. Schick den Mitgliedern unter „Austausch“ ein neues Paket."))
        }
      />
    );
  }

  return (
    <>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      {view.forms.length === 0 && (
        <Empty icon="list" title={t("Noch kein Formular für das {term}", { term: view.term })}>
          <p>
            {manage
              ? t("Das Formular legt fest, was aus jeder Quelle festgehalten wird. Im Pilot stimmt ihr es ab; danach bestätigt die Leitung es.")
              : t("Das Formular legt fest, was aus jeder Quelle festgehalten wird. Die Leitung veröffentlicht es.")}
          </p>
          {manage && (
            <div className="inline-actions center">
              {view.starters.map((s) => (
                <button key={s.purpose} className="primary" onClick={() => setEditing({ form: null, name: s.name, purpose: s.purpose, fields: s.fields.map((f) => toDraft(f, false)) })}>
                  {t("Mit dem Formular der Vorlage beginnen")}
                </button>
              ))}
              <button onClick={() => setEditing({ form: null, name: t("{term}-Formular", { term: view.term }), purpose: "charting", fields: [] })}>{t("Leeres Formular")}</button>
            </div>
          )}
        </Empty>
      )}
      {view.forms.map((form) => (
        <FormCard
          key={form.id}
          form={form}
          manage={manage}
          onEdit={() => {
            const current = latest(form);
            setEditing({ form, name: current.name, purpose: form.purpose, fields: current.fields.map((f) => toDraft(f, true)) });
          }}
          onConfirm={() => run(() => api.confirmForm(review.id, form.id), t("Formular bestätigt."))}
        />
      ))}
    </>
  );
}
