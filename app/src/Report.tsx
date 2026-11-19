// Bericht: Flussdiagramm, Zahlen je Ebene, Protokoll mit Abweichungen, Anhänge und Checkliste. Alles
// kommt fertig aus dem Kern; hier wird nur angezeigt und gespeichert.

import React, { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import { api, canManage, Checklist, errorText, FlowLevel, Protocol, Report, ReportFileKind, ReviewDetail, term } from "./api";
import { formatDate, formatNumber, Key, outputLanguage, setOutputLanguage, t, tIn, tn } from "./i18n";
import { today } from "./Search";
import { Banner, Empty, Group, Icon, LanguageSelect, Pill, Row, Stat } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

// Dateinamen sind Schlüssel des Katalogs; gespeichert wird unter dem Namen in der Sprache des Berichts.
const FILES: Record<ReportFileKind, { name: Key; type: string; extension: string }> = {
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
  criteria: t("Kriterien"),
  search: t("Suche"),
  form: t("Formular"),
  workflow: t("Ablauf"),
  gate: t("Gate"),
};

const CHECK_STATUS: Record<string, { label: string; tone: string }> = {
  ready: { label: t("liegt vor"), tone: "green" },
  open: { label: t("fehlt noch"), tone: "yellow" },
  manual: { label: t("selbst schreiben"), tone: "gray" },
  optional: { label: t("freiwillig"), tone: "gray" },
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
        <label htmlFor="p-date">{t("Registriert am")}</label>
        <input id="p-date" type="date" value={date} onChange={(e) => setDate(e.target.value)} required />
      </Row>
      <Row>
        <label htmlFor="p-registry">{t("Register oder Ort")} <small>{t("z. B. OSF, Zeitschrift, internes Dokument")}</small></label>
        <input id="p-registry" value={registry} onChange={(e) => setRegistry(e.target.value)} required />
      </Row>
      <Row>
        <label htmlFor="p-id">{t("Kennung oder Adresse")} <small>{t("falls vorhanden")}</small></label>
        <input id="p-id" value={identifier} onChange={(e) => setIdentifier(e.target.value)} />
      </Row>
      <Row>
        <label htmlFor="p-note">{t("Anmerkung")}</label>
        <input id="p-note" value={note} onChange={(e) => setNote(e.target.value)} />
      </Row>
      <div className="sheet-actions">
        <button type="button" onClick={onCancel}>{t("Abbrechen")}</button>
        <button className="primary" type="submit">{r ? t("Angaben berichtigen") : t("Registrierung festhalten")}</button>
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
      title={t("Protokoll und Abweichungen")}
      action={
        <span className="report-actions">
          {manage && !editing && <button className="small" onClick={() => setEditing(true)}>{r ? t("Berichtigen") : t("Registrierung eintragen")}</button>}
          <button className="small" onClick={onSave}><Icon name="download" size={13} /> {t("Speichern")}</button>
        </span>
      }
      footer={
        r
          ? t("Als Abweichung zählt jede neue Version von Kriterien, Suchen und Ablauf seit dem Eintrag der Registrierung, dazu jede Formularänderung nach der Bestätigung und jedes übersteuerte Gate. Der Zeitpunkt des ersten Eintrags bleibt, auch wenn die Angaben berichtigt werden.")
          : t("Ohne Registrierung fehlt der Bezugspunkt: Es erscheinen nur Formularänderungen nach der Bestätigung und übersteuerte Gates. Trag die Registrierung ein, bevor Kriterien oder Suchen geändert werden.")
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
              <strong>{r ? t("Registriert am {date}: {registry}", { date: formatDate(r.registered_on), registry: r.registry }) : t("Kein Protokoll registriert")}</strong>
              {r && (r.identifier || r.note) && <span className="muted">{[r.identifier, r.note].filter(Boolean).join(" · ")}</span>}
              {r && (
                <span className="muted">
                  {r.corrected
                    ? t("Im Review festgehalten am {date}, später berichtigt", { date: formatDate(r.recorded_at.slice(0, 10)) })
                    : t("Im Review festgehalten am {date}", { date: formatDate(r.recorded_at.slice(0, 10)) })}
                </span>
              )}
            </div>
          </div>
          {protocol.deviations.length === 0 && <div className="report-file"><div><span className="muted">{t("Keine Abweichungen.")}</span></div></div>}
          {protocol.deviations.map((d, i) => (
            <div className="report-file" key={i}>
              <div>
                <strong>{d.title}</strong>
                <span className="muted">{formatDate(d.at.slice(0, 10))} · {d.note || t("ohne Begründung")}</span>
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
      title={t("Checkliste {name}", { name: checklist.name })}
      action={<button className="small" onClick={onSave}><Icon name="download" size={13} /> {t("Speichern")}</button>}
      footer={tn(
        "{ready} von {total} Punkten deckt das Review mit Material ab, {n} fehlt noch. Die Punkte stehen hier in eigenen Worten; maßgeblich ist die veröffentlichte Checkliste.",
        "{ready} von {total} Punkten deckt das Review mit Material ab, {n} fehlen noch. Die Punkte stehen hier in eigenen Worten; maßgeblich ist die veröffentlichte Checkliste.",
        open,
        { ready, total: checklist.items.length },
      )}
    >
      <table className="table checklist">
        <thead><tr><th>{t("Nr.")}</th><th>{t("Punkt")}</th><th>{t("Stand")}</th><th>{t("Material")}</th></tr></thead>
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
        {level.rounds > 1 && <span className="muted"> · {tn("{n} Runde", "{n} Runden", level.rounds)}</span>}
      </th>
      <td>{level.input}</td>
      <td>{r ? r.unavailable + r.open : "–"}</td>
      <td>{level.assessed}</td>
      {branch && <td>{level.other_assessed}</td>}
      <td>{level.included}</td>
      <td>{level.excluded}</td>
      <td>
        {level.open + level.not_assigned + (r?.waiting ?? 0)}
        {level.blinded > 0 && <> <Pill tone="violet">{t("{n} verblindet", { n: level.blinded })}</Pill></>}
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
          <button className="small" onClick={copy}><Icon name={copied ? "check" : "list"} size={13} /> {copied ? t("Kopiert") : t("Kopieren")}</button>
          <button className="small" onClick={onSave}><Icon name="download" size={13} /> {t("Speichern")}</button>
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

  // Diagramm, Texte und Dateien erscheinen in der Sprache, in der berichtet wird.
  const [lang, setLang] = useState(outputLanguage);
  const load = () => api.report(review.id, lang).then(setReport).catch((e) => setError(errorText(e)));
  useEffect(() => {
    setReport(null);
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [review, lang]);

  async function saveFile(kind: ReportFileKind) {
    const file = FILES[kind];
    const path = await save({ defaultPath: tIn(lang, file.name), filters: [{ name: file.type, extensions: [file.extension] }] });
    if (path) await run(() => api.exportReport(review.id, kind, path, lang), t("Datei gespeichert."));
  }

  if (error) return <Banner kind="error">{error}</Banner>;
  if (!report) return null;
  const flow = report.flow;
  // Sätze der Oberfläche brauchen die Begriffe in ihrer Sprache; der Bericht liefert sie in der Sprache der Ausgabe.
  const terms = {
    record_term: term(review.workflow.terms, "record", t("Eintrag")),
    records_term: term(review.workflow.terms, "records", t("Einträge")),
    source_term: term(review.workflow.terms, "source", t("Quelle")),
    sources_term: term(review.workflow.terms, "sources", t("Quellen")),
  };
  const hasLevels = flow.levels.length > 0;
  // Zweig „andere Quellen“: Treffer aus Zitationssuche und anderen Wegen
  const branch = flow.other_identified > 0;
  const sources = flow.included_sources ?? flow.included;
  const bundled = flow.included !== null && sources !== null && sources !== flow.included;
  const nIncluded = flow.included ?? 0;
  const includedTerm = flow.included === 1 ? terms.record_term : terms.records_term;

  return (
    <>
      <div className="stats">
        <Stat
          label={t("Identifiziert")}
          value={formatNumber(flow.identified)}
          detail={
            branch
              ? tn("{n} Import-Quelle, davon {other} aus anderen Quellen", "{n} Import-Quellen, davon {other} aus anderen Quellen", flow.sources.length, { other: flow.other_identified })
              : tn("{n} Import-Quelle", "{n} Import-Quellen", flow.sources.length)
          }
        />
        <Stat
          label={t("Dubletten entfernt")}
          value={formatNumber(flow.exact_duplicates + flow.resolved_duplicates)}
          detail={t("{count} {records} bleiben", { count: formatNumber(flow.unique), records: terms.records_term })}
        />
        <Stat
          label={t("Eingeschlossen")}
          tone={flow.complete ? "inc" : "may"}
          value={sources === null ? "–" : formatNumber(sources)}
          detail={[
            sources === null ? t("noch keine Auswahl") : flow.complete ? t("Auswahl abgeschlossen") : t("vorläufig, die Auswahl läuft noch"),
            bundled
              ? tn("{sources}, gebündelt aus {n} Eintrag", "{sources}, gebündelt aus {n} {records}n", nIncluded, { sources: terms.sources_term, records: terms.records_term })
              : "",
          ].filter(Boolean).join(" · ")}
        />
      </div>

      {report.problems.length > 0 && (
        <Banner kind="error">
          {t("Das Diagramm geht nicht auf: {problems} Bitte unter „Nachweis“ die Ketten prüfen.", { problems: report.problems.join(" ") })}
        </Banner>
      )}

      <Group
        title={t("Flussdiagramm")}
        action={
          <span className="inline-actions">
            <LanguageSelect value={lang} label={t("Sprache der Ausgabe")} onChange={(code) => { setOutputLanguage(code); setLang(code); }} />
            <button className="small" onClick={() => saveFile("diagram")}><Icon name="download" size={13} /> {t("Als SVG speichern")}</button>
          </span>
        }
        footer={t("Eine Ebene ist eine Station mit allen Runden; für einen Eintrag gilt die letzte Runde. Stichproben wie der Pilot sortieren nichts aus und stehen deshalb nicht im Diagramm. Einträge in einer verblindeten Stufe zählen als offen.")}
      >
        {/* Das SVG baut der Kern; Texte aus dem Review sind dort maskiert. */}
        <div className="flow-diagram" dangerouslySetInnerHTML={{ __html: report.svg }} />
      </Group>

      {hasLevels ? (
        <Group title={t("Zahlen je Ebene")}>
          <table className="report-table">
            <thead>
              <tr>
                <th scope="col">{t("Ebene")}</th>
                <th scope="col">{t("Eingang")}</th>
                <th scope="col">{t("Nicht beschafft")}</th>
                <th scope="col">{t("Geprüft")}</th>
                {branch && <th scope="col">{t("davon andere Quellen")}</th>}
                <th scope="col">{t("Eingeschlossen")}</th>
                <th scope="col">{t("Ausgeschlossen")}</th>
                <th scope="col">{t("Offen")}</th>
              </tr>
            </thead>
            <tbody>{flow.levels.map((level) => <LevelRow key={level.key} level={level} branch={branch} />)}</tbody>
          </table>
        </Group>
      ) : (
        <Empty icon="chart" title={t("Noch keine Auswahl")}>{t("Sobald die erste Stufe nach dem Pilot geöffnet ist, erscheinen hier die Ebenen der Auswahl.")}</Empty>
      )}

      <Group title={t("Dateien")}>
        <div className="report-file">
          <div>
            <strong>{t("Ausgeschlossene Volltexte mit Grund")}</strong>
            <span className="muted">
              {tn("CSV, {n} Zeile; auch die nicht beschaffbaren mit Vermerk", "CSV, {n} Zeilen; auch die nicht beschaffbaren mit Vermerk", report.n_excluded_rows)}
            </span>
          </div>
          <button className="small" onClick={() => saveFile("excluded")}><Icon name="csv" size={13} /> {t("Speichern")}</button>
        </div>
        <div className="report-file">
          <div>
            <strong>{t("Eingeschlossene {sources}", { sources: terms.sources_term })}</strong>
            <span className="muted">
              {flow.complete
                ? t("RIS, {count} {records}; lässt sich in Zotero importieren", { count: nIncluded, records: includedTerm })
                : t("RIS, {count} {records} (vorläufig); lässt sich in Zotero importieren", { count: nIncluded, records: includedTerm })}
            </span>
          </div>
          <button className="small" disabled={!hasLevels} onClick={() => saveFile("included")}><Icon name="download" size={13} /> {t("Speichern")}</button>
        </div>
        <div className="report-file">
          <div>
            <strong>{t("{sources} mit Bündeln und Quellenart", { sources: terms.sources_term })}</strong>
            <span className="muted">
              {t("CSV, {count} {sources}; je {record} eine Zeile, {records} desselben Bündels tragen dieselbe Nummer", {
                count: sources ?? 0,
                sources: sources === 1 ? terms.source_term : terms.sources_term,
                record: terms.record_term,
                records: terms.records_term,
              })}
              {flow.source_types.length > 0 && ` · ${flow.source_types.map((type) => `${type.label}: ${type.n}`).join(", ")}`}
            </span>
          </div>
          <button className="small" disabled={!hasLevels} onClick={() => saveFile("sources")}><Icon name="csv" size={13} /> {t("Speichern")}</button>
        </div>
        <div className="report-file">
          <div>
            <strong>{t("Tabelle der eingeschlossenen {sources}", { sources: terms.sources_term })}</strong>
            <span className="muted">
              {t("CSV, je {source} eine Zeile mit den gültigen Werten aus dem Formular", { source: terms.source_term })}
              {" · "}
              {report.table.n_fields > 0
                ? tn("{charted} von {total} extrahiert und abgeglichen, {n} Feld", "{charted} von {total} extrahiert und abgeglichen, {n} Felder", report.table.n_fields, {
                    charted: report.table.n_charted,
                    total: report.table.n_sources,
                  })
                : t("noch ohne Werte, das Formular kommt mit der ersten Stufe dazu")}
            </span>
          </div>
          <button className="small" disabled={!hasLevels} onClick={() => saveFile("table")}><Icon name="csv" size={13} /> {t("Speichern")}</button>
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
          }, t("Registrierung festgehalten."))
        }
      />

      <TextBlock
        title={t("Methodentext")}
        text={report.methods}
        onSave={() => saveFile("methods")}
        footer={t("Aus dem Log erzeugt, ohne Namen. Als Grundlage für den Methodenteil gedacht; die Formulierungen lassen sich frei anpassen.")}
      />
      <TextBlock title={t("Suchanhang")} text={report.search_appendix} onSave={() => saveFile("search")} />
      {report.checklist && <ChecklistGroup checklist={report.checklist} onSave={() => saveFile("checklist")} />}
    </>
  );
}
