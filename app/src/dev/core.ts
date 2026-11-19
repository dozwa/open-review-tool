// Texte, die in der App der Kern in der gewählten Sprache liefert (Vorlagen, Begriffe, Stationsnamen).
// Die Vorschau schlägt sie im Katalog des Kerns nach, damit sie der Sprache der Oberfläche folgt.

import catalog from "../../../crates/ort-core/i18n/en.json";
import { language } from "../i18n";

const en: Record<string, string> = catalog;

/** Text des Kerns in der Sprache der Oberfläche; ohne Eintrag im Katalog bleibt er, wie er ist. */
export const core = (text: string): string => (language === "en" ? en[text] ?? text : text);
