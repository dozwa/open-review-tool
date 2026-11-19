//! Austausch zwischen Personen über Dateien.
//!
//! - **Identitätsdatei** (`.ortid`): ID, Anzeigename und öffentlicher Schlüssel einer Person.
//! - **Paket** (`.ortpkg`, vom Owner oder Admin an eine Person): Records, Importe, Zuweisungen und
//!   die Ereignisse, die diese Person sehen darf. In einer verblindeten Stufe fehlen die Voten und
//!   Markierungen aller anderen Personen vollständig. Die Verblindung hängt also nicht an der
//!   Oberfläche: Was nicht in der Datei steht, kann niemand sehen.
//! - **Rückmeldung** (`.ortvotes`, von einer Person zurück): die eigenen, signierten Ereignisse.
//!
//! Beim Einlesen ([`merge`]) wird alles geprüft: Prüfsumme und Signatur jedes Ereignisses,
//! lückenlose Ketten, Rolle der Person, Stufe offen, Prüfsummen über Records und Zuweisungen.
//! Ist eine Prüfsumme oder Signatur falsch, wird die ganze Datei abgewiesen. Fachlich
//! unzulässige Ereignisse (z. B. Voten nach Abschluss) werden einzeln abgewiesen, mit Grund.
//!
//! Grenze: Die Rollenprüfung nutzt die Rollen nach dem Zusammenführen, nicht den Stand zum
//! Zeitpunkt des Ereignisses.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::catalog::{self, is_blindable, StageRule, Writer};
use crate::criteria::{check_reasons, Criteria};
use crate::error::{conflict, forbidden, integrity, invalid, not_found, Error, Result};
use crate::event::{now_timestamp, parse_public_key, stream_for, Event, Identity, PublicIdentity, MAIN_STREAM};
use crate::forms::{check_value, normal, Forms};
use crate::projection::{order_key, string_list, ReviewState, StageState};
use crate::search::Searches;
use crate::service::{require, stage_criteria, Review, MANAGE_ROLES, ROLES, WRITE_ROLES};
use crate::store::{AssignmentRow, BatchRow, Head, RecordRow, Store};
use crate::tr;

pub const BUNDLE_FORMAT: &str = "ort-bundle";
pub const IDENTITY_FORMAT: &str = "ort-identity";
/// Version 2: Ereignisse werden gegen das Register geprüft, neue Typen kommen dazu. Eine ältere App
/// weist solche Dateien mit einer klaren Meldung ab, statt einzelne Ereignisse zu verwerfen.
pub const FORMAT_VERSION: u32 = 2;
/// Die Identitätsdatei ist unverändert und bleibt für ältere Apps lesbar.
pub const IDENTITY_VERSION: u32 = 1;

// ---------------------------------------------------------------- Identität

#[derive(Serialize, Deserialize)]
struct IdentityFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    person: PublicIdentity,
}

pub fn identity_file(person: &PublicIdentity) -> Result<String> {
    Ok(serde_json::to_string_pretty(&IdentityFile {
        format: IDENTITY_FORMAT.into(),
        version: IDENTITY_VERSION,
        person: person.clone(),
    })?)
}

pub fn read_identity_file(text: &str) -> Result<PublicIdentity> {
    let file: IdentityFile =
        serde_json::from_str(text).map_err(|_| invalid(tr!("Das ist keine Identitätsdatei des Open Review Tool.")))?;
    if file.format != IDENTITY_FORMAT {
        return Err(invalid(tr!("Das ist keine Identitätsdatei des Open Review Tool.")));
    }
    if file.version > IDENTITY_VERSION {
        return Err(invalid(tr!("Die Identitätsdatei stammt aus einer neueren Version der App.")));
    }
    parse_public_key(&file.person.public_key)?;
    Ok(file.person)
}

