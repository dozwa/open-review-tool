//! Markierungen und Kommentare im Volltext.
//!
//! Die Datei des Volltexts bleibt, wie sie ist: Eine Markierung ist ein Ereignis in der Kette der Stufe,
//! in der sie gesetzt wurde (`FullTextHighlightAdded`, `FullTextHighlightRemoved`), und damit verblindbar
//! wie eine Stimme. Sie nennt Seite, Rechtecke in Zehntausendsteln der Seite, den markierten Text, eine
//! Farbe und einen Kommentar, dazu die Prüfsumme der Datei. Wird dem Eintrag später eine andere Datei
//! zugeordnet, gilt die Markierung als veraltet (`stale`) und wird nicht mehr eingezeichnet.
//!
//! Setzen und ändern darf, wer Schreibrecht hat, solange die Stufe offen ist, und nur die eigenen.
//! Zu sehen sind die eigenen immer, die der anderen erst, wenn die Stufe nicht mehr verblindet ist.

use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::catalog::{HIGHLIGHT_COLORS, HIGHLIGHT_UNITS, MAX_HIGHLIGHT_RECTS, MAX_HIGHLIGHT_TEXT};
use crate::error::{conflict, invalid, not_found, Result};
use crate::projection::{HighlightInfo, StageState};
use crate::service::{require, Review, ROLES, WRITE_ROLES};
use crate::tr;

/// Eine neue Markierung, wie die Oberfläche sie meldet.
#[derive(Clone, Debug, Deserialize)]
pub struct NewHighlight {
    pub page: i64,
    /// Rechtecke `[x, y, Breite, Höhe]` in Zehntausendsteln der Seite, von links oben.
    pub rects: Vec<[i64; 4]>,
    pub text: String,
    pub color: String,
    #[serde(default)]
    pub note: String,
}

/// Eine Markierung für die Anzeige.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Highlight {
    pub id: Uuid,
    pub stage_id: Uuid,
    pub stage_name: String,
    pub actor_id: Uuid,
    pub name: String,
    pub mine: bool,
    /// Die eigene Markierung in einer offenen Stufe.
    pub editable: bool,
    /// Sie gehört zu einer Datei, die dem Eintrag nicht mehr zugeordnet ist.
    pub stale: bool,
    #[serde(flatten)]
    pub info: HighlightInfo,
}

fn check_text(text: &str) -> Result<()> {
    if text.chars().count() > MAX_HIGHLIGHT_TEXT {
        return Err(invalid(tr!("Der Text ist länger als {} Zeichen.", MAX_HIGHLIGHT_TEXT)));
    }
    Ok(())
}

fn check_color(color: &str) -> Result<()> {
    if !HIGHLIGHT_COLORS.contains(&color) {
        return Err(invalid(tr!("Diese Farbe gibt es für Markierungen nicht.")));
    }
    Ok(())
}

impl Review {
    /// Prüfsumme des beschafften Volltexts; ohne Volltext gibt es nichts zu markieren.
    fn highlight_file(&self, record_id: &Uuid) -> Result<String> {
        self.fulltext_of(record_id)?
            .filter(|f| f.status == "retrieved")
            .map(|f| f.sha256)
            .ok_or_else(|| not_found(tr!("Zu diesem Eintrag ist kein Volltext beschafft.")))
    }

