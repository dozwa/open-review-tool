/// Fehler mit deutschem Text für die Oberfläche. Die Varianten entsprechen den früheren HTTP-Status:
/// `Invalid` 400, `Forbidden` 403, `NotFound` 404, `Conflict` 409.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    /// Datei beschädigt, verändert oder Signatur ungültig.
    #[error("{0}")]
    Integrity(String),
    #[error("Datenbankfehler: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Ungültiges JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Dateifehler: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

pub(crate) fn forbidden(message: impl Into<String>) -> Error {
    Error::Forbidden(message.into())
}

pub(crate) fn not_found(message: impl Into<String>) -> Error {
    Error::NotFound(message.into())
}

pub(crate) fn conflict(message: impl Into<String>) -> Error {
    Error::Conflict(message.into())
}

pub(crate) fn integrity(message: impl Into<String>) -> Error {
    Error::Integrity(message.into())
}
