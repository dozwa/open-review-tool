//! Einstellungen auf zwei Ebenen, beide nur auf diesem Rechner.
//!
//! - **Programm** ([`Settings`]): eine Datei `settings.json` im Datenverzeichnis der App; gilt für alle
//!   Reviews.
//! - **Projekt** ([`ProjectSettings`]): ein JSON-Wert in der Tabelle `meta` der Review-Datei; überschreibt
//!   einzelne Programmwerte für dieses Review.
//!
//! Nichts davon steht im Log oder in einem Paket: Farben, Ordner und Ansicht gehören der Person, nicht
//! dem Team. [`resolve`] ist die einzige Stelle, die beide Ebenen zusammenführt.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{invalid, Result};
use crate::service::Review;
use crate::{i18n, tr};

/// Aktuelle Version der Datei `settings.json`.
pub const VERSION: u32 = 1;
const FILE: &str = "settings.json";
const META_KEY: &str = "settings";

pub const APPEARANCES: [&str; 3] = ["system", "light", "dark"];
pub const ACCENTS: [&str; 8] = ["system", "blue", "purple", "pink", "red", "orange", "green", "graphite"];
pub const PALETTES: [&str; 2] = ["standard", "colorblind"];
pub const FONT_SIZES: [&str; 3] = ["small", "normal", "large"];
pub const DENSITIES: [&str; 2] = ["compact", "normal"];
/// Kennfarben eines Reviews; leer heißt ohne.
pub const PROJECT_COLORS: [&str; 8] = ["blue", "purple", "pink", "red", "orange", "yellow", "green", "gray"];

/// Aufbau der Screening-Seite.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Screening {
    /// Spalte mit der Liste der Einträge.
    pub show_list: bool,
    /// Spalte mit den Kriterien.
    pub show_criteria: bool,
    /// Autoren und Zeitschrift beim Eintrag.
    pub show_source: bool,
    /// Schlagwörter der Kriterien im Text hervorheben.
    pub highlight: bool,
}

impl Default for Screening {
    fn default() -> Self {
        Self { show_list: true, show_criteria: true, show_source: false, highlight: true }
    }
}

/// Tastenkürzel der Arbeit an der Literatur. Ein Kürzel ist eine Taste, davor beliebig viele der Zusatztasten
/// in fester Reihenfolge, mit „+“ verbunden (`M`, `Alt+M`, `Ctrl+Shift+ArrowDown`). Leer heißt: kein Kürzel.
/// Die Ziffern 1 bis 9 allein wählen den Ausschlussgrund und sind deshalb nicht frei.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    pub include: String,
    pub maybe: String,
    pub exclude: String,
    /// Nächster und voriger Eintrag der Liste.
    pub next: String,
    pub previous: String,
    /// Fokusmodus ein und aus.
    pub focus: String,
    /// KI-Vorschlag zum offenen Eintrag einholen.
    pub ai: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            include: "I".into(),
            maybe: "M".into(),
            exclude: "E".into(),
            next: "J".into(),
            previous: "K".into(),
            focus: "F".into(),
            ai: "A".into(),
        }
    }
}

/// Zusatztasten in der Reihenfolge, in der sie in einem Kürzel stehen.
pub const MODIFIERS: [&str; 4] = ["Ctrl", "Alt", "Shift", "Meta"];
/// Tasten, die fest belegt sind oder ein Feld verlassen würden.
const RESERVED_KEYS: [&str; 6] = ["Escape", "Tab", "Enter", "Space", "Backspace", "Delete"];

impl Shortcuts {
    fn all(&self) -> [&String; 7] {
        [&self.include, &self.maybe, &self.exclude, &self.next, &self.previous, &self.focus, &self.ai]
    }

