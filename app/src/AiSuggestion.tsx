// KI-Vorschlag auf der Screening-Seite: der Dialog mit der Antwort der KI und die Zeile zum letzten Vorschlag.
// Die Anfrage stellt der Kern zusammen und die App sendet sie (`ai_suggest`); der Vorschlag steht danach im
// Log. Er ist keine Stimme: Übernehmen gibt die Stimme der Person ab, Ablehnen lässt den Eintrag offen.

import React from "react";

import { Criterion, Suggestion } from "./api";
import { DECISION_LABEL } from "./StagePages";
import { t } from "./i18n";
import { Banner, Icon, Kbd, Modal } from "./ui";

/** Was der Dialog gerade zeigt. `prompt` fehlt, wenn ein früherer Vorschlag noch einmal angesehen wird. */
export type AiState =
  | { step: "loading"; recordId: string }
  | { step: "error"; recordId: string; text: string }
  | { step: "done"; recordId: string; suggestion: Suggestion; prompt?: string };

const OUTCOME_LABEL: Record<string, string> = { accepted: t("übernommen"), declined: t("abgelehnt") };

/** Entscheidung und Grund eines Vorschlags in einem Wort, etwa „Exclude · E2“. */
export function suggestionLabel(suggestion: Suggestion): string {
  const decision = DECISION_LABEL[suggestion.suggested] ?? suggestion.suggested;
  return suggestion.reason ? `${decision} · ${suggestion.reason}` : decision;
}

export function AiDialog({ state, model, reasons, required, busy, onAccept, onDecline, onClose, onRetry }: {
  state: AiState;
  /** Das eingestellte Modell, solange noch keine Antwort da ist. */
  model: string;
  reasons: Criterion[];
  /** Die Stufe verlangt zu jedem Ausschluss einen Hauptgrund. */
  required: boolean;
  busy: boolean;
  onAccept: (suggestion: Suggestion) => void;
  onDecline: (suggestion: Suggestion) => void;
  onClose: () => void;
  onRetry: () => void;
}) {
  if (state.step === "loading") {
    return (
      <Modal title={t("KI-Vorschlag")} onClose={onClose} footer={<button onClick={onClose}>{t("Abbrechen")}</button>}>
        <p className="ai-wait"><span className="spinner" aria-hidden="true" /> {t("{model} liest den Eintrag …", { model })}</p>
        <p className="muted">{t("Trifft die Antwort nach dem Abbrechen noch ein, wird sie trotzdem im Log vermerkt.")}</p>
      </Modal>
    );
  }
  if (state.step === "error") {
    return (
      <Modal
        title={t("KI-Vorschlag")}
        onClose={onClose}
        footer={<><button onClick={onClose}>{t("Schließen")}</button><button className="primary" onClick={onRetry}>{t("Erneut versuchen")}</button></>}
      >
        <Banner kind="error"><span className="ai-error">{state.text}</span></Banner>
        <p className="muted">{t("Es wurde nichts vermerkt. Adresse, Modell und Schlüssel stehen in den Einstellungen unter „KI-Unterstützung“.")}</p>
      </Modal>
    );
  }

  const { suggestion, prompt } = state;
  const open = suggestion.outcome === null;
  const reason = reasons.find((r) => r.code === suggestion.reason);
  // Ohne gültigen Grund lässt sich ein Ausschluss in dieser Stufe nicht speichern.
  const canAccept = !(suggestion.suggested === "exclude" && required && !suggestion.reason);
  const close = open ? () => onDecline(suggestion) : onClose;
  const footer = open ? (
    <>
      <button disabled={busy} onClick={() => onDecline(suggestion)}>{t("Ablehnen")} <Kbd>esc</Kbd></button>
      <button className="primary" disabled={busy || !canAccept} onClick={() => onAccept(suggestion)}>{t("Übernehmen")} <Kbd>↵</Kbd></button>
    </>
  ) : (
    <button onClick={onClose}>{t("Schließen")}</button>
  );

  return (
    <Modal title={t("KI-Vorschlag")} onClose={close} onEnter={open && canAccept && !busy ? () => onAccept(suggestion) : undefined} footer={footer}>
      <p className={`ai-decision d-${suggestion.suggested}`}>
        <strong>{DECISION_LABEL[suggestion.suggested] ?? suggestion.suggested}</strong>
        {suggestion.reason && <span>{reason ? `${suggestion.reason} ${reason.text}` : suggestion.reason}</span>}
      </p>
      {suggestion.rationale ? <p className="ai-rationale">{suggestion.rationale}</p> : <p className="muted">{t("Die KI hat keine Begründung genannt.")}</p>}
      {open && !canAccept && <p className="reason-hint">{t("Diese Stufe verlangt zu jedem Ausschluss einen Hauptgrund, und die KI nennt keinen gültigen. Lehne ab und entscheide selbst.")}</p>}
      <p className="ai-meta">
        {t("Modell: {model}", { model: suggestion.model })}
        {suggestion.outcome && ` · ${OUTCOME_LABEL[suggestion.outcome] ?? suggestion.outcome}`}
      </p>
      {prompt && (
        <details className="ai-prompt">
          <summary>{t("Gesendeter Prompt")}</summary>
          <pre>{prompt}</pre>
        </details>
      )}
      <p className="muted">{open ? t("Der Vorschlag ist im Log vermerkt. Die Entscheidung triffst du.") : t("Der Vorschlag und was daraus wurde, stehen im Log.")}</p>
    </Modal>
  );
}

/** Zeile unter den Stimmknöpfen: der letzte eigene Vorschlag zu diesem Eintrag. */
export function AiLine({ suggestion, onShow }: { suggestion: Suggestion; onShow: () => void }) {
  return (
    <p className="ai-line">
      <Icon name="spark" size={13} />
      <span>
        {t("KI-Vorschlag: {decision}", { decision: suggestionLabel(suggestion) })}
        {" · "}
        {suggestion.outcome ? OUTCOME_LABEL[suggestion.outcome] ?? suggestion.outcome : t("noch offen")}
      </span>
      <button className="link" onClick={onShow}>{t("Ansehen")}</button>
    </p>
  );
}
