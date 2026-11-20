// Rückmeldung an die Entwicklung: Fehler melden oder Funktion wünschen. Der Knopf steht in der Seitenleiste
// und damit auf jeder Seite. Der Dialog stellt ein paar Fragen und übergibt Titel und Text dem Rust-Teil
// (`feedback_open`), der einen Entwurf im Mailprogramm oder das ausgefüllte Issue-Formular auf GitHub öffnet.
// Die App sendet nichts selbst; abgeschickt wird dort.

import React, { useState } from "react";
import { useLocation } from "react-router-dom";

import { api, errorText } from "./api";
import { t } from "./i18n";
import { Banner, Icon, Modal, Segmented } from "./ui";

type Kind = "bug" | "feature";
type Channel = "email" | "github";

/** Die Fragen je Art der Rückmeldung; die erste braucht eine Antwort. */
const QUESTIONS: Record<Kind, { key: string; label: string; hint: string }[]> = {
  bug: [
    { key: "what", label: t("Was ist kaputt?"), hint: t("Was siehst du, und was stimmt daran nicht?") },
    { key: "steps", label: t("Wie bist du vorgegangen?"), hint: t("Schritt für Schritt, bis der Fehler auftritt.") },
    { key: "expected", label: t("Was hast du erwartet?"), hint: "" },
    { key: "impact", label: t("Welche Auswirkungen hat das?"), hint: t("Kannst du weiterarbeiten? Sind Daten betroffen?") },
  ],
  feature: [
    { key: "what", label: t("Was wünschst du dir?"), hint: "" },
    { key: "why", label: t("Wobei würde es dir helfen?"), hint: "" },
    { key: "now", label: t("Wie behilfst du dir bisher?"), hint: "" },
  ],
};

const KINDS: { value: Kind; label: string }[] = [{ value: "bug", label: t("Fehler") }, { value: "feature", label: t("Wunsch") }];
const CHANNELS: { value: Channel; label: string }[] = [{ value: "email", label: t("E-Mail") }, { value: "github", label: "GitHub" }];

const EMPTY = { kind: "bug" as Kind, channel: "email" as Channel, title: "", answers: {} as Record<string, string> };

/** Die offene Seite ohne Kennungen: Sie sagen der Entwicklung nichts und gehören nicht in ein öffentliches Issue. */
function pageOf(pathname: string, search: string): string {
  const path = pathname.replace(/[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}/gi, ":id");
  const tab = new URLSearchParams(search).get("tab");
  return tab ? `${path}?tab=${tab}` : path;
}

export function FeedbackButton() {
  const location = useLocation();
  const [open, setOpen] = useState(false);
  // Der Entwurf bleibt erhalten, wenn der Dialog versehentlich zugeht; erst nach dem Übergeben wird er geleert.
  const [draft, setDraft] = useState(EMPTY);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [sent, setSent] = useState(false);

  const page = pageOf(location.pathname, location.search);
  const questions = QUESTIONS[draft.kind];
  const answer = (key: string) => draft.answers[`${draft.kind}.${key}`] ?? "";
  const ready = draft.title.trim() !== "" && answer(questions[0].key).trim() !== "";

  function close() {
    setOpen(false);
    setError("");
    if (sent) {
      setSent(false);
      setDraft({ ...EMPTY, channel: draft.channel });
    }
  }

  async function send() {
    const parts = questions
      .filter((q) => answer(q.key).trim() !== "")
      .map((q) => `## ${q.label}\n${answer(q.key).trim()}`);
    parts.push(`${t("Seite")}: ${page}`);
    setBusy(true);
    setError("");
    try {
      await api.openFeedback(draft.channel, draft.kind, draft.title, parts.join("\n\n"));
      setSent(true);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  const footer = sent ? (
    <>
      <button onClick={() => setSent(false)}>{t("Zurück zum Text")}</button>
      <button className="primary" onClick={close}>{t("Schließen")}</button>
    </>
  ) : (
    <>
      <button onClick={close}>{t("Abbrechen")}</button>
      <button className="primary" disabled={!ready || busy} onClick={send}>
        {draft.channel === "email" ? t("Weiter zum Mailprogramm") : t("Weiter zu GitHub")}
      </button>
    </>
  );

  return (
    <>
      <button className="nav-item" onClick={() => setOpen(true)} title={t("Fehler melden oder Funktion wünschen")}>
        <Icon name="chat" /> {t("Fehler melden")}
      </button>
      {open && (
        <Modal title={t("Rückmeldung an die Entwicklung")} onClose={close} footer={footer}>
          {sent ? (
            <p>
              {draft.channel === "email"
                ? t("Der Entwurf liegt in deinem Mailprogramm. Gesendet ist er erst, wenn du ihn dort abschickst.")
                : t("Das ausgefüllte Formular ist im Browser offen. Angelegt ist das Issue erst, wenn du es dort abschickst.")}
            </p>
          ) : (
            <div className="feedback">
              <div className="feedback-choice">
                <Segmented value={draft.kind} options={KINDS} onChange={(kind) => setDraft({ ...draft, kind })} label={t("Art der Rückmeldung")} />
                <span className="muted">{t("senden über")}</span>
                <Segmented value={draft.channel} options={CHANNELS} onChange={(channel) => setDraft({ ...draft, channel })} label={t("Weg der Rückmeldung")} />
              </div>
              <label>
                <span>{t("Kurz in einem Satz")}</span>
                <input value={draft.title} maxLength={200} autoFocus onChange={(e) => setDraft({ ...draft, title: e.target.value })} />
              </label>
              {questions.map((q, i) => (
                <label key={`${draft.kind}.${q.key}`}>
                  <span>{q.label}{i > 0 && <small> {t("(freiwillig)")}</small>}</span>
                  <textarea
                    rows={i === 0 ? 3 : 2}
                    maxLength={900}
                    placeholder={q.hint}
                    value={answer(q.key)}
                    onChange={(e) => setDraft({ ...draft, answers: { ...draft.answers, [`${draft.kind}.${q.key}`]: e.target.value } })}
                  />
                </label>
              ))}
              {error && <Banner kind="error">{error}</Banner>}
              <p className="muted">
                {draft.channel === "email"
                  ? t("Es öffnet sich ein Entwurf in deinem Mailprogramm; dort kannst du auch ein Bildschirmfoto anhängen.")
                  : t("Es öffnet sich das Formular für ein neues Issue; du brauchst ein Konto bei GitHub. Issues sind öffentlich: Schreib nichts hinein, was im Review vertraulich ist.")}
                {" "}
                {t("Angehängt werden die offene Seite ({page}) und die Versionen von App und macOS, sonst nichts aus deinen Reviews.", { page })}
              </p>
            </div>
          )}
        </Modal>
      )}
    </>
  );
}
