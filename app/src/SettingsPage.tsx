// Einstellungen: die Seite des Programms und der Bereich „Einstellungen“ eines Reviews. Beide gelten nur
// auf diesem Rechner; jede Änderung wird sofort gespeichert.

import React, { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { api, canManage, errorText, Person, ReviewDetail } from "./api";
import { LANGUAGES, setLanguage, t } from "./i18n";
import {
  ACCENTS, Ai, DEFAULTS, forgetProjectSettings, PROJECT_COLORS, saveProjectSettings, saveSettings, ScreeningLayout, Settings, Shortcuts, storageInfo,
  useProjectSettings, useSettings,
} from "./settings";
import { allowed, SHORTCUT_ACTIONS, ShortcutAction, shortcutLabel, shortcutOf } from "./shortcuts";
import { useRefreshSidebar } from "./shared";
import { Avatar, Banner, Group, Icon, PageHeader, Pill, Row, Segmented } from "./ui";

const ACCENT_LABEL: Record<(typeof ACCENTS)[number], string> = {
  system: t("Wie im System"), blue: t("Blau"), purple: t("Violett"), pink: t("Pink"), red: t("Rot"),
  orange: t("Orange"), green: t("Grün"), graphite: t("Graphit"),
};
const COLOR_LABEL: Record<(typeof PROJECT_COLORS)[number], string> = {
  blue: t("Blau"), purple: t("Violett"), pink: t("Pink"), red: t("Rot"), orange: t("Orange"), yellow: t("Gelb"), green: t("Grün"), gray: t("Grau"),
};
const DECISIONS = [["inc", "Include"], ["may", "Maybe"], ["cla", t("Zur Klärung")], ["dis", t("Konflikt")], ["exc", "Exclude"]];

/** Farbfelder zur Auswahl; `none` ist der Eintrag ohne Farbe. */
function Swatches<T extends string>({ value, options, onChange, label, none }: {
  value: T | "";
  options: { value: T; label: string }[];
  onChange: (value: T | "") => void;
  label: string;
  none?: string;
}) {
  return (
    <div className="swatches" role="radiogroup" aria-label={label}>
      {none && <button role="radio" aria-checked={value === ""} className="swatch swatch-none" title={none} aria-label={none} onClick={() => onChange("")} />}
      {options.map((o) => (
        <button key={o.value} role="radio" aria-checked={value === o.value} className={`swatch tag-${o.value}`} title={o.label} aria-label={o.label} onClick={() => onChange(o.value)} />
      ))}
    </div>
  );
}

/** Zeile mit Beschriftung links und Bedienelement rechts. */
function Setting({ label, hint, children }: { label: string; hint?: React.ReactNode; children: React.ReactNode }) {
  return (
    <Row>
      <label>
        {label}
        {hint && <small>{hint}</small>}
      </label>
      <span className="inline-actions">{children}</span>
    </Row>
  );
}

function Switch({ label, hint, checked, onChange }: { label: string; hint?: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <Setting label={label} hint={hint}>
      <input type="checkbox" className="switch" aria-label={label} checked={checked} onChange={(e) => onChange(e.target.checked)} />
    </Setting>
  );
}

/** Die vier Schalter zum Aufbau der Screening-Seite. */
function ScreeningRows({ value, onChange }: { value: ScreeningLayout; onChange: (next: ScreeningLayout) => void }) {
  const set = (patch: Partial<ScreeningLayout>) => onChange({ ...value, ...patch });
  return (
    <>
      <Switch label={t("Liste der Einträge")} hint={t("Linke Spalte mit Suche und Filtern.")} checked={value.show_list} onChange={(show_list) => set({ show_list })} />
      <Switch label={t("Kriterien")} hint={t("Rechte Spalte mit Ein- und Ausschlusskriterien.")} checked={value.show_criteria} onChange={(show_criteria) => set({ show_criteria })} />
      <Switch label={t("Autoren und Zeitschrift")} hint={t("Ausgeblendet lenkt beim Bewerten weniger ab.")} checked={value.show_source} onChange={(show_source) => set({ show_source })} />
      <Switch label={t("Schlagwörter hervorheben")} hint={t("Markiert die Schlagwörter der Kriterien in Titel und Abstract.")} checked={value.highlight} onChange={(highlight) => set({ highlight })} />
    </>
  );
}

/** Ein Ordner, in dem Dialoge beginnen. `inherited` ist der Wert des Programms, wenn das Projekt keinen eigenen hat. */
function FolderSetting({ label, hint, value, inherited, onChange }: {
  label: string;
  hint: string;
  value: string | null;
  inherited?: string;
  onChange: (path: string | null) => void;
}) {
  async function choose() {
    const path = await open({ multiple: false, directory: true, title: label, defaultPath: value || inherited || undefined });
    if (typeof path === "string") onChange(path);
  }
  const shown = value || inherited;
  return (
    <Row className="folder-row">
      <label>
        {label}
        <small>{hint}</small>
        <span className="folder-path">
          {shown ? <code>{shown}</code> : <span className="muted">{t("Kein Ordner gewählt: Der Dialog beginnt, wo er zuletzt war.")}</span>}
          {inherited !== undefined && !value && shown && <Pill>{t("wie im Programm")}</Pill>}
        </span>
      </label>
      <span className="inline-actions">
        <button className="small" onClick={choose}>{t("Wählen …")}</button>
        {value && <button className="small" onClick={() => onChange(inherited === undefined ? "" : null)}>{t("Zurücksetzen")}</button>}
      </span>
    </Row>
  );
}

/** Ein Speicherort, der nur gezeigt wird. */
function Location({ label, hint, path, onReveal }: { label: string; hint: string; path: string; onReveal: () => void }) {
  return (
    <Row className="folder-row">
      <label>
        {label}
        <small>{hint}</small>
        <span className="folder-path"><code>{path}</code></span>
      </label>
      <span className="inline-actions">
        <button className="small" onClick={onReveal}>{t("Im Finder zeigen")}</button>
      </span>
    </Row>
  );
}

const SHORTCUT_LABEL: Record<ShortcutAction, string> = {
  include: "Include", maybe: "Maybe", exclude: "Exclude",
  next: t("Nächster Eintrag"), previous: t("Voriger Eintrag"), focus: t("Fokusmodus ein und aus"), ai: t("KI-Vorschlag einholen"),
};

/** Ein Tastenkürzel: Klick, dann die neue Taste oder Kombination drücken. Esc bricht ab. */
function ShortcutInput({ label, value, onChange }: { label: string; value: string; onChange: (shortcut: string) => void }) {
  const [recording, setRecording] = useState(false);
  useEffect(() => {
    if (!recording) return;
    // Solange aufgenommen wird, gehört jeder Tastendruck der Aufnahme und erreicht sonst nichts.
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") return setRecording(false);
      const shortcut = shortcutOf(event);
      if (!shortcut) return; // nur eine Zusatztaste: die eigentliche Taste kommt noch
      setRecording(false);
      // Sonst löst das Loslassen der Leertaste den Knopf gleich noch einmal aus.
      (document.activeElement as HTMLElement | null)?.blur();
      onChange(shortcut);
    };
    const stop = () => setRecording(false);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", stop);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", stop);
      window.removeEventListener("blur", stop);
    };
  }, [recording, onChange]);
  return (
    <>
      <button className={`shortcut${recording ? " recording" : ""}`} aria-label={t("Tastenkürzel für {action} ändern", { action: label })} onClick={() => setRecording(!recording)}>
        {recording ? t("Taste drücken …") : value ? <kbd>{shortcutLabel(value)}</kbd> : <span className="muted">{t("Kein Kürzel")}</span>}
      </button>
      <button className="small" disabled={!value || recording} onClick={() => onChange("")}>{t("Entfernen")}</button>
    </>
  );
}