// ---------------------------------------------------------------- Pakete und Rückmeldungen

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BundleKind {
    Package,
    Return,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PersonRow {
    pub actor_id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bundle {
    pub format: String,
    pub version: u32,
    pub kind: BundleKind,
    pub review_id: Uuid,
    pub title: String,
    pub created_at: String,
    pub created_by: Uuid,
    pub recipient: Option<Uuid>,
    pub people: Vec<PersonRow>,
    pub batches: Vec<BatchRow>,
    pub records: Vec<RecordRow>,
    pub assignments: Vec<AssignmentRow>,
    pub events: Vec<Event>,
}

impl Bundle {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(text: &str) -> Result<Bundle> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|_| invalid(tr!("Die Datei ist kein Paket und keine Rückmeldung.")))?;
        if value.get("format").and_then(|f| f.as_str()) != Some(BUNDLE_FORMAT) {
            return Err(invalid(tr!("Die Datei ist kein Paket und keine Rückmeldung des Open Review Tool.")));
        }
        if value.get("version").and_then(|v| v.as_u64()).unwrap_or(0) > FORMAT_VERSION as u64 {
            return Err(invalid(tr!("Diese Datei stammt aus einer neueren Version der App.")));
        }
        serde_json::from_value(value).map_err(|e| integrity(tr!("Die Datei ist beschädigt: {}", e)))
    }
}

fn people_rows(review: &Review, state: &ReviewState, only: Option<&Uuid>) -> Result<Vec<PersonRow>> {
    let mut out = Vec::new();
    for actor in state.members.keys() {
        if only.is_none_or(|o| o == actor) {
            if let Some(name) = review.store.person_name(actor)? {
                out.push(PersonRow { actor_id: *actor, name });
            }
        }
    }
    Ok(out)
}

/// Paket für eine Person. Bei verblindeten Stufen enthält es nur deren eigene Voten.
pub fn export_package(review: &Review, recipient: &Uuid) -> Result<Bundle> {
    let state = review.state()?;
    require(&state, &review.me.actor_id, &MANAGE_ROLES)?;
    if !state.members.contains_key(recipient) {
        return Err(not_found(tr!("Diese Person ist kein Mitglied des Reviews.")));
    }
    let events = review.store.events()?;
    let mut blinded = BTreeSet::new();
    for stage_id in &state.stage_ids {
        if StageState::build(stage_id, &events).is_some_and(|s| s.blind_active()) {
            blinded.insert(*stage_id);
        }
    }
    let visible: Vec<Event> = events
        .into_iter()
        .filter(|e| {
            let hidden = is_blindable(&e.event_type)
                && e.stage_instance_id.is_some_and(|s| blinded.contains(&s))
                && e.actor_id != *recipient;
            !hidden
        })
        .collect();
    let mut records = review.store.records_where("ORDER BY id", &[])?;
    // Originale vor Dubletten, damit die Verweise beim Einlesen schon existieren.
    records.sort_by_key(|r| r.duplicate_of.is_some());
    Ok(Bundle {
        format: BUNDLE_FORMAT.into(),
        version: FORMAT_VERSION,
        kind: BundleKind::Package,
        review_id: review.id()?,
        title: state.title.clone(),
        created_at: now_timestamp(),
        created_by: review.me.actor_id,
        recipient: Some(*recipient),
        people: people_rows(review, &state, None)?,
        batches: review.store.batches()?.into_iter().filter(|b| b.status == "done").collect(),
        records,
        assignments: review.store.assignments(None)?,
        events: visible,
    })
}

