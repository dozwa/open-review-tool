// Vorschau eines Volltexts: Blättern, Zoom, Suche, Markierungen und Kommentare. Die Datei bleibt, wie sie
// ist: Markierungen stehen im Log des Reviews und liegen als eigene Ebene über der Seite.
// Den Inhalt liefert der Rust-Kern nach Prüfung von Pfad und Prüfsumme; die WebView liest keine Datei selbst.

import React, { useCallback, useEffect, useRef, useState } from "react";
import { getDocument, GlobalWorkerOptions, TextLayer } from "pdfjs-dist/legacy/build/pdf.mjs";
import type { PDFDocumentProxy, RenderTask } from "pdfjs-dist/legacy/build/pdf.mjs";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import "pdfjs-dist/web/pdf_viewer.css";

import { errorText, Highlight, HIGHLIGHT_COLORS, HIGHLIGHT_UNITS, HighlightRect } from "./api";
import type { Marks } from "./highlights";
import { formatPercent, t, tn } from "./i18n";
import { Banner, Icon } from "./ui";

GlobalWorkerOptions.workerSrc = workerUrl;

const ZOOM_STEPS = [0.5, 0.75, 1, 1.25, 1.5, 2, 3];
type Size = { w: number; h: number };

/** Hebt in einer gerenderten Textebene die Stücke hervor, die den Suchbegriff enthalten. */
function mark(layer: Element | null, query: string) {
  const q = query.trim().toLowerCase();
  layer?.querySelectorAll("span").forEach((span) => {
    span.classList.toggle("hit", q.length >= 2 && (span.textContent ?? "").toLowerCase().includes(q));
  });
}

const COLOR_LABEL: Record<string, string> = {
  yellow: t("Gelb"), green: t("Grün"), blue: t("Blau"), pink: t("Rosa"), orange: t("Orange"), purple: t("Violett"),
};
// Die zuletzt benutzte Farbe gilt für „Kommentieren“ ohne Farbwahl.
let lastColor: string = HIGHLIGHT_COLORS[0];

/**
 * Macht aus den Rechtecken einer Auswahl eines je Zeile, in Zehntausendsteln der Seite. Der Browser meldet
 * je Textstück ein Rechteck, oft doppelt; Stücke auf einer Zeile, die aneinanderstoßen, werden eins.
 */
function lineRects(found: DOMRect[], page: DOMRect): HighlightRect[] {
  const boxes = found
    .map((r) => ({ l: Math.max(r.left, page.left), t: Math.max(r.top, page.top), r: Math.min(r.right, page.right), b: Math.min(r.bottom, page.bottom) }))
    .filter((r) => r.r - r.l > 1 && r.b - r.t > 1 && r.b - r.t < page.height * 0.2)
    .sort((a, b) => a.t - b.t || a.l - b.l);
  const lines: typeof boxes = [];
  for (const box of boxes) {
    const height = box.b - box.t;
    const same = lines.find((line) => {
      const overlap = Math.min(line.b, box.b) - Math.max(line.t, box.t);
      return overlap > 0.5 * Math.min(height, line.b - line.t) && box.l <= line.r + height && box.r >= line.l - height;
    });
    if (same) {
      same.l = Math.min(same.l, box.l);
      same.t = Math.min(same.t, box.t);
      same.r = Math.max(same.r, box.r);
      same.b = Math.max(same.b, box.b);
    } else {
      lines.push({ ...box });
    }
  }
  const unit = (value: number, size: number) => Math.min(HIGHLIGHT_UNITS, Math.max(0, Math.round((value / size) * HIGHLIGHT_UNITS)));
  return lines.slice(0, 200).map((line) => {
    const x = Math.min(unit(line.l - page.left, page.width), HIGHLIGHT_UNITS - 1);
    const y = Math.min(unit(line.t - page.top, page.height), HIGHLIGHT_UNITS - 1);
    const w = Math.max(1, Math.min(unit(line.r - line.l, page.width), HIGHLIGHT_UNITS - x));
    const h = Math.max(1, Math.min(unit(line.b - line.t, page.height), HIGHLIGHT_UNITS - y));
    return [x, y, w, h];
  });
}