/** Die Tastenkürzel. Ein Kürzel kann nur einer Aktion gehören. */
function ShortcutRows({ value, onChange, onError }: { value: Shortcuts; onChange: (next: Shortcuts) => void; onError: (text: string) => void }) {
  function set(action: ShortcutAction, shortcut: string) {
    const taken = shortcut ? SHORTCUT_ACTIONS.find((other) => other !== action && value[other] === shortcut) : undefined;
    if (taken) return onError(t("„{shortcut}“ gehört schon zu „{action}“.", { shortcut: shortcutLabel(shortcut), action: SHORTCUT_LABEL[taken] }));
    if (shortcut && !allowed(shortcut)) return onError(t("„{shortcut}“ ist fest belegt. Mit ⌃, ⌥ oder ⌘ davor geht die Taste.", { shortcut: shortcutLabel(shortcut) }));
    onChange({ ...value, [action]: shortcut });
  }
  return (
    <>
      {SHORTCUT_ACTIONS.map((action) => (
        <Setting key={action} label={SHORTCUT_LABEL[action]}>
          <ShortcutInput label={SHORTCUT_LABEL[action]} value={value[action]} onChange={(shortcut) => set(action, shortcut)} />
        </Setting>
      ))}
    </>
  );
}

/** Textfeld, das beim Verlassen speichert, damit nicht jeder Tastendruck in die Datei geht. */
function TextSetting({ label, hint, value, placeholder, mono, onSave }: {
  label: string;
  hint?: React.ReactNode;
  value: string;
  placeholder?: string;
  mono?: boolean;
  onSave: (text: string) => void;
}) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <Setting label={label} hint={hint}>
      <input
        className={mono ? "mono" : undefined}
        value={text}
        placeholder={placeholder}
        aria-label={label}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text.trim() !== value && onSave(text.trim())}
        onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
      />
    </Setting>
  );
}

