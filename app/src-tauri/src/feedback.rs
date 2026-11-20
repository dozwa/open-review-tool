//! Rückmeldung an die Entwicklung: ein Fehler oder ein Wunsch, als E-Mail oder als Issue auf GitHub.
//!
//! Die App sendet nichts selbst und braucht dafür weder Konto noch Schlüssel. Sie baut einen Link mit
//! Betreff und Text und übergibt ihn dem System: `mailto:` öffnet einen Entwurf im Mailprogramm, der Link
//! zu GitHub das ausgefüllte Formular für ein neues Issue im Browser. Abgeschickt wird erst dort, von der
//! Person selbst. Beide Ziele stehen hier fest; die Oberfläche kann keine andere Adresse öffnen lassen.

use ort_core::tr;

/// Postfach der Entwicklung.
const MAIL: &str = "dorian.zwanzig@gmail.com";
/// Formular für ein neues Issue im Repository.
const ISSUES: &str = "https://github.com/dozwa/open-review-tool/issues/new";

const MAX_TITLE: usize = 200;
/// Der Text reist im Link; GitHub nimmt Links bis etwa 8.000 Zeichen an.
const MAX_BODY: usize = 4000;
const MAX_URL: usize = 8000;

/// Prozentkodierung nach RFC 3986: alles außer Buchstaben, Ziffern und `-._~` wird kodiert.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Der Link für eine Rückmeldung. `channel` ist `email` oder `github`, `kind` ist `bug` oder `feature`.
/// `system` (App- und Systemversion) wird an den Text gehängt.
pub fn link(channel: &str, kind: &str, title: &str, body: &str, system: &str) -> Result<String, String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let body = body.trim();
    if title.is_empty() {
        return Err(tr!("Der Rückmeldung fehlt ein Titel."));
    }
    if title.chars().count() > MAX_TITLE || body.chars().count() > MAX_BODY {
        return Err(tr!("Die Rückmeldung ist zu lang. Kürze den Text oder schick Einzelheiten nach."));
    }
    let label = match kind {
        "bug" => "bug",
        "feature" => "enhancement",
        _ => return Err(tr!("Unbekannte Art der Rückmeldung.")),
    };
    let body = format!("{body}\n\n---\n{system}");
    let url = match channel {
        // Im Mailprogramm trennen CRLF die Zeilen (RFC 6068).
        "email" => format!(
            "mailto:{MAIL}?subject={}&body={}",
            encode(&format!("[Open Review Tool] {title}")),
            encode(&body.replace('\n', "\r\n"))
        ),
        "github" => format!("{ISSUES}?labels={label}&title={}&body={}", encode(&title), encode(&body)),
        _ => return Err(tr!("Unbekannter Weg für die Rückmeldung.")),
    };
    if url.len() > MAX_URL {
        return Err(tr!("Die Rückmeldung ist zu lang. Kürze den Text oder schick Einzelheiten nach."));
    }
    Ok(url)
}

/// App- und Systemversion, wie sie unter der Rückmeldung stehen.
pub fn system() -> String {
    let macos = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|version| version.trim().to_string())
        .filter(|version| !version.is_empty());
    match macos {
        Some(version) => format!("Open Review Tool {} · macOS {version}", env!("CARGO_PKG_VERSION")),
        None => format!("Open Review Tool {}", env!("CARGO_PKG_VERSION")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_link_carries_subject_and_text() {
        let url =
            link("email", "bug", "  Stimme  geht verloren ", "Zeile 1\nZeile 2 & mehr", "ORT 1 · macOS 15").unwrap();
        assert!(url.starts_with(
            "mailto:dorian.zwanzig@gmail.com?subject=%5BOpen%20Review%20Tool%5D%20Stimme%20geht%20verloren&body="
        ));
        assert!(url.contains("Zeile%201%0D%0AZeile%202%20%26%20mehr"));
        assert!(url.ends_with("---%0D%0AORT%201%20%C2%B7%20macOS%2015"));
    }

    #[test]
    fn github_link_opens_the_issue_form_with_a_label() {
        let url = link("github", "feature", "Excel-Export", "Wäre schön?", "ORT 1").unwrap();
        assert!(url.starts_with("https://github.com/dozwa/open-review-tool/issues/new?labels=enhancement&title=Excel-Export&body=W%C3%A4re%20sch%C3%B6n%3F"));
        assert!(link("github", "bug", "x", "y", "z").unwrap().contains("labels=bug&"));
    }

    #[test]
    fn empty_long_and_unknown_are_rejected() {
        assert!(link("email", "bug", "  ", "Text", "").is_err());
        assert!(link("email", "bug", "Titel", &"x".repeat(MAX_BODY + 1), "").is_err());
        assert!(link("github", "bug", "Titel", &"ä".repeat(MAX_BODY), "").is_err());
        assert!(link("fax", "bug", "Titel", "Text", "").is_err());
        assert!(link("email", "lob", "Titel", "Text", "").is_err());
    }
}