    pub fn check(&self) -> Result<()> {
        for (i, shortcut) in self.all().into_iter().enumerate() {
            if shortcut.is_empty() {
                continue;
            }
            let mut parts: Vec<&str> = shortcut.split('+').collect();
            let key = parts.pop().unwrap_or_default();
            // Die Zusatztasten stehen in fester Reihenfolge, jede höchstens einmal.
            let positions: Vec<usize> =
                parts.iter().filter_map(|part| MODIFIERS.iter().position(|m| m == part)).collect();
            let ordered = positions.len() == parts.len() && positions.windows(2).all(|pair| pair[0] < pair[1]);
            let plain = parts.iter().all(|part| *part == "Shift");
            let digit = key.len() == 1 && key.chars().all(|c| c.is_ascii_digit());
            let reserved = plain && (digit || RESERVED_KEYS.contains(&key));
            let named = key.chars().count() > 1 && key.chars().all(|c| c.is_ascii_alphanumeric());
            let single = key.chars().count() == 1 && !key.chars().any(|c| c.is_whitespace() || c.is_lowercase());
            if !ordered || reserved || !(named || single) || MODIFIERS.contains(&key) {
                return Err(invalid(tr!("Das Tastenkürzel „{}“ geht nicht.", shortcut)));
            }
            if self.all()[..i].contains(&shortcut) {
                return Err(invalid(tr!("Das Tastenkürzel „{}“ ist doppelt vergeben.", shortcut)));
            }
        }
        Ok(())
    }
}

/// Zugang zu einer KI für Vorschläge im Screening: eine API nach dem Muster von OpenAI (`chat/completions`),
/// etwa ein lokales Ollama. Der API-Schlüssel steht nicht hier, sondern im Schlüsselbund.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ai {
    pub enabled: bool,
    /// Adresse der API bis vor den Pfad, z. B. `http://localhost:11434/v1`.
    pub url: String,
    pub model: String,
    /// Eigene Vorlage für den Prompt; leer heißt Standardvorlage.
    pub prompt: String,
    /// Eigene Vorlage für den Prompt der Extraktion aus dem Volltext; leer heißt Standardvorlage.
    pub extraction_prompt: String,
    /// Autoren, Jahr, Zeitschrift und DOI mitsenden.
    pub publication: bool,
    /// Pfad des Endpunkts hinter der Adresse.
    pub path: String,
    /// JSON-Objekt, dessen Felder in die Anfrage kommen und gleichnamige ersetzen, z. B. `{"temperature": 0}`.
    pub extra_body: String,
    /// JSON-Zeiger auf den Antworttext in der Antwort der API.
    pub answer_path: String,
    /// Wie lange auf die Antwort gewartet wird, in Sekunden.
    pub timeout_s: u32,
}

pub const AI_PATH: &str = "/chat/completions";
pub const AI_ANSWER_PATH: &str = "/choices/0/message/content";
pub const AI_TIMEOUT: u32 = 120;
const AI_TIMEOUTS: std::ops::RangeInclusive<u32> = 5..=900;

impl Default for Ai {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            model: String::new(),
            prompt: String::new(),
            extraction_prompt: String::new(),
            publication: false,
            path: AI_PATH.into(),
            extra_body: String::new(),
            answer_path: AI_ANSWER_PATH.into(),
            timeout_s: AI_TIMEOUT,
        }
    }
}

fn check_url(url: &str) -> Result<()> {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"));
    match url.is_empty() || rest.is_some_and(|r| !r.is_empty() && !r.contains(char::is_whitespace)) {
        true => Ok(()),
        false => Err(invalid(tr!("Die Adresse der KI beginnt mit http:// oder https://: {}", url))),
    }
}

fn check_extra_body(text: &str) -> Result<()> {
    match text.trim().is_empty() || serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text).is_ok() {
        true => Ok(()),
        false => Err(invalid(tr!("Die zusätzlichen Felder der Anfrage sind kein JSON-Objekt."))),
    }
}

fn check_slash(value: &str, what: String) -> Result<()> {
    match value.starts_with('/') && !value.contains(char::is_whitespace) {
        true => Ok(()),
        false => Err(invalid(tr!("{} beginnt mit „/“: {}", what, value))),
    }
}