/** Mehrzeiliger Text über die ganze Breite; speichert beim Verlassen. */
function LongText({ label, hint, value, placeholder, rows, mono, onSave, children }: {
  label: string;
  hint?: React.ReactNode;
  value: string;
  placeholder?: string;
  rows: number;
  mono?: boolean;
  onSave: (text: string) => void;
  children?: React.ReactNode;
}) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <Row className="stacked">
      <label>
        {label}
        {hint && <small>{hint}</small>}
      </label>
      <textarea
        className={mono ? "mono" : undefined}
        rows={rows}
        value={text}
        placeholder={placeholder}
        aria-label={label}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text.trim() !== value && onSave(text.trim())}
      />
      {children && <span className="inline-actions">{children}</span>}
    </Row>
  );
}

const PLACEHOLDER_HINT = t("Platzhalter: {{review}} (Titel, Art, Stufe), {{questions}} (Fragestellung), {{criteria}} (Kriterien der Stufe), {{record}} (Eintrag), {{format}} (verlangtes Antwortformat). Fehlen {{record}} oder {{format}}, kommen sie ans Ende.");

const EXTRACTION_HINT = t("Platzhalter: {{review}} (Titel, Art, Stufe), {{questions}} (Fragestellung), {{source}} (Titel der Quelle), {{fields}} (Felder des Formulars), {{fulltext}} (Text des Volltexts mit Seitenmarken), {{format}} (verlangtes Antwortformat). Fehlen {{fields}}, {{fulltext}} oder {{format}}, kommen sie ans Ende.");

/** Die Vorlage des Prompts fürs Screening oder für die Extraktion. Leer gilt die Standardvorlage; sie lässt sich als Ausgangspunkt laden. */
function PromptSetting({ label, value, kind = "screening", onSave, onError }: { label: string; value: string; kind?: "screening" | "extraction"; onSave: (text: string) => void; onError: (text: string) => void }) {
  return (
    <LongText label={label} hint={kind === "extraction" ? EXTRACTION_HINT : PLACEHOLDER_HINT} rows={value ? 14 : 3} value={value} placeholder={t("Leer: Es gilt die Standardvorlage.")} onSave={onSave}>
      {value
        ? <button className="small" onClick={() => onSave("")}>{t("Standardvorlage wiederherstellen")}</button>
        : <button className="small" onClick={() => api.aiDefaultPrompt(kind).then(onSave).catch((e) => onError(errorText(e)))}>{t("Standardvorlage zum Bearbeiten laden")}</button>}
    </LongText>
  );
}

