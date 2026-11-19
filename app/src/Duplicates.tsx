// Mögliche Dubletten: Paare, die der exakte Abgleich beim Import nicht erkennt, nebeneinander zur Entscheidung.

import React, { useEffect, useState } from "react";

import { api, canManage, DuplicateList, DuplicatePair, errorText, PairDecision, RecordItem, ReviewDetail } from "./api";
import { t } from "./i18n";
import { Banner, Group, Icon, Pill } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;

function Side({ record, source, other, kept }: { record: RecordItem; source: string; other: RecordItem; kept: boolean }) {
  // Felder, die sich vom Gegenüber unterscheiden, sind hervorgehoben.
  const differs = (a: string | number | null, b: string | number | null) => (String(a ?? "") !== String(b ?? "") ? "differs" : "");
  return (
    <div className={`dup-side${kept ? " kept" : ""}`}>
      <p className={`dup-title ${differs(record.title, other.title)}`}>{record.title}</p>
      <p className={differs(record.authors, other.authors)}>{record.authors || <span className="muted">{t("ohne Autoren")}</span>}</p>
      <p>
        <span className={differs(record.year, other.year)}>{record.year ?? t("ohne Jahr")}</span>
        {record.journal && <span className={differs(record.journal, other.journal)}> · {record.journal}</span>}
      </p>
      <p className={`dup-ids ${differs(record.doi, other.doi)}`}>{record.doi ? `DOI ${record.doi}` : t("ohne DOI")}</p>
      <p className="muted">
        {source || t("ohne Quelle")} · {record.abstract ? t("mit Abstract") : t("ohne Abstract")}
        {kept && <> · <strong>{t("Vorschlag: bleibt")}</strong></>}
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
    t("{n} beim Import erkannt", { n: list.n_exact }),
    t("{n} als Dublette entschieden", { n: list.n_resolved }),
    t("{n} als verschieden entschieden", { n: list.n_distinct }),
  ].join(" · ");

  return (
    <Group
      title={list.total > 0 ? t("Mögliche Dubletten ({n})", { n: list.total }) : t("Dubletten")}
      footer={
        list.total > 0
          ? t("{counts}. Vorgeschlagen werden Paare mit gleichem oder sehr ähnlichem Titel. Wer als Dublette entschieden ist, fehlt in neuen Stufen; offene Stufen ändern sich nicht.", { counts })
          : t("{counts}. Keine offenen Paare.", { counts })
      }
      action={manage && same.length > 1 && (
        <button className="small" onClick={() => resolve(same.map((p) => duplicate(p, p.keep)), t("{n} Paare als Dubletten entschieden.", { n: same.length }))}>
          {t("Alle {n} mit gleichem Titel bestätigen", { n: same.length })}
        </button>
      )}
    >
      {list.pairs.map((p) => (
        <div className="dup-pair" key={`${p.a.id}-${p.b.id}`}>
          <div className="dup-head">
            <Pill tone={p.similarity === 100 ? "green" : "yellow"}>{p.similarity === 100 ? t("gleicher Titel") : t("Titel zu {percent} % gleich", { percent: p.similarity })}</Pill>
          </div>
          <div className="dup-sides">
            <Side record={p.a} source={p.a_source} other={p.b} kept={p.keep === p.a.id} />
            <Side record={p.b} source={p.b_source} other={p.a} kept={p.keep === p.b.id} />
          </div>
          {manage && (
            <div className="dup-actions">
              <button className="small" onClick={() => resolve([duplicate(p, p.a.id)], t("Als Dublette entschieden."))}><Icon name="check" size={13} /> {t("Dublette, linken behalten")}</button>
              <button className="small" onClick={() => resolve([duplicate(p, p.b.id)], t("Als Dublette entschieden."))}><Icon name="check" size={13} /> {t("Dublette, rechten behalten")}</button>
              <button className="small" onClick={() => resolve([distinct(p)], t("Als verschieden entschieden."))}><Icon name="x" size={13} /> {t("Keine Dublette")}</button>
            </div>
          )}
        </div>
      ))}
      {list.total > list.pairs.length && (
        <p className="muted dup-more">{t("Gezeigt sind die ersten {shown} von {total} Paaren; nach dem Entscheiden rücken weitere nach.", { shown: list.pairs.length, total: list.total })}</p>
      )}
    </Group>
  );
}
