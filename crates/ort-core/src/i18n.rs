//! Sprachen.
//!
//! Der deutsche Text im Quelltext ist zugleich der Schlüssel. Jede weitere Sprache ist ein Katalog
//! `i18n/<code>.json`, der den deutschen Text auf seine Übersetzung abbildet, plus ein Eintrag in
//! [`LANGUAGES`]. Fehlt eine Übersetzung, erscheint der deutsche Text.
//!
//! Die Sprache ist eine Einstellung der Person und steht nie im Log: Was Menschen schreiben (Kriterien,
//! Suchstrings, Formulare, Notizen), bleibt, wie es geschrieben wurde. Übersetzt wird nur, was die App
//! selbst sagt: Fehlermeldungen, Texte der mitgelieferten Vorlagen, Bericht, Leitlinien.
//!
//! Die App setzt die Sprache mit [`set_default`] für den ganzen Prozess. [`with`] gilt nur für den
//! laufenden Thread und einen Aufruf, etwa für einen Bericht in einer anderen Sprache oder in Tests.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Language {
    /// Sprachcode nach BCP 47, z. B. `de`.
    pub code: &'static str,
    /// Name der Sprache in dieser Sprache.
    pub name: &'static str,
    #[serde(skip)]
    catalog: &'static str,
}

/// Alle Sprachen der App. Die erste ist die Sprache des Quelltextes und braucht keinen Katalog.
pub const LANGUAGES: [Language; 2] = [
    Language { code: "de", name: "Deutsch", catalog: "{}" },
    Language { code: "en", name: "English", catalog: include_str!("../i18n/en.json") },
];

/// Sprache des Quelltextes.
pub const SOURCE: &str = LANGUAGES[0].code;

static DEFAULT: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static OVERRIDE: Cell<Option<usize>> = const { Cell::new(None) };
}

fn index(code: &str) -> Option<usize> {
    // `en-GB` zählt als `en`.
    let base = code.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
    LANGUAGES.iter().position(|l| l.code == base)
}

/// Ob die App diese Sprache kennt.
pub fn is_known(code: &str) -> bool {
    index(code).is_some()
}

/// Setzt die Sprache für den ganzen Prozess. Unbekannte Codes ändern nichts und geben `false` zurück.
pub fn set_default(code: &str) -> bool {
    index(code).map(|i| DEFAULT.store(i, Ordering::Relaxed)).is_some()
}

fn current_index() -> usize {
    OVERRIDE.with(Cell::get).unwrap_or_else(|| DEFAULT.load(Ordering::Relaxed))
}

/// Code der Sprache, die gerade gilt.
pub fn current() -> &'static str {
    LANGUAGES[current_index()].code
}

/// Führt `run` in der Sprache `code` aus; danach gilt wieder die vorige. Unbekannte Codes ändern nichts.
pub fn with<T>(code: &str, run: impl FnOnce() -> T) -> T {
    struct Reset(Option<usize>);
    impl Drop for Reset {
        fn drop(&mut self) {
            OVERRIDE.with(|o| o.set(self.0));
        }
    }
    let _reset = Reset(OVERRIDE.with(Cell::get));
    if let Some(i) = index(code) {
        OVERRIDE.with(|o| o.set(Some(i)));
    }
    run()
}

fn catalogs() -> &'static Vec<HashMap<String, String>> {
    static CATALOGS: OnceLock<Vec<HashMap<String, String>>> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        LANGUAGES
            .iter()
            .map(|l| serde_json::from_str(l.catalog).expect("mitgelieferter Sprachkatalog ist gültiges JSON"))
            .collect()
    })
}

/// Katalog einer Sprache: deutscher Text → Übersetzung. Für Tests und Werkzeuge.
pub fn catalog(code: &str) -> Option<&'static HashMap<String, String>> {
    index(code).map(|i| &catalogs()[i])
}

/// Übersetzt einen festen Text in die geltende Sprache. Ohne Eintrag bleibt der Text, wie er ist;
/// damit lassen sich auch Texte aus dem Log gefahrlos durchreichen (Namen von Stationen der
/// mitgelieferten Vorlagen werden übersetzt, eigene Texte bleiben stehen).
pub fn text(source: &str) -> String {
    lookup(source).to_string()
}

/// Wie [`text`], aber für ein Wort, das je nach Zusammenhang anders zu übersetzen ist: Der Katalog
/// kann es unter „Zusammenhang: Wort“ eigens führen (z. B. „Herkunft: Zitationssuche“ klein im Satz,
/// „Zitationssuche“ groß als Name einer Station). Ohne solchen Eintrag gilt [`text`].
pub fn text_in(context: &str, source: &str) -> String {
    match current_index() {
        0 => source.to_string(),
        i => catalogs()[i].get(&format!("{context}: {source}")).cloned().unwrap_or_else(|| text(source)),
    }
}

fn lookup(source: &str) -> &str {
    match current_index() {
        0 => source,
        i => catalogs()[i].get(source).map_or(source, String::as_str),
    }
}

