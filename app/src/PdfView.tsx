// Vorschau eines Volltexts: Blättern, Zoom und Suche. Nur lesen; die Datei bleibt, wie sie ist.
// Den Inhalt liefert der Rust-Kern nach Prüfung von Pfad und Prüfsumme; die WebView liest keine Datei selbst.

import React, { useCallback, useEffect, useRef, useState } from "react";
import { getDocument, GlobalWorkerOptions, TextLayer } from "pdfjs-dist/legacy/build/pdf.mjs";
import type { PDFDocumentProxy, RenderTask } from "pdfjs-dist/legacy/build/pdf.mjs";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import "pdfjs-dist/web/pdf_viewer.css";

import { errorText } from "./api";
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

function PdfPage({ doc, n, scale, base, query, root }: {
  doc: PDFDocumentProxy;
  n: number;
  scale: number;
  base: Size;
  query: string;
  root: React.RefObject<HTMLDivElement>;
}) {
  const holder = useRef<HTMLDivElement>(null);
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
    const element = holder.current;
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
      element.style.setProperty("--scale-factor", String(scale));
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

  return <div className="pdf-page" data-page={n} ref={holder} style={{ width: size.w * scale, height: size.h * scale }} />;
}

export function PdfView({ load, onExternal }: { load: () => Promise<ArrayBuffer>; onExternal?: () => void }) {
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
        {onExternal && <button className="small" onClick={onExternal} title={t("Im Standardprogramm öffnen")}><Icon name="file" size={13} /> {t("Extern öffnen")}</button>}
      </div>
      <div className="pdf-scroll" ref={scroller} onScroll={onScroll} tabIndex={0} aria-label={t("Volltext")}>
        {doc && Array.from({ length: doc.numPages }, (_, i) => (
          <PdfPage key={i + 1} doc={doc} n={i + 1} scale={scale} base={base} query={query} root={scroller} />
        ))}
      </div>
    </div>
  );
}
