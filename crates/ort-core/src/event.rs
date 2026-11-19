//! Ereignis-Umschlag, Hash, Signatur und Identität einer Person.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::canonical::to_canonical;
use crate::catalog::{self, Chain};
use crate::error::{integrity, invalid, Result};

pub const MAIN_STREAM: &str = "main";

/// Voten und Markierungen liegen in einer eigenen Kette je Person und Stufe, damit ein Paket sie
/// bei verblindeten Stufen als Ganzes weglassen kann. Welche Typen das sind, steht in [`crate::catalog`].
pub use crate::catalog::is_blindable;

pub fn stage_stream(stage_id: &Uuid) -> String {
    format!("stage:{stage_id}")
}

pub fn stream_for(event_type: &str, stage_id: Option<&Uuid>) -> Result<String> {
    let spec = catalog::spec(event_type).ok_or_else(|| invalid(format!("Unbekannter Ereignistyp {event_type}.")))?;
    match spec.chain {
        Chain::Stage => {
            let stage = stage_id.ok_or_else(|| invalid(format!("{event_type} braucht eine Stufe.")))?;
            Ok(stage_stream(stage))
        }
        Chain::Main => Ok(MAIN_STREAM.to_string()),
    }
}

/// UTC-Zeitstempel mit fester Länge (Mikrosekunden), damit der Textvergleich die Zeitordnung ergibt.
pub fn now_timestamp() -> String {
    let t = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        t.microsecond()
    )
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event_id: Uuid,
    pub review_id: Uuid,
    pub actor_id: Uuid,
    pub stream: String,
    pub seq: i64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub schema_version: i64,
    pub occurred_at: String,
    pub stage_instance_id: Option<Uuid>,
    pub payload: Value,
    pub prev_hash: Option<String>,
    pub hash: String,
    pub signature: String,
}

impl Event {
    /// Alles außer Hash und Signatur, kanonisch serialisiert. Darüber wird der Hash gebildet.
    pub fn hashed_content(&self) -> Result<String> {
        to_canonical(&json!({
            "event_id": self.event_id.to_string(),
            "review_id": self.review_id.to_string(),
            "actor_id": self.actor_id.to_string(),
            "stream": self.stream,
            "seq": self.seq,
            "type": self.event_type,
            "schema_version": self.schema_version,
            "occurred_at": self.occurred_at,
            "stage_instance_id": self.stage_instance_id.map(|s| s.to_string()),
            "payload": self.payload,
            "prev_hash": self.prev_hash,
        }))
    }

    pub fn compute_hash(&self) -> Result<String> {
        Ok(sha256_hex(self.hashed_content()?.as_bytes()))
    }

    /// Prüft Hash und Signatur. Die Kette (Vorgänger) prüft der Aufrufer.
    pub fn verify(&self, key: &VerifyingKey) -> Result<()> {
        if self.compute_hash()? != self.hash {
            return Err(integrity(format!(
                "Prüfsumme stimmt nicht: Ereignis {} ({}) wurde verändert.",
                self.event_id, self.event_type
            )));
        }
        let hash = hex::decode(&self.hash).map_err(|_| integrity("Prüfsumme ist kein Hex-Text."))?;
        let sig_bytes = B64.decode(&self.signature).map_err(|_| integrity("Signatur ist kein Base64-Text."))?;
        let sig_array: [u8; 64] =
            sig_bytes.as_slice().try_into().map_err(|_| integrity("Signatur hat die falsche Länge."))?;
        key.verify(&hash, &Signature::from_bytes(&sig_array)).map_err(|_| {
            integrity(format!(
                "Signatur ungültig: Ereignis {} ({}) stammt nicht von der angegebenen Person.",
                self.event_id, self.event_type
            ))
        })
    }

    pub fn payload_str(&self, key: &str) -> Option<&str> {
        self.payload.get(key).and_then(Value::as_str)
    }
}

/// Eine Person mit privatem Schlüssel. Nur auf dem eigenen Rechner vorhanden.
#[derive(Clone)]
pub struct Identity {
    pub actor_id: Uuid,
    pub name: String,
    key: SigningKey,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity").field("actor_id", &self.actor_id).field("name", &self.name).finish()
    }
}