/// Rückmeldung: alle eigenen Ereignisse. Doppelt eingelesen ändert sie nichts.
pub fn export_return(review: &Review) -> Result<Bundle> {
    let state = review.state()?;
    require(&state, &review.me.actor_id, &ROLES)?;
    let me = review.me.actor_id;
    let events: Vec<Event> = review.store.events()?.into_iter().filter(|e| e.actor_id == me).collect();
    Ok(Bundle {
        format: BUNDLE_FORMAT.into(),
        version: FORMAT_VERSION,
        kind: BundleKind::Return,
        review_id: review.id()?,
        title: state.title.clone(),
        created_at: now_timestamp(),
        created_by: me,
        recipient: None,
        people: people_rows(review, &state, Some(&me))?,
        batches: Vec::new(),
        records: Vec::new(),
        assignments: Vec::new(),
        events,
    })
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MergeReport {
    pub accepted: usize,
    pub already_known: usize,
    pub rejected: Vec<String>,
    pub records_added: usize,
}

/// Legt aus einem Paket ein neues Review an (erstes Einlesen bei einer Person).
pub fn create_from_package(path: &Path, me: Identity, bundle: &Bundle) -> Result<(Review, MergeReport)> {
    if bundle.kind != BundleKind::Package {
        return Err(invalid(tr!(
            "Ein neues Review lässt sich nur aus einem Paket anlegen, nicht aus einer Rückmeldung."
        )));
    }
    if path.exists() {
        return Err(conflict(tr!("Dieses Review gibt es auf diesem Rechner schon.")));
    }
    let result = (|| {
        let store = Store::open(path)?;
        store.set_meta("review_id", &bundle.review_id.to_string())?;
        let review = Review { store, me };
        let report = merge(&review, bundle)?;
        if review.state()?.role_of(&review.me.actor_id).is_none() {
            return Err(forbidden(tr!("Du bist in diesem Paket nicht als Mitglied eingetragen.")));
        }
        Ok((review, report))
    })();
    if result.is_err() {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
    result
}

/// Schlüssel und Rollen aus `ReviewCreated` und `MemberRoleSet`, in zeitlicher Ordnung angewandt.
/// Ein `MemberRoleSet` zählt nur, wenn es von einem Owner oder Admin stammt und gültig signiert ist.
fn keys_and_roles(events: &[&Event]) -> Result<(HashMap<Uuid, VerifyingKey>, HashMap<Uuid, String>)> {
    let mut list: Vec<&Event> =
        events.iter().copied().filter(|e| matches!(e.event_type.as_str(), "ReviewCreated" | "MemberRoleSet")).collect();
    list.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
    let mut seen = BTreeSet::new();
    let mut keys = HashMap::new();
    let mut roles = HashMap::new();
    let mut created = false;
    for e in list {
        if !seen.insert(e.event_id) {
            continue;
        }
        match e.event_type.as_str() {
            "ReviewCreated" => {
                if created {
                    continue;
                }
                let owner = e.payload_str("owner").and_then(|s| Uuid::parse_str(s).ok());
                if owner != Some(e.actor_id) || e.stream != MAIN_STREAM || e.seq != 1 {
                    return Err(integrity(tr!("Das Ereignis ReviewCreated ist ungültig.")));
                }
                let key = parse_public_key(e.payload_str("public_key").unwrap_or_default())?;
                e.verify(&key)?;
                keys.insert(e.actor_id, key);
                roles.insert(e.actor_id, "owner".to_string());
                created = true;
            }
            _ => {
                let Some(author_key) = keys.get(&e.actor_id) else { continue };
                if !roles.get(&e.actor_id).is_some_and(|r| MANAGE_ROLES.contains(&r.as_str())) {
                    continue;
                }
                e.verify(author_key)?;
                let target = e.payload_str("actor_id").and_then(|s| Uuid::parse_str(s).ok());
                let role = e.payload_str("role").filter(|r| ROLES.contains(r));
                let key = e.payload_str("public_key").map(parse_public_key).transpose()?;
                if let (Some(target), Some(role)) = (target, role) {
                    if let Some(key) = key {
                        keys.insert(target, key);
                    }
                    roles.insert(target, role.to_string());
                }
            }
        }
    }
    Ok((keys, roles))
}

/// Liest ein Paket oder eine Rückmeldung in ein bestehendes Review ein.
pub fn merge(review: &Review, bundle: &Bundle) -> Result<MergeReport> {
    let store = &review.store;
    let review_id = store.review_id()?;
    if bundle.review_id != review_id {
        return Err(invalid(tr!("Die Datei gehört zu einem anderen Review.")));
    }
    if bundle.kind == BundleKind::Package && bundle.recipient.is_some_and(|r| r != review.me.actor_id) {
        return Err(forbidden(tr!("Dieses Paket ist für eine andere Person bestimmt.")));
    }

    // 1. Prüfsummen aller Ereignisse
    for e in &bundle.events {
        if e.review_id != review_id {
            return Err(integrity(tr!("Die Datei enthält Ereignisse eines anderen Reviews.")));
        }
        if e.compute_hash()? != e.hash {
            return Err(integrity(tr!(
                "Die Datei ist beschädigt oder wurde verändert: Prüfsumme von Ereignis {} ({}) stimmt nicht.",
                e.event_id,
                e.event_type
            )));
        }
    }

    // 2. Schlüssel und Rollen aus lokalem und eingehendem Log
    let local = store.events()?;
    let all: Vec<&Event> = local.iter().chain(bundle.events.iter()).collect();
    let (keys, roles) = keys_and_roles(&all)?;

    // 3. Signaturen
    for e in &bundle.events {
        let key = keys.get(&e.actor_id).ok_or_else(|| {
            forbidden(tr!(
                "Die Datei enthält Ereignisse einer Person, die in diesem Review nicht Mitglied ist ({}).",
                &e.actor_id.to_string()[..8]
            ))
        })?;
        e.verify(key)?;
    }

    // Stand vor dem Einlesen und Stufen nach dem Einlesen
    let local_state = ReviewState::build(&local);
    let closed_before: BTreeSet<Uuid> = local_state
        .stage_ids
        .iter()
        .filter(|id| StageState::build(id, &local).is_some_and(|s| s.closed))
        .copied()
        .collect();
    let all_owned: Vec<Event> = all.iter().map(|e| (*e).clone()).collect();
    let merged_state = ReviewState::build(&all_owned);
    let merged_stages: Vec<StageState> =
        merged_state.stage_ids.iter().filter_map(|id| StageState::build(id, &all_owned)).collect();
    let context = MergeContext {
        roles: &roles,
        has_created: local.iter().any(|e| e.event_type == "ReviewCreated"),
        known_stages: merged_state.stage_ids.iter().copied().collect(),
        closed_before,
        blind_after: merged_stages.iter().filter(|s| s.blind_active()).map(|s| s.config.id).collect(),
        // Bei Dritt-Entscheid: wer zu welchem Eintrag selbst abgestimmt hat
        own_votes: merged_stages
            .iter()
            .filter(|s| s.config.conflict_rule == "third_person")
            .flat_map(|s| s.votes.keys().map(|(record, actor)| (s.config.id, record.clone(), *actor)))
            .collect(),
        reason_rules: merged_stages
            .iter()
            .filter(|s| s.config.reason_required)
            .map(|s| (s.config.id, stage_criteria(&merged_state, s)))
            .collect(),
        searches: Searches::build(&all_owned).known(),
        form_stages: merged_stages.iter().filter_map(|s| s.config.form_id.map(|form| (s.config.id, form))).collect(),
        forms: Forms::build(&all_owned),
    };

    let mut report = MergeReport::default();
    let tx = store.conn().unchecked_transaction()?;

    // 4. Ketten einzeln, Verwaltung zuerst
    let mut chains: BTreeMap<(bool, Uuid, String), Vec<&Event>> = BTreeMap::new();
    for e in &bundle.events {
        chains.entry((e.stream != MAIN_STREAM, e.actor_id, e.stream.clone())).or_default().push(e);
    }
    for ((_, actor, stream), mut chain) in chains {
        chain.sort_by_key(|e| e.seq);
        let who = store.person_name(&actor).ok().flatten().unwrap_or_else(|| {
            bundle
                .people
                .iter()
                .find(|p| p.actor_id == actor)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| actor.to_string()[..8].to_string())
        });
        let mut head: Option<Head> = store.head(&actor, &stream)?;
        let mut stopped = false;
        for e in chain {
            if stopped {
                continue;
            }
            if let Some(existing) = store.event_by_id(&e.event_id)? {
                if existing.hash != e.hash {
                    return Err(integrity(tr!(
                        "Ereignis {} liegt hier schon in einer anderen Fassung vor.",
                        e.event_id
                    )));
                }
                report.already_known += 1;
                continue;
            }
            let reject = |reason: String, report: &mut MergeReport| {
                report.rejected.push(tr!("{}, {} Nr. {}: {}", who, e.event_type, e.seq, reason));
            };
            if store.event_at(&actor, &stream, e.seq)?.is_some() {
                reject(tr!("An dieser Stelle der Kette steht hier schon ein anderes Ereignis. Die Kette wurde auf zwei Rechnern getrennt fortgeschrieben."), &mut report);
                stopped = true;
                continue;
            }
            let expected = head.as_ref().map_or(1, |h| h.seq + 1);
            if e.seq != expected || e.prev_hash != head.as_ref().map(|h| h.hash.clone()) {
                reject(
                    tr!(
                        "Lücke in der Kette (erwartet Nr. {}). Bitte eine vollständige Rückmeldung anfordern.",
                        expected
                    ),
                    &mut report,
                );
                stopped = true;
                continue;
            }
            if let Err(reason) = authorize(e, &context) {
                reject(reason, &mut report);
                stopped = true;
                continue;
            }
            store.insert(e)?;
            head = Some(Head { seq: e.seq, hash: e.hash.clone() });
            report.accepted += 1;
        }
    }

    // 5. Stammdaten
    for p in &bundle.people {
        if roles.contains_key(&p.actor_id) && !p.name.trim().is_empty() {
            store.set_person(&p.actor_id, p.name.trim())?;
        }
    }
    for b in &bundle.batches {
        store.insert_batch(b)?;
    }
    let mut touched_batches = BTreeSet::new();
    let mut records: Vec<&RecordRow> = bundle.records.iter().collect();
    records.sort_by_key(|r| r.duplicate_of.is_some());
    for r in records {
        if store.insert_record(r)? {
            report.records_added += 1;
            touched_batches.insert(r.batch_id);
        }
    }
    let mut touched_stages = BTreeSet::new();
    for a in &bundle.assignments {
        if store.insert_assignment(a)? {
            touched_stages.insert(a.stage_instance_id);
        }
    }

    // 6. Prüfsummen über Records und Zuweisungen gegen die signierten Ereignisse
    let after = store.events_of_types(&["RecordsImported", "StageInstanceOpened"])?;
    for batch in touched_batches {
        let event = after
            .iter()
            .find(|e| e.event_type == "RecordsImported" && e.payload_str("batch_id") == Some(&batch.to_string()));
        let expected = event.and_then(|e| e.payload_str("records_sha256"));
        if expected != Some(store.records_digest(&batch)?.as_str()) {
            return Err(integrity(tr!(
                "Die Records in der Datei passen nicht zur signierten Prüfsumme ihres Imports."
            )));
        }
    }
    for stage in touched_stages {
        let event = after.iter().find(|e| e.event_type == "StageInstanceOpened" && e.stage_instance_id == Some(stage));
        let expected = event.and_then(|e| e.payload_str("record_ids_sha256"));
        if expected != Some(store.assignment_digest(&stage)?.as_str()) {
            return Err(integrity(tr!("Die Zuweisungen einer Stufe passen nicht zur signierten Prüfsumme der Stufe.")));
        }
    }

    tx.commit()?;
    Ok(report)
}

/// Was beim Einlesen über das Review bekannt sein muss, um ein einzelnes Ereignis zu prüfen.
struct MergeContext<'a> {
    roles: &'a HashMap<Uuid, String>,
    has_created: bool,
    known_stages: BTreeSet<Uuid>,
    closed_before: BTreeSet<Uuid>,
    blind_after: BTreeSet<Uuid>,
    /// (Stufe, Eintrag, Person) für Stufen mit Dritt-Entscheid.
    own_votes: BTreeSet<(Uuid, String, Uuid)>,
    /// Kriterien der Stufen, in denen jeder Ausschluss einen Hauptgrund braucht.
    reason_rules: BTreeMap<Uuid, Criteria>,
    /// Bekannte Suchversionen nach dem Einlesen.
    searches: BTreeSet<(Uuid, i64)>,
    /// Formular-Stufen mit ihrem Formular.
    form_stages: BTreeMap<Uuid, Uuid>,
    forms: Forms,
}

