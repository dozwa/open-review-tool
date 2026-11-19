// Neues Review in drei Schritten: Angaben mit Review-Typ, Treffer, Team. Nur der erste Schritt ist Pflicht.

import React, { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { open } from "@tauri-apps/plugin-dialog";

import { api, errorText, TemplateInfo } from "./api";
import { t } from "./i18n";
import { ROLE_LABEL, useRefreshSidebar } from "./shared";
import { Banner, Group, Icon, PageHeader, Row } from "./ui";

const STEPS = [t("Angaben"), t("Treffer"), t("Team")];

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function NewReviewPage() {
  const navigate = useNavigate();
  const refresh = useRefreshSidebar();
  const [step, setStep] = useState(0);
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [title, setTitle] = useState("");
  const [template, setTemplate] = useState("");
  const [files, setFiles] = useState<{ path: string; source: string }[]>([]);
  const [people, setPeople] = useState<{ path: string; role: string }[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.templates()
      .then((list) => {
        setTemplates(list);
        setTemplate((current) => current || list[0]?.key || "");
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  async function addFiles() {
    const picked = await open({ multiple: true, directory: false, filters: [{ name: t("RIS oder BibTeX"), extensions: ["ris", "bib", "bibtex", "txt", "nbib"] }] });
    const paths = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
    setFiles((current) => [...current, ...paths.filter((p) => !current.some((f) => f.path === p)).map((path) => ({ path, source: "" }))]);
  }

  async function addPeople() {
    const picked = await open({ multiple: true, directory: false, filters: [{ name: t("Identitätsdatei"), extensions: ["ortid"] }] });
    const paths = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
    setPeople((current) => [...current, ...paths.filter((p) => !current.some((f) => f.path === p)).map((path) => ({ path, role: "reviewer" }))]);
  }

  /** Legt das Review an und holt danach Treffer und Team nach. Was dabei scheitert, lässt sich im Review wiederholen. */
  async function create() {
    setBusy(true);
    setError("");
    try {
      const review = await api.createReview(title, template || null);
      const problems: string[] = [];
      for (const file of files) {
        try {
          const batch = await api.importRecords(review.id, file.path, { source: file.source, origin: "database" });
          if (batch.status === "failed") problems.push(`${fileName(file.path)}: ${batch.error}`);
        } catch (e) {
          problems.push(`${fileName(file.path)}: ${errorText(e)}`);
        }
      }
      for (const person of people) {
        try {
          await api.addMember(review.id, person.path, person.role);
        } catch (e) {
          problems.push(`${fileName(person.path)}: ${errorText(e)}`);
        }
      }
      refresh();
      navigate(`/reviews/${review.id}`, { state: problems.length ? { problems } : undefined });
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  }

  function next(event: React.FormEvent) {
    event.preventDefault();
    if (step < STEPS.length - 1) setStep(step + 1);
    else create();
  }

  const chosen = templates.find((option) => option.key === template);

  return (
    <form className="wizard" onSubmit={next}>
      <PageHeader back={<Link to="/" className="back-link"><Icon name="back" size={14} /> {t("Übersicht")}</Link>} title={t("Neues Review")} />
      <ol className="wizard-steps" aria-label={t("Schritte")}>
        {STEPS.map((label, i) => (
          <li key={label} className={i === step ? "current" : i < step ? "done" : ""} aria-current={i === step ? "step" : undefined}>
            <span className="step-num">{i < step ? <Icon name="check" size={12} /> : i + 1}</span> {label}
          </li>
        ))}
      </ol>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}

      {step === 0 && (
        <>
          <Group title={t("Titel")}>
            <div className="stack-field">
              <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder={t("Titel des Reviews")} aria-label={t("Titel des Reviews")} required autoFocus />
            </div>
          </Group>
          <Group title={t("Review-Typ")} footer={t("Der Typ legt Stationen, Regeln und Begriffe fest. Die Vorlage wird ins Review kopiert und gilt dort unabhängig von der App-Version.")}>
            {templates.map((option) => (
              <label key={option.key} className={`template-option${option.key === template ? " selected" : ""}`}>
                <input type="radio" name="template" value={option.key} checked={option.key === template} onChange={() => setTemplate(option.key)} />
                <span>
                  <strong>{option.name}</strong>
                  <small>{option.description}</small>
                </span>
              </label>
            ))}
          </Group>
          {chosen && (
            <p className="template-flow" aria-label={t("Stationen der Vorlage")}>
              {chosen.stations.map((s, i) => <span key={s}>{i > 0 && <span className="flow-arrow">›</span>}{s}</span>)}
            </p>
          )}
        </>
      )}

      {step === 1 && (
        <Group title={t("Treffer importieren")} footer={t("Exportiere die Treffer deiner Suche als RIS oder BibTeX. Dubletten werden erkannt. Du kannst das auch später im Review nachholen.")}>
          {files.length === 0 && <Row><span className="muted">{t("Noch keine Datei gewählt.")}</span></Row>}
          {files.map((f, i) => (
            <Row key={f.path}>
              <span className="person">
                <span className="file-icon"><Icon name="file" /></span>
                <span>{fileName(f.path)}</span>
              </span>
              <span className="inline-actions">
                <input value={f.source} onChange={(e) => setFiles(files.map((x, j) => (j === i ? { ...x, source: e.target.value } : x)))} placeholder={t("Quelle, z. B. PubMed")} aria-label={t("Quelle von {file}", { file: fileName(f.path) })} />
                <button type="button" className="plain" onClick={() => setFiles(files.filter((_, j) => j !== i))} aria-label={t("{file} entfernen", { file: fileName(f.path) })}><Icon name="x" size={14} /></button>
              </span>
            </Row>
          ))}
          <Row>
            <span className="muted">{t("RIS oder BibTeX")}</span>
            <button type="button" onClick={addFiles}><Icon name="upload" size={14} /> {t("Dateien wählen …")}</button>
          </Row>
        </Group>
      )}

      {step === 2 && (
        <Group title={t("Team")} footer={t("Jede Person speichert in ihrer App eine Identitätsdatei (.ortid) und schickt sie dir. Auch das geht später im Review.")}>
          {people.length === 0 && <Row><span className="muted">{t("Noch niemand hinzugefügt. Du selbst leitest das Review.")}</span></Row>}
          {people.map((p, i) => (
            <Row key={p.path}>
              <span className="person">
                <span className="file-icon"><Icon name="key" /></span>
                <span>{fileName(p.path)}</span>
              </span>
              <span className="inline-actions">
                <select value={p.role} onChange={(e) => setPeople(people.map((x, j) => (j === i ? { ...x, role: e.target.value } : x)))} aria-label={t("Rolle für {file}", { file: fileName(p.path) })}>
                  {Object.entries(ROLE_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                </select>
                <button type="button" className="plain" onClick={() => setPeople(people.filter((_, j) => j !== i))} aria-label={t("{file} entfernen", { file: fileName(p.path) })}><Icon name="x" size={14} /></button>
              </span>
            </Row>
          ))}
          <Row>
            <span className="muted">{t("Identitätsdateien")}</span>
            <button type="button" onClick={addPeople}><Icon name="file" size={14} /> {t("Dateien wählen …")}</button>
          </Row>
        </Group>
      )}

      <div className="sheet-actions">
        {step > 0 && <button type="button" onClick={() => setStep(step - 1)} disabled={busy}>{t("Zurück")}</button>}
        {step < STEPS.length - 1 && <button type="button" onClick={create} disabled={busy || !title.trim()}>{t("Jetzt anlegen")}</button>}
        <button className="primary" type="submit" disabled={busy}>{step < STEPS.length - 1 ? t("Weiter") : t("Review anlegen")}</button>
      </div>
    </form>
  );
}