impl Ai {
    pub fn check(&self) -> Result<()> {
        check_url(&self.url)?;
        check_slash(&self.path, tr!("Der Pfad des Endpunkts"))?;
        check_extra_body(&self.extra_body)?;
        check_slash(&self.answer_path, tr!("Der Pfad zur Antwort"))?;
        match AI_TIMEOUTS.contains(&self.timeout_s) {
            true => Ok(()),
            false => Err(invalid(tr!(
                "Das Zeitlimit liegt zwischen {} und {} Sekunden.",
                AI_TIMEOUTS.start(),
                AI_TIMEOUTS.end()
            ))),
        }
    }

    /// Setzt einzelne Werte, die diese Version nicht annimmt, auf den Standard zurück.
    fn repair(&mut self) {
        let standard = Self::default();
        if check_url(&self.url).is_err() {
            self.url.clear();
        }
        if check_slash(&self.path, String::new()).is_err() {
            self.path = standard.path;
        }
        if check_extra_body(&self.extra_body).is_err() {
            self.extra_body.clear();
        }
        if check_slash(&self.answer_path, String::new()).is_err() {
            self.answer_path = standard.answer_path;
        }
        if !AI_TIMEOUTS.contains(&self.timeout_s) {
            self.timeout_s = standard.timeout_s;
        }
    }
}

/// Einstellungen des Programms. Leere Texte heißen „keine eigene Wahl“: Sprache des Systems, Ausgabe in
/// der Sprache der Oberfläche, Dialoge im zuletzt benutzten Ordner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub language: String,
    pub output_language: String,
    pub appearance: String,
    pub accent: String,
    pub palette: String,
    pub font_size: String,
    pub density: String,
    pub screening: Screening,
    pub shortcuts: Shortcuts,
    pub ai: Ai,
    /// Ordner, in dem Dialoge für Berichte, Tabellen und andere Exporte beginnen.
    pub export_dir: String,
    /// Ordner, in dem Dialoge für Pakete und Rückmeldungen beginnen.
    pub package_dir: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            language: String::new(),
            output_language: String::new(),
            appearance: "system".into(),
            accent: "system".into(),
            palette: "standard".into(),
            font_size: "normal".into(),
            density: "normal".into(),
            screening: Screening::default(),
            shortcuts: Shortcuts::default(),
            ai: Ai::default(),
            export_dir: String::new(),
            package_dir: String::new(),
        }
    }
}

/// Einstellungen eines Reviews. `None` heißt „wie im Programm“.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    pub output_language: Option<String>,
    pub export_dir: Option<String>,
    pub package_dir: Option<String>,
    pub screening: Option<Screening>,
    /// Eigene Vorlage für den KI-Prompt in diesem Review.
    pub ai_prompt: Option<String>,
    /// Eigene Vorlage für den Prompt der Extraktion in diesem Review.
    pub ai_extraction_prompt: Option<String>,
    pub ai_publication: Option<bool>,
    /// Kennfarbe in der Liste der Reviews; leer heißt ohne.
    pub color: String,
}

/// Was für ein Review gilt: der Wert des Projekts, sonst der des Programms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Effective {
    pub output_language: String,
    pub export_dir: String,
    pub package_dir: String,
    pub screening: Screening,
    pub ai_prompt: String,
    pub ai_extraction_prompt: String,
    pub ai_publication: bool,
}

/// Führt Programm und Projekt zusammen.
pub fn resolve(program: &Settings, project: &ProjectSettings) -> Effective {
    Effective {
        output_language: project.output_language.clone().unwrap_or_else(|| program.output_language.clone()),
        export_dir: project.export_dir.clone().unwrap_or_else(|| program.export_dir.clone()),
        package_dir: project.package_dir.clone().unwrap_or_else(|| program.package_dir.clone()),
        screening: project.screening.clone().unwrap_or_else(|| program.screening.clone()),
        ai_prompt: project.ai_prompt.clone().unwrap_or_else(|| program.ai.prompt.clone()),
        ai_extraction_prompt: project
            .ai_extraction_prompt
            .clone()
            .unwrap_or_else(|| program.ai.extraction_prompt.clone()),
        ai_publication: project.ai_publication.unwrap_or(program.ai.publication),
    }
}

fn one_of(value: &str, allowed: &[&str], what: String) -> Result<()> {
    match allowed.contains(&value) {
        true => Ok(()),
        false => Err(invalid(tr!("{}: „{}“ gibt es nicht.", what, value))),
    }
}

