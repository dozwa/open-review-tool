// Registrierung vorbereiten: geführt durch die Felder des Registrierungsformulars von OSF.
// Vorschläge kommen aus dem Review; was die Leitung selbst schreibt, steht als Ereignis im Log.

import React, { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import { api, errorText, RegistrationDraft, RegistrationItem, ReviewDetail, safeFileName } from "./api";
import { formatDate, outputLanguage, setOutputLanguage, t, tn, tx } from "./i18n";
import { Banner, Group, Icon, LanguageSelect, Pill, Row } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

const STATUS_LABEL: Record<RegistrationItem["status"], string> = { answered: t("eigener Text"), suggested: t("Vorschlag"), open: t("offen") };
const STATUS_TONE: Record<RegistrationItem["status"], string | undefined> = { answered: "green", suggested: "blue", open: undefined };
const today = () => new Date().toISOString().slice(0, 10);

function ItemField({ item, canEdit, onSave }: { item: RegistrationItem; canEdit: boolean; onSave: (text: string) => Promise<void> }) {
  const [text, setText] = useState(item.answer ?? "");
  const [copied, setCopied] = useState(false);
  useEffect(() => setText(item.answer ?? ""), [item.answer]);
  const shown = item.answer ?? item.suggestion;
  const rows = Math.min(14, Math.max(3, (text || item.suggestion).split("\n").length + 1));

  async function copy() {
    try {
      await navigator.clipboard.writeText(text.trim() || item.suggestion);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  }

  return (
    <fieldset className="reg-item">
      <legend>
        <span className="reg-number">{item.number}</span>
        {item.label}
        <small>{item.original}</small>
        <Pill tone={STATUS_TONE[item.status]}>{STATUS_LABEL[item.status]}</Pill>
      </legend>
      <p className="muted">{item.hint}</p>
      {canEdit ? (
        <textarea
          rows={rows}
          value={text}
          placeholder={item.suggestion ? t("Leer lassen, um den Vorschlag zu übernehmen, oder eigenen Text schreiben") : t("Text für dieses Feld")}
          aria-label={item.label}
          onChange={(e) => setText(e.target.value)}
          onBlur={() => text.trim() !== (item.answer ?? "") && onSave(text)}
        />
      ) : (
        shown && <pre className="reg-text">{shown}</pre>
      )}
      {item.suggestion && (canEdit || item.answer !== null) && (
        <details className="reg-suggestion" open={item.status === "suggested"}>
          <summary>{t("Vorschlag aus dem Review")}</summary>
          <pre className="reg-text">{item.suggestion}</pre>
        </details>
      )}
      <div className="reg-actions">
        {canEdit && item.suggestion && item.answer === null && (
          <button className="small" onClick={() => setText(item.suggestion)} title={t("Den Vorschlag ins Feld holen und anpassen")}>{t("Vorschlag bearbeiten")}</button>
        )}
        {canEdit && item.answer !== null && item.suggestion && (
          <button className="small" onClick={() => onSave("")} title={t("Eigenen Text verwerfen")}>{t("Zurück zum Vorschlag")}</button>
        )}
        {(text.trim() || item.suggestion) && <button className="small" onClick={copy}>{copied ? t("Kopiert") : t("Kopieren")}</button>}
      </div>
    </fieldset>
  );
}

function RegisterForm({ draft, onSave }: { draft: RegistrationDraft; onSave: (date: string, registry: string, identifier: string, note: string) => void }) {
  const r = draft.registration;
  const [date, setDate] = useState(r?.registered_on ?? today());
  const [registry, setRegistry] = useState(r?.registry ?? draft.registry);
  const [identifier, setIdentifier] = useState(r?.identifier ?? "");
  const [note, setNote] = useState(r?.note ?? "");
  return (
    <form onSubmit={(e) => { e.preventDefault(); onSave(date, registry, identifier, note); }}>
      <Group
        title={r ? t("Registrierung berichtigen") : t("Registrierung eintragen")}
        footer={t("Der erste Eintrag ist der Bezugspunkt: Jede spätere neue Version von Kriterien, Suchen oder Ablauf erscheint im Bericht als Abweichung vom Protokoll.")}
      >
        <Row>
          <label htmlFor="reg-date">{t("Registriert am")}</label>
          <input id="reg-date" type="date" value={date} onChange={(e) => setDate(e.target.value)} required />
        </Row>
        <Row>
          <label htmlFor="reg-registry">{t("Register")}</label>
          <input id="reg-registry" value={registry} onChange={(e) => setRegistry(e.target.value)} required />
        </Row>
        <Row>
          <label htmlFor="reg-id">{t("Kennung oder Adresse")} <small>{t("z. B. osf.io/abc12 oder die DOI")}</small></label>
          <input id="reg-id" value={identifier} onChange={(e) => setIdentifier(e.target.value)} />
        </Row>
        <Row>
          <label htmlFor="reg-note">{t("Anmerkung")}</label>
          <input id="reg-note" value={note} onChange={(e) => setNote(e.target.value)} />
        </Row>
      </Group>
      <div className="sheet-actions">
        <button className="primary" type="submit">{r ? t("Angaben berichtigen") : t("Registrierung festhalten")}</button>
      </div>
    </form>
  );
}

export function RegistrationTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const [draft, setDraft] = useState<RegistrationDraft | null>(null);
  const [step, setStep] = useState(0);
  // Feldnamen, Hinweise und Vorschläge erscheinen in der Sprache, in der registriert wird.
  const [lang, setLang] = useState(outputLanguage);
  const [error, setError] = useState("");

  useEffect(() => {
    api.registrationDraft(review.id, lang).then(setDraft).catch((e) => setError(errorText(e)));
  }, [review, lang]);

  if (!draft) return error ? <Banner kind="error">{error}</Banner> : null;
  const last = draft.sections.length;
  const section = draft.sections[step];
  const items = section ? draft.items.filter((i) => i.section === section.key) : [];
  const total = draft.items.length;
  const ready = draft.n_answered + draft.n_suggested;

  async function answer(key: string, text: string) {
    setError("");
    try {
      setDraft(await api.answerRegistration(review.id, key, text, lang));
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function exportFile() {
    const path = await save({
      defaultPath: t("{title}-registrierung-osf.md", { title: safeFileName(review.title) }),
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (!path) return;
    await run(async () => {
      await api.exportRegistration(review.id, path, lang);
    }, t("Vorbereitung gespeichert."));
  }

  return (
    <>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      <Group
        title={t("Registrierung vorbereiten: {name}", { name: draft.name })}
        action={
          <span className="inline-actions">
            <LanguageSelect value={lang} label={t("Sprache der Ausgabe")} onChange={(code) => { setOutputLanguage(code); setLang(code); }} />
            <button className="small" onClick={exportFile}><Icon name="download" size={13} /> {t("Als Markdown speichern")}</button>
          </span>
        }
        footer={tx(
          "Die App reicht nichts ein. Sie sammelt die Texte für das Formular „{name}“ bei {registry}; dort legst du die Registrierung an und kopierst Feld für Feld. Vorschläge stammen aus dem Review und sind vor dem Einreichen zu prüfen. Das Formular beschreiben {citation} {link}",
          {
            name: draft.name,
            registry: draft.registry,
            citation: draft.citation,
            link: <button className="link" onClick={() => api.openUrl(draft.url)} title={draft.url}>{t("Original öffnen")}</button>,
          },
        )}
      >
        <Row>
          <span>
            {draft.registration
              ? <>{t("Registriert am {date}: {registry}", { date: formatDate(draft.registration.registered_on), registry: draft.registration.registry })}{draft.registration.identifier && ` · ${draft.registration.identifier}`}</>
              : tn("{ready} von {total} Feldern haben einen Text, {n} ist offen.", "{ready} von {total} Feldern haben einen Text, {n} sind offen.", draft.n_open, { ready, total })}
          </span>
          <span className="inline-actions">
            <Pill tone="green">{t("{count} eigene", { count: draft.n_answered })}</Pill>
            <Pill tone="blue">{tn("{n} Vorschlag", "{n} Vorschläge", draft.n_suggested)}</Pill>
            <Pill>{t("{count} offen", { count: draft.n_open })}</Pill>
          </span>
        </Row>
        {!draft.can_edit && <Row><span className="muted">{t("Die Texte schreibt die Leitung. Du liest mit.")}</span></Row>}
      </Group>

      <ol className="reg-steps" aria-label={t("Seiten des Formulars")}>
        {draft.sections.map((s, i) => {
          const own = draft.items.filter((it) => it.section === s.key);
          const open = own.filter((it) => it.status === "open").length;
          return (
            <li key={s.key} className={i === step ? "current" : ""}>
              <button onClick={() => setStep(i)} aria-current={i === step ? "step" : undefined}>
                {s.label}
                <small>{open === 0 ? t("vollständig") : t("{count} offen", { count: open })}</small>
              </button>
            </li>
          );
        })}
        <li className={step === last ? "current" : ""}>
          <button onClick={() => setStep(last)} aria-current={step === last ? "step" : undefined}>
            {t("Einreichen")}
            <small>{draft.registration ? t("eingetragen") : t("letzter Schritt")}</small>
          </button>
        </li>
      </ol>

      {section && (
        <section className="reg-section" aria-label={section.label}>
          <h2>{section.label} <small>{t("Seite „{page}“ im Register", { page: section.original })}</small></h2>
          {items.map((item) => (
            <ItemField key={item.key} item={item} canEdit={draft.can_edit} onSave={(text) => answer(item.key, text)} />
          ))}
        </section>
      )}

      {step === last && (
        <section className="reg-section" aria-label={t("Einreichen")}>
          <h2>{t("Einreichen")}</h2>
          <Group title={t("So geht es weiter")}>
            <Row>
              <span>
                {draft.n_open > 0
                  ? t("1. Offene Felder füllen und Vorschläge prüfen. Noch {count} offen.", { count: draft.n_open })
                  : t("1. Offene Felder füllen und Vorschläge prüfen. Kein Feld ist mehr offen.")}
              </span>
            </Row>
            <Row><span>{t("2. Die Vorbereitung als Markdown speichern oder Feld für Feld kopieren. Jedes Feld trägt den Namen, den es im Register hat.")}</span></Row>
            <Row>
              <span>
                {t("3. Bei {registry} eine Registrierung mit dem Formular „{name}“ anlegen, die Texte einfügen, Protokoll und Suchstrategien anhängen und einreichen.", { registry: draft.registry, name: draft.name })}
              </span>
            </Row>
            <Row><span>{t("4. Hier Datum und Kennung eintragen. Ab dann zählt jede Änderung an Kriterien, Suchen oder Ablauf als Abweichung.")}</span></Row>
          </Group>
          {draft.can_edit && (
            <RegisterForm
              draft={draft}
              onSave={(date, registry, identifier, note) =>
                run(async () => {
                  await api.registerProtocol(review.id, date, registry, identifier, note);
                }, t("Registrierung festgehalten."))
              }
            />
          )}
        </section>
      )}

      <div className="sheet-actions">
        <button disabled={step === 0} onClick={() => setStep(step - 1)}>{t("Zurück")}</button>
        <button className="primary" disabled={step === last} onClick={() => setStep(step + 1)}>{t("Weiter")}</button>
      </div>
    </>
  );
}