impl Identity {
    pub fn generate(name: &str) -> Result<Identity> {
        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut secret).map_err(|e| invalid(format!("Kein Zufall verfügbar: {e}")))?;
        Ok(Identity { actor_id: Uuid::new_v4(), name: name.trim().to_string(), key: SigningKey::from_bytes(&secret) })
    }

    pub fn from_secret(actor_id: Uuid, name: &str, secret: &[u8]) -> Result<Identity> {
        let secret: [u8; 32] = secret.try_into().map_err(|_| invalid("Privater Schlüssel hat die falsche Länge."))?;
        Ok(Identity { actor_id, name: name.to_string(), key: SigningKey::from_bytes(&secret) })
    }

    pub fn secret_bytes(&self) -> [u8; 32] {
        self.key.to_bytes()
    }

    pub fn public_key(&self) -> String {
        B64.encode(self.key.verifying_key().to_bytes())
    }

    pub fn public(&self) -> PublicIdentity {
        PublicIdentity { actor_id: self.actor_id, name: self.name.clone(), public_key: self.public_key() }
    }

    pub(crate) fn sign_hash(&self, hash_hex: &str) -> Result<String> {
        let hash = hex::decode(hash_hex).map_err(|_| invalid("Prüfsumme ist kein Hex-Text."))?;
        Ok(B64.encode(self.key.sign(&hash).to_bytes()))
    }
}

/// Was andere über eine Person wissen: ID, Anzeigename, öffentlicher Schlüssel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublicIdentity {
    pub actor_id: Uuid,
    pub name: String,
    pub public_key: String,
}

pub fn parse_public_key(text: &str) -> Result<VerifyingKey> {
    let bytes = B64.decode(text).map_err(|_| integrity("Öffentlicher Schlüssel ist kein Base64-Text."))?;
    let array: [u8; 32] =
        bytes.as_slice().try_into().map_err(|_| integrity("Öffentlicher Schlüssel hat die falsche Länge."))?;
    VerifyingKey::from_bytes(&array).map_err(|_| integrity("Öffentlicher Schlüssel ist ungültig."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(me: &Identity) -> Event {
        let mut e = Event {
            event_id: Uuid::new_v4(),
            review_id: Uuid::new_v4(),
            actor_id: me.actor_id,
            stream: MAIN_STREAM.into(),
            seq: 1,
            event_type: "ReviewCreated".into(),
            schema_version: 1,
            occurred_at: now_timestamp(),
            stage_instance_id: None,
            payload: json!({"title": "Test"}),
            prev_hash: None,
            hash: String::new(),
            signature: String::new(),
        };
        e.hash = e.compute_hash().unwrap();
        e.signature = me.sign_hash(&e.hash).unwrap();
        e
    }

    #[test]
    fn signed_event_verifies_and_detects_changes() {
        let me = Identity::generate("Anna").unwrap();
        let key = parse_public_key(&me.public_key()).unwrap();
        let e = sample(&me);
        e.verify(&key).unwrap();

        let mut changed = e.clone();
        changed.payload = json!({"title": "Anders"});
        assert!(changed.verify(&key).is_err());

        // Neu gehasht, aber ohne den Schlüssel neu signiert: Signatur passt nicht mehr.
        changed.hash = changed.compute_hash().unwrap();
        assert!(changed.verify(&key).is_err());

        let other = Identity::generate("Ben").unwrap();
        assert!(e.verify(&parse_public_key(&other.public_key()).unwrap()).is_err());
    }

    #[test]
    fn identity_roundtrips_through_secret() {
        let me = Identity::generate("Anna").unwrap();
        let again = Identity::from_secret(me.actor_id, "Anna", &me.secret_bytes()).unwrap();
        assert_eq!(me.public(), again.public());
    }

    #[test]
    fn timestamps_sort_as_text() {
        let a = now_timestamp();
        let b = now_timestamp();
        assert_eq!(a.len(), 27);
        assert!(a <= b);
    }
}
