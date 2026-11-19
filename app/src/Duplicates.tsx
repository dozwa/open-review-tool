// Mögliche Dubletten: Paare, die der exakte Abgleich beim Import nicht erkennt, nebeneinander zur Entscheidung.

import React, { useEffect, useState } from "react";

import { api, canManage, DuplicateList, DuplicatePair, errorText, PairDecision, RecordItem, ReviewDetail } from "./api";
import { Banner, Group, Icon, Pill } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

function Side({ record, source, other, kept }: { record: RecordItem; source: string; other: RecordItem; kept: boolean }) {
  // Felder, die sich vom Gegenüber unterscheiden, sind hervorgehoben.
  const differs = (a: string | number | null, b: string | number | null) => (String(a ?? "") !== String(b ?? "") ? "differs" : "");
  return (
    <div className={`dup-side${kept ? " kept" : ""}`}>
      <p className={`dup-title ${differs(record.title, other.title)}`}>{record.title}</p>
      <p className={differs(record.authors, other.authors)}>{record.authors || <span className="muted">ohne Autoren</span>}</p>
      <p>
        <span className={differs(record.year, other.year)}>{record.year ?? "ohne Jahr"}</span>
        {record.journal && <span className={differs(record.journal, other.journal)}> · {record.journal}</span>}
      </p>
      <p className={`dup-ids ${differs(record.doi, other.doi)}`}>{record.doi ? `DOI ${record.doi}` : "ohne DOI"}</p>
      <p className="muted">
        {source || "ohne Quelle"} · {record.abstract ? "mit Abstract" : "ohne Abstract"}
        {kept && <> · <strong>Vorschlag: bleibt</strong></>}
      </p>
    </div>
  );
}

export function Duplicates({ review, run }: { review: ReviewDetail; run: Run }) {
  const manage = canManage(review.role);
  const [list, setList] = useState<DuplicateList | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    api.duplicates(review.id).then(setList).catch((e) => setError(errorText(e)));
  }, [review]);

  if (error) return <Banner kind="error">{error}</Banner>;
  if (!list || (list.total === 0 && list.n_exact + list.n_resolved + list.n_distinct === 0)) return null;

  const resolve = (pairs: PairDecision[], done: string) => run(async () => void (await api.resolveDuplicates(review.id, pairs)), done);
  const duplicate = (p: DuplicatePair, keep: string): PairDecision => ({ a: p.a.id, b: p.b.id, decision: "duplicate", keep });
  const distinct = (p: DuplicatePair): PairDecision => ({ a: p.a.id, b: p.b.id, decision: "distinct" });
  const same = list.pairs.filter((p) => p.similarity === 100);

  const counts = [
    `${list.n_exact} beim Import erkannt`,
    `${list.n_resolved} als Dublette entschieden`,
    `${list.n_distinct} als verschieden entschieden`,
  ].join(" · ");

  return (
    <Group
      title={list.total > 0 ? `Mögliche Dubletten (${list.total})` : "Dubletten"}
      footer={
        list.total > 0
          ? `${counts}. Vorgeschlagen werden Paare mit gleichem oder sehr ähnlichem Titel. Wer als Dublette entschieden ist, fehlt in neuen Stufen; offene Stufen ändern sich nicht.`
          : `${counts}. Keine offenen Paare.`
      }
      action={manage && same.length > 1 && (
        <button className="small" onClick={() => resolve(same.map((p) => duplicate(p, p.keep)), `${same.length} Paare als Dubletten entschieden.`)}>
          Alle {same.length} mit gleichem Titel bestätigen
        </button>
      )}
    >
      {list.pairs.map((p) => (
        <div className="dup-pair" key={`${p.a.id}-${p.b.id}`}>
          <div className="dup-head">
            <Pill tone={p.similarity === 100 ? "green" : "yellow"}>{p.similarity === 100 ? "gleicher Titel" : `Titel zu ${p.similarity} % gleich`}</Pill>
          </div>
          <div className="dup-sides">
            <Side record={p.a} source={p.a_source} other={p.b} kept={p.keep === p.a.id} />
            <Side record={p.b} source={p.b_source} other={p.a} kept={p.keep === p.b.id} />
          </div>
          {manage && (
            <div className="dup-actions">
              <button className="small" onClick={() => resolve([duplicate(p, p.a.id)], "Als Dublette entschieden.")}><Icon name="check" size={13} /> Dublette, linken behalten</button>
              <button className="small" onClick={() => resolve([duplicate(p, p.b.id)], "Als Dublette entschieden.")}><Icon name="check" size={13} /> Dublette, rechten behalten</button>
              <button className="small" onClick={() => resolve([distinct(p)], "Als verschieden entschieden.")}><Icon name="x" size={13} /> Keine Dublette</button>
            </div>
          )}
        </div>
      ))}
      {list.total > list.pairs.length && (
        <p className="muted dup-more">Gezeigt sind die ersten {list.pairs.length} von {list.total} Paaren; nach dem Entscheiden rücken weitere nach.</p>
      )}
    </Group>
  );
}