fn check_language(code: &str) -> Result<()> {
    match code.is_empty() || i18n::LANGUAGES.iter().any(|l| l.code == code) {
        true => Ok(()),
        false => Err(invalid(tr!("Unbekannte Sprache: {}", code))),
    }
}

/// Ein Ordner ist leer (keine Wahl) oder ein vollständiger Pfad. Ob es ihn gerade gibt, wird nicht
/// geprüft: Ein Laufwerk darf fehlen, der Dialog beginnt dann woanders.
fn check_dir(path: &str) -> Result<()> {
    match path.is_empty() || Path::new(path).is_absolute() {
        true => Ok(()),
        false => Err(invalid(tr!("Ein Ordner braucht den vollständigen Pfad: {}", path))),
    }
}

impl Settings {
    pub fn check(&self) -> Result<()> {
        check_language(&self.language)?;
        check_language(&self.output_language)?;
        one_of(&self.appearance, &APPEARANCES, tr!("Erscheinungsbild"))?;
        one_of(&self.accent, &ACCENTS, tr!("Akzentfarbe"))?;
        one_of(&self.palette, &PALETTES, tr!("Entscheidungsfarben"))?;
        one_of(&self.font_size, &FONT_SIZES, tr!("Schriftgröße"))?;
        one_of(&self.density, &DENSITIES, tr!("Dichte"))?;
        self.shortcuts.check()?;
        self.ai.check()?;
        check_dir(&self.export_dir)?;
        check_dir(&self.package_dir)
    }

    /// Liest die Einstellungen aus dem Datenverzeichnis. Ohne Datei gelten die Standardwerte. Eine
    /// unlesbare Datei oder ein Wert, den diese Version nicht kennt, sperrt die App nicht aus: Der
    /// einzelne Wert fällt auf den Standard zurück.
    pub fn load(data_dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(data_dir.join(FILE)) else {
            return Self::default();
        };
        let mut settings: Self = serde_json::from_str(&text).unwrap_or_default();
        let standard = Self::default();
        let known = |value: &mut String, allowed: &[&str], fallback: &str| {
            if !allowed.contains(&value.as_str()) {
                *value = fallback.to_string();
            }
        };
        known(&mut settings.appearance, &APPEARANCES, &standard.appearance);
        known(&mut settings.accent, &ACCENTS, &standard.accent);
        known(&mut settings.palette, &PALETTES, &standard.palette);
        known(&mut settings.font_size, &FONT_SIZES, &standard.font_size);
        known(&mut settings.density, &DENSITIES, &standard.density);
        for language in [&mut settings.language, &mut settings.output_language] {
            if check_language(language).is_err() {
                language.clear();
            }
        }
        for dir in [&mut settings.export_dir, &mut settings.package_dir] {
            if check_dir(dir).is_err() {
                dir.clear();
            }
        }
        if settings.shortcuts.check().is_err() {
            // Ein Kürzel, das mit dieser Version dazukam, darf ein selbst gewähltes nicht verdrängen.
            settings.shortcuts.ai.clear();
        }
        if settings.shortcuts.check().is_err() {
            settings.shortcuts = standard.shortcuts;
        }
        settings.ai.repair();
        settings.version = VERSION;
        settings
    }

    /// Ob im Datenverzeichnis schon Einstellungen liegen.
    pub fn stored(data_dir: &Path) -> bool {
        data_dir.join(FILE).is_file()
    }

    /// Prüft und schreibt die Einstellungen. Geschrieben wird erst in eine zweite Datei, die dann die
    /// alte ersetzt; ein Abbruch lässt nie eine halbe Datei zurück.
    pub fn save(&self, data_dir: &Path) -> Result<()> {
        self.check()?;
        let mut settings = self.clone();
        settings.version = VERSION;
        let tmp = data_dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_string_pretty(&settings)?)?;
        std::fs::rename(tmp, data_dir.join(FILE))?;
        Ok(())
    }
}