const percent = (value: number) => `${value / (HIGHLIGHT_UNITS / 100)}%`;

function PdfPage({ doc, n, scale, base, query, root, marks, flash }: {
  doc: PDFDocumentProxy;
  n: number;
  scale: number;
  base: Size;
  query: string;
  root: React.RefObject<HTMLDivElement>;
  /** Markierungen auf dieser Seite. */
  marks: Highlight[];
  /** Die Markierung, zu der gerade gesprungen wurde. */
  flash: string;
}) {
  const holder = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const current = useRef(query);
  const [visible, setVisible] = useState(false);
  const [size, setSize] = useState(base);
  current.current = query;

  // Gerendert wird nur, was in der Nähe des sichtbaren Ausschnitts liegt; große Dateien bleiben so flüssig.
  useEffect(() => {
    const element = holder.current;
    if (!element) return;
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting), { root: root.current, rootMargin: "120% 0px" });
    observer.observe(element);
    return () => observer.disconnect();
  }, [root]);

  useEffect(() => {
    const element = content.current;
    if (!visible || !element) return;
    let stale = false;
    let task: RenderTask | null = null;
    let layer: TextLayer | null = null;
    (async () => {
      const page = await doc.getPage(n);
      if (stale) return;
      const viewport = page.getViewport({ scale });
      setSize({ w: viewport.width / scale, h: viewport.height / scale });
      const ratio = window.devicePixelRatio || 1;
      const canvas = document.createElement("canvas");
      canvas.width = Math.floor(viewport.width * ratio);
      canvas.height = Math.floor(viewport.height * ratio);
      canvas.style.width = `${viewport.width}px`;
      canvas.style.height = `${viewport.height}px`;
      const text = document.createElement("div");
      text.className = "textLayer";
      holder.current?.style.setProperty("--scale-factor", String(scale));
      element.replaceChildren(canvas, text);
      task = page.render({ canvasContext: canvas.getContext("2d")!, viewport, transform: ratio !== 1 ? [ratio, 0, 0, ratio, 0, 0] : undefined });
      await task.promise;
      layer = new TextLayer({ textContentSource: await page.getTextContent(), container: text, viewport });
      await layer.render();
      if (!stale) mark(text, current.current);
    })().catch(() => undefined); // abgebrochenes Rendern beim Weiterblättern ist kein Fehler
    return () => {
      stale = true;
      task?.cancel();
      layer?.cancel();
      element.replaceChildren();
    };
  }, [visible, doc, n, scale]);

  useEffect(() => mark(holder.current?.querySelector(".textLayer") ?? null, query), [query]);

  // Die Ebene der Markierungen liegt über dem Text und nimmt keine Klicks an; so bleibt der Text wählbar.
  return (
    <div className="pdf-page" data-page={n} ref={holder} style={{ width: size.w * scale, height: size.h * scale }}>
      <div ref={content} />
      <div className="pdf-marks" aria-hidden="true">
        {marks.map((mark) => (
          <React.Fragment key={`${mark.actor_id}-${mark.id}`}>
            {mark.rects.map(([x, y, w, h], i) => (
              <span
                key={i}
                className={`pdf-mark mark-${mark.color}${flash === mark.id ? " flash" : ""}`}
                data-mark={i === 0 ? mark.id : undefined}
                style={{ left: percent(x), top: percent(y), width: percent(w), height: percent(h) }}
              />
            ))}
            {mark.note && <span className={`pdf-mark-note mark-${mark.color}`} style={{ left: percent(mark.rects[0][0]), top: percent(mark.rects[0][1]) }} />}
          </React.Fragment>
        ))}
      </div>
    </div>
  );
}

