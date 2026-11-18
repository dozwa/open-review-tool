//! Fachkern des Open Review Tool.
//!
//! Alle Entscheidungen eines Reviews stehen als signierte, unveränderliche Ereignisse in einer
//! SQLite-Datei. Jede Person schreibt eigene Hash-Ketten: eine für Verwaltung (`main`) und je Stufe
//! eine für Voten und Markierungen (`stage:<id>`). Dadurch kann ein Austauschpaket die verblindeten
//! Voten anderer Personen als ganze Ketten weglassen, ohne Lücken zu reißen.
//!
//! Schichten: [`event`] (Umschlag, Hash, Signatur) → [`store`] (einzige Schreibstelle) →
//! [`projection`] und [`agreement`] (aus dem Log berechnet) → [`service`] (Fachlogik, Rechte,
//! Verblindung) → [`exchange`] (Dateien zwischen Personen).

pub mod agreement;
pub mod canonical;
pub mod error;
pub mod event;
pub mod exchange;
pub mod import;
pub mod projection;
pub mod service;
pub mod store;

pub use error::{Error, Result};
pub use event::{Event, Identity, PublicIdentity};
pub use service::Review;