/// Setzt Werte in ein Muster ein: `{}` der Reihe nach, `{0}`, `{1}` nach Position (für Sprachen mit
/// anderer Wortstellung), `{{` und `}}` für geschweifte Klammern. Aufruf über [`tr!`](crate::tr).
pub fn render(source: &str, args: &[&dyn Display]) -> String {
    debug_assert_eq!(placeholders(source), args.len(), "Zahl der Platzhalter passt nicht zu den Werten: {source}");
    fill(lookup(source), args)
}

fn fill(pattern: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(pattern.len() + 16 * args.len());
    let mut next = 0;
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                for d in chars.by_ref() {
                    if d == '}' {
                        break;
                    }
                    name.push(d);
                }
                let position = if name.is_empty() {
                    next += 1;
                    Some(next - 1)
                } else {
                    name.parse::<usize>().ok()
                };
                match position.and_then(|p| args.get(p)) {
                    Some(value) => out.push_str(&value.to_string()),
                    None => {
                        out.push('{');
                        out.push_str(&name);
                        out.push('}');
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Zahl der Werte, die ein Muster erwartet.
pub fn placeholders(pattern: &str) -> usize {
    let cleaned = pattern.replace("{{", "").replace("}}", "");
    let mut sequential = 0;
    let mut highest = 0;
    let mut rest = cleaned.as_str();
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else { break };
        let name = &rest[start + 1..start + len];
        if name.is_empty() {
            sequential += 1;
        } else if let Ok(n) = name.parse::<usize>() {
            highest = highest.max(n + 1);
        }
        rest = &rest[start + len + 1..];
    }
    sequential.max(highest)
}

/// Text in der geltenden Sprache. Das erste Argument ist der deutsche Text als Literal, danach
/// folgen die Werte für `{}`. Formatangaben wie `{:.1}` und Namen wie `{name}` gibt es nicht:
/// Werte vorher formatieren und der Reihe nach übergeben.
///
/// ```
/// let n = 3;
/// assert_eq!(ort_core::tr!("{} Einträge", n), "3 Einträge");
/// ```
#[macro_export]
macro_rules! tr {
    ($source:literal $(,)?) => {
        $crate::i18n::render($source, &[])
    };
    ($source:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::render($source, &[$(&$arg as &dyn ::std::fmt::Display),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_in_order_and_by_position() {
        assert_eq!(fill("{} von {}", &[&2, &5]), "2 von 5");
        assert_eq!(fill("{1} of {0}", &[&2, &5]), "5 of 2");
        assert_eq!(fill("{{}} bleibt, {} nicht", &[&"x"]), "{} bleibt, x nicht");
        assert_eq!(placeholders("{} von {}"), 2);
        assert_eq!(placeholders("{1} of {0}"), 2);
        assert_eq!(placeholders("{{}} ohne"), 0);
    }

    #[test]
    fn language_holds_for_one_call_only() {
        assert_eq!(current(), SOURCE);
        let inside = with("en-GB", || (current(), with("de", current)));
        assert_eq!(inside, ("en", "de"));
        assert_eq!(current(), SOURCE);
        assert_eq!(with("xx", current), SOURCE);
        assert!(!is_known("xx"));
    }

    #[test]
    fn unknown_text_stays_as_written() {
        assert_eq!(with("en", || text("Ein Text, den kein Katalog kennt")), "Ein Text, den kein Katalog kennt");
    }

    /// Jede Übersetzung erwartet dieselben Werte wie der deutsche Text.
    #[test]
    fn translations_take_the_same_values() {
        for language in &LANGUAGES[1..] {
            for (source, translation) in catalog(language.code).unwrap() {
                assert_eq!(placeholders(source), placeholders(translation), "{}: {source}", language.code);
                assert!(!translation.trim().is_empty(), "{}: leere Übersetzung für {source}", language.code);
            }
        }
    }

    /// Jeder Text, den der Kern mit `tr!` ausgibt, steht in jedem Katalog.
    #[test]
    fn every_message_is_translated() {
        let literal = regex::Regex::new(r#"\btr!\(\s*"((?:[^"\\]|\\.)*)""#).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let files = [root.join("src"), root.join("../../app/src-tauri/src")]
            .into_iter()
            .flat_map(|dir| std::fs::read_dir(dir).unwrap())
            .map(|entry| entry.unwrap().path());
        let mut missing = Vec::new();
        for path in files {
            if path.file_name().is_some_and(|n| n == "i18n.rs") {
                continue;
            }
            let code = std::fs::read_to_string(&path).unwrap();
            for found in literal.captures_iter(&code) {
                // Das Literal so lesen, wie Rust es liest.
                let source: String =
                    serde_json::from_str(&format!("\"{}\"", &found[1])).unwrap_or(found[1].to_string());
                for language in &LANGUAGES[1..] {
                    if !catalog(language.code).unwrap().contains_key(&source) {
                        missing.push(format!("{} [{}]: {source}", path.display(), language.code));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "Ohne Übersetzung:\n{}", missing.join("\n"));
    }
}