/** Der API-Schlüssel liegt im Schlüsselbund. Die Oberfläche erfährt nur, ob einer hinterlegt ist. */
function ApiKeySetting({ onError }: { onError: (text: string) => void }) {
  const [stored, setStored] = useState<boolean | null>(null);
  const [key, setKey] = useState("");
  useEffect(() => {
    api.aiKeyStatus().then(setStored).catch((e) => onError(errorText(e)));
  }, [onError]);
  const run = (action: Promise<void>, now: boolean) =>
    action.then(() => {
      setStored(now);
      setKey("");
    }).catch((e) => onError(errorText(e)));
  return (
    <Setting label={t("API-Schlüssel")} hint={t("Liegt im Schlüsselbund dieses Rechners, nicht in den Einstellungen. Ein lokales Modell braucht meist keinen.")}>
      {stored ? (
        <>
          <Pill tone="green">{t("hinterlegt")}</Pill>
          <button className="small" onClick={() => run(api.aiKeyClear(), false)}>{t("Entfernen")}</button>
        </>
      ) : (
        <>
          <input
            type="password"
            value={key}
            placeholder={t("Schlüssel einfügen")}
            aria-label={t("API-Schlüssel")}
            autoComplete="off"
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && key.trim() && run(api.aiKeySet(key), true)}
          />
          <button className="small" disabled={!key.trim()} onClick={() => run(api.aiKeySet(key), true)}>{t("Speichern")}</button>
        </>
      )}
    </Setting>
  );
}

/** Zugang zur KI: Adresse, Modell, Schlüssel und Prompt, darunter die Abweichungen vom Muster der API. */
function AiRows({ value, onChange, onError }: { value: Ai; onChange: (next: Ai) => void; onError: (text: string) => void }) {
  const [advanced, setAdvanced] = useState(false);
  const [test, setTest] = useState<{ ok: boolean; text: string } | "running" | null>(null);
  const set = (patch: Partial<Ai>) => onChange({ ...value, ...patch });
  const runTest = () => {
    setTest("running");
    api.aiTest().then((answer) => setTest({ ok: true, text: t("Die KI antwortet: {answer}", { answer }) })).catch((e) => setTest({ ok: false, text: errorText(e) }));
  };
  return (
    <>
      <Switch label={t("KI-Vorschläge")} hint={t("Zeigt auf der Screening-Seite den Knopf „KI-Vorschlag“ und bei der Eingabe im Formular „KI-Vorschlag aus dem Volltext“.")} checked={value.enabled} onChange={(enabled) => set({ enabled })} />
      <TextSetting label={t("Adresse")} hint={t("Bis vor den Pfad, etwa http://localhost:11434/v1 für Ollama.")} value={value.url} placeholder="http://localhost:11434/v1" mono onSave={(url) => set({ url })} />
      <TextSetting label={t("Modell")} hint={t("Name, wie ihn die API erwartet.")} value={value.model} placeholder="llama3.1" mono onSave={(model) => set({ model })} />
      <ApiKeySetting onError={onError} />
      <Switch label={t("Publikationsangaben mitsenden")} hint={t("Autoren, Jahr, Zeitschrift und DOI. Ausgeschaltet urteilt die KI nur nach Titel, Abstract und Schlagwörtern.")} checked={value.publication} onChange={(publication) => set({ publication })} />
      <PromptSetting label={t("Prompt fürs Screening")} value={value.prompt} onSave={(prompt) => set({ prompt })} onError={onError} />
      <PromptSetting label={t("Prompt für die Extraktion")} kind="extraction" value={value.extraction_prompt} onSave={(extraction_prompt) => set({ extraction_prompt })} onError={onError} />
      <Setting label={t("Verbindung")} hint={test && test !== "running" ? <span className={test.ok ? "test-ok" : "test-failed"}>{test.text}</span> : t("Schickt eine kurze Probe mit den Werten oben.")}>
        <button className="small" disabled={test === "running" || !value.enabled} onClick={runTest}>{test === "running" ? t("Probe läuft …") : t("Verbindung testen")}</button>
      </Setting>
      <Switch label={t("Erweitert")} hint={t("Für APIs, die vom Muster abweichen.")} checked={advanced} onChange={setAdvanced} />
      {advanced && (
        <>
          <TextSetting label={t("Pfad des Endpunkts")} hint={t("Hinter der Adresse.")} value={value.path} mono onSave={(path) => set({ path: path || DEFAULTS.ai.path })} />
          <LongText
            label={t("Zusätzliche Felder der Anfrage")}
            hint={t("Ein JSON-Objekt. Seine Felder kommen in die Anfrage und ersetzen gleichnamige, etwa {\"temperature\": 0}.")}
            rows={3}
            mono
            value={value.extra_body}
            placeholder={'{"temperature": 0}'}
            onSave={(extra_body) => set({ extra_body })}
          />
          <TextSetting label={t("Pfad zur Antwort")} hint={t("JSON-Zeiger auf den Antworttext in der Antwort der API.")} value={value.answer_path} mono onSave={(answer_path) => set({ answer_path: answer_path || DEFAULTS.ai.answer_path })} />
          <TextSetting label={t("Zeitlimit in Sekunden")} hint={t("So lange wartet die App auf die Antwort.")} value={String(value.timeout_s)} mono onSave={(text) => set({ timeout_s: Number.parseInt(text, 10) || DEFAULTS.ai.timeout_s })} />
        </>
      )}
    </>
  );
}

