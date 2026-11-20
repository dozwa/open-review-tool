// KI-Vorschlag auf der Eingabeseite einer Formular-Stufe: Werte für die Felder des Formulars aus dem Volltext.
// Die Oberfläche liest den Text des PDFs Seite für Seite, der Kern baut daraus die Anfrage, die App sendet sie
// (`ai_extract`). Der Vorschlag steht danach im Log, eingetragen ist damit nichts: Die Person wählt je Feld,
// was sie übernimmt, und kann den Dialog schließen, um die Belegstellen erst im Volltext nachzulesen.

import React, { useEffect, useState } from "react";

import { Extraction, FormField, FormValue, showValue } from "./api";
import { t, tn } from "./i18n";
import { Banner, Icon, Modal, Pill } from "./ui";

/** Was der Dialog gerade zeigt. */
export type ExtractionState =
  | { step: "loading"; recordId: string }
  | { step: "error"; recordId: string; text: string }
  | { step: "done"; recordId: string; suggestion: Extraction };

const same = (a: FormValue | undefined, b: FormValue) => JSON.stringify(a ?? null) === JSON.stringify(b);

export function ExtractionDialog({ state, model, fields, values, busy, onApply, onClose, onRetry }: {
  state: ExtractionState;
  /** Das eingestellte Modell, solange noch keine Antwort da ist. */
  model: string;
  fields: FormField[];
  /** Die eigenen Werte, wie sie jetzt im Formular stehen. */
  values: Record<string, FormValue>;
  busy: boolean;
  /** Übernimmt die genannten Felder; eine leere Liste verwirft den Vorschlag. */
  onApply: (suggestion: Extraction, keys: string[]) => void;
  onClose: () => void;
  onRetry: () => void;
}) {
  const suggestion = state.step === "done" ? state.suggestion : null;
  const [chosen, setChosen] = useState<Record<string, boolean>>({});
  // Vorbelegt ist, was nichts Eigenes überschreibt.
  useEffect(() => {
    if (!suggestion) return;
    setChosen(Object.fromEntries(Object.keys(suggestion.fields).map((key) => [key, (values[key] ?? null) === null])));
  }, [suggestion?.id]);

  if (state.step === "loading") {
    return (
      <Modal title={t("KI-Vorschlag zur Extraktion")} onClose={onClose} footer={<button onClick={onClose}>{t("Abbrechen")}</button>}>
        <p className="ai-wait"><span className="spinner" aria-hidden="true" /> {t("{model} liest den Volltext …", { model })}</p>
        <p className="muted">{t("Der Text des Volltexts geht an die eingestellte Adresse der KI. Bei langen Texten dauert die Antwort eine Weile. Trifft sie nach dem Abbrechen noch ein, wird sie trotzdem im Log vermerkt.")}</p>
      </Modal>
    );
  }
  if (state.step === "error" || !suggestion) {
    return (
      <Modal
        title={t("KI-Vorschlag zur Extraktion")}
        onClose={onClose}
        footer={<><button onClick={onClose}>{t("Schließen")}</button><button className="primary" onClick={onRetry}>{t("Erneut versuchen")}</button></>}
      >
        <Banner kind="error"><span className="ai-error">{state.step === "error" ? state.text : ""}</span></Banner>
        <p className="muted">{t("Es wurde nichts vermerkt. Adresse, Modell und Schlüssel stehen in den Einstellungen unter „KI-Unterstützung“.")}</p>
      </Modal>
    );
  }

  const open = suggestion.accepted === null;
  const proposed = fields.filter((f) => suggestion.fields[f.key]);
  const without = fields.filter((f) => !suggestion.fields[f.key]);
  const keys = proposed.filter((f) => chosen[f.key] && !same(values[f.key], suggestion.fields[f.key].value)).map((f) => f.key);
  const footer = open ? (
    <>
      <button disabled={busy} onClick={() => onApply(suggestion, [])}>{t("Verwerfen")}</button>
      <button className="primary" disabled={busy || keys.length === 0} onClick={() => onApply(suggestion, keys)}>
        {tn("{n} Wert übernehmen", "{n} Werte übernehmen", keys.length)}
      </button>
    </>
  ) : (
    <button onClick={onClose}>{t("Schließen")}</button>
  );

  return (
    <Modal title={t("KI-Vorschlag zur Extraktion")} onClose={onClose} footer={footer}>
      <div className="ai-extract">
        {suggestion.truncated && <Banner kind="notice">{t("Der Volltext war zu lang. Die KI hat nur den Anfang gelesen; was weiter hinten steht, fehlt im Vorschlag.")}</Banner>}
        {proposed.length === 0 && <p>{t("Die KI hat im Volltext zu keinem Feld eine Angabe gefunden.")}</p>}
        {proposed.map((field) => {
          const item = suggestion.fields[field.key];
          const own = values[field.key] ?? null;
          const taken = suggestion.accepted?.includes(field.key);
          return (
            <label className="ai-field" key={field.key}>
              {open && (
                <input
                  type="checkbox"
                  checked={(chosen[field.key] ?? false) && !same(own, item.value)}
                  disabled={busy || same(own, item.value)}
                  onChange={(e) => setChosen({ ...chosen, [field.key]: e.target.checked })}
                />
              )}
              <span className="ai-field-body">
                <span className="ai-field-head">
                  <strong>{field.label}</strong>
                  {open
                    ? same(own, item.value) ? <Pill tone="gray">{t("steht schon so da")}</Pill> : own !== null && <Pill tone="yellow">{t("ersetzt deinen Wert")}</Pill>
                    : <Pill tone={taken ? "green" : "gray"}>{taken ? t("übernommen") : t("nicht übernommen")}</Pill>}
                </span>
                <span className="ai-value">{showValue(item.value)}</span>
                {open && own !== null && !same(own, item.value) && <span className="muted">{t("Bisher eingetragen: {value}", { value: showValue(own) })}</span>}
                {item.quote && (
                  <span className="ai-quote">
                    „{item.quote}“{item.page !== null && <em> {t("S. {page}", { page: item.page })}</em>}
                  </span>
                )}
              </span>
            </label>
          );
        })}
        {without.length > 0 && proposed.length > 0 && <p className="muted">{t("Ohne Vorschlag: {fields}", { fields: without.map((f) => f.label).join(", ") })}</p>}
        <p className="ai-meta">{t("Modell: {model}", { model: suggestion.model })}</p>
        <p className="muted">
          {open
            ? t("Der Vorschlag ist im Log vermerkt, eingetragen ist noch nichts. Prüfe die Belegstellen im Volltext; du kannst den Dialog schließen und später wieder öffnen. Was du übernimmst, gilt als deine Angabe.")
            : t("Der Vorschlag und was daraus übernommen wurde, stehen im Log.")}
        </p>
      </div>
    </Modal>
  );
}

/** Zeile im Formular: der letzte eigene Vorschlag zu dieser Quelle. */
export function ExtractionLine({ suggestion, onShow }: { suggestion: Extraction; onShow: () => void }) {
  const n = Object.keys(suggestion.fields).length;
  return (
    <p className="ai-line ai-line-form">
      <Icon name="spark" size={13} />
      <span>
        {tn("KI-Vorschlag mit {n} Wert", "KI-Vorschlag mit {n} Werten", n)}
        {" · "}
        {suggestion.accepted === null ? t("noch offen") : t("{n} übernommen", { n: suggestion.accepted.length })}
      </span>
      <button className="link" onClick={onShow}>{t("Ansehen")}</button>
    </p>
  );
}
