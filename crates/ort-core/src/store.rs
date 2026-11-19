//! SQLite-Datei eines Reviews. Einzige Schreibstelle für das Event-Log.
//!
//! Das Log ist nur anhängbar: Trigger brechen jedes UPDATE und DELETE auf `event` ab. Records,
//! Importe und Stufenzuweisungen sind gewöhnliche Tabellen (Stammdaten, keine Entscheidungen);
//! ihre Inhalte sind über Prüfsummen in den Ereignissen `RecordsImported` und `StageInstanceOpened`
//! abgesichert.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::canonical::to_canonical;
use crate::catalog;
use crate::error::{invalid, Result};
use crate::event::{now_timestamp, sha256_hex, stream_for, Event, Identity};

/// Schritte des Tabellenschemas. Die Nummer des letzten ausgeführten Schritts steht in
/// `PRAGMA user_version`. Neue Schritte nur anhängen, bestehende nie ändern.
///
/// Schritt 1 ist das Schema der ersten App-Version. Er ist wiederholbar (`IF NOT EXISTS`), weil
/// Dateien aus dieser Version noch `user_version` 0 tragen.
///
/// Schritt 2: `append` fragt je Ereignis nach der letzten Zeit der Person. Ohne Index liest das alle
/// ihre Ereignisse; 40.000 Stimmen anzuhängen dauerte damit über zwei Minuten.
const MIGRATIONS: &[&str] =
    &[SCHEMA, "CREATE INDEX IF NOT EXISTS event_actor_time_idx ON event (actor_id, occurred_at);"];

const SCHEMA: &str = r#"

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS event (
  local_seq         INTEGER PRIMARY KEY AUTOINCREMENT,  -- Reihenfolge des Eintreffens auf diesem Rechner
  event_id          TEXT NOT NULL UNIQUE,
  review_id         TEXT NOT NULL,
  actor_id          TEXT NOT NULL,
  stream            TEXT NOT NULL,
  seq               INTEGER NOT NULL,
  type              TEXT NOT NULL,
  schema_version    INTEGER NOT NULL,
  occurred_at       TEXT NOT NULL,
  stage_instance_id TEXT,
  payload           TEXT NOT NULL,                      -- kanonisches JSON
  prev_hash         TEXT,
  hash              TEXT NOT NULL,
  signature         TEXT NOT NULL,
  UNIQUE (actor_id, stream, seq)
);
CREATE INDEX IF NOT EXISTS event_stage_idx ON event (stage_instance_id, type);

CREATE TRIGGER IF NOT EXISTS event_no_update BEFORE UPDATE ON event
BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;
CREATE TRIGGER IF NOT EXISTS event_no_delete BEFORE DELETE ON event
BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;

