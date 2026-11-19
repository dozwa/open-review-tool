// Einstellungen: die Seite des Programms und der Bereich „Einstellungen“ eines Reviews. Beide gelten nur
// auf diesem Rechner; jede Änderung wird sofort gespeichert.

import React, { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { api, errorText, Person, ReviewDetail } from "./api";
import { LANGUAGES, setLanguage, t } from "./i18n";
import {
  ACCENTS, DEFAULTS, forgetProjectSettings, PROJECT_COLORS, saveProjectSettings, saveSettings, ScreeningLayout, Settings, Shortcuts, storageInfo,
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
  next: t("Nächster Eintrag"), previous: t("Voriger Eintrag"), focus: t("Fokusmodus ein und aus"),
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

export function ProjectSettingsTab({ review }: { review: ReviewDetail }) {
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