/// Prüft ein einzelnes fremdes Ereignis gegen das Register: Rolle, Kette, Stufe und Payload.
fn authorize(e: &Event, context: &MergeContext) -> std::result::Result<(), String> {
    let MergeContext {
        roles,
        has_created,
        known_stages,
        closed_before,
        blind_after,
        own_votes,
        reason_rules,
        searches,
        form_stages,
        forms,
    } = context;
    let has_created = *has_created;
    let role = roles.get(&e.actor_id).map(String::as_str).unwrap_or("");
    let t = e.event_type.as_str();
    let spec = catalog::spec(t).ok_or_else(|| tr!("Unbekannter Ereignistyp {}.", t))?;
    match spec.writer {
        Writer::Creator if has_created => return Err(tr!("Das Review ist hier schon angelegt.")),
        Writer::Creator => {}
        Writer::Manage if !MANAGE_ROLES.contains(&role) => return Err(tr!("Nur Owner und Admins dürfen das.")),
        Writer::Manage => {}
        Writer::Write if !WRITE_ROLES.contains(&role) => return Err(tr!("Diese Person darf nicht abstimmen.")),
        Writer::Write => {}
    }
    catalog::validate_event(e)?;
    if stream_for(t, e.stage_instance_id.as_ref()).ok().as_deref() != Some(e.stream.as_str()) {
        return Err(tr!("Das Ereignis liegt in der falschen Kette."));
    }
    // Ein Import oder ein Peer-Review nennt eine Suchversion, die es geben muss.
    let search_version = match t {
        "RecordsImported" => e.payload.get("search_version"),
        "SearchPeerReviewed" => e.payload.get("version"),
        _ => None,
    };
    if let Some(id) = e.payload_str("search_id").filter(|_| search_version.is_some()) {
        let known = Uuid::parse_str(id).ok().zip(search_version.and_then(|v| v.as_i64()));
        if !known.is_some_and(|key| searches.contains(&key)) {
            return Err(tr!("Die Suchversion ist hier nicht bekannt."));
        }
    }
    if t == "FormConfirmed" {
        let id = e.payload_str("form_id").and_then(|id| Uuid::parse_str(id).ok());
        let version = e.payload.get("version").and_then(|v| v.as_i64());
        let known =
            id.and_then(|id| forms.get(&id)).is_some_and(|f| f.versions.iter().any(|v| Some(v.version) == version));
        if !known {
            return Err(tr!("Die Formularversion ist hier nicht bekannt."));
        }
    }
    if matches!(spec.stage, StageRule::Open | StageRule::OpenUnblinded) {
        let stage = e.stage_instance_id.ok_or_else(|| tr!("Stufe fehlt."))?;
        if !known_stages.contains(&stage) {
            return Err(tr!("Die Stufe ist hier nicht bekannt."));
        }
        if closed_before.contains(&stage) {
            return Err(tr!("Die Stufe war bereits geschlossen."));
        }
        if spec.stage == StageRule::OpenUnblinded && blind_after.contains(&stage) {
            return Err(tr!("Konsens in einer verblindeten Stufe."));
        }
        // Stimmen gehören in Screening-Stufen, Formularwerte in Formular-Stufen.
        match (t.starts_with("Form"), form_stages.get(&stage)) {
            (true, None) => return Err(tr!("Die Stufe arbeitet nicht mit einem Formular.")),
            (false, Some(_)) => return Err(tr!("Die Stufe arbeitet mit einem Formular, nicht mit Stimmen.")),
            (true, Some(form)) if t != "FormEntryCompleted" => {
                let version = e.payload.get("form_version").and_then(|v| v.as_i64()).unwrap_or(0);
                let field = forms.field_in(form, version, e.payload_str("field").unwrap_or_default());
                let field = field.ok_or_else(|| tr!("Das Feld gibt es in dieser Version des Formulars nicht."))?;
                check_value(field, &normal(e.payload.get("value").unwrap_or(&serde_json::Value::Null)))?;
            }
            _ => {}
        }
        let record = e.payload_str("record_id").unwrap_or_default().to_string();
        if t == "ConsensusRecorded" && own_votes.contains(&(stage, record, e.actor_id)) {
            return Err(tr!("Dritt-Entscheid: Diese Person hat zu dem Eintrag selbst abgestimmt."));
        }
        if let (Some(criteria), Some(decision)) = (reason_rules.get(&stage), e.payload_str("decision")) {
            let reasons = string_list(e.payload.get("reasons"));
            check_reasons(criteria, true, decision, e.payload_str("primary_reason"), &reasons)?;
        }
    }
    Ok(())
}

