// Quellen: Einträge derselben Studie zu einer Quelle bündeln und jeder Quelle eine Quellenart geben.
// Was eine Quelle ist und wie gezählt wird, rechnet der Kern.

import React, { useEffect, useState } from "react";

import { api, canManage, errorText, ReviewDetail, SourceList, SourceRecord, SourceView, term } from "./api";
import { Banner, Empty, Group, Icon, Pill, Stat } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

function RecordLine({ record }: { record: SourceRecord }) {
  return (
    <>
      <p className="source-title">{record.title}</p>
      <p className="muted">
        {[record.authors, record.year, record.journal].filter(Boolean).join(" · ") || "ohne Angaben"}
        {record.doi && ` · DOI ${record.doi}`}
      </p>
    </>
  );
}

function TypeField({ source, types, disabled, onChange }: {
  source: SourceView;
  types: SourceList["source_types"];
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const [text, setText] = useState(source.source_type);
  useEffect(() => setText(source.source_type), [source.source_type]);
  const label = `Quellenart von „${source.records[0]?.title ?? ""}“`;
  if (types.length === 0) {
    return (
      <input
        className="source-type"
        aria-label={label}
        value={text}
        disabled={disabled}
        placeholder="Quellenart"
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text.trim() !== source.source_type && onChange(text)}
      />
    );
  }
  const known = types.some((t) => t.key === source.source_type);
  return (
    <select className="source-type" aria-label={label} value={source.source_type} disabled={disabled} onChange={(e) => onChange(e.target.value)}>
      <option value="">ohne Quellenart</option>
      {!known && source.source_type && <option value={source.source_type}>{source.source_type}</option>}
      {types.map((t) => <option key={t.key} value={t.key}>{t.label}</option>)}
    </select>
  );
}

