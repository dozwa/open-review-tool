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
}

/// Führt Programm und Projekt zusammen.
pub fn resolve(program: &Settings, project: &ProjectSettings) -> Effective {
    Effective {
        output_language: project.output_language.clone().unwrap_or_else(|| program.output_language.clone()),
        export_dir: project.export_dir.clone().unwrap_or_else(|| program.export_dir.clone()),
        package_dir: project.package_dir.clone().unwrap_or_else(|| program.package_dir.clone()),
        screening: project.screening.clone().unwrap_or_else(|| program.screening.clone()),
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

        // Eine kaputte Datei sperrt niemanden aus.
        std::fs::write(dir.path().join(FILE), "{kaputt").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
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
