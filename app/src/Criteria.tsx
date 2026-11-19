// Kriterien als Liste mit Kürzeln: Anzeige (Review und Screening-Seite) und Editor für eine neue Version.

import React, { useState } from "react";

import { Criteria, Criterion, SchemaElement } from "./api";
import { t } from "./i18n";
import { Group, Icon } from "./ui";

const KIND_LABEL: Record<string, string> = { include: t("Einschluss"), exclude: t("Ausschluss") };
const CODE_PATTERN = /^[A-Za-z0-9]{1,8}$/;

/** Kriterien nach Element des Schemas gruppiert, mit Erläuterung je Kriterium und Erläuterungsdokument. */
export function CriteriaView({ criteria, schema }: { criteria: Criteria; schema: SchemaElement[] }) {
  if (criteria.items.length === 0 && !criteria.guidance) return <p className="muted criteria-empty">{t("Noch keine Kriterien hinterlegt.")}</p>;
  const known = new Set(schema.map((s) => s.key));
  const groups = [
    ...schema.map((s) => ({ label: s.label, items: criteria.items.filter((c) => c.element === s.key) })),
    { label: schema.length ? t("Weitere") : "", items: criteria.items.filter((c) => !known.has(c.element)) },
  ].filter((g) => g.items.length > 0);
  return (
    <div className="criteria-view">
      {groups.map((g) => (
        <section key={g.label}>
          {g.label && <h3>{g.label}</h3>}
          <ul>
            {g.items.map((c, i) => (
              <li key={c.code || i}>
                {c.code && <span className={`code code-${c.kind}`}>{c.code}</span>}
                <span>
                  {c.text}
                  {c.note && <small>{c.note}</small>}
                </span>
              </li>
            ))}
          </ul>
        </section>
      ))}
      {criteria.guidance && (
        <section>
          <h3>{t("Erläuterungen")}</h3>
          <p className="guidance">{criteria.guidance}</p>
        </section>
      )}
    </div>
  );
}

/** Nächstes freies Kürzel der Art, z. B. I3 oder E2. */
function nextCode(items: Criterion[], kind: string): string {
  const prefix = kind === "include" ? "I" : "E";
  const used = new Set(items.map((c) => c.code.toUpperCase()));
  let n = 1;
  while (used.has(`${prefix}${n}`)) n += 1;
  return `${prefix}${n}`;
}

/** Kriterien aus der Zeit vor den Kürzeln bekommen im Editor eines vorgeschlagen. */
function withCodes(items: Criterion[]): Criterion[] {
  const out: Criterion[] = [];
  for (const item of items) {
    out.push(CODE_PATTERN.test(item.code) ? item : { ...item, code: nextCode(out, item.kind) });
  }
  return out;
}