-- Klarnamen getrennt vom Log, damit sie löschbar bleiben. Im Log steht nur die actor_id.
CREATE TABLE IF NOT EXISTS person (
  actor_id TEXT PRIMARY KEY,
  name     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS import_batch (
  id           TEXT PRIMARY KEY,
  filename     TEXT NOT NULL,
  format       TEXT NOT NULL,
  source       TEXT NOT NULL DEFAULT '',
  sha256       TEXT NOT NULL,
  status       TEXT NOT NULL,
  n_records    INTEGER NOT NULL DEFAULT 0,
  n_duplicates INTEGER NOT NULL DEFAULT 0,
  error        TEXT NOT NULL DEFAULT '',
  created_by   TEXT NOT NULL,
  created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS record (
  id           TEXT PRIMARY KEY,
  batch_id     TEXT NOT NULL REFERENCES import_batch (id),
  title        TEXT NOT NULL DEFAULT '',
  abstract     TEXT NOT NULL DEFAULT '',
  authors      TEXT NOT NULL DEFAULT '',
  year         INTEGER,
  journal      TEXT NOT NULL DEFAULT '',
  doi          TEXT NOT NULL DEFAULT '',
  pmid         TEXT NOT NULL DEFAULT '',
  url          TEXT NOT NULL DEFAULT '',
  keywords     TEXT NOT NULL DEFAULT '',
  raw          TEXT NOT NULL DEFAULT '{}',
  dedup_key    TEXT NOT NULL,
  duplicate_of TEXT REFERENCES record (id)
);
CREATE INDEX IF NOT EXISTS record_dedup_idx ON record (dedup_key);
CREATE INDEX IF NOT EXISTS record_batch_idx ON record (batch_id);

CREATE TABLE IF NOT EXISTS stage_assignment (
  stage_instance_id TEXT NOT NULL,
  record_id         TEXT NOT NULL REFERENCES record (id),
  position          INTEGER NOT NULL,
  PRIMARY KEY (stage_instance_id, record_id)
);
"#;

pub struct Store {
    conn: Connection,
}

/// Letztes Ereignis einer Kette.
#[derive(Clone, Debug)]
pub struct Head {
    pub seq: i64,
    pub hash: String,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        // Gilt je Verbindung und wirkt nicht innerhalb einer Transaktion, deshalb vor den Schritten.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let done: usize = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?.max(0) as usize;
        if done > MIGRATIONS.len() {
            return Err(invalid("Diese Review-Datei stammt aus einer neueren Version der App."));
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(done) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (index + 1) as i64)?;
            tx.commit()?;
        }
        Ok(Store { conn })
    }

    /// Nummer des letzten ausgeführten Schema-Schritts.
    pub fn schema_step(&self) -> Result<i64> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn review_id(&self) -> Result<Uuid> {
        let text = self.meta("review_id")?.ok_or_else(|| invalid("Die Datei enthält kein Review."))?;
        Uuid::parse_str(&text).map_err(|_| invalid("Ungültige Review-ID in der Datei."))
    }

    pub fn head(&self, actor_id: &Uuid, stream: &str) -> Result<Option<Head>> {
        Ok(self
            .conn
            .query_row(
                "SELECT seq, hash FROM event WHERE actor_id = ?1 AND stream = ?2 ORDER BY seq DESC LIMIT 1",
                params![actor_id.to_string(), stream],
                |r| Ok(Head { seq: r.get(0)?, hash: r.get(1)? }),
            )
            .optional()?)
    }

    /// Hängt ein eigenes Ereignis an die passende Kette an und signiert es. Den Payload prüft die
    /// Fachlogik vorher (`Review::append`); hier zählt nur, dass der Typ bekannt ist.
    pub fn append(
        &self,
        me: &Identity,
        event_type: &str,
        stage_instance_id: Option<Uuid>,
        payload: Value,
    ) -> Result<Event> {
        let review_id = self.review_id()?;
        let stream = stream_for(event_type, stage_instance_id.as_ref())?;
        let head = self.head(&me.actor_id, &stream)?;
        // Zeit pro Person nie rückwärts, damit Zeit- und Sequenzordnung übereinstimmen.
        let last: Option<String> = self.conn.query_row(
            "SELECT max(occurred_at) FROM event WHERE actor_id = ?1",
            [me.actor_id.to_string()],
            |r| r.get(0),
        )?;
        let now = now_timestamp();
        let occurred_at = match last {
            Some(last) if last > now => last,
            _ => now,
        };
        let mut event = Event {
            event_id: Uuid::new_v4(),
            review_id,
            actor_id: me.actor_id,
            stream,
            seq: head.as_ref().map_or(1, |h| h.seq + 1),
            event_type: event_type.to_string(),
            schema_version: catalog::spec(event_type).map_or(1, |s| s.version),
            occurred_at,
            stage_instance_id,
            payload,
            prev_hash: head.map(|h| h.hash),
            hash: String::new(),
            signature: String::new(),
        };
        event.hash = event.compute_hash()?;
        event.signature = me.sign_hash(&event.hash)?;
        self.insert(&event)?;
        Ok(event)
    }

    /// Fügt ein bereits geprüftes Ereignis ein (eigenes oder aus einer Austauschdatei).
    pub fn insert(&self, e: &Event) -> Result<()> {
        self.conn.execute(
            "INSERT INTO event (event_id, review_id, actor_id, stream, seq, type, schema_version, occurred_at, \
             stage_instance_id, payload, prev_hash, hash, signature) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                e.event_id.to_string(),
                e.review_id.to_string(),
                e.actor_id.to_string(),
                e.stream,
                e.seq,
                e.event_type,
                e.schema_version,
                e.occurred_at,
                e.stage_instance_id.map(|s| s.to_string()),
                to_canonical(&e.payload)?,
                e.prev_hash,
                e.hash,
                e.signature,
            ],
        )?;
        Ok(())
    }

    pub fn events(&self) -> Result<Vec<Event>> {
        self.query_events("SELECT * FROM event ORDER BY local_seq", [])
    }

    pub fn events_of_types(&self, types: &[&str]) -> Result<Vec<Event>> {
        let placeholders = vec!["?"; types.len()].join(",");
        let sql = format!("SELECT * FROM event WHERE type IN ({placeholders}) ORDER BY local_seq");
        self.query_events(&sql, rusqlite::params_from_iter(types.iter()))
    }

    pub fn stage_events(&self, stage_id: &Uuid) -> Result<Vec<Event>> {
        self.query_events("SELECT * FROM event WHERE stage_instance_id = ?1 ORDER BY local_seq", [stage_id.to_string()])
    }

    pub fn event_by_id(&self, event_id: &Uuid) -> Result<Option<Event>> {
        Ok(self.query_events("SELECT * FROM event WHERE event_id = ?1", [event_id.to_string()])?.pop())
    }

    pub fn event_at(&self, actor_id: &Uuid, stream: &str, seq: i64) -> Result<Option<Event>> {
        Ok(self
            .query_events(
                "SELECT * FROM event WHERE actor_id = ?1 AND stream = ?2 AND seq = ?3",
                params![actor_id.to_string(), stream, seq],
            )?
            .pop())
    }

    fn query_events<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, row_to_event)?;
        let mut out = Vec::new();
        for row in rows {
            let (mut event, payload) = row?;
            event.payload = serde_json::from_str(&payload)?;
            out.push(event);
        }
        Ok(out)
    }

    pub fn person_name(&self, actor_id: &Uuid) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT name FROM person WHERE actor_id = ?1", [actor_id.to_string()], |r| r.get(0))
            .optional()?)
    }

    pub fn set_person(&self, actor_id: &Uuid, name: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO person (actor_id, name) VALUES (?1, ?2) \
             ON CONFLICT (actor_id) DO UPDATE SET name = excluded.name",
            params![actor_id.to_string(), name],
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------- Stammdaten-Zeilen

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchRow {
    pub id: Uuid,
    pub filename: String,
    pub format: String,
    pub source: String,
    pub sha256: String,
    pub status: String,
    pub n_records: i64,
    pub n_duplicates: i64,
    pub error: String,
    pub created_by: Uuid,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordRow {
    pub id: Uuid,
    pub batch_id: Uuid,
    pub title: String,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub authors: String,
    pub year: Option<i64>,
    pub journal: String,
    pub doi: String,
    pub pmid: String,
    pub url: String,
    pub keywords: String,
    pub raw: Value,
    pub dedup_key: String,
    pub duplicate_of: Option<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssignmentRow {
    pub stage_instance_id: Uuid,
    pub record_id: Uuid,
    pub position: i64,
}

impl Store {
    pub fn insert_batch(&self, b: &BatchRow) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO import_batch (id, filename, format, source, sha256, status, n_records, \
             n_duplicates, error, created_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                b.id.to_string(),
                b.filename,
                b.format,
                b.source,
                b.sha256,
                b.status,
                b.n_records,
                b.n_duplicates,
                b.error,
                b.created_by.to_string(),
                b.created_at
            ],
        )?;
        Ok(n > 0)
    }

    pub fn finish_batch(&self, b: &BatchRow) -> Result<()> {
        self.conn.execute(
            "UPDATE import_batch SET status = ?2, n_records = ?3, n_duplicates = ?4, error = ?5 WHERE id = ?1",
            params![b.id.to_string(), b.status, b.n_records, b.n_duplicates, b.error],
        )?;
        Ok(())
    }

    pub fn batches(&self) -> Result<Vec<BatchRow>> {
        let mut stmt = self.conn.prepare("SELECT * FROM import_batch ORDER BY created_at, id")?;
        let rows = stmt.query_map([], |r| {
            Ok(BatchRow {
                id: parse_uuid(r.get("id")?)?,
                filename: r.get("filename")?,
                format: r.get("format")?,
                source: r.get("source")?,
                sha256: r.get("sha256")?,
                status: r.get("status")?,
                n_records: r.get("n_records")?,
                n_duplicates: r.get("n_duplicates")?,
                error: r.get("error")?,
                created_by: parse_uuid(r.get("created_by")?)?,
                created_at: r.get("created_at")?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn insert_record(&self, r: &RecordRow) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO record (id, batch_id, title, abstract, authors, year, journal, doi, pmid, url, \
             keywords, raw, dedup_key, duplicate_of) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                r.id.to_string(),
                r.batch_id.to_string(),
                r.title,
                r.abstract_text,
                r.authors,
                r.year,
                r.journal,
                r.doi,
                r.pmid,
                r.url,
                r.keywords,
                serde_json::to_string(&r.raw)?,
                r.dedup_key,
                r.duplicate_of.map(|d| d.to_string()),
            ],
        )?;
        Ok(n > 0)
    }

    pub fn records_where(&self, clause: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<RecordRow>> {
        let sql = format!("SELECT * FROM record {clause}");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params, |r| {
            let raw: String = r.get("raw")?;
            let dup: Option<String> = r.get("duplicate_of")?;
            Ok(RecordRow {
                id: parse_uuid(r.get("id")?)?,
                batch_id: parse_uuid(r.get("batch_id")?)?,
                title: r.get("title")?,
                abstract_text: r.get("abstract")?,
                authors: r.get("authors")?,
                year: r.get("year")?,
                journal: r.get("journal")?,
                doi: r.get("doi")?,
                pmid: r.get("pmid")?,
                url: r.get("url")?,
                keywords: r.get("keywords")?,
                raw: serde_json::from_str(&raw).unwrap_or(Value::Null),
                dedup_key: r.get("dedup_key")?,
                duplicate_of: dup.map(parse_uuid).transpose()?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn record(&self, id: &Uuid) -> Result<Option<RecordRow>> {
        Ok(self.records_where("WHERE id = ?1", &[&id.to_string()])?.pop())
    }

    pub fn insert_assignment(&self, a: &AssignmentRow) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO stage_assignment (stage_instance_id, record_id, position) VALUES (?1, ?2, ?3)",
            params![a.stage_instance_id.to_string(), a.record_id.to_string(), a.position],
        )?;
        Ok(n > 0)
    }

    pub fn assignments(&self, stage_id: Option<&Uuid>) -> Result<Vec<AssignmentRow>> {
        let (sql, arg) = match stage_id {
            Some(s) => ("SELECT * FROM stage_assignment WHERE stage_instance_id = ?1 ORDER BY position", s.to_string()),
            None => {
                ("SELECT * FROM stage_assignment WHERE ?1 = ?1 ORDER BY stage_instance_id, position", String::new())
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([arg], |r| {
            Ok(AssignmentRow {
                stage_instance_id: parse_uuid(r.get("stage_instance_id")?)?,
                record_id: parse_uuid(r.get("record_id")?)?,
                position: r.get("position")?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn is_assigned(&self, stage_id: &Uuid, record_id: &Uuid) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM stage_assignment WHERE stage_instance_id = ?1 AND record_id = ?2",
                params![stage_id.to_string(), record_id.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Prüfsumme über die Records eines Imports (ohne Rohdaten), steht in `RecordsImported`.
    pub fn records_digest(&self, batch_id: &Uuid) -> Result<String> {
        let rows = self.records_where("WHERE batch_id = ?1 ORDER BY id", &[&batch_id.to_string()])?;
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id.to_string(), "title": r.title, "abstract": r.abstract_text, "authors": r.authors,
                    "year": r.year, "journal": r.journal, "doi": r.doi, "pmid": r.pmid, "url": r.url,
                    "keywords": r.keywords, "dedup_key": r.dedup_key,
                    "duplicate_of": r.duplicate_of.map(|d| d.to_string()),
                })
            })
            .collect();
        Ok(sha256_hex(to_canonical(&Value::Array(list))?.as_bytes()))
    }

    /// Prüfsumme über die Zuweisungen einer Stufe in ihrer Reihenfolge, steht in `StageInstanceOpened`.
    pub fn assignment_digest(&self, stage_id: &Uuid) -> Result<String> {
        let ids: Vec<String> = self.assignments(Some(stage_id))?.iter().map(|a| a.record_id.to_string()).collect();
        Ok(sha256_hex(ids.join("\n").as_bytes()))
    }

    /// Letztes Ereignis jeder Kette, sortiert. Grundlage für Snapshots.
    pub fn heads(&self) -> Result<Vec<(Uuid, String, i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT actor_id, stream, seq, hash FROM event e \
             WHERE seq = (SELECT max(seq) FROM event x WHERE x.actor_id = e.actor_id AND x.stream = e.stream) \
             ORDER BY actor_id, stream",
        )?;
        let rows = stmt.query_map([], |r| Ok((parse_uuid(r.get(0)?)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn parse_uuid(text: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&text)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

fn row_to_event(r: &Row) -> rusqlite::Result<(Event, String)> {
    let stage: Option<String> = r.get("stage_instance_id")?;
    Ok((
        Event {
            event_id: parse_uuid(r.get("event_id")?)?,
            review_id: parse_uuid(r.get("review_id")?)?,
            actor_id: parse_uuid(r.get("actor_id")?)?,
            stream: r.get("stream")?,
            seq: r.get("seq")?,
            event_type: r.get("type")?,
            schema_version: r.get("schema_version")?,
            occurred_at: r.get("occurred_at")?,
            stage_instance_id: stage.map(parse_uuid).transpose()?,
            payload: Value::Null,
            prev_hash: r.get("prev_hash")?,
            hash: r.get("hash")?,
            signature: r.get("signature")?,
        },
        r.get("payload")?,
    ))
}