const Reset = ({ onClick }: { onClick: () => void }) => <button className="link" onClick={onClick}>{t("Zurücksetzen")}</button>;

export function SettingsPage({ me, onIdentity }: { me: Person; onIdentity: () => void }) {
  const settings = useSettings();
  const [error, setError] = useState("");
  const change = (patch: Partial<Settings>) => {
    setError("");
    saveSettings(patch).catch((e) => setError(errorText(e)));
  };
  const storage = storageInfo();

  return (
    <>
      <PageHeader title={t("Einstellungen")} subtitle={t("Gelten für alle Reviews auf diesem Rechner. Ein Review kann einzelne Werte überschreiben; das steht dort unter „Einstellungen“.")} />
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}

      <Group title={t("Identität")} footer={t("Die Identitätsdatei enthält deinen Namen und deinen öffentlichen Schlüssel. Die Leitung eines Reviews braucht sie, um dich aufzunehmen.")}>
        <Row>
          <span className="person">
            <Avatar name={me.name} size={30} />
            <span><strong>{me.name}</strong><small className="mono">{me.actor_id}</small></span>
          </span>
          <button className="small" onClick={onIdentity}><Icon name="key" size={13} /> {t("Identitätsdatei speichern")}</button>
        </Row>
      </Group>

      <Group title={t("Sprache")}>
        <Setting label={t("Sprache der Oberfläche")} hint={t("Ein Wechsel lädt die Oberfläche neu.")}>
          <select value={settings.language} aria-label={t("Sprache der Oberfläche")} onChange={(e) => setLanguage(e.target.value as "").catch((x) => setError(errorText(x)))}>
            <option value="">{t("Wie im System")}</option>
            {LANGUAGES.map((l) => <option key={l.code} value={l.code}>{l.name}</option>)}
          </select>
        </Setting>
        <Setting label={t("Sprache der Ausgabe")} hint={t("Für Bericht und Registrierung. Ein Review kann eine eigene wählen.")}>
          <select value={settings.output_language} aria-label={t("Sprache der Ausgabe")} onChange={(e) => change({ output_language: e.target.value })}>
            <option value="">{t("Wie die Oberfläche")}</option>
            {LANGUAGES.map((l) => <option key={l.code} value={l.code}>{l.name}</option>)}
          </select>
        </Setting>
      </Group>

      <Group
        title={t("Darstellung")}
        action={<Reset onClick={() => change({ appearance: DEFAULTS.appearance, accent: DEFAULTS.accent, palette: DEFAULTS.palette, font_size: DEFAULTS.font_size, density: DEFAULTS.density })} />}
      >
        <Setting label={t("Erscheinungsbild")}>
          <Segmented
            label={t("Erscheinungsbild")}
            value={settings.appearance}
            options={[{ value: "system", label: t("Wie im System") }, { value: "light", label: t("Hell") }, { value: "dark", label: t("Dunkel") }]}
            onChange={(appearance) => change({ appearance })}
          />
        </Setting>
        <Setting label={t("Akzentfarbe")} hint={ACCENT_LABEL[settings.accent as (typeof ACCENTS)[number]]}>
          <Swatches
            label={t("Akzentfarbe")}
            value={settings.accent as (typeof ACCENTS)[number]}
            options={ACCENTS.map((value) => ({ value, label: ACCENT_LABEL[value] }))}
            onChange={(accent) => change({ accent: accent || "system" })}
          />
        </Setting>
        <Setting
          label={t("Entscheidungsfarben")}
          hint={<span className="decision-preview">{DECISIONS.map(([key, name]) => <span key={key}><i style={{ background: `var(--c-${key})` }} />{name}</span>)}</span>}
        >
          <Segmented
            label={t("Entscheidungsfarben")}
            value={settings.palette}
            options={[{ value: "standard", label: t("Standard") }, { value: "colorblind", label: t("Für Farbfehlsichtigkeit") }]}
            onChange={(palette) => change({ palette })}
          />
        </Setting>
        <Setting label={t("Schriftgröße")}>
          <Segmented
            label={t("Schriftgröße")}
            value={settings.font_size}
            options={[{ value: "small", label: t("Klein") }, { value: "normal", label: t("Normal") }, { value: "large", label: t("Groß") }]}
            onChange={(font_size) => change({ font_size })}
          />
        </Setting>
        <Setting label={t("Dichte")} hint={t("Kompakt zeigt mehr Zeilen auf einmal.")}>
          <Segmented
            label={t("Dichte")}
            value={settings.density}
            options={[{ value: "compact", label: t("Kompakt") }, { value: "normal", label: t("Normal") }]}
            onChange={(density) => change({ density })}
          />
        </Setting>
      </Group>

      <Group title={t("Screening-Seite")} action={<Reset onClick={() => change({ screening: DEFAULTS.screening })} />}>
        <ScreeningRows value={settings.screening} onChange={(screening) => change({ screening })} />
      </Group>

      <Group
        title={t("Tastenkürzel")}
        action={<Reset onClick={() => change({ shortcuts: DEFAULTS.shortcuts })} />}
        footer={t("Zum Ändern auf ein Kürzel klicken und die neue Taste oder Kombination drücken, etwa ⌥M. Die Tasten 1 bis 9 wählen den Ausschlussgrund und Esc beendet den Fokusmodus; beide sind fest. Beim Tippen in einem Feld gelten nur Kürzel mit ⌃ oder ⌘.")}
      >
        <ShortcutRows value={settings.shortcuts} onChange={(shortcuts) => change({ shortcuts })} onError={setError} />
      </Group>

      <Group
        title={t("KI-Unterstützung")}
        footer={t("Für einen Vorschlag im Screening gehen Titel, Abstract und Schlagwörter des Eintrags sowie Titel, Fragestellung und Kriterien des Reviews an die eingetragene Adresse. Für einen Vorschlag zur Extraktion gehen der Text des Volltexts und die Felder des Formulars dorthin. Bei einem lokalen Modell bleibt alles auf diesem Rechner. Jeder Vorschlag wird im Log vermerkt; die Entscheidung trifft immer ein Mensch.")}
      >
        <AiRows value={settings.ai} onChange={(ai) => change({ ai })} onError={setError} />
      </Group>

      <Group
        title={t("Speicherorte")}
        action={<Reset onClick={() => change({ export_dir: "", package_dir: "" })} />}
        footer={t("Die Reviews liegen fest im Datenordner der App; ein Ordner in einer Cloud kann die Dateien beschädigen. Weitergegeben werden Pakete, nie die Review-Dateien selbst.")}
      >
        <Location label={t("Reviews")} hint={t("Je Review eine Datei mit allen Entscheidungen.")} path={storage.reviews_dir} onReveal={() => api.revealStorage().catch((e) => setError(errorText(e)))} />
        <FolderSetting label={t("Exporte")} hint={t("Berichte, Tabellen und andere Dateien zum Weitergeben.")} value={settings.export_dir} onChange={(path) => change({ export_dir: path ?? "" })} />
        <FolderSetting label={t("Pakete")} hint={t("Pakete, Rückmeldungen und Identitätsdateien.")} value={settings.package_dir} onChange={(path) => change({ package_dir: path ?? "" })} />
      </Group>
    </>
  );
}