    /// Die offene Stufe, in der die Person markieren darf.
    fn highlight_stage(&self, stage_id: &Uuid) -> Result<StageState> {
        require(&self.state()?, &self.me().actor_id, &WRITE_ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        Ok(stage)
    }

    fn append_highlight(
        &self,
        stage: &StageState,
        record_id: &Uuid,
        id: Uuid,
        info: &HighlightInfo,
    ) -> Result<Highlight> {
        let payload = json!({
            "record_id": record_id.to_string(),
            "highlight": id.to_string(),
            "sha256": info.sha256,
            "page": info.page,
            "rects": info.rects,
            "text": info.text,
            "color": info.color,
            "note": info.note,
        });
        let event = self.append("FullTextHighlightAdded", Some(stage.config.id), payload)?;
        let at = if info.at.is_empty() { event.occurred_at } else { info.at.clone() };
        Ok(Highlight {
            id,
            stage_id: stage.config.id,
            stage_name: stage.config.name.clone(),
            actor_id: self.me().actor_id,
            name: self.name_of(&self.me().actor_id)?,
            mine: true,
            editable: true,
            stale: false,
            info: HighlightInfo { at, ..info.clone() },
        })
    }

    /// Setzt eine Markierung im Volltext eines Eintrags. Sie gehört zur Stufe, in der gerade gelesen wird.
    pub fn add_highlight(&self, stage_id: &Uuid, record_id: &Uuid, new: &NewHighlight) -> Result<Highlight> {
        let stage = self.highlight_stage(stage_id)?;
        let sha256 = self.highlight_file(record_id)?;
        check_color(&new.color)?;
        check_text(&new.note)?;
        let inside = |r: &[i64; 4]| {
            r[0] >= 0
                && r[1] >= 0
                && r[2] >= 1
                && r[3] >= 1
                && r[0] + r[2] <= HIGHLIGHT_UNITS
                && r[1] + r[3] <= HIGHLIGHT_UNITS
        };
        if new.page < 1
            || new.rects.is_empty()
            || new.rects.len() > MAX_HIGHLIGHT_RECTS
            || !new.rects.iter().all(inside)
        {
            return Err(invalid(tr!("Die Markierung liegt nicht auf der Seite.")));
        }
        let info = HighlightInfo {
            sha256,
            page: new.page,
            rects: new.rects.clone(),
            // Ein sehr langer markierter Text wird gekürzt; die Rechtecke zeigen weiter die ganze Stelle.
            text: new.text.trim().chars().take(MAX_HIGHLIGHT_TEXT).collect(),
            color: new.color.clone(),
            note: new.note.trim().to_string(),
            at: String::new(),
        };
        self.append_highlight(&stage, record_id, Uuid::new_v4(), &info)
    }

    /// Ändert Farbe und Kommentar einer eigenen Markierung.
    pub fn update_highlight(
        &self,
        stage_id: &Uuid,
        record_id: &Uuid,
        highlight: &Uuid,
        color: &str,
        note: &str,
    ) -> Result<Highlight> {
        let stage = self.highlight_stage(stage_id)?;
        check_color(color)?;
        check_text(note)?;
        let old = stage
            .highlights
            .get(&(record_id.to_string(), self.me().actor_id, *highlight))
            .ok_or_else(|| not_found(tr!("Markierung nicht gefunden.")))?;
        let info = HighlightInfo { color: color.to_string(), note: note.trim().to_string(), ..old.clone() };
        let mut changed = self.append_highlight(&stage, record_id, *highlight, &info)?;
        changed.stale = self.highlight_file(record_id).ok().as_deref() != Some(info.sha256.as_str());
        Ok(changed)
    }

    /// Entfernt eine eigene Markierung.
    pub fn remove_highlight(&self, stage_id: &Uuid, record_id: &Uuid, highlight: &Uuid) -> Result<()> {
        let stage = self.highlight_stage(stage_id)?;
        if !stage.highlights.contains_key(&(record_id.to_string(), self.me().actor_id, *highlight)) {
            return Err(not_found(tr!("Markierung nicht gefunden.")));
        }
        let payload = json!({"record_id": record_id.to_string(), "highlight": highlight.to_string()});
        self.append("FullTextHighlightRemoved", Some(stage.config.id), payload)?;
        Ok(())
    }

    /// Alle Markierungen zum Volltext eines Eintrags, die die Person sehen darf, aus allen Stufen: die
    /// eigenen immer, die der anderen nur aus Stufen, die nicht verblindet sind. Geordnet nach Seite
    /// und Lage auf der Seite.
    pub fn highlights(&self, record_id: &Uuid) -> Result<Vec<Highlight>> {
        let state = self.state()?;
        require(&state, &self.me().actor_id, &ROLES)?;
        let me = self.me().actor_id;
        let can_write = state.members.get(&me).is_some_and(|m| WRITE_ROLES.contains(&m.role.as_str()));
        let file = self.highlight_file(record_id).ok();
        let record = record_id.to_string();
        let mut out = Vec::new();
        for stage in self.stage_states(&state)? {
            let blind = stage.blind_active();
            let range = (record.clone(), Uuid::nil(), Uuid::nil())..=(record.clone(), Uuid::max(), Uuid::max());
            for ((_, actor, id), info) in stage.highlights.range(range) {
                if blind && *actor != me {
                    continue;
                }
                out.push(Highlight {
                    id: *id,
                    stage_id: stage.config.id,
                    stage_name: stage.config.name.clone(),
                    actor_id: *actor,
                    name: self.name_of(actor)?,
                    mine: *actor == me,
                    editable: *actor == me && can_write && !stage.closed,
                    stale: file.as_deref() != Some(info.sha256.as_str()),
                    info: info.clone(),
                });
            }
        }
        let top = |h: &Highlight| h.info.rects.first().map_or((0, 0), |r| (r[1], r[0]));
        out.sort_by(|a, b| (a.info.page, top(a), &a.info.at).cmp(&(b.info.page, top(b), &b.info.at)));
        Ok(out)
    }
}
