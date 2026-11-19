// Bericht: Flussdiagramm, Zahlen je Ebene, Protokoll mit Abweichungen, Anhänge und Checkliste. Alles
// kommt fertig aus dem Kern; hier wird nur angezeigt und gespeichert.

import React, { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import { api, canManage, Checklist, errorText, FlowLevel, Protocol, Report, ReportFileKind, ReviewDetail } from "./api";
import { dateText, today } from "./Search";
import { Banner, Empty, Group, Icon, Pill, Row, Stat } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

const FILES: Record<ReportFileKind, { name: string; type: string; extension: string }> = {
  diagram: { name: "flussdiagramm.svg", type: "SVG", extension: "svg" },
  excluded: { name: "ausgeschlossene-volltexte.csv", type: "CSV", extension: "csv" },
  included: { name: "eingeschlossen.ris", type: "RIS", extension: "ris" },
  sources: { name: "eingeschlossene-quellen.csv", type: "CSV", extension: "csv" },
  search: { name: "suchanhang.md", type: "Markdown", extension: "md" },
  methods: { name: "methoden-auswahl.md", type: "Markdown", extension: "md" },
  table: { name: "tabelle-quellen.csv", type: "CSV", extension: "csv" },
  deviations: { name: "protokoll-abweichungen.md", type: "Markdown", extension: "md" },
  checklist: { name: "checkliste.md", type: "Markdown", extension: "md" },
};

const DEVIATION_LABEL: Record<string, string> = {
  criteria: "Kriterien",
  search: "Suche",
  form: "Formular",
  workflow: "Ablauf",
  gate: "Gate",
};

const CHECK_STATUS: Record<string, { label: string; tone: string }> = {
  ready: { label: "liegt vor", tone: "green" },
  open: { label: "fehlt noch", tone: "yellow" },
  manual: { label: "selbst schreiben", tone: "gray" },
  optional: { label: "freiwillig", tone: "gray" },
};

function RegistrationForm({ protocol, onSave, onCancel }: {
  protocol: Protocol;
  onSave: (date: string, registry: string, identifier: string, note: string) => void;
  onCancel: () => void;
}) {
  const r = protocol.registration;
  const [date, setDate] = useState(r?.registered_on ?? today());
  const [registry, setRegistry] = useState(r?.registry ?? "");
  const [identifier, setIdentifier] = useState(r?.identifier ?? "");
  const [note, setNote] = useState(r?.note ?? "");
  return (
    <form
      className="search-form"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(date, registry, identifier, note);
      }}
    >
      <Row>
        <label htmlFor="p-date">Registriert am</label>
        <input id="p-date" type="date" value={date} onChange={(e) => setDate(e.target.value)} required />
      </Row>
      <Row>
        <label htmlFor="p-registry">Register oder Ort <small>z. B. OSF, Zeitschrift, internes Dokument</small></label>
        <input id="p-registry" value={registry} onChange={(e) => setRegistry(e.target.value)} required />
      </Row>
      <Row>
        <label htmlFor="p-id">Kennung oder Adresse <small>falls vorhanden</small></label>
        <input id="p-id" value={identifier} onChange={(e) => setIdentifier(e.target.value)} />
      </Row>
      <Row>
        <label htmlFor="p-note">Anmerkung</label>
        <input id="p-note" value={note} onChange={(e) => setNote(e.target.value)} />
      </Row>
      <div className="sheet-actions">
        <button type="button" onClick={onCancel}>Abbrechen</button>
        <button className="primary" type="submit">{r ? "Angaben berichtigen" : "Registrierung festhalten"}</button>
      </div>
    </form>
  );
}

