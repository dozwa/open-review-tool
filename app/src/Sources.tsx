// Quellen: Einträge derselben Studie zu einer Quelle bündeln und jeder Quelle eine Quellenart geben.
// Was eine Quelle ist und wie gezählt wird, rechnet der Kern.

import React, { useEffect, useState } from "react";

import { api, canManage, errorText, ReviewDetail, SourceList, SourceRecord, SourceView, term } from "./api";
import { capital, formatNumber, t, tn } from "./i18n";
import { Banner, Empty, Group, Icon, Pill, Stat } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

function RecordLine({ record }: { record: SourceRecord }) {
  return (
    <>
      <p className="source-title">{record.title}</p>
      <p className="muted">
        {[record.authors, record.year, record.journal].filter(Boolean).join(" · ") || t("ohne Angaben")}
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
  const label = t("Quellenart von „{title}“", { title: source.records[0]?.title ?? "" });
  if (types.length === 0) {
    return (
      <input
        className="source-type"
        aria-label={label}
        value={text}
        disabled={disabled}
        placeholder={t("Quellenart")}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text.trim() !== source.source_type && onChange(text)}
      />
    );
  }
  const known = types.some((st) => st.key === source.source_type);
  return (
    <select className="source-type" aria-label={label} value={source.source_type} disabled={disabled} onChange={(e) => onChange(e.target.value)}>
      <option value="">{t("ohne Quellenart")}</option>
      {!known && source.source_type && <option value={source.source_type}>{source.source_type}</option>}
      {types.map((st) => <option key={st.key} value={st.key}>{st.label}</option>)}
    </select>
  );
}

export function SourcesTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const manage = canManage(review.role);
  const terms = review.workflow.terms;
  const sourceTerm = term(terms, "source", t("Quelle"));
  const sourcesTerm = term(terms, "sources", t("Quellen"));
  const recordsTerm = term(terms, "records", t("Einträge"));
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
      <Empty icon="stack" title={t("Noch keine eingeschlossenen {sources}", { sources: sourcesTerm })}>
        {t("Gebündelt wird, was am Ende der Auswahl eingeschlossen ist. Sobald dort Entscheidungen stehen, erscheinen die {records} hier.", { records: recordsTerm })}
      </Empty>
    );
  }

  const toggle = (id: string) => setSelected((current) => (current.includes(id) ? current.filter((x) => x !== id) : [...current, id]));
  const ids = (source: SourceView) => source.records.map((r) => r.id);
  const setType = (records: string[], value: string) =>
    run(async () => void (await api.setSourceType(review.id, records, value)), value ? t("Quellenart gesetzt.") : t("Quellenart zurückgenommen."));
  // In der Reihenfolge der Liste; der erste gewählte Eintrag wird Haupteintrag.
  const chosen = list.sources.filter((s) => selected.includes(s.id)).map((s) => s.id);
  const bundle = () =>
    run(async () => {
      await api.linkStudy(review.id, null, label, chosen, chosen[0]);
      setSelected([]);
      setLabel("");
    }, t("{n} {records} zu einer {source} gebündelt.", { n: chosen.length, records: recordsTerm, source: sourceTerm }));
  const makePrimary = (source: SourceView, primary: string) =>
    run(async () => void (await api.linkStudy(review.id, source.study_id, source.label, ids(source), primary)), t("Haupteintrag geändert."));
  const dissolve = (source: SourceView) =>
    run(() => api.dissolveStudy(review.id, source.study_id ?? ""), t("Bündel gelöst."));
  const bundles = list.sources.filter((s) => s.study_id).length;

  return (
    <>
      <div className="stats">
        <Stat label={t("Eingeschlossene {records}", { records: recordsTerm })} value={formatNumber(list.n_records)} detail={list.level ? t("aus „{level}“", { level: list.level }) : undefined} />
        <Stat label={capital(sourcesTerm)} tone="inc" value={formatNumber(list.n_sources)} detail={tn("{n} Bündel", "{n} Bündel", bundles)} />
        <Stat
          label={t("Ohne Quellenart")}
          tone={list.n_without_type > 0 ? "may" : undefined}
          value={formatNumber(list.n_without_type)}
          detail={list.by_type.filter((st) => st.key).map((st) => `${st.label}: ${st.n}`).join(" · ") || undefined}
        />
      </div>

      {!list.complete && (
        <Banner kind="notice">{t("Die Auswahl läuft noch. Es können weitere {sources} dazukommen; was hier gebündelt wird, bleibt bestehen.", { sources: sourcesTerm })}</Banner>
      )}

      {manage && chosen.length > 0 && (
        <Group title={t("{n} gewählt", { n: chosen.length })}>
          <div className="source-actions">
            {chosen.length > 1 && (
              <>
                <input aria-label={t("Bezeichnung des Bündels")} value={label} maxLength={200} onChange={(e) => setLabel(e.target.value)} placeholder={t("Bezeichnung, z. B. Name der Studie (optional)")} />
                <button className="primary" onClick={bundle}><Icon name="stack" size={14} /> {t("Zu einer {source} bündeln", { source: sourceTerm })}</button>
              </>
            )}
            {list.source_types.length > 0 && (
              <select aria-label={t("Quellenart für die Auswahl")} value="" onChange={(e) => e.target.value && setType(chosen, e.target.value)}>
                <option value="">{t("Quellenart für die Auswahl …")}</option>
                {list.source_types.map((st) => <option key={st.key} value={st.key}>{st.label}</option>)}
              </select>
            )}
            <button className="small" onClick={() => setSelected([])}>{t("Auswahl aufheben")}</button>
          </div>
        </Group>
      )}

      <Group
        title={capital(sourcesTerm)}
        footer={t("Mehrere Veröffentlichungen derselben Studie zählen als eine {source}. Der Haupteintrag steht für das Bündel; seine Quellenart gilt für alle. Sortiert nach Erstautor, damit Begleitpublikationen nebeneinander stehen.", { source: sourceTerm })}
      >
        {list.sources.map((source) => (
          <div className={`source${source.study_id ? " bundled" : ""}`} key={source.id}>
            <div className="source-head">
              {manage && !source.study_id && (
                <input type="checkbox" aria-label={t("„{title}“ wählen", { title: source.records[0]?.title ?? "" })} checked={selected.includes(source.id)} onChange={() => toggle(source.id)} />
              )}
              <div className="source-main">
                {source.study_id && (
                  <p>
                    <Pill tone="blue">{t("Bündel · {n} {records}", { n: source.records.length, records: recordsTerm })}</Pill>
                    {source.label && <strong> {source.label}</strong>}
                  </p>
                )}
                {source.records.map((record) => (
                  <div className={`source-record${record.primary ? "" : " companion"}`} key={record.id}>
                    <RecordLine record={record} />
                    {source.study_id && (
                      <p className="source-role">
                        {record.primary ? <Pill tone="green">{t("Haupteintrag")}</Pill> : manage && record.included && (
                          <button className="small" onClick={() => makePrimary(source, record.id)}>{t("Als Haupteintrag")}</button>
                        )}
                        {!record.included && <Pill>{t("nicht eingeschlossen")}</Pill>}
                      </p>
                    )}
                  </div>
                ))}
              </div>
              <div className="source-side">
                <TypeField source={source} types={list.source_types} disabled={!manage} onChange={(value) => setType(ids(source), value)} />
                {manage && source.study_id && <button className="small" onClick={() => dissolve(source)}><Icon name="x" size={13} /> {t("Bündel lösen")}</button>}
              </div>
            </div>
          </div>
        ))}
      </Group>
    </>
  );
}