impl ProjectSettings {
    pub fn check(&self) -> Result<()> {
        check_language(self.output_language.as_deref().unwrap_or_default())?;
        check_dir(self.export_dir.as_deref().unwrap_or_default())?;
        check_dir(self.package_dir.as_deref().unwrap_or_default())?;
        match self.color.is_empty() {
            true => Ok(()),
            false => one_of(&self.color, &PROJECT_COLORS, tr!("Kennfarbe")),
        }
    }
}

impl Review {
    /// Einstellungen dieses Reviews auf diesem Rechner. Jedes Mitglied hat eigene, auch wer nur liest.
    pub fn project_settings(&self) -> Result<ProjectSettings> {
        let stored = self.store.meta(META_KEY)?.unwrap_or_default();
        let mut project: ProjectSettings = serde_json::from_str(&stored).unwrap_or_default();
        if !PROJECT_COLORS.contains(&project.color.as_str()) {
            project.color.clear();
        }
        Ok(project)
    }

    pub fn set_project_settings(&self, project: &ProjectSettings) -> Result<()> {
        project.check()?;
        self.store.set_meta(META_KEY, &serde_json::to_string(project)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_allow_combinations_and_reject_clashes() {
        let with = |include: &str, maybe: &str| Shortcuts {
            include: include.into(),
            maybe: maybe.into(),
            ..Shortcuts::default()
        };
        for good in ["Alt+M", "Ctrl+Alt+Shift+Meta+ArrowDown", "Shift+Y", "Alt+1", "F5", ",", ""] {
            with(good, "M").check().unwrap_or_else(|e| panic!("{good}: {e}"));
        }
        // Falsche Reihenfolge, doppelte oder fehlende Taste, feste Tasten, Kleinbuchstaben.
        for bad in ["Alt+Ctrl+M", "Alt+Alt+M", "Alt+", "Alt", "Hyper+M", "1", "Shift+3", "Escape", "Enter", "m", "A B"]
        {
            assert!(with(bad, "M").check().is_err(), "{bad}");
        }
        assert!(with("Alt+M", "Alt+M").check().is_err());
        // Ohne Kürzel dürfen mehrere Aktionen sein.
        with("", "").check().unwrap();
        let settings = Settings { shortcuts: with("J", "M"), ..Settings::default() };
        assert!(settings.check().is_err());
    }

    #[test]
    fn project_value_overrides_program_value() {
        let program = Settings {
            output_language: "en".into(),
            export_dir: "/a".into(),
            package_dir: "/p".into(),
            ..Settings::default()
        };
        let own = Screening { show_criteria: false, ..Screening::default() };
        let project = ProjectSettings {
            export_dir: Some("/b".into()),
            output_language: Some(String::new()),
            screening: Some(own.clone()),
            ..ProjectSettings::default()
        };

        let both = resolve(&program, &project);
        assert_eq!(both.export_dir, "/b");
        assert_eq!(both.package_dir, "/p");
        // Ein leerer Projektwert ist eine Wahl („wie die Oberfläche“), kein Rückfall auf das Programm.
        assert_eq!(both.output_language, "");
        assert_eq!(both.screening, own);

        let plain = resolve(&program, &ProjectSettings::default());
        assert_eq!(plain.output_language, "en");
        assert_eq!(plain.export_dir, "/a");
        assert_eq!(plain.screening, Screening::default());
    }

    #[test]
    fn file_round_trip_and_fallbacks() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!Settings::stored(dir.path()));
        assert_eq!(Settings::load(dir.path()), Settings::default());

        let wanted = Settings { appearance: "dark".into(), language: "en".into(), ..Settings::default() };
        wanted.save(dir.path()).unwrap();
        assert!(Settings::stored(dir.path()));
        assert_eq!(Settings::load(dir.path()), wanted);

        // Fehlende Felder nehmen den Standard, unbekannte Werte auch; Unbekanntes daneben stört nicht.
        let text = r#"{"version": 7, "accent": "neon", "density": "compact", "language": "xx", "neu": 1,
            "export_dir": "relativ", "screening": {"show_list": false}}"#;
        std::fs::write(dir.path().join(FILE), text).unwrap();
        let loaded = Settings::load(dir.path());
        assert_eq!(loaded.accent, "system");
        assert_eq!(loaded.density, "compact");
        assert_eq!(loaded.language, "");
        assert_eq!(loaded.export_dir, "");
        assert_eq!(loaded.version, VERSION);
        assert!(!loaded.screening.show_list && loaded.screening.show_criteria);

