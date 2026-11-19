// Bericht: Flussdiagramm, Zahlen je Ebene und die Anhänge zur Auswahl. Alles kommt fertig aus dem
// Kern; hier wird nur angezeigt und gespeichert.

import React, { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import { api, errorText, FlowLevel, Report, ReportFileKind, ReviewDetail } from "./api";
import { Banner, Empty, Group, Icon, Pill, Stat } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

const FILES: Record<ReportFileKind, { name: string; type: string; extension: string }> = {
  diagram: { name: "flussdiagramm.svg", type: "SVG", extension: "svg" },
  excluded: { name: "ausgeschlossene-volltexte.csv", type: "CSV", extension: "csv" },
  included: { name: "eingeschlossen.ris", type: "RIS", extension: "ris" },
  search: { name: "suchanhang.md", type: "Markdown", extension: "md" },
  methods: { name: "methoden-auswahl.md", type: "Markdown", extension: "md" },
};

function LevelRow({ level }: { level: FlowLevel }) {
  const r = level.retrieval;
  return (
    <tr>
      <th scope="row">
        {level.label}
        {level.rounds > 1 && <span className="muted"> · {level.rounds} Runden</span>}
      </th>
      <td>{level.input}</td>
      <td>{r ? r.unavailable + r.open : "–"}</td>
      <td>{level.assessed}</td>
      <td>{level.included}</td>
      <td>{level.excluded}</td>
      <td>
        {level.open + level.not_assigned + (r?.waiting ?? 0)}
        {level.blinded > 0 && <> <Pill tone="violet">{level.blinded} verblindet</Pill></>}
      </td>
    </tr>
  );
}

function TextBlock({ title, text, onSave, footer }: { title: string; text: string; onSave: () => void; footer?: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  }
  return (
    <Group
      title={title}
      footer={footer}
      action={
        <span className="report-actions">
          <button className="small" onClick={copy}><Icon name={copied ? "check" : "list"} size={13} /> {copied ? "Kopiert" : "Kopieren"}</button>
          <button className="small" onClick={onSave}><Icon name="download" size={13} /> Speichern</button>
        </span>
      }
    >
      <pre className="report-text">{text}</pre>
    </Group>
  );
}

export function ReportTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    setReport(null);
    api.report(review.id).then(setReport).catch((e) => setError(errorText(e)));
  }, [review]);

  async function saveFile(kind: ReportFileKind) {
    const file = FILES[kind];
    const path = await save({ defaultPath: file.name, filters: [{ name: file.type, extensions: [file.extension] }] });
    if (path) await run(() => api.exportReport(review.id, kind, path), "Datei gespeichert.");
  }

  if (error) return <Banner kind="error">{error}</Banner>;
  if (!report) return null;
  const flow = report.flow;
  const hasLevels = flow.levels.length > 0;

  return (
    <>
      <div className="stats">
        <Stat label="Identifiziert" value={flow.identified.toLocaleString("de-DE")} detail={`${flow.sources.length} ${flow.sources.length === 1 ? "Import-Quelle" : "Import-Quellen"}`} />
        <Stat label="Dubletten entfernt" value={(flow.exact_duplicates + flow.resolved_duplicates).toLocaleString("de-DE")} detail={`${flow.unique.toLocaleString("de-DE")} ${flow.records_term} bleiben`} />
        <Stat
          label="Eingeschlossen"
          tone={flow.complete ? "inc" : "may"}
          value={flow.included === null ? "–" : flow.included.toLocaleString("de-DE")}
          detail={flow.included === null ? "noch keine Auswahl" : flow.complete ? "Auswahl abgeschlossen" : "vorläufig, die Auswahl läuft noch"}
        />
      </div>

      {report.problems.length > 0 && (
        <Banner kind="error">
          Das Diagramm geht nicht auf: {report.problems.join(" ")} Bitte unter „Nachweis“ die Ketten prüfen.
        </Banner>
      )}

      <Group
        title="Flussdiagramm"
        action={<button className="small" onClick={() => saveFile("diagram")}><Icon name="download" size={13} /> Als SVG speichern</button>}
        footer="Eine Ebene ist eine Station mit allen Runden; für einen Eintrag gilt die letzte Runde. Stichproben wie der Pilot sortieren nichts aus und stehen deshalb nicht im Diagramm. Einträge in einer verblindeten Stufe zählen als offen."
      >
        {/* Das SVG baut der Kern; Texte aus dem Review sind dort maskiert. */}
        <div className="flow-diagram" dangerouslySetInnerHTML={{ __html: report.svg }} />
      </Group>

      {hasLevels ? (
        <Group title="Zahlen je Ebene">
          <table className="report-table">
            <thead>
              <tr>
                <th scope="col">Ebene</th>
                <th scope="col">Eingang</th>
                <th scope="col">Nicht beschafft</th>
                <th scope="col">Geprüft</th>
                <th scope="col">Eingeschlossen</th>
                <th scope="col">Ausgeschlossen</th>
                <th scope="col">Offen</th>
              </tr>
            </thead>
            <tbody>{flow.levels.map((level) => <LevelRow key={level.key} level={level} />)}</tbody>
          </table>
        </Group>
      ) : (
        <Empty icon="chart" title="Noch keine Auswahl">Sobald die erste Stufe nach dem Pilot geöffnet ist, erscheinen hier die Ebenen der Auswahl.</Empty>
      )}

      <Group title="Dateien">
        <div className="report-file">
          <div>
            <strong>Ausgeschlossene Volltexte mit Grund</strong>
            <span className="muted">CSV, {report.n_excluded_rows} {report.n_excluded_rows === 1 ? "Zeile" : "Zeilen"}; auch die nicht beschaffbaren mit Vermerk</span>
          </div>
          <button className="small" onClick={() => saveFile("excluded")}><Icon name="csv" size={13} /> Speichern</button>
        </div>
        <div className="report-file">
          <div>
            <strong>Eingeschlossene {flow.sources_term}</strong>
            <span className="muted">RIS, {flow.included ?? 0} {flow.included === 1 ? flow.record_term : flow.records_term}{flow.complete ? "" : " (vorläufig)"}; lässt sich in Zotero importieren</span>
          </div>
          <button className="small" disabled={!hasLevels} onClick={() => saveFile("included")}><Icon name="download" size={13} /> Speichern</button>
        </div>
      </Group>

      <TextBlock
        title="Methodentext zur Auswahl"
        text={report.methods}
        onSave={() => saveFile("methods")}
        footer="Aus dem Log erzeugt, ohne Namen. Als Grundlage für den Methodenteil gedacht; die Formulierungen lassen sich frei anpassen."
      />
      <TextBlock title="Suchanhang" text={report.search_appendix} onSave={() => saveFile("search")} />
    </>
  );
}
