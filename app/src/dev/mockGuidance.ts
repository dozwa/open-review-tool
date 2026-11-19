// Leitlinien und Registrierungsvorbereitung für die Vorschau im Browser. Die Leitlinien und das
// Formular kommen aus denselben Dateien wie im Kern; die Vorschläge sind hier von Hand nachgebaut.

import jbiDe from "../../../crates/ort-core/guidance/de/jbi_scoping.json";
import rapidDe from "../../../crates/ort-core/guidance/de/rapid_review.json";
import sotaDe from "../../../crates/ort-core/guidance/de/state_of_the_art.json";
import jbiEn from "../../../crates/ort-core/guidance/en/jbi_scoping.json";
import rapidEn from "../../../crates/ort-core/guidance/en/rapid_review.json";
import sotaEn from "../../../crates/ort-core/guidance/en/state_of_the_art.json";
import osfDe from "../../../crates/ort-core/registration/de/osf_gsrrf.json";
import osfEn from "../../../crates/ort-core/registration/en/osf_gsrrf.json";
import { language } from "../i18n";

// Wie im Kern: Leitlinien und Formular in der Sprache der Oberfläche. Die Vorschläge und Antworten
// darunter sind Inhalte des Beispiel-Reviews und bleiben, wie sie sind.
const english = language === "en";
const guidelines = english ? [jbiEn, rapidEn, sotaEn] : [jbiDe, rapidDe, sotaDe];
type OsfForm = {
  key: string; name: string; registry: string; citation: string; url: string;
  sections: unknown[]; items: ({ key: string } & Record<string, unknown>)[];
};
const osf: OsfForm = english ? osfEn : osfDe;

const suggestions: Record<string, string> = {
  title: "Digitale Interventionen bei Migräne",
  contributors: "Dorothea Muster; Anna Berger; Ben Krüger",
  tasks_roles: "- Dorothea Muster: Leitung: Protokoll, Kriterien, Suche, Öffnen und Schließen der Stufen\n- Anna Berger: Auswahl und Extraktion\n- Ben Krüger: Auswahl und Extraktion",
  review_type: "JBI Scoping Review (Vorlage „JBI Scoping Review“ im Open Review Tool). Das Vorgehen folgt: JBI Manual for Evidence Synthesis, Kapitel 10: Scoping reviews. JBI, Fassung 2024. Berichtet wird nach PRISMA-ScR.",
  review_stages: "Protokoll und Team; Suche; Import und Dubletten; Pilot Titel/Abstract; Titel/Abstract; Volltexte beschaffen; Volltext-Screening; Zitationssuche; Quellen bündeln; Charting-Pilot; Charting; Auswertung und Bericht",
  current_stage: "Abgeschlossen: Protokoll und Team; Suche; Import und Dubletten. In Arbeit: Pilot Titel/Abstract. Eingelesen sind 412 Treffer.",
  software: "Open Review Tool 0.2.0 für Import, Dubletten, Auswahl, Extraktion und Bericht.",
  databases: "MEDLINE; Embase; CINAHL",
  interfaces: "- MEDLINE über Ovid\n- Embase über Ovid\n- CINAHL über EBSCOhost",
  criteria: "Einschluss:\n- I1 (Population): Erwachsene mit Migräne\n- I2 (Concept): Digitale Intervention\n\nAusschluss:\n- E1 (Population): Kinder und Jugendliche\n- E2 (Quellenart): Kein Volltext, nur Abstract",
  query_strings: "MEDLINE (Ovid), Version 2:\n(migraine OR headache) AND (digital OR app OR telehealth)\nGrenzen: ab 2015",
  screening_stages: "- Pilot Titel/Abstract: Das ganze Team bewertet eine Zufallsstichprobe.\n- Titel/Abstract: Alle Einträge ohne Dubletten, danach Klärung der Abweichungen.\n- Volltext-Screening: Eingeschlossene aus Titel/Abstract mit beschafftem Volltext prüfen.",
  screening_reliability: "- Pilot Titel/Abstract: 2 Personen bewerten jeden Eintrag unabhängig; weiter ab 75 % Übereinstimmung\n- Titel/Abstract: 2 Personen bewerten jeden Eintrag unabhängig",
  entities: "- Land: Land, in dem die Arbeit entstand oder durchgeführt wurde.\n- Ziel oder Fragestellung: Ziel der Quelle in eigenen Worten.",
};
const answers: Record<string, string> = {
  background: "Digitale Angebote für Menschen mit Migräne nehmen zu. Eine Übersicht, welche Arten von Interventionen untersucht sind, fehlt bisher.",
  primary_questions: "Welche digitalen Interventionen für Erwachsene mit Migräne sind beschrieben, und in welchem Setting wurden sie untersucht?",
};
let updatedAt: string | null = "2026-11-18T16:20:00Z";

function draft(registration: unknown, canEdit: boolean) {
  const items = osf.items.map((item) => {
    const suggestion = suggestions[item.key] ?? "";
    const answer = answers[item.key] ?? null;
    return { ...item, suggestion, answer, status: answer !== null ? "answered" : suggestion ? "suggested" : "open" };
  });
  const count = (status: string) => items.filter((i) => i.status === status).length;
  return {
    form: osf.key, name: osf.name, registry: osf.registry, citation: osf.citation, url: osf.url, sections: osf.sections, items,
    n_answered: count("answered"), n_suggested: count("suggested"), n_open: count("open"),
    updated_at: updatedAt, registration, can_edit: canEdit,
  };
}

/** Antwortet auf die Befehle zu Leitlinie und Registrierung; `undefined` heißt: nicht zuständig. */
export function mockGuidance(cmd: string, a: Record<string, unknown>, registration: () => unknown): unknown {
  switch (cmd) {
    case "guidance_list": return guidelines;
    case "registration_draft": return draft(a.reviewId === "r2" ? null : registration(), true);
    case "registration_answer": {
      const text = String(a.text).trim();
      if (text) answers[String(a.key)] = text;
      else delete answers[String(a.key)];
      updatedAt = new Date().toISOString();
      return draft(registration(), true);
    }
    case "registration_export": return null;
    default: return undefined;
  }
}