        // Ein Kürzel, das nicht geht, setzt alle Kürzel auf den Standard zurück.
        std::fs::write(dir.path().join(FILE), r#"{"shortcuts": {"include": "Alt+", "maybe": "Alt+M"}}"#).unwrap();
        assert_eq!(Settings::load(dir.path()).shortcuts, Shortcuts::default());

        // Das Kürzel für den KI-Vorschlag kam später dazu: Belegt jemand „A“ schon selbst, bleibt die eigene Wahl.
        std::fs::write(dir.path().join(FILE), r#"{"shortcuts": {"focus": "A"}}"#).unwrap();
        let loaded = Settings::load(dir.path()).shortcuts;
        assert_eq!((loaded.focus.as_str(), loaded.ai.as_str()), ("A", ""));

        // Werte der KI fallen einzeln zurück; Adresse, Modell und Prompt bleiben.
        let text = r#"{"ai": {"enabled": true, "url": "http://localhost:11434/v1", "model": "llama3", "prompt": "P",
            "path": "chat", "extra_body": "[1]", "answer_path": "", "timeout_s": 0}}"#;
        std::fs::write(dir.path().join(FILE), text).unwrap();
        let ai = Settings::load(dir.path()).ai;
        let kept = Ai {
            enabled: true,
            url: "http://localhost:11434/v1".into(),
            model: "llama3".into(),
            prompt: "P".into(),
            ..Ai::default()
        };
        assert_eq!(ai, kept);

        // Eine kaputte Datei sperrt niemanden aus.
        std::fs::write(dir.path().join(FILE), "{kaputt").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }

    #[test]
    fn ai_access_is_checked_and_overridden_per_review() {
        let with = |change: fn(&mut Ai)| {
            let mut ai = Ai::default();
            change(&mut ai);
            ai.check()
        };
        assert!(with(|_| {}).is_ok());
        assert!(with(|ai| ai.url = "https://api.example.org/v1".into()).is_ok());
        assert!(with(|ai| ai.extra_body = r#"{"temperature": 0, "response_format": {"type": "json_object"}}"#.into())
            .is_ok());
        assert!(with(|ai| ai.url = "localhost:11434".into()).is_err());
        assert!(with(|ai| ai.url = "http://".into()).is_err());
        assert!(with(|ai| ai.path = "chat/completions".into()).is_err());
        assert!(with(|ai| ai.extra_body = "temperature=0".into()).is_err());
        assert!(with(|ai| ai.answer_path = "choices.0".into()).is_err());
        assert!(with(|ai| ai.timeout_s = 1).is_err());

        let program = Settings { ai: Ai { prompt: "Programm".into(), ..Ai::default() }, ..Settings::default() };
        let plain = resolve(&program, &ProjectSettings::default());
        assert_eq!((plain.ai_prompt.as_str(), plain.ai_publication), ("Programm", false));
        let project = ProjectSettings {
            ai_prompt: Some("Projekt".into()),
            ai_publication: Some(true),
            ..ProjectSettings::default()
        };
        let both = resolve(&program, &project);
        assert_eq!((both.ai_prompt.as_str(), both.ai_publication), ("Projekt", true));
    }

    #[test]
    fn invalid_values_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for broken in [
            Settings { appearance: "bunt".into(), ..Settings::default() },
            Settings { language: "xx".into(), ..Settings::default() },
            Settings { export_dir: "ordner".into(), ..Settings::default() },
        ] {
            assert!(broken.save(dir.path()).is_err());
        }
        assert!(!Settings::stored(dir.path()));
        assert!(ProjectSettings { color: "neon".into(), ..ProjectSettings::default() }.check().is_err());
        assert!(ProjectSettings { package_dir: Some("x".into()), ..ProjectSettings::default() }.check().is_err());
        assert!(ProjectSettings { color: "green".into(), ..ProjectSettings::default() }.check().is_ok());
    }
}