/** Auswahl der Ausgabesprache eines Reviews: Wert des Programms, Sprache der Oberfläche oder eine feste Sprache. */
const PROGRAM = "program";

export function ProjectSettingsTab({ review, run }: { review: ReviewDetail; run: (action: () => Promise<string | void>, done?: string) => Promise<void> }) {
  const view = useProjectSettings(review.id);
  const program = useSettings();
  const refreshSidebar = useRefreshSidebar();
  const [error, setError] = useState("");
  if (!view) return null;
  const { project } = view;

  const change = (patch: Parameters<typeof saveProjectSettings>[1]) => {
    setError("");
    return saveProjectSettings(review.id, patch).catch((e) => setError(errorText(e)));
  };
  async function setFolder(path: string | null) {
    setError("");
    try {
      await api.setFulltextFolder(review.id, path);
      forgetProjectSettings(review.id);
    } catch (e) {
      setError(errorText(e));
    }
  }
  async function chooseFulltextFolder() {
    const path = await open({ multiple: false, directory: true, title: t("Ordner mit den Volltexten"), defaultPath: view?.fulltext_folder ?? undefined });
    if (typeof path === "string") await setFolder(path);
  }

  return (
    <div className="tab-body">
      {error && <Banner kind="error" onClose={() => setError("")}>{error}</Banner>}
      <Group title={t("Für das ganze Team")} footer={t("Diese Einstellung steht im Log und gilt für alle, sobald sie das nächste Paket eingelesen haben. Schon vermerkte Vorschläge bleiben im Log; der Bericht nennt, in welchem Umfang die KI genutzt wurde.")}>
        {canManage(review.role) ? (
          <Switch
            label={t("KI-Vorschläge erlauben")}
            hint={t("Ausgeschaltet kann im Screening dieses Reviews niemand einen KI-Vorschlag einholen.")}
            checked={review.ai_allowed}
            onChange={(allowed) => run(() => api.aiPolicySet(review.id, allowed))}
          />
        ) : (
          <Setting label={t("KI-Vorschläge")} hint={t("Das legt die Leitung des Reviews fest.")}>
            <span className="muted">{review.ai_allowed ? t("erlaubt") : t("abgeschaltet")}</span>
          </Setting>
        )}
      </Group>

      <p className="hint settings-hint"><Icon name="lock" size={14} /> {t("Diese Einstellungen gelten nur für dich auf diesem Rechner. Sie stehen nicht im Log und gehen in kein Paket.")}</p>

      <Group title={t("Kennzeichnung")}>
        <Setting label={t("Kennfarbe")} hint={t("Markiert das Review in der Liste der Reviews.")}>
          <Swatches
            label={t("Kennfarbe")}
            none={t("Ohne Farbe")}
            value={project.color as (typeof PROJECT_COLORS)[number] | ""}
            options={PROJECT_COLORS.map((value) => ({ value, label: COLOR_LABEL[value] }))}
            onChange={(color) => change({ color }).then(refreshSidebar)}
          />
        </Setting>
      </Group>

      <Group title={t("Sprache")}>
        <Setting label={t("Sprache der Ausgabe")} hint={t("Für Bericht und Registrierung dieses Reviews.")}>
          <select
            value={project.output_language ?? PROGRAM}
            aria-label={t("Sprache der Ausgabe")}
            onChange={(e) => change({ output_language: e.target.value === PROGRAM ? null : e.target.value })}
          >
            <option value={PROGRAM}>{t("Wie im Programm")}</option>
            <option value="">{t("Wie die Oberfläche")}</option>
            {LANGUAGES.map((l) => <option key={l.code} value={l.code}>{l.name}</option>)}
          </select>
        </Setting>
      </Group>

      <Group title={t("Speicherorte")}>
        <Location label={t("Review-Datei")} hint={t("Alle Entscheidungen dieses Reviews. Der Ort ist fest.")} path={view.file} onReveal={() => api.revealStorage(review.id).catch((e) => setError(errorText(e)))} />
        <Row className="folder-row">
          <label>
            {t("Volltexte")}
            <small>{t("Ordner mit den PDFs, wie er auf diesem Rechner liegt.")}</small>
            <span className="folder-path">{view.fulltext_folder ? <code>{view.fulltext_folder}</code> : <span className="muted">{t("Auf diesem Rechner ist kein Ordner verknüpft.")}</span>}</span>
          </label>
          <span className="inline-actions">
            <button className="small" onClick={chooseFulltextFolder}>{t("Wählen …")}</button>
            {view.fulltext_folder && <button className="small" onClick={() => setFolder(null)}>{t("Lösen")}</button>}
          </span>
        </Row>
        <FolderSetting label={t("Exporte")} hint={t("Berichte, Tabellen und andere Dateien zum Weitergeben.")} value={project.export_dir} inherited={program.export_dir} onChange={(export_dir) => change({ export_dir })} />
        <FolderSetting label={t("Pakete")} hint={t("Pakete und Rückmeldungen dieses Reviews.")} value={project.package_dir} inherited={program.package_dir} onChange={(package_dir) => change({ package_dir })} />
      </Group>

      <Group title={t("KI-Unterstützung")} footer={review.ai_allowed ? t("Adresse, Modell und Schlüssel stehen in den Einstellungen des Programms.") : t("Die Leitung hat KI-Vorschläge für dieses Review abgeschaltet. Diese Angaben wirken erst wieder, wenn sie erlaubt sind.")}>
        <Setting label={t("Publikationsangaben mitsenden")} hint={t("Autoren, Jahr, Zeitschrift und DOI.")}>
          <select
            value={project.ai_publication === null ? PROGRAM : project.ai_publication ? "yes" : "no"}
            aria-label={t("Publikationsangaben mitsenden")}
            onChange={(e) => change({ ai_publication: e.target.value === PROGRAM ? null : e.target.value === "yes" })}
          >
            <option value={PROGRAM}>{t("Wie im Programm")}</option>
            <option value="yes">{t("Ja")}</option>
            <option value="no">{t("Nein")}</option>
          </select>
        </Setting>
        <Switch
          label={t("Prompt fürs Screening wie im Programm")}
          hint={t("Ausgeschaltet bekommt dieses Review eine eigene Vorlage.")}
          checked={project.ai_prompt === null}
          onChange={(same) => change({ ai_prompt: same ? null : view.effective.ai_prompt })}
        />
        {project.ai_prompt !== null && <PromptSetting label={t("Prompt für dieses Review")} value={project.ai_prompt} onSave={(ai_prompt) => change({ ai_prompt })} onError={setError} />}
        <Switch
          label={t("Prompt für die Extraktion wie im Programm")}
          hint={t("Ausgeschaltet bekommt dieses Review eine eigene Vorlage.")}
          checked={project.ai_extraction_prompt === null}
          onChange={(same) => change({ ai_extraction_prompt: same ? null : view.effective.ai_extraction_prompt })}
        />
        {project.ai_extraction_prompt !== null && <PromptSetting label={t("Prompt für die Extraktion in diesem Review")} kind="extraction" value={project.ai_extraction_prompt} onSave={(ai_extraction_prompt) => change({ ai_extraction_prompt })} onError={setError} />}
      </Group>

      <Group title={t("Screening-Seite")}>
        <Switch
          label={t("Wie im Programm")}
          hint={t("Ausgeschaltet bekommt dieses Review einen eigenen Aufbau.")}
          checked={project.screening === null}
          onChange={(same) => change({ screening: same ? null : view.effective.screening })}
        />
        {project.screening && <ScreeningRows value={project.screening} onChange={(screening) => change({ screening })} />}
      </Group>
    </div>
  );
}