function ProtocolGroup({ protocol, manage, onRegister, onSave }: {
  protocol: Protocol;
  manage: boolean;
  onRegister: (date: string, registry: string, identifier: string, note: string) => Promise<void>;
  onSave: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const r = protocol.registration;
  return (
    <Group
      title="Protokoll und Abweichungen"
      action={
        <span className="report-actions">
          {manage && !editing && <button className="small" onClick={() => setEditing(true)}>{r ? "Berichtigen" : "Registrierung eintragen"}</button>}
          <button className="small" onClick={onSave}><Icon name="download" size={13} /> Speichern</button>
        </span>
      }
      footer={
        r
          ? "Als Abweichung zählt jede neue Version von Kriterien, Suchen und Ablauf seit dem Eintrag der Registrierung, dazu jede Formularänderung nach der Bestätigung und jedes übersteuerte Gate. Der Zeitpunkt des ersten Eintrags bleibt, auch wenn die Angaben berichtigt werden."
          : "Ohne Registrierung fehlt der Bezugspunkt: Es erscheinen nur Formularänderungen nach der Bestätigung und übersteuerte Gates. Trag die Registrierung ein, bevor Kriterien oder Suchen geändert werden."
      }
    >
      {editing ? (
        <RegistrationForm
          protocol={protocol}
          onCancel={() => setEditing(false)}
          onSave={async (...args) => {
            await onRegister(...args);
            setEditing(false);
          }}
        />
      ) : (
        <>
          <div className="report-file">
            <div>
              <strong>{r ? `Registriert am ${dateText(r.registered_on)}: ${r.registry}` : "Kein Protokoll registriert"}</strong>
              {r && (r.identifier || r.note) && <span className="muted">{[r.identifier, r.note].filter(Boolean).join(" · ")}</span>}
              {r && <span className="muted">Im Review festgehalten am {dateText(r.recorded_at.slice(0, 10))}{r.corrected ? ", später berichtigt" : ""}</span>}
            </div>
          </div>
          {protocol.deviations.length === 0 && <div className="report-file"><div><span className="muted">Keine Abweichungen.</span></div></div>}
          {protocol.deviations.map((d, i) => (
            <div className="report-file" key={i}>
              <div>
                <strong>{d.title}</strong>
                <span className="muted">{dateText(d.at.slice(0, 10))} · {d.note || "ohne Begründung"}</span>
              </div>
              <Pill tone={d.kind === "gate" ? "yellow" : "blue"}>{DEVIATION_LABEL[d.kind] ?? d.kind}</Pill>
            </div>
          ))}
        </>
      )}
    </Group>
  );
}

function ChecklistGroup({ checklist, onSave }: { checklist: Checklist; onSave: () => void }) {
  const ready = checklist.items.filter((i) => i.status === "ready").length;
  const open = checklist.items.filter((i) => i.status === "open").length;
  return (
    <Group
      title={`Checkliste ${checklist.name}`}
      action={<button className="small" onClick={onSave}><Icon name="download" size={13} /> Speichern</button>}
      footer={`${ready} von ${checklist.items.length} Punkten deckt das Review mit Material ab, ${open} ${open === 1 ? "fehlt" : "fehlen"} noch. Die Punkte stehen hier in eigenen Worten; maßgeblich ist die veröffentlichte Checkliste.`}
    >
      <table className="table checklist">
        <thead><tr><th>Nr.</th><th>Punkt</th><th>Stand</th><th>Material</th></tr></thead>
        <tbody>
          {checklist.items.map((item) => (
            <tr key={item.number}>
              <td>{item.number}</td>
              <td>{item.topic}<div className="muted small-text">{item.section}</div></td>
              <td><Pill tone={CHECK_STATUS[item.status]?.tone}>{CHECK_STATUS[item.status]?.label ?? item.status}</Pill></td>
              <td className="muted">{item.material}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Group>
  );
}

function LevelRow({ level, branch }: { level: FlowLevel; branch: boolean }) {
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
      {branch && <td>{level.other_assessed}</td>}
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

  const load = () => api.report(review.id).then(setReport).catch((e) => setError(errorText(e)));
  useEffect(() => {
    setReport(null);
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
  // Zweig „andere Quellen“: Treffer aus Zitationssuche und anderen Wegen
  const branch = flow.other_identified > 0;
  const sources = flow.included_sources ?? flow.included;
  const bundled = flow.included !== null && sources !== null && sources !== flow.included;

  return (
    <>
      <div className="stats">
        <Stat label="Identifiziert" value={flow.identified.toLocaleString("de-DE")} detail={`${flow.sources.length} ${flow.sources.length === 1 ? "Import-Quelle" : "Import-Quellen"}${branch ? `, davon ${flow.other_identified} aus anderen Quellen` : ""}`} />
        <Stat label="Dubletten entfernt" value={(flow.exact_duplicates + flow.resolved_duplicates).toLocaleString("de-DE")} detail={`${flow.unique.toLocaleString("de-DE")} ${flow.records_term} bleiben`} />
        <Stat
          label="Eingeschlossen"
          tone={flow.complete ? "inc" : "may"}
          value={sources === null ? "–" : sources.toLocaleString("de-DE")}
          detail={[
            sources === null ? "noch keine Auswahl" : flow.complete ? "Auswahl abgeschlossen" : "vorläufig, die Auswahl läuft noch",
            bundled ? `${flow.sources_term}, gebündelt aus ${flow.included} ${flow.included === 1 ? "Eintrag" : `${flow.records_term}n`}` : "",
          ].filter(Boolean).join(" · ")}
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
                {branch && <th scope="col">davon andere Quellen</th>}
                <th scope="col">Eingeschlossen</th>
                <th scope="col">Ausgeschlossen</th>
                <th scope="col">Offen</th>
              </tr>
            </thead>
            <tbody>{flow.levels.map((level) => <LevelRow key={level.key} level={level} branch={branch} />)}</tbody>
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
        <div className="report-file">
          <div>
            <strong>{flow.sources_term} mit Bündeln und Quellenart</strong>
            <span className="muted">
              CSV, {sources ?? 0} {sources === 1 ? flow.source_term : flow.sources_term}; je {flow.record_term} eine Zeile, {flow.records_term} desselben Bündels tragen dieselbe Nummer
              {flow.source_types.length > 0 && ` · ${flow.source_types.map((t) => `${t.label}: ${t.n}`).join(", ")}`}
            </span>
          </div>
          <button className="small" disabled={!hasLevels} onClick={() => saveFile("sources")}><Icon name="csv" size={13} /> Speichern</button>
        </div>
        <div className="report-file">
          <div>
            <strong>Tabelle der eingeschlossenen {flow.sources_term}</strong>
            <span className="muted">
              CSV, je {flow.source_term} eine Zeile mit den gültigen Werten aus dem Formular
              {report.table.n_fields > 0
                ? ` · ${report.table.n_charted} von ${report.table.n_sources} extrahiert und abgeglichen, ${report.table.n_fields} Felder`
                : " · noch ohne Werte, das Formular kommt mit der ersten Stufe dazu"}
            </span>
          </div>
          <button className="small" disabled={!hasLevels} onClick={() => saveFile("table")}><Icon name="csv" size={13} /> Speichern</button>
        </div>
      </Group>

      <ProtocolGroup
        protocol={report.protocol}
        manage={canManage(review.role)}
        onSave={() => saveFile("deviations")}
        onRegister={(date, registry, identifier, note) =>
          run(async () => {
            await api.registerProtocol(review.id, date, registry, identifier, note);
            await load();
          }, "Registrierung festgehalten.")
        }
      />

      <TextBlock
        title="Methodentext"
        text={report.methods}
        onSave={() => saveFile("methods")}
        footer="Aus dem Log erzeugt, ohne Namen. Als Grundlage für den Methodenteil gedacht; die Formulierungen lassen sich frei anpassen."
      />
      <TextBlock title="Suchanhang" text={report.search_appendix} onSave={() => saveFile("search")} />
      {report.checklist && <ChecklistGroup checklist={report.checklist} onSave={() => saveFile("checklist")} />}
    </>
  );
}