export function SourcesTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const manage = canManage(review.role);
  const terms = review.workflow.terms;
  const [list, setList] = useState<SourceList | null>(null);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<string[]>([]);
  const [label, setLabel] = useState("");

  useEffect(() => {
    api.sources(review.id).then((found) => {
      setList(found);
      // Was inzwischen gebündelt oder nicht mehr dabei ist, fällt aus der Auswahl.
      setSelected((current) => current.filter((id) => found.sources.some((s) => s.id === id && !s.study_id)));
    }).catch((e) => setError(errorText(e)));
  }, [review]);

  if (error) return <Banner kind="error">{error}</Banner>;
  if (!list) return null;
  if (list.n_records === 0) {
    return (
      <Empty icon="stack" title={`Noch keine eingeschlossenen ${term(terms, "sources", "Quellen")}`}>
        Gebündelt wird, was am Ende der Auswahl eingeschlossen ist. Sobald dort Entscheidungen stehen, erscheinen die {term(terms, "records", "Einträge")} hier.
      </Empty>
    );
  }

  const toggle = (id: string) => setSelected((current) => (current.includes(id) ? current.filter((x) => x !== id) : [...current, id]));
  const ids = (source: SourceView) => source.records.map((r) => r.id);
  const setType = (records: string[], value: string) =>
    run(async () => void (await api.setSourceType(review.id, records, value)), value ? "Quellenart gesetzt." : "Quellenart zurückgenommen.");
  // In der Reihenfolge der Liste; der erste gewählte Eintrag wird Haupteintrag.
  const chosen = list.sources.filter((s) => selected.includes(s.id)).map((s) => s.id);
  const bundle = () =>
    run(async () => {
      await api.linkStudy(review.id, null, label, chosen, chosen[0]);
      setSelected([]);
      setLabel("");
    }, `${chosen.length} ${term(terms, "records", "Einträge")} zu einer ${term(terms, "source", "Quelle")} gebündelt.`);
  const makePrimary = (source: SourceView, primary: string) =>
    run(async () => void (await api.linkStudy(review.id, source.study_id, source.label, ids(source), primary)), "Haupteintrag geändert.");
  const dissolve = (source: SourceView) =>
    run(() => api.dissolveStudy(review.id, source.study_id ?? ""), "Bündel gelöst.");
  const bundles = list.sources.filter((s) => s.study_id).length;

  return (
    <>
      <div className="stats">
        <Stat label={`Eingeschlossene ${term(terms, "records", "Einträge")}`} value={list.n_records.toLocaleString("de-DE")} detail={list.level ? `aus „${list.level}“` : undefined} />
        <Stat label={term(terms, "sources", "Quellen")} tone="inc" value={list.n_sources.toLocaleString("de-DE")} detail={bundles === 1 ? "1 Bündel" : `${bundles} Bündel`} />
        <Stat
          label="Ohne Quellenart"
          tone={list.n_without_type > 0 ? "may" : undefined}
          value={list.n_without_type.toLocaleString("de-DE")}
          detail={list.by_type.filter((t) => t.key).map((t) => `${t.label}: ${t.n}`).join(" · ") || undefined}
        />
      </div>

      {!list.complete && (
        <Banner kind="notice">Die Auswahl läuft noch. Es können weitere {term(terms, "sources", "Quellen")} dazukommen; was hier gebündelt wird, bleibt bestehen.</Banner>
      )}

      {manage && chosen.length > 0 && (
        <Group title={`${chosen.length} gewählt`}>
          <div className="source-actions">
            {chosen.length > 1 && (
              <>
                <input aria-label="Bezeichnung des Bündels" value={label} maxLength={200} onChange={(e) => setLabel(e.target.value)} placeholder="Bezeichnung, z. B. Name der Studie (optional)" />
                <button className="primary" onClick={bundle}><Icon name="stack" size={14} /> Zu einer {term(terms, "source", "Quelle")} bündeln</button>
              </>
            )}
            {list.source_types.length > 0 && (
              <select aria-label="Quellenart für die Auswahl" value="" onChange={(e) => e.target.value && setType(chosen, e.target.value)}>
                <option value="">Quellenart für die Auswahl …</option>
                {list.source_types.map((t) => <option key={t.key} value={t.key}>{t.label}</option>)}
              </select>
            )}
            <button className="small" onClick={() => setSelected([])}>Auswahl aufheben</button>
          </div>
        </Group>
      )}

      <Group
        title={term(terms, "sources", "Quellen")}
        footer={`Mehrere Veröffentlichungen derselben Studie zählen als eine ${term(terms, "source", "Quelle")}. Der Haupteintrag steht für das Bündel; seine Quellenart gilt für alle. Sortiert nach Erstautor, damit Begleitpublikationen nebeneinander stehen.`}
      >
        {list.sources.map((source) => (
          <div className={`source${source.study_id ? " bundled" : ""}`} key={source.id}>
            <div className="source-head">
              {manage && !source.study_id && (
                <input type="checkbox" aria-label={`„${source.records[0]?.title ?? ""}“ wählen`} checked={selected.includes(source.id)} onChange={() => toggle(source.id)} />
              )}
              <div className="source-main">
                {source.study_id && (
                  <p>
                    <Pill tone="blue">Bündel · {source.records.length} {term(terms, "records", "Einträge")}</Pill>
                    {source.label && <strong> {source.label}</strong>}
                  </p>
                )}
                {source.records.map((record) => (
                  <div className={`source-record${record.primary ? "" : " companion"}`} key={record.id}>
                    <RecordLine record={record} />
                    {source.study_id && (
                      <p className="source-role">
                        {record.primary ? <Pill tone="green">Haupteintrag</Pill> : manage && record.included && (
                          <button className="small" onClick={() => makePrimary(source, record.id)}>Als Haupteintrag</button>
                        )}
                        {!record.included && <Pill>nicht eingeschlossen</Pill>}
                      </p>
                    )}
                  </div>
                ))}
              </div>
              <div className="source-side">
                <TypeField source={source} types={list.source_types} disabled={!manage} onChange={(value) => setType(ids(source), value)} />
                {manage && source.study_id && <button className="small" onClick={() => dissolve(source)}><Icon name="x" size={13} /> Bündel lösen</button>}
              </div>
            </div>
          </div>
        ))}
      </Group>
    </>
  );
}
