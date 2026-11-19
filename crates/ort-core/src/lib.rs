//! Fachkern des Open Review Tool.
//!
//! Alle Entscheidungen eines Reviews stehen als signierte, unveränderliche Ereignisse in einer
//! SQLite-Datei. Jede Person schreibt eigene Hash-Ketten: eine für Verwaltung (`main`) und je Stufe
//! eine für Voten und Markierungen (`stage:<id>`). Dadurch kann ein Austauschpaket die verblindeten
//! Voten anderer Personen als ganze Ketten weglassen, ohne Lücken zu reißen.
//!
//! Schichten: [`event`] (Umschlag, Hash, Signatur) und [`catalog`] (Register der Ereignistypen) →
//! [`store`] (einzige Schreibstelle) →
//! [`projection`], [`workflow`] und [`agreement`] (aus dem Log berechnet) → [`service`] (Fachlogik,
//! Rechte, Verblindung) → [`exchange`] (Dateien zwischen Personen). [`report`] rechnet aus alldem
//! das Flussdiagramm und die Anhänge, [`sources`] bündelt Einträge zu Quellen, [`forms`] hält Formulare,
//! Werte und den Abgleich je Feld, [`protocol`] die Registrierung des Protokolls, die Abweichungen danach
//! und die Checkliste des Berichts. [`settings`] hält, was nur für diesen Rechner gilt: Einstellungen des
//! Programms und je Review. [`ai`] baut den Prompt für einen KI-Vorschlag im Screening und liest die
//! Antwort; gesendet wird in der App.

pub mod agreement;
pub mod ai;
pub mod canonical;
pub mod catalog;
pub mod criteria;
pub mod duplicates;
pub mod error;
pub mod event;
pub mod exchange;
pub mod forms;
pub mod fulltext;
pub mod guidance;
pub mod i18n;
pub mod import;
pub mod projection;
pub mod protocol;
pub mod registration;
pub mod report;
pub mod search;
pub mod service;
pub mod settings;
pub mod sources;
pub mod store;
pub mod workflow;

pub use error::{Error, Result};
pub use event::{Event, Identity, PublicIdentity};
pub use service::Review;
