//! Eigene Identität: Name und öffentlicher Schlüssel in `identity.json`, der private Schlüssel im
//! macOS-Schlüsselbund. Mit `ORT_KEY_STORE=file` liegt er stattdessen in `identity.key` im
//! Datenverzeichnis (nur für Entwicklung und Tests, z. B. eine zweite Identität per `ORT_DATA_DIR`).

use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ort_core::{Identity, PublicIdentity};

const KEYCHAIN_SERVICE: &str = "open-review-tool";

pub struct IdentityStore {
    dir: PathBuf,
    file_keys: bool,
}

impl IdentityStore {
    pub fn new(dir: &Path) -> IdentityStore {
        let file_keys = std::env::var("ORT_KEY_STORE").is_ok_and(|v| v == "file");
        IdentityStore { dir: dir.to_path_buf(), file_keys }
    }

    fn public_path(&self) -> PathBuf {
        self.dir.join("identity.json")
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join("identity.key")
    }

    pub fn load(&self) -> Result<Option<Identity>, String> {
        let Ok(text) = std::fs::read_to_string(self.public_path()) else { return Ok(None) };
        let public: PublicIdentity =
            serde_json::from_str(&text).map_err(|e| format!("identity.json ist beschädigt: {e}"))?;
        let secret = self.read_secret(&public)?;
        let identity = Identity::from_secret(public.actor_id, &public.name, &secret).map_err(|e| e.to_string())?;
        if identity.public_key() != public.public_key {
            return Err("Der private Schlüssel passt nicht zu identity.json.".into());
        }
        Ok(Some(identity))
    }

    pub fn create(&self, name: &str) -> Result<Identity, String> {
        if name.trim().is_empty() {
            return Err("Bitte einen Namen eingeben.".into());
        }
        if self.public_path().exists() {
            return Err("Auf diesem Rechner gibt es schon eine Identität.".into());
        }
        let identity = Identity::generate(name).map_err(|e| e.to_string())?;
        self.write_secret(&identity)?;
        let text = serde_json::to_string_pretty(&identity.public()).map_err(|e| e.to_string())?;
        std::fs::write(self.public_path(), text).map_err(|e| e.to_string())?;
        Ok(identity)
    }

    fn read_secret(&self, public: &PublicIdentity) -> Result<Vec<u8>, String> {
        let encoded = if self.file_keys {
            std::fs::read_to_string(self.key_path()).map_err(|e| format!("identity.key fehlt: {e}"))?
        } else {
            keyring::Entry::new(KEYCHAIN_SERVICE, &public.actor_id.to_string())
                .and_then(|entry| entry.get_password())
                .map_err(|e| format!("Privater Schlüssel nicht im Schlüsselbund gefunden: {e}"))?
        };
        B64.decode(encoded.trim()).map_err(|_| "Privater Schlüssel ist beschädigt.".to_string())
    }

    fn write_secret(&self, identity: &Identity) -> Result<(), String> {
        let encoded = B64.encode(identity.secret_bytes());
        if self.file_keys {
            write_private(&self.key_path(), &encoded)
        } else {
            keyring::Entry::new(KEYCHAIN_SERVICE, &identity.actor_id.to_string())
                .and_then(|entry| entry.set_password(&encoded))
                .map_err(|e| format!("Schlüssel konnte nicht im Schlüsselbund gespeichert werden: {e}"))
        }
    }
}

fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file =
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())
}