function Swatches({ value, onPick }: { value?: string; onPick: (color: string) => void }) {
  return (
    <span className="mark-swatches" role="group" aria-label={t("Farbe")}>
      {HIGHLIGHT_COLORS.map((color) => (
        <button
          key={color}
          type="button"
          className={`mark-swatch mark-${color}`}
          aria-pressed={value === undefined ? undefined : value === color}
          aria-label={COLOR_LABEL[color]}
          title={COLOR_LABEL[color]}
          onClick={() => onPick(color)}
        />
      ))}
    </span>
  );
}

/** Wo ein kleines Fenster neben einer Stelle im Fenster Platz hat. */
function place(x: number, y: number, width: number, height: number): React.CSSProperties {
  return { left: Math.max(8, Math.min(x, window.innerWidth - width - 8)), top: Math.max(8, Math.min(y, window.innerHeight - height - 8)) };
}

/** Kommentar, Farbe und Löschen zu einer Markierung. Der Kommentar wird beim Verlassen des Feldes gespeichert. */
function MarkEditor({ mark, at, marks, onClose }: { mark: Highlight; at: { x: number; y: number }; marks: Marks; onClose: () => void }) {
  const [note, setNote] = useState(mark.note);
  const [copied, setCopied] = useState(false);
  const saved = useRef(mark.note);

  async function save(color = mark.color) {
    if (!mark.editable || (note.trim() === saved.current && color === mark.color)) return;
    saved.current = note.trim();
    await marks.onUpdate(mark, color, note);
  }

  return (
    <div className="mark-editor" style={place(at.x, at.y + 10, 300, 260)} role="dialog" aria-label={t("Markierung")}>
      <p className="mark-who">
        <span className={`mark-dot mark-${mark.color}`} />
        {mark.mine ? t("Deine Markierung") : mark.name} · {mark.stage_name} · {t("S. {page}", { page: mark.page })}
      </p>
      {mark.text && <blockquote className="mark-quote">{mark.text}</blockquote>}
      {mark.editable ? (
        <>
          <textarea
            value={note}
            autoFocus={!mark.note}
            rows={3}
            maxLength={4000}
            placeholder={t("Kommentar …")}
            aria-label={t("Kommentar")}
            onChange={(e) => setNote(e.target.value)}
            onBlur={() => void save()}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void save().then(onClose);
            }}
          />
          <Swatches
            value={mark.color}
            onPick={(color) => {
              lastColor = color;
              void save(color);
            }}
          />
        </>
      ) : (
        mark.note && <p className="mark-note">{mark.note}</p>
      )}
      <div className="mark-actions">
        {mark.text && (
          <button className="small" onClick={() => navigator.clipboard.writeText(mark.text).then(() => setCopied(true)).catch(() => undefined)}>
            {copied ? t("Kopiert") : t("Zitat kopieren")}
          </button>
        )}
        {mark.editable && <button className="small" onClick={() => void marks.onRemove(mark).then(onClose)}>{t("Löschen")}</button>}
        <button className="small" onClick={() => void save().then(onClose)}>{t("Schließen")}</button>
      </div>
    </div>
  );
}

/** Eine Auswahl im Text, die noch keine Markierung ist. */
interface Draft {
  page: number;
  rects: HighlightRect[];
  text: string;
  x: number;
  y: number;
}

/** Der Text eines PDFs je Seite, etwa für den KI-Vorschlag zur Extraktion. Zeilenenden bleiben erhalten. */
export async function pdfPages(data: ArrayBuffer): Promise<string[]> {
  const task = getDocument({ data: new Uint8Array(data), isEvalSupported: false });
  try {
    const doc = await task.promise;
    const pages: string[] = [];
    for (let n = 1; n <= doc.numPages; n += 1) {
      const content = await (await doc.getPage(n)).getTextContent();
      pages.push(content.items.map((item) => ("str" in item ? item.str + (item.hasEOL ? "\n" : " ") : "")).join(""));
    }
    return pages;
  } finally {
    await task.destroy();
  }
}

