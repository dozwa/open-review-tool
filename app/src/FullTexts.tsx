// Volltexte beschaffen: Ordner oder Zotero verknüpfen, Zuordnungen bestätigen, Status je Eintrag.
// Die Dateien bleiben, wo sie sind; im Log stehen nur Prüfsumme und relativer Pfad oder Zotero-Schlüssel.

import React, { useCallback, useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import {
  api, canManage, canVote, errorText, FileProblem, FileSuggestion, FullTextOverview, FullTextRow, ReviewDetail,
  ZoteroLibrary, ZoteroSuggestion,
} from "./api";
import { PdfView } from "./PdfView";
import { Banner, Empty, Group, Icon, Pill, Segmented, Stat } from "./ui";

type Run = (action: () => Promise<string | void>, done?: string) => Promise<void>;
type Filter = "open" | "retrieved" | "unavailable" | "all";

const BASIS: Record<string, string> = {
  doi_metadata: "DOI in der Datei",
  doi_filename: "DOI im Dateinamen",
  title: "Titel im Dateinamen",
  author_year: "Autor und Jahr im Dateinamen",
  doi: "gleiche DOI",
};
const PROBLEM: Record<string, string> = { missing: "Datei fehlt", changed: "Datei wurde ersetzt oder geändert" };

function statusOf(row: FullTextRow): Filter {
  return row.fulltext ? row.fulltext.status : "open";
}

function byline(authors: string, year: number | null): string {
  const first = authors.split(";")[0]?.trim() ?? "";
  const more = authors.includes(";") ? " et al." : "";
  return [first ? first + more : "", year ?? ""].filter(Boolean).join(" · ");
}

/** Vorschau über dem Fenster; schließt mit Escape. */
export function FullTextPreview({ reviewId, recordId, title, onClose }: { reviewId: string; recordId: string; title: string; onClose: () => void }) {
  const load = useCallback(() => api.readFulltext(reviewId, recordId), [reviewId, recordId]);
  const [error, setError] = useState("");
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="pdf-overlay" role="dialog" aria-label={`Volltext: ${title}`}>
      <div className="pdf-overlay-head">
        <strong>{title}</strong>
        <button className="small" onClick={onClose}><Icon name="x" size={13} /> Schließen</button>
      </div>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      <PdfView load={load} onExternal={() => api.openFulltext(reviewId, recordId).catch((e) => setError(errorText(e)))} />
    </div>
  );
}

/** Liste von Vorschlägen mit Häkchen; bestätigt wird nur, was angehakt ist. */
function Suggestions<T extends { record_id: string }>({ items, describe, onConfirm, onDismiss }: {
  items: T[];
  describe: (item: T) => { from: string; basis: string; sure: boolean; note?: string };
  onConfirm: (chosen: T[]) => void;
  onDismiss: () => void;
}) {
  const [off, setOff] = useState<Set<string>>(new Set(items.filter((i) => !describe(i).sure).map((i) => i.record_id)));
  const chosen = items.filter((i) => !off.has(i.record_id));
  const toggle = (id: string) => {
    const next = new Set(off);
    if (!next.delete(id)) next.add(id);
    setOff(next);
  };
  if (items.length === 0) {
    return <p className="muted ft-none">Nichts gefunden, das zu einem offenen Eintrag passt. <button className="link" onClick={onDismiss}>Schließen</button></p>;
  }
  return (
    <div className="ft-suggestions">
      {items.map((item) => {
        const d = describe(item);
        return (
          <label className="ft-suggestion" key={item.record_id}>
            <input type="checkbox" checked={!off.has(item.record_id)} onChange={() => toggle(item.record_id)} />
            <span>
              <span className="ft-file">{d.from}</span>
              <span className="ft-arrow">gehört zu</span>
              <span className="ft-title">{(item as unknown as { title: string }).title}</span>
              <span className="ft-meta"><Pill tone={d.sure ? "green" : "yellow"}>{d.basis}</Pill>{d.note && <span className="muted"> {d.note}</span>}</span>
            </span>
          </label>
        );
      })}
      <div className="ft-confirm">
        <span className="muted">Unsichere Vorschläge sind nicht angehakt. Nichts gilt, bevor du bestätigst.</span>
        <button onClick={onDismiss}>Verwerfen</button>
        <button className="primary" disabled={chosen.length === 0} onClick={() => onConfirm(chosen)}>{chosen.length} bestätigen</button>
      </div>
    </div>
  );
}