export function CriteriaEditor({ criteria, schema, onPublish }: {
  criteria: Criteria;
  schema: SchemaElement[];
  onPublish: (data: Record<string, unknown>) => Promise<void>;
}) {
  const start = criteria.items.length
    ? withCodes(criteria.items)
    : [
        { code: "I1", element: "", kind: "include" as const, text: "", note: "" },
        { code: "E1", element: "", kind: "exclude" as const, text: "", note: "" },
      ];
  const [items, setItems] = useState<Criterion[]>(start);
  const [guidance, setGuidance] = useState(criteria.guidance);
  const [include, setInclude] = useState(criteria.include_keywords.join(", "));
  const [exclude, setExclude] = useState(criteria.exclude_keywords.join(", "));
  const [changeNote, setChangeNote] = useState("");
  const legacy = criteria.items.some((c) => !CODE_PATTERN.test(c.code));

  const change = (index: number, patch: Partial<Criterion>) => setItems(items.map((c, i) => (i === index ? { ...c, ...patch } : c)));
  const add = (kind: "include" | "exclude") => setItems([...items, { code: nextCode(items, kind), element: "", kind, text: "", note: "" }]);
  const duplicate = (code: string) => items.filter((c) => c.code === code).length > 1;

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    // Nach dem Veröffentlichen setzt die Seite den Editor über die neue Versionsnummer zurück.
    await onPublish({ items: items.filter((c) => c.text.trim()), guidance, include_keywords: include, exclude_keywords: exclude, change_note: changeNote });
  }

  return (
    <form onSubmit={submit}>
      <Group
        title={criteria.version ? t("Version {version}", { version: criteria.version }) : t("Noch keine Version")}
        footer={
          legacy
            ? t("Diese Version stammt aus der Zeit vor den Kürzeln. Die vorgeschlagenen Kürzel gelten ab der neuen Version; alte Stufen behalten ihre Gründe.")
            : t("Das Kürzel eines Ausschlusskriteriums ist der Ausschlussgrund im Screening. Jede Änderung ist eine neue Version; Stufen behalten die Version, mit der sie geöffnet wurden.")
        }
      >
        {items.map((c, i) => (
          <div className="criteria-row" key={i}>
            <input
              className={`code-input${duplicate(c.code) || !CODE_PATTERN.test(c.code) ? " invalid" : ""}`}
              value={c.code}
              onChange={(e) => change(i, { code: e.target.value.trim() })}
              aria-label={t("Kürzel von Kriterium {n}", { n: i + 1 })}
              maxLength={8}
              required
            />
            <select value={c.kind} onChange={(e) => change(i, { kind: e.target.value as Criterion["kind"] })} aria-label={t("Art von {code}", { code: c.code })}>
              {Object.entries(KIND_LABEL).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
            {schema.length > 0 && (
              <select value={c.element} onChange={(e) => change(i, { element: e.target.value })} aria-label={t("Element von {code}", { code: c.code })}>
                <option value="">{t("ohne Zuordnung")}</option>
                {schema.map((s) => <option key={s.key} value={s.key}>{s.label}</option>)}
              </select>
            )}
            <input className="criteria-text" value={c.text} onChange={(e) => change(i, { text: e.target.value })} placeholder={t("Kriterium")} aria-label={t("Text von {code}", { code: c.code })} />
            <button type="button" className="plain" onClick={() => setItems(items.filter((_, j) => j !== i))} aria-label={t("{code} entfernen", { code: c.code })}><Icon name="x" size={14} /></button>
            <input className="criteria-note" value={c.note} onChange={(e) => change(i, { note: e.target.value })} placeholder={t("Erläuterung, z. B. Grenzfälle (optional)")} aria-label={t("Erläuterung zu {code}", { code: c.code })} />
          </div>
        ))}
        <div className="criteria-add">
          <button type="button" onClick={() => add("include")}><Icon name="plus" size={14} /> {t("Einschlusskriterium")}</button>
          <button type="button" onClick={() => add("exclude")}><Icon name="plus" size={14} /> {t("Ausschlusskriterium")}</button>
        </div>
      </Group>
      <Group title={t("Erläuterungen und Schlagwörter")}>
        <div className="stack-field">
          <label htmlFor="crit-guidance">{t("Erläuterungsdokument")} <small>{t("wie die Kriterien anzuwenden sind; steht beim Screening neben dem Eintrag")}</small></label>
          <textarea id="crit-guidance" rows={4} value={guidance} onChange={(e) => setGuidance(e.target.value)} />
        </div>
        <div className="stack-field">
          <label htmlFor="crit-inc">{t("Schlagwörter für Einschluss")} <small>{t("durch Komma getrennt, werden grün hervorgehoben")}</small></label>
          <input id="crit-inc" value={include} onChange={(e) => setInclude(e.target.value)} />
        </div>
        <div className="stack-field">
          <label htmlFor="crit-exc">{t("Schlagwörter für Ausschluss")} <small>{t("durch Komma getrennt, werden rot hervorgehoben")}</small></label>
          <input id="crit-exc" value={exclude} onChange={(e) => setExclude(e.target.value)} />
        </div>
        <div className="stack-field">
          <label htmlFor="crit-note">{t("Was hat sich geändert?")} {criteria.version === 0 && <small>{t("bei der ersten Version optional")}</small>}</label>
          <input id="crit-note" value={changeNote} onChange={(e) => setChangeNote(e.target.value)} required={criteria.version > 0} />
        </div>
      </Group>
      <div className="sheet-actions">
        <button className="primary" type="submit">{t("Als Version {version} veröffentlichen", { version: criteria.version + 1 })}</button>
      </div>
    </form>
  );
}