export function PdfView({ load, onExternal, marks }: {
  load: () => Promise<ArrayBuffer>;
  onExternal?: () => void;
  /** Markierungen zu diesem Volltext; ohne Angabe ist der Leser eine reine Vorschau. */
  marks?: Marks;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const texts = useRef<string[] | null>(null);
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [base, setBase] = useState<Size>({ w: 595, h: 842 });
  const [error, setError] = useState("");
  const [zoom, setZoom] = useState<number | null>(null);
  const [fit, setFit] = useState(1);
  const [page, setPage] = useState(1);
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<{ pages: number[]; total: number } | null>(null);
  const [searching, setSearching] = useState(false);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [editing, setEditing] = useState<{ id: string; actor: string; x: number; y: number } | null>(null);
  const [listed, setListed] = useState(false);
  const [flash, setFlash] = useState("");
  const all = marks?.items ?? [];
  const drawn = all.filter((mark) => !mark.stale);
  const edited = editing ? all.find((mark) => mark.id === editing.id && mark.actor_id === editing.actor) : undefined;

  useEffect(() => {
    let stale = false;
    let task: ReturnType<typeof getDocument> | null = null;
    setDoc(null);
    setError("");
    setFound(null);
    setQuery("");
    setInput("");
    setPage(1);
    texts.current = null;
    (async () => {
      const data = await load();
      if (stale) return;
      task = getDocument({ data: new Uint8Array(data), isEvalSupported: false });
      const loaded = await task.promise;
      const first = (await loaded.getPage(1)).getViewport({ scale: 1 });
      if (stale) return;
      setBase({ w: first.width, h: first.height });
      setDoc(loaded);
    })().catch((e) => !stale && setError(errorText(e)));
    return () => {
      stale = true;
      task?.destroy();
    };
  }, [load]);

  // „Breite“ passt die Seite in die Spalte ein und folgt der Fenstergröße.
  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const measure = () => setFit(Math.min(1.5, Math.max(0.4, (element.clientWidth - 36) / base.w)));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [base, doc]);

  const scale = zoom ?? fit;

  const goTo = useCallback((n: number) => {
    scroller.current?.querySelector(`[data-page="${n}"]`)?.scrollIntoView({ block: "start" });
  }, []);

  function onScroll() {
    const element = scroller.current;
    if (!element) return;
    const line = element.scrollTop + element.clientHeight / 3;
    const pages = Array.from(element.querySelectorAll<HTMLElement>(".pdf-page"));
    const at = pages.findIndex((p) => p.offsetTop + p.offsetHeight > line);
    setPage(at < 0 ? pages.length : at + 1);
  }

  function step(direction: 1 | -1) {
    const next = direction > 0 ? ZOOM_STEPS.find((z) => z > scale + 0.01) : [...ZOOM_STEPS].reverse().find((z) => z < scale - 0.01);
    if (next) setZoom(next);
  }

  /** Sucht in allen Seiten; die Eingabetaste springt danach von Fundseite zu Fundseite. */
  async function search(event: React.FormEvent) {
    event.preventDefault();
    const q = input.trim().toLowerCase();
    if (!doc || q.length < 2) {
      setQuery("");
      setFound(null);
      return;
    }
    if (q === query && found) {
      const next = found.pages.find((p) => p > page) ?? found.pages[0];
      if (next) goTo(next);
      return;
    }
    setSearching(true);
    try {
      if (!texts.current) {
        const all: string[] = [];
        for (let n = 1; n <= doc.numPages; n += 1) {
          const content = await (await doc.getPage(n)).getTextContent();
          all.push(content.items.map((item) => ("str" in item ? item.str : "")).join(" ").toLowerCase());
        }
        texts.current = all;
      }
      const pages: number[] = [];
      let total = 0;
      texts.current.forEach((text, index) => {
        const count = text.split(q).length - 1;
        if (count > 0) pages.push(index + 1);
        total += count;
      });
      setQuery(q);
      setFound({ pages, total });
      const first = pages.find((p) => p >= page) ?? pages[0];
      if (first) goTo(first);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSearching(false);
    }
  }

  // Solange ein kleines Fenster offen ist, schließt Escape nur dieses, und ein Klick daneben auch.
  const floating = draft !== null || edited !== undefined;
  useEffect(() => {
    if (!floating) return;
    const close = () => {
      setDraft(null);
      setEditing(null);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopImmediatePropagation();
      close();
    };
    const onDown = (event: MouseEvent) => {
      if (!(event.target as Element).closest?.(".mark-editor, .mark-draft")) close();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", onDown, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", onDown, true);
    };
  }, [floating]);

  /** Nach dem Loslassen der Maus: Ist Text einer Seite ausgewählt, erscheint die Farbwahl. */
  function onMouseUp() {
    if (!marks?.onAdd) return;
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount === 0) return;
    const pageOf = (node: Node) => (node instanceof Element ? node : node.parentElement)?.closest<HTMLElement>(".pdf-page") ?? null;
    let range = selection.getRangeAt(0);
    const start = pageOf(range.startContainer);
    if (!start || !scroller.current?.contains(start)) return;
    let text = selection.toString();
    // Eine Markierung liegt auf einer Seite; reicht die Auswahl weiter, zählt der Teil auf der ersten.
    if (pageOf(range.endContainer) !== start) {
      const last = start.querySelector(".textLayer")?.lastChild;
      if (!last) return;
      range = range.cloneRange();
      range.setEndAfter(last);
      text = range.toString();
    }
    const rects = lineRects(Array.from(range.getClientRects()), start.getBoundingClientRect());
    text = text.replace(/\s+/g, " ").trim();
    if (rects.length === 0 || !text) return;
    const end = range.getBoundingClientRect();
    setEditing(null);
    setDraft({ page: Number(start.dataset.page), rects, text, x: end.left, y: Math.min(end.bottom, window.innerHeight - 60) });
  }

  async function add(color: string, comment: boolean) {
    if (!draft || !marks?.onAdd) return;
    const { x, y, ...highlight } = draft;
    lastColor = color;
    setDraft(null);
    window.getSelection()?.removeAllRanges();
    const added = await marks.onAdd({ ...highlight, color, note: "" });
    if (added && comment) setEditing({ id: added.id, actor: added.actor_id, x, y });
  }

  /** Ein Klick auf eine Markierung öffnet sie. Die Ebene selbst nimmt keine Klicks an, also zählt die Lage. */
  function onClick(event: React.MouseEvent) {
    if (!marks || !window.getSelection()?.isCollapsed) return;
    const holder = (event.target as Element).closest<HTMLElement>(".pdf-page");
    if (!holder) return;
    const box = holder.getBoundingClientRect();
    const x = ((event.clientX - box.left) / box.width) * HIGHLIGHT_UNITS;
    const y = ((event.clientY - box.top) / box.height) * HIGHLIGHT_UNITS;
    const n = Number(holder.dataset.page);
    const hit = drawn.filter((mark) => mark.page === n && mark.rects.some(([l, top, w, h]) => x >= l && x <= l + w && y >= top && y <= top + h));
    const mark = hit.find((m) => m.mine) ?? hit[0];
    if (mark) setEditing({ id: mark.id, actor: mark.actor_id, x: event.clientX, y: event.clientY });
  }

  function showMark(mark: Highlight) {
    if (mark.stale) return;
    scroller.current?.querySelector(`[data-mark="${mark.id}"]`)?.scrollIntoView({ block: "center" });
    setFlash(mark.id);
    window.setTimeout(() => setFlash(""), 1400);
  }

  if (error) return <div className="pdf-view"><Banner kind="error">{error}</Banner></div>;

  return (
    <div className="pdf-view">
      <div className="pdf-bar">
        <span className="pdf-pages">{doc ? t("Seite {page} von {total}", { page, total: doc.numPages }) : t("Lädt …")}</span>
        <span className="pdf-zoom">
          <button className="small" onClick={() => step(-1)} aria-label={t("Verkleinern")} disabled={!doc}>−</button>
          <button className="small" onClick={() => setZoom(null)} aria-pressed={zoom === null} disabled={!doc} title={t("An die Breite anpassen")}>{formatPercent(Math.round(scale * 100))}</button>
          <button className="small" onClick={() => step(1)} aria-label={t("Vergrößern")} disabled={!doc}>+</button>
        </span>
        <form className="pdf-search" onSubmit={search} role="search">
          <input value={input} onChange={(e) => setInput(e.target.value)} placeholder={t("Im Volltext suchen")} aria-label={t("Im Volltext suchen")} disabled={!doc} />
          {found && <span className="pdf-found">{found.total === 0 ? t("nichts gefunden") : tn("{total} Treffer auf {n} Seite", "{total} Treffer auf {n} Seiten", found.pages.length, { total: found.total })}</span>}
          {searching && <span className="pdf-found">{t("sucht …")}</span>}
        </form>
        {marks && (
          <button className="small" onClick={() => setListed(!listed)} aria-pressed={listed} title={t("Markierungen und Kommentare zu diesem Volltext")}>
            <Icon name="pen" size={13} /> {t("Markierungen")}{all.length > 0 && ` ${all.length}`}
          </button>
        )}
        {onExternal && <button className="small" onClick={onExternal} title={t("Im Standardprogramm öffnen")}><Icon name="file" size={13} /> {t("Extern öffnen")}</button>}
      </div>
      <div className="pdf-body">
        <div
          className="pdf-scroll"
          ref={scroller}
          onScroll={() => {
            onScroll();
            if (draft) setDraft(null);
          }}
          onMouseUp={onMouseUp}
          onClick={onClick}
          tabIndex={0}
          aria-label={t("Volltext")}
        >
          {doc && Array.from({ length: doc.numPages }, (_, i) => (
            <PdfPage key={i + 1} doc={doc} n={i + 1} scale={scale} base={base} query={query} root={scroller} marks={drawn.filter((mark) => mark.page === i + 1)} flash={flash} />
          ))}
        </div>
        {marks && listed && (
          <aside className="mark-list" aria-label={t("Markierungen")}>
            {all.length === 0 && (
              <p className="muted">
                {marks.onAdd ? t("Noch keine Markierungen. Wähle Text im Volltext aus und klicke eine Farbe.") : t("Zu diesem Volltext gibt es keine Markierungen.")}
              </p>
            )}
            {all.map((mark) => (
              <button key={`${mark.actor_id}-${mark.id}`} className="mark-item" onClick={() => showMark(mark)} disabled={mark.stale}>
                <span className="mark-who">
                  <span className={`mark-dot mark-${mark.color}`} />
                  {t("S. {page}", { page: mark.page })}{!mark.mine && ` · ${mark.name}`}
                  {mark.stale && <span className="mark-stale" title={t("Diese Markierung gehört zu einer früheren Datei und wird nicht eingezeichnet.")}>{t("andere Datei")}</span>}
                </span>
                {mark.text && <span className="mark-quote">{mark.text}</span>}
                {mark.note && <span className="mark-note">{mark.note}</span>}
              </button>
            ))}
          </aside>
        )}
      </div>
      {draft && marks?.onAdd && (
        <div className="mark-draft" style={place(draft.x, draft.y + 8, 250, 40)} onMouseDown={(e) => e.preventDefault()}>
          <Swatches onPick={(color) => void add(color, false)} />
          <button className="small" onClick={() => void add(lastColor, true)} title={t("Markieren und einen Kommentar dazuschreiben")}>{t("Kommentieren")}</button>
        </div>
      )}
      {edited && editing && marks && <MarkEditor key={`${edited.actor_id}-${edited.id}`} mark={edited} at={editing} marks={marks} onClose={() => setEditing(null)} />}
    </div>
  );
}