export function FullTextTab({ review, run }: { review: ReviewDetail; run: Run }) {
  const write = canVote(review.role);
  const manage = canManage(review.role);
  const [data, setData] = useState<FullTextOverview | null>(null);
  const [error, setError] = useState("");
  const [filter, setFilter] = useState<Filter>("open");
  const [query, setQuery] = useState("");
  const [files, setFiles] = useState<FileSuggestion[] | null>(null);
  const [problems, setProblems] = useState<FileProblem[] | null>(null);
  const [libraries, setLibraries] = useState<ZoteroLibrary[] | null>(null);
  const [library, setLibrary] = useState("");
  const [collection, setCollection] = useState("");
  const [zotero, setZotero] = useState<ZoteroSuggestion[] | null>(null);
  const [busy, setBusy] = useState("");
  const [reasonFor, setReasonFor] = useState("");
  const [reason, setReason] = useState("");
  const [preview, setPreview] = useState<FullTextRow | null>(null);

  useEffect(() => {
    api.fulltexts(review.id).then(setData).catch((e) => setError(errorText(e)));
  }, [review]);

  /** Lesende Schritte (suchen, prüfen) laufen hier; alles, was das Log ändert, läuft über `run`. */
  async function look<T>(label: string, action: () => Promise<T>, done: (value: T) => void) {
    setBusy(label);
    setError("");
    try {
      done(await action());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy("");
    }
  }

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (data?.rows ?? []).filter((r) => (filter === "all" || statusOf(r) === filter) && (!q || `${r.title} ${r.authors}`.toLowerCase().includes(q)));
  }, [data, filter, query]);

  if (!data) return error ? <Banner kind="error">{error}</Banner> : null;
  const open_ = data.needed - data.retrieved - data.unavailable;
  const count = (f: Filter) => data.rows.filter((r) => f === "all" || statusOf(r) === f).length;

  async function chooseFolder() {
    const path = await open({ multiple: false, directory: true, title: "Ordner mit den Volltexten" });
    if (typeof path === "string") await run(() => api.setFulltextFolder(review.id, path), "Ordner verknüpft.");
  }

  async function chooseFile(row: FullTextRow) {
    const path = await open({ multiple: false, directory: false, defaultPath: data?.folder ?? undefined, filters: [{ name: "PDF", extensions: ["pdf"] }] });
    if (typeof path === "string") await run(async () => void (await api.assignFulltexts(review.id, [{ record_id: row.record_id, path }])), "Volltext zugeordnet.");
  }

  const confirmFiles = (chosen: FileSuggestion[]) => {
    setFiles(null);
    return run(async () => void (await api.assignFulltexts(review.id, chosen.map((s) => ({ record_id: s.record_id, path: s.path })))), `${chosen.length} Volltexte zugeordnet.`);
  };
  const confirmZotero = (chosen: ZoteroSuggestion[]) => {
    setZotero(null);
    return run(async () => {
      const n = await api.confirmZotero(review.id, chosen.map((s) => ({ record_id: s.record_id, item_key: s.item_key, attachment_key: s.attachment_key })));
      return `${chosen.length} Einträge Zotero zugeordnet, ${n} davon mit PDF.`;
    });
  };
  const linkZotero = () => {
    const lib = libraries?.find((l) => `${l.library_type}:${l.library_id}` === library);
    if (!lib) return;
    const col = lib.collections.find((c) => c.key === collection);
    setLibraries(null);
    return run(() => api.linkZotero(review.id, {
      library_type: lib.library_type, library_id: lib.library_id, collection_key: col?.key ?? null, name: col ? `${lib.name} › ${col.name}` : lib.name,
    }), "Zotero verknüpft.");
  };
  const saveReason = (row: FullTextRow) => {
    const text = reason;
    setReasonFor("");
    setReason("");
    return run(() => api.fulltextUnavailable(review.id, row.record_id, text), "Als nicht beschaffbar vermerkt.");
  };

  return (
    <>
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      <div className="stats compact">
        <Stat label="Brauchen einen Volltext" value={data.needed} detail="eingeschlossen nach Titel und Abstract" />
        <Stat label="Beschafft" value={data.retrieved} tone="inc" />
        <Stat label="Nicht beschaffbar" value={data.unavailable} tone="may" />
        <Stat label="Offen" value={open_} tone="pen" />
      </div>

      <Group
        title="Ordner"
        footer="Wo der Ordner liegt, gilt nur für diesen Rechner und steht nicht im Log. Liegt er in einer geteilten Ablage, verknüpft jede Person ihn bei sich; zugeordnet wird nur einmal. Die App liest die Dateien und ändert nichts daran."
        action={
          <span className="ft-actions">
            {data.folder && write && <button className="small" disabled={!!busy} onClick={() => look("ordner", () => api.fulltextSuggestions(review.id), setFiles)}><Icon name="search" size={13} /> {busy === "ordner" ? "Sucht …" : "Im Ordner suchen"}</button>}
            {data.folder && <button className="small" disabled={!!busy} onClick={() => look("pruefen", () => api.verifyFulltexts(review.id), setProblems)}><Icon name="shield" size={13} /> {busy === "pruefen" ? "Prüft …" : "Dateien prüfen"}</button>}
            <button className="small" onClick={chooseFolder}>{data.folder ? "Anderen Ordner wählen" : "Ordner wählen"}</button>
            {data.folder && <button className="small" onClick={() => run(() => api.setFulltextFolder(review.id, null), "Verknüpfung gelöst.")}>Lösen</button>}
          </span>
        }
      >
        <p className="ft-folder">
          {data.folder ? <><Icon name="file" size={14} /> <code>{data.folder}</code> {!data.folder_found && <Pill tone="red">nicht erreichbar</Pill>}</> : <span className="muted">Auf diesem Rechner ist kein Ordner verknüpft.</span>}
        </p>
        {files && <Suggestions items={files} onConfirm={confirmFiles} onDismiss={() => setFiles(null)} describe={(s) => ({ from: s.path, basis: BASIS[s.basis], sure: s.basis !== "author_year" && s.similarity >= 95, note: s.basis === "title" && s.similarity < 100 ? `${s.similarity} % gleich` : undefined })} />}
        {problems && (problems.length === 0
          ? <p className="muted ft-none">Alle zugeordneten Dateien sind vorhanden und unverändert. <button className="link" onClick={() => setProblems(null)}>Schließen</button></p>
          : (
            <ul className="ft-problems">
              {problems.map((p) => <li key={p.record_id}><Pill tone="red">{PROBLEM[p.problem]}</Pill> <code>{p.path}</code> <span className="muted">{p.title}</span></li>)}
            </ul>
          ))}
      </Group>

      <Group
        title="Zotero"
        footer="Gelesen wird über die lokale Schnittstelle von Zotero (ab Version 7; in den Zotero-Einstellungen unter „Erweitert“ einschalten). Die App schreibt nichts nach Zotero. Eine eigene Bibliothek kennt nur dieser Rechner; für das Team eignet sich eine Gruppenbibliothek."
        action={manage && (
          <span className="ft-actions">
            {data.zotero && <button className="small" disabled={!!busy} onClick={() => look("zotero", () => api.zoteroSuggestions(review.id), setZotero)}><Icon name="search" size={13} /> {busy === "zotero" ? "Liest Zotero …" : "In Zotero suchen"}</button>}
            <button className="small" disabled={!!busy} onClick={() => look("bibliotheken", api.zoteroLibraries, (list) => { setLibraries(list); setLibrary(list[0] ? `${list[0].library_type}:${list[0].library_id}` : ""); setCollection(""); })}>
              {busy === "bibliotheken" ? "Fragt Zotero …" : data.zotero ? "Andere Bibliothek wählen" : "Bibliothek wählen"}
            </button>
          </span>
        )}
      >
        <p className="ft-folder">{data.zotero ? <>Verknüpft mit <strong>{data.zotero.name}</strong> <Pill>{data.zotero.library_type === "group" ? "Gruppenbibliothek" : "eigene Bibliothek"}</Pill></> : <span className="muted">Keine Bibliothek verknüpft.</span>}</p>
        {libraries && (
          <div className="ft-link">
            <select value={library} onChange={(e) => { setLibrary(e.target.value); setCollection(""); }} aria-label="Bibliothek">
              {libraries.map((l) => <option key={`${l.library_type}:${l.library_id}`} value={`${l.library_type}:${l.library_id}`}>{l.name}</option>)}
            </select>
            <select value={collection} onChange={(e) => setCollection(e.target.value)} aria-label="Collection">
              <option value="">ganze Bibliothek</option>
              {(libraries.find((l) => `${l.library_type}:${l.library_id}` === library)?.collections ?? []).map((c) => <option key={c.key} value={c.key}>{c.name}</option>)}
            </select>
            <button onClick={() => setLibraries(null)}>Abbrechen</button>
            <button className="primary" onClick={linkZotero} disabled={!library}>Verknüpfen</button>
          </div>
        )}
        {zotero && <Suggestions items={zotero} onConfirm={confirmZotero} onDismiss={() => setZotero(null)} describe={(s) => ({ from: s.item_title || s.item_key, basis: BASIS[s.basis], sure: true, note: s.attachment_key ? undefined : "ohne PDF in Zotero: bleibt offen" })} />}
      </Group>

      <Group title="Einträge" action={<input className="ft-query" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Titel oder Autor" aria-label="Einträge filtern" />}>
        <div className="ft-filter">
          <Segmented<Filter>
            label="Status"
            value={filter}
            onChange={setFilter}
            options={[
              { value: "open", label: `Offen (${count("open")})` },
              { value: "retrieved", label: `Beschafft (${count("retrieved")})` },
              { value: "unavailable", label: `Nicht beschaffbar (${count("unavailable")})` },
              { value: "all", label: `Alle (${count("all")})` },
            ]}
          />
        </div>
        {data.rows.length === 0 && (
          <Empty icon="file" title="Noch nichts zu beschaffen">Sobald Titel und Abstract entblindet oder abgeschlossen sind, stehen hier die eingeschlossenen Einträge.</Empty>
        )}
        {rows.map((row) => {
          const ft = row.fulltext;
          return (
            <div className="ft-row" key={row.record_id}>
              <div className="ft-row-main">
                <p className="ft-row-title">{row.title || "(ohne Titel)"}</p>
                <p className="muted">
                  {byline(row.authors, row.year)}{row.doi && ` · DOI ${row.doi}`}
                  {!row.needed && " · gehört nicht (mehr) zu den Eingeschlossenen"}
                </p>
                {ft?.status === "retrieved" && (
                  <p className="ft-where">
                    <Pill tone={row.file === "missing" ? "red" : "green"}>{row.file === "missing" ? "Datei fehlt hier" : ft.origin === "zotero" ? "Zotero" : "Ordner"}</Pill>{" "}
                    <code>{ft.origin === "zotero" ? `Anhang ${ft.zotero_key}` : ft.path}</code> <span className="muted">{(ft.size / 1024 / 1024).toLocaleString("de-DE", { maximumFractionDigits: 1 })} MB</span>
                  </p>
                )}
                {ft?.status === "unavailable" && <p className="ft-where"><Pill tone="yellow">Nicht beschaffbar</Pill> {ft.reason}</p>}
                {!ft && row.zotero_item && <p className="ft-where"><Pill>Zotero</Pill> <span className="muted">zugeordnet, dort ohne PDF</span></p>}
                {reasonFor === row.record_id && (
                  <form className="ft-reason" onSubmit={(e) => { e.preventDefault(); saveReason(row); }}>
                    <input autoFocus value={reason} onChange={(e) => setReason(e.target.value)} placeholder="Grund, z. B. Fernleihe erfolglos, Autorin nicht erreichbar" aria-label="Grund" required />
                    <button type="button" onClick={() => setReasonFor("")}>Abbrechen</button>
                    <button className="primary" type="submit">Vermerken</button>
                  </form>
                )}
              </div>
              <div className="ft-row-actions">
                {ft?.status === "retrieved" && <button className="small" onClick={() => setPreview(row)}><Icon name="eye" size={13} /> Ansehen</button>}
                {write && data.folder && <button className="small" onClick={() => chooseFile(row)}>{ft?.status === "retrieved" ? "Andere Datei" : "Datei zuordnen"}</button>}
                {write && !ft && <button className="small" onClick={() => { setReasonFor(row.record_id); setReason(""); }}>Nicht beschaffbar</button>}
                {write && ft && <button className="small" onClick={() => run(() => api.resetFulltext(review.id, row.record_id), "Zurückgenommen; der Eintrag ist wieder offen.")}>Zurücknehmen</button>}
              </div>
            </div>
          );
        })}
        {data.rows.length > 0 && rows.length === 0 && <p className="muted ft-none">Kein Eintrag passt zu diesem Filter.</p>}
      </Group>

      {preview && <FullTextPreview reviewId={review.id} recordId={preview.record_id} title={preview.title} onClose={() => setPreview(null)} />}
    </>
  );
}
