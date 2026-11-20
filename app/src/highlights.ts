// Markierungen und Kommentare im Volltext eines Eintrags. Der Kern entscheidet, was die Person sieht
// (die eigenen immer, die der anderen erst nach der Entblindung) und was sie ändern darf.

import { useEffect, useMemo, useState } from "react";

import { api, errorText, Highlight, NewHighlight } from "./api";

/** Was der PDF-Leser zum Zeichnen und Ändern von Markierungen braucht. */
export interface Marks {
  items: Highlight[];
  /** Fehlt, wenn hier nicht markiert werden darf (nur Ansicht, geschlossene Stufe, Volltext-Reiter). */
  onAdd?: (highlight: NewHighlight) => Promise<Highlight | null>;
  onUpdate: (highlight: Highlight, color: string, note: string) => Promise<void>;
  onRemove: (highlight: Highlight) => Promise<void>;
}

const top = (h: Highlight) => h.rects[0]?.[1] ?? 0;
const ordered = (list: Highlight[]) => [...list].sort((a, b) => a.page - b.page || top(a) - top(b));

/**
 * Markierungen zum Volltext von `recordId`. Mit `stageId` lassen sich neue setzen; sie gehören dann zu
 * dieser Stufe. Ohne Eintrag gibt es nichts.
 */
export function useHighlights(reviewId: string, recordId: string | null, stageId: string | null, onError: (text: string) => void): Marks | undefined {
  const [items, setItems] = useState<Highlight[]>([]);

  useEffect(() => {
    setItems([]);
    if (!recordId) return;
    let stale = false;
    api.highlights(reviewId, recordId).then((list) => !stale && setItems(list)).catch(() => undefined);
    return () => {
      stale = true;
    };
  }, [reviewId, recordId]);

  return useMemo(() => {
    if (!recordId) return undefined;
    const fail = (e: unknown) => onError(errorText(e));
    return {
      items,
      onAdd: stageId
        ? async (highlight) => {
            try {
              const added = await api.addHighlight(reviewId, stageId, recordId, highlight);
              setItems((list) => ordered([...list, added]));
              return added;
            } catch (e) {
              fail(e);
              return null;
            }
          }
        : undefined,
      onUpdate: async (highlight, color, note) => {
        try {
          const changed = await api.updateHighlight(reviewId, highlight.stage_id, recordId, highlight.id, color, note);
          setItems((list) => list.map((h) => (h.id === changed.id && h.actor_id === changed.actor_id ? changed : h)));
        } catch (e) {
          fail(e);
        }
      },
      onRemove: async (highlight) => {
        try {
          await api.removeHighlight(reviewId, highlight.stage_id, recordId, highlight.id);
          setItems((list) => list.filter((h) => h !== highlight && !(h.id === highlight.id && h.actor_id === highlight.actor_id)));
        } catch (e) {
          fail(e);
        }
      },
    };
    // `onError` ist auf den Seiten eine Setzfunktion und bleibt gleich.
  }, [items, reviewId, recordId, stageId]);
}
