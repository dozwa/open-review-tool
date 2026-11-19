//! Anfrage an eine KI über eine API nach dem Muster von OpenAI (`chat/completions`), etwa ein lokales
//! Ollama. Den Prompt baut der Kern (`ort_core::ai`); hier wird nur gesendet und der Antworttext gelesen.
//! Adresse, Modell und die Abweichungen vom Muster stehen in den Einstellungen, der API-Schlüssel im
//! Schlüsselbund.

use std::time::Duration;

use ort_core::ai::Message;
use ort_core::settings::Ai;
use ort_core::tr;
use serde_json::{json, Value};

/// Name, unter dem der API-Schlüssel abgelegt ist.
pub const KEY_NAME: &str = "ai-api-key";

/// Der Anfang eines Textes, höchstens `max` Zeichen.
pub fn excerpt(text: &str, max: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{} …", &text[..end]),
        None => text.to_string(),
    }
}

/// Der Körper der Anfrage: Modell und Nachrichten, dazu die Felder aus den Einstellungen. Diese ersetzen
/// gleichnamige, damit sich die Anfrage an eine abweichende API anpassen lässt.
pub fn body(access: &Ai, messages: &[Message]) -> Value {
    let mut body = json!({"model": access.model.trim(), "messages": messages, "stream": false});
    if let Ok(Value::Object(extra)) = serde_json::from_str::<Value>(&access.extra_body) {
        for (key, value) in extra {
            body[key] = value;
        }
    }
    body
}

/// Liest den Antworttext an der eingestellten Stelle der Antwort.
pub fn answer(access: &Ai, response: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(response)
        .map_err(|_| tr!("Die Antwort der KI ist kein JSON: {}", excerpt(response, 300)))?;
    match value.pointer(&access.answer_path).and_then(Value::as_str) {
        Some(text) => Ok(text.to_string()),
        None => {
            Err(tr!("In der Antwort der KI steht unter {} kein Text: {}", access.answer_path, excerpt(response, 300)))
        }
    }
}

/// Sendet die Nachrichten und gibt den Antworttext zurück. Blockiert, bis die Antwort da ist.
pub fn complete(access: &Ai, key: Option<&str>, messages: &[Message]) -> Result<String, String> {
    let url = format!("{}{}", access.url.trim_end_matches('/'), access.path);
    let agent =
        ureq::AgentBuilder::new().timeout(Duration::from_secs(u64::from(access.timeout_s))).redirects(0).build();
    let mut request = agent.post(&url).set("Content-Type", "application/json");
    if let Some(key) = key {
        request = request.set("Authorization", &format!("Bearer {key}"));
    }
    match request.send_string(&body(access, messages).to_string()) {
        Ok(response) => {
            let text = response.into_string().map_err(|e| tr!("Antwort der KI nicht lesbar: {}", e))?;
            answer(access, &text)
        }
        Err(ureq::Error::Status(code @ (401 | 403), _)) => {
            Err(tr!("Die KI weist den Zugang ab ({}). Prüfe den API-Schlüssel in den Einstellungen.", code))
        }
        Err(ureq::Error::Status(404, response)) => Err(tr!(
            "Die KI kennt die Adresse oder das Modell nicht (404). Prüfe Adresse, Pfad und Modell in den Einstellungen. {}",
            excerpt(&response.into_string().unwrap_or_default(), 300)
        )),
        Err(ureq::Error::Status(code, response)) => {
            Err(tr!("Die KI meldet einen Fehler ({}): {}", code, excerpt(&response.into_string().unwrap_or_default(), 300)))
        }
        Err(ureq::Error::Transport(e)) => Err(tr!("Die KI antwortet nicht: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_answer_follow_the_settings() {
        let messages = [Message { role: "user".into(), content: "Hallo".into() }];
        let standard = Ai { model: " llama3 ".into(), ..Ai::default() };
        let plain = body(&standard, &messages);
        assert_eq!(
            plain,
            json!({"model": "llama3", "messages": [{"role": "user", "content": "Hallo"}], "stream": false})
        );
        let response = r#"{"choices": [{"message": {"role": "assistant", "content": "OK"}}]}"#;
        assert_eq!(answer(&standard, response).unwrap(), "OK");
        assert!(answer(&standard, "<html>").unwrap_err().contains("kein JSON"));
        assert!(answer(&standard, r#"{"error": "x"}"#).unwrap_err().contains("kein Text"));

        // Eine abweichende API: weitere und ersetzte Felder in der Anfrage, die Antwort an anderer Stelle.
        let other = Ai {
            extra_body: r#"{"temperature": 0, "model": "anders", "options": {"num_ctx": 8192}}"#.into(),
            answer_path: "/message/content".into(),
            ..standard
        };
        let changed = body(&other, &messages);
        assert_eq!((changed["model"].as_str(), changed["temperature"].as_i64()), (Some("anders"), Some(0)));
        assert_eq!(changed["options"]["num_ctx"], 8192);
        assert_eq!(answer(&other, r#"{"message": {"content": "Ja"}}"#).unwrap(), "Ja");

        assert_eq!(excerpt("  kurz ", 10), "kurz");
        assert_eq!(excerpt("äöüß", 2), "äö …");
    }
}
