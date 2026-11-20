//! Lesender Zugriff auf eine öffentliche Registrierung bei OSF (`https://api.osf.io/v2`).
//!
//! Die Leitung nennt Adresse, Kennung oder DOI einer Registrierung. Geholt werden die Registrierung, ihr
//! Formular mit dessen Blöcken und die genannten Mitwirkenden; an OSF geht dabei nur die Kennung. Es
//! braucht kein Konto, deshalb sind private und gesperrte Registrierungen nicht lesbar. Wie die Texte
//! auf die Felder kommen, rechnet der Kern (`ort_core::registration::osf_import`). Geschrieben wird nichts.

use std::time::Duration;

use ort_core::registration::{osf_import, OsfImport};
use ort_core::tr;
use serde_json::Value;

const API: &str = "https://api.osf.io/v2";
/// Mehr Seiten einer Liste werden nicht gelesen.
const MAX_PAGES: usize = 10;

/// Die Kennung aus dem, was die Person eingibt: `abc12`, `osf.io/abc12`, die Adresse der Seite oder die
/// DOI (`10.17605/OSF.IO/ABC12`).
pub fn id_of(input: &str) -> Result<String, String> {
    let lower = input.trim().to_lowercase();
    // Entweder die bloße Kennung oder etwas, worin `osf.io/` vorkommt; andere Adressen gelten nicht.
    let path = lower.rsplit_once("osf.io/").map_or(lower.as_str(), |(_, rest)| rest);
    path.split(['/', '?', '#'])
        .find(|part| (5..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric()))
        .filter(|_| !path.contains(char::is_whitespace) && (path.len() < lower.len() || !path.contains(['/', '.'])))
        .map(str::to_string)
        .ok_or_else(|| tr!("Das ist keine Adresse einer Registrierung bei OSF. Erwartet wird etwa osf.io/abc12."))
}

fn get(url: &str) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).redirects(0).build();
    let unreadable = |e: &dyn std::fmt::Display| tr!("Antwort von OSF nicht lesbar: {}", e);
    match agent.get(url).set("User-Agent", "open-review-tool").set("Accept", "application/vnd.api+json").call() {
        Ok(response) => {
            serde_json::from_str(&response.into_string().map_err(|e| unreadable(&e))?).map_err(|e| unreadable(&e))
        }
        Err(ureq::Error::Status(401 | 403, _)) => Err(tr!(
            "Diese Registrierung ist nicht öffentlich. Die App liest nur, was bei OSF ohne Anmeldung zu sehen ist."
        )),
        Err(ureq::Error::Status(404 | 410, _)) => Err(tr!("OSF kennt unter dieser Adresse keine Registrierung.")),
        Err(ureq::Error::Status(code, _)) => Err(tr!("OSF meldet einen Fehler ({}).", code)),
        Err(ureq::Error::Transport(e)) => Err(tr!("OSF antwortet nicht: {}", e)),
    }
}

/// Alle Einträge einer Liste, Seite für Seite. Folgeseiten werden nur von OSF selbst geholt.
fn list(url: String) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    let mut next = Some(url);
    for _ in 0..MAX_PAGES {
        let Some(url) = next.filter(|url| url.starts_with(API)) else { break };
        let mut page = get(&url)?;
        next = page["links"]["next"].as_str().map(str::to_string);
        if let Some(Value::Array(data)) = page.get_mut("data").map(Value::take) {
            out.extend(data);
        }
    }
    Ok(out)
}

/// Liest eine Registrierung und verteilt ihre Texte auf die Felder des Formulars.
pub fn fetch(input: &str) -> Result<OsfImport, String> {
    let mut id = id_of(input)?;
    let registration = match get(&format!("{API}/registrations/{id}/")) {
        Ok(found) => found,
        // Die Adresse eines Projekts statt der Registrierung: Hat es genau eine, ist sie gemeint.
        Err(missing) => {
            let own = list(format!("{API}/nodes/{id}/registrations/")).unwrap_or_default();
            let ids: Vec<&str> = own.iter().filter_map(|r| r["id"].as_str()).collect();
            match ids[..] {
                [] => return Err(missing),
                [only] => id = only.to_string(),
                _ => {
                    let links: Vec<String> = ids.iter().map(|id| format!("osf.io/{id}")).collect();
                    return Err(tr!(
                        "Das ist ein Projekt mit mehreren Registrierungen. Trag eine davon ein: {}",
                        links.join(", ")
                    ));
                }
            }
            get(&format!("{API}/registrations/{id}/"))?
        }
    };
    let schema_id =
        registration["data"]["relationships"]["registration_schema"]["data"]["id"].as_str().unwrap_or_default();
    if schema_id.is_empty() || !schema_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(tr!("Unter dieser Adresse liegt keine lesbare Registrierung."));
    }
    let schema = get(&format!("{API}/schemas/registrations/{schema_id}/"))?;
    let blocks = list(format!("{API}/schemas/registrations/{schema_id}/schema_blocks/?page%5Bsize%5D=100"))?;
    let contributors = list(format!("{API}/registrations/{id}/bibliographic_contributors/?page%5Bsize%5D=100"))?;
    osf_import(&registration, &schema, &blocks, &contributors).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_read_from_address_doi_or_itself() {
        for input in [
            "abc12",
            " osf.io/ABC12 ",
            "https://osf.io/abc12/",
            "https://osf.io/abc12/overview?view_only=",
            "https://doi.org/10.17605/OSF.IO/ABC12",
            "https://osf.io/registries/osf/abc12",
        ] {
            assert_eq!(id_of(input).as_deref(), Ok("abc12"), "{input}");
        }
        for input in ["", "osf.io", "https://example.org/abc12", "osf.io/abc12 und mehr", "mein protokoll", "a-b_c"] {
            assert!(id_of(input).is_err(), "{input}");
        }
    }

    /// Liest eine echte Registrierung: `ORT_OSF_TEST=<Kennung> cargo test -p open-review-tool osf -- --ignored --nocapture`.
    #[test]
    #[ignore = "braucht das Netz und eine Kennung in ORT_OSF_TEST"]
    fn a_public_registration_is_read() {
        let found = fetch(&std::env::var("ORT_OSF_TEST").expect("ORT_OSF_TEST fehlt")).unwrap();
        println!("{} · {} · {}", found.identifier, found.schema, found.registered_on);
        println!("{} Felder, {} ohne Feld, {} Dateien", found.answers.len(), found.unmatched.len(), found.files.len());
        for rest in &found.unmatched {
            println!("ohne Feld: {} ({} Zeichen)", rest.label, rest.text.chars().count());
        }
        assert!(!found.answers.is_empty());
    }
}