/// Liest eine Datei und entscheidet, ob sie ein neues Review anlegt oder in ein bestehendes geht.
pub fn read_bundle_file(path: &Path) -> Result<Bundle> {
    let text = std::fs::read_to_string(path).map_err(Error::Io)?;
    Bundle::from_json(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::stage_stream;
    use serde_json::json;

    fn vote(stage: Uuid, actor: Uuid) -> Event {
        Event {
            event_id: Uuid::new_v4(),
            review_id: Uuid::new_v4(),
            actor_id: actor,
            stream: stage_stream(&stage),
            seq: 1,
            event_type: "VoteCast".into(),
            schema_version: 1,
            occurred_at: now_timestamp(),
            stage_instance_id: Some(stage),
            payload: json!({"record_id": Uuid::new_v4().to_string(), "decision": "include"}),
            prev_hash: None,
            hash: String::new(),
            signature: String::new(),
        }
    }

    #[test]
    fn authorize_follows_the_catalog() {
        let (stage, actor) = (Uuid::new_v4(), Uuid::new_v4());
        let roles = HashMap::from([(actor, "reviewer".to_string())]);
        let known = BTreeSet::from([stage]);
        let none = BTreeSet::new();
        let context =
            |roles: &'static HashMap<Uuid, String>, closed: &BTreeSet<Uuid>, blind: &BTreeSet<Uuid>| MergeContext {
                roles,
                has_created: true,
                known_stages: known.clone(),
                closed_before: closed.clone(),
                blind_after: blind.clone(),
                own_votes: BTreeSet::new(),
                reason_rules: BTreeMap::new(),
                searches: BTreeSet::new(),
                form_stages: BTreeMap::new(),
                forms: Forms::default(),
            };
        let roles: &'static HashMap<Uuid, String> = Box::leak(Box::new(roles));
        let check =
            |e: &Event, closed: &BTreeSet<Uuid>, blind: &BTreeSet<Uuid>| authorize(e, &context(roles, closed, blind));
        let good = vote(stage, actor);
        assert_eq!(check(&good, &none, &none), Ok(()));
        assert!(check(&good, &known, &none).unwrap_err().contains("geschlossen"));
        // Votum in der Verwaltungskette: wäre für die Verblindung unsichtbar
        let wrong_chain = Event { stream: MAIN_STREAM.into(), ..good.clone() };
        assert!(check(&wrong_chain, &none, &none).unwrap_err().contains("falschen Kette"));
        let newer = Event { schema_version: 2, ..good.clone() };
        assert!(check(&newer, &none, &none).unwrap_err().contains("neueren Version"));
        let unknown_stage = vote(Uuid::new_v4(), actor);
        assert!(check(&unknown_stage, &none, &none).unwrap_err().contains("nicht bekannt"));
        // Verwaltung bleibt Owner und Admins vorbehalten, Konsens gibt es erst nach dem Entblinden
        let opened = Event { event_type: "StageUnblinded".into(), stream: MAIN_STREAM.into(), ..good.clone() };
        assert!(check(&opened, &none, &none).unwrap_err().contains("Owner und Admins"));
        let consensus = Event { event_type: "ConsensusRecorded".into(), stream: MAIN_STREAM.into(), ..good.clone() };
        assert_eq!(check(&consensus, &none, &none), Ok(()));
        assert!(check(&consensus, &none, &known).unwrap_err().contains("verblindeten"));
        // Dritt-Entscheid: wer selbst abgestimmt hat, hält keinen Konsens fest
        let record = consensus.payload_str("record_id").unwrap().to_string();
        let mut third = context(roles, &none, &none);
        third.own_votes.insert((stage, record, actor));
        assert!(authorize(&consensus, &third).unwrap_err().contains("Dritt-Entscheid"));
        let viewer: &'static HashMap<Uuid, String> =
            Box::leak(Box::new(HashMap::from([(actor, "viewer".to_string())])));
        assert!(authorize(&good, &context(viewer, &none, &none)).unwrap_err().contains("nicht abstimmen"));
    }
}
