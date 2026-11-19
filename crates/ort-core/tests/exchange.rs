//! Austauschdateien: was beim Einlesen abgewiesen werden muss.

mod common;

use common::{person, send_package, wire, Team};
use ort_core::error::Error;
use ort_core::exchange::{
    create_from_package, export_package, export_return, identity_file, merge, read_identity_file, Bundle,
};
use ort_core::Review;
use serde_json::json;

/// Team mit offener, verblindeter Stufe; r1 hat zwei Records bewertet.
fn screened() -> (Team, uuid::Uuid) {
    let t = Team::new();
    let stage = t.open_stage(|_| {});
    send_package(&t.owner, &t.r1);
    for decision in ["include", "exclude"] {
        let record = t.r1.next_record(&stage.id).unwrap().record.unwrap();
        t.r1.cast_vote(&stage.id, &record.id.to_string(), decision, &[], "").unwrap();
    }
    (t, stage.id)
}

fn votes_of(bundle: &Bundle) -> Vec<usize> {
    (0..bundle.events.len()).filter(|&i| bundle.events[i].event_type == "VoteCast").collect()
}

#[test]
fn identity_file_roundtrip_and_rejects_other_files() {
    let me = person("Anna Berger");
    let text = identity_file(&me.public()).unwrap();
    assert_eq!(read_identity_file(&text).unwrap(), me.public());
    assert!(read_identity_file("{\"format\": \"etwas\"}").is_err());
    assert!(Bundle::from_json(&text).is_err(), "Identität ist kein Paket");
}

#[test]
fn return_twice_changes_nothing() {
    let (t, _) = screened();
    let ret = wire(&export_return(&t.r1).unwrap());
    let first = merge(&t.owner, &ret).unwrap();
    assert_eq!(first.accepted, 2);
    let second = merge(&t.owner, &ret).unwrap();
    assert_eq!((second.accepted, second.rejected.len()), (0, 0));
    assert_eq!(second.already_known, ret.events.len());
}

#[test]
fn changed_payload_is_rejected() {
    let (t, _) = screened();
    let mut ret = wire(&export_return(&t.r1).unwrap());
    let i = votes_of(&ret)[0];
    ret.events[i].payload["decision"] = json!("include-geändert");
    let err = merge(&t.owner, &ret).unwrap_err();
    assert!(matches!(err, Error::Integrity(_)), "{err}");
    assert!(err.to_string().contains("Prüfsumme"));
}

#[test]
fn rehashed_but_unsigned_change_is_rejected() {
    let (t, _) = screened();
    let mut ret = wire(&export_return(&t.r1).unwrap());
    let i = votes_of(&ret)[0];
    ret.events[i].payload["decision"] = json!("exclude");
    ret.events[i].hash = ret.events[i].compute_hash().unwrap();
    let err = merge(&t.owner, &ret).unwrap_err();
    assert!(err.to_string().contains("Signatur"), "{err}");
    assert_eq!(t.owner.events().unwrap().iter().filter(|e| e.event_type == "VoteCast").count(), 0);
}

#[test]
fn outsider_is_rejected() {
    let (t, stage) = screened();
    // Ein Außenstehender bekommt eine Kopie des Pakets von r1 und stimmt mit eigenem Schlüssel ab.
    let dir = tempfile::TempDir::new().unwrap();
    let outsider_review = {
        let pkg = wire(&export_package(&t.owner, &t.r1.me().actor_id).unwrap());
        let mut pkg = pkg;
        pkg.recipient = None;
        let path = dir.path().join("outsider.sqlite");
        let err = create_from_package(&path, person("outsider"), &pkg).err().unwrap();
        assert!(err.to_string().contains("nicht als Mitglied"), "{err}");
        assert!(!path.exists(), "abgewiesene Datei wird wieder entfernt");
        // Direkt ins Log schreiben, als hätte jemand die App umgangen:
        let store = ort_core::store::Store::in_memory().unwrap();
        store.set_meta("review_id", &t.owner.id().unwrap().to_string()).unwrap();
        let fake = person("outsider");
        let e = store.append(&fake, "VoteCast", Some(stage), json!({"record_id": "x", "decision": "include"})).unwrap();
        let mut ret = wire(&export_return(&t.r1).unwrap());
        ret.events = vec![e];
        ret
    };
    let err = merge(&t.owner, &outsider_review).unwrap_err();
    assert!(matches!(err, Error::Forbidden(_)), "{err}");
}

#[test]
fn gap_in_chain_is_rejected() {
    let (t, stage) = screened();
    let record = t.r1.next_record(&stage).unwrap().record.unwrap();
    t.r1.cast_vote(&stage, &record.id.to_string(), "maybe", &[], "").unwrap();
    let mut ret = wire(&export_return(&t.r1).unwrap());
    let second_vote = votes_of(&ret)[1];
    ret.events.remove(second_vote);
    let report = merge(&t.owner, &ret).unwrap();
    assert_eq!(report.accepted, 1, "nur das Votum vor der Lücke");
    assert_eq!(report.rejected.len(), 1);
    assert!(report.rejected[0].contains("Lücke"), "{:?}", report.rejected);
    // Mit der vollständigen Rückmeldung kommt der Rest nach.
    let full = merge(&t.owner, &wire(&export_return(&t.r1).unwrap())).unwrap();
    assert_eq!(full.accepted, 2);
}

#[test]
fn package_for_someone_else_is_rejected() {
    let t = Team::new();
    let pkg = wire(&export_package(&t.owner, &t.r2.me().actor_id).unwrap());
    assert!(matches!(merge(&t.r1, &pkg), Err(Error::Forbidden(_))));
}

#[test]
fn changed_record_in_package_is_rejected() {
    let t = Team::new();
    let newcomer = person("neu");
    t.owner.set_member(&newcomer.public(), "reviewer").unwrap();
    let mut pkg = wire(&export_package(&t.owner, &newcomer.actor_id).unwrap());
    pkg.records[0].title = "Untergeschobener Titel".into();
    let dir = tempfile::TempDir::new().unwrap();
    let err = create_from_package(&dir.path().join("neu.sqlite"), newcomer, &pkg).err().unwrap();
    assert!(matches!(err, Error::Integrity(_)), "{err}");
}

#[test]
fn reviewer_cannot_grant_roles_by_editing_the_log() {
    let t = Team::new();
    let friend = person("Freund");
    // r1 umgeht die Rechteprüfung der App und schreibt selbst ein MemberRoleSet.
    t.r1.store()
        .append(
            t.r1.me(),
            "MemberRoleSet",
            None,
            json!({"actor_id": friend.actor_id.to_string(), "role": "admin", "public_key": friend.public_key()}),
        )
        .unwrap();
    let report = merge(&t.owner, &wire(&export_return(&t.r1).unwrap())).unwrap();
    assert_eq!(report.accepted, 0);
    assert!(report.rejected[0].contains("Owner und Admins"), "{:?}", report.rejected);
    assert!(t.owner.state().unwrap().role_of(&friend.actor_id).is_none());
}

#[test]
fn package_into_wrong_review_is_rejected() {
    let t = Team::new();
    let other = Review::create_in_memory(person("x"), "Anderes Review").unwrap();
    let pkg = wire(&export_package(&t.owner, &t.r1.me().actor_id).unwrap());
    assert!(matches!(merge(&other, &pkg), Err(Error::Invalid(_))));
}

#[test]
fn unblinded_package_contains_all_votes() {
    let (t, stage) = screened();
    common::send_return(&t.r1, &t.owner);
    let before = export_package(&t.owner, &t.r2.me().actor_id).unwrap();
    assert_eq!(before.events.iter().filter(|e| e.event_type == "VoteCast").count(), 0);
    t.owner.unblind_stage(&stage).unwrap();
    let after = export_package(&t.owner, &t.r2.me().actor_id).unwrap();
    assert_eq!(after.events.iter().filter(|e| e.event_type == "VoteCast").count(), 2);
}

#[test]
fn invalid_payload_is_rejected_one_by_one() {
    let (t, stage) = screened();
    // r1 umgeht die App und schreibt ein Votum mit unzulässiger Entscheidung und danach ein gültiges.
    let record = t.r1.next_record(&stage).unwrap().record.unwrap().id.to_string();
    t.r1.store().append(t.r1.me(), "VoteCast", Some(stage), json!({"record_id": record, "decision": "ja"})).unwrap();
    t.r1.cast_vote(&stage, &record, "include", &[], "").unwrap();
    let report = merge(&t.owner, &wire(&export_return(&t.r1).unwrap())).unwrap();
    assert_eq!(report.accepted, 2, "die beiden gültigen Voten davor");
    assert_eq!(report.rejected.len(), 1, "{:?}", report.rejected);
    assert!(report.rejected[0].contains("unzulässigen Wert"), "{:?}", report.rejected);
    // Die Kette endet am abgewiesenen Ereignis; das Votum danach kommt nicht an.
    assert_eq!(t.owner.events().unwrap().iter().filter(|e| e.event_type.starts_with("Vote")).count(), 2);
}

#[test]
fn file_from_newer_app_is_rejected_with_a_clear_message() {
    let (t, _) = screened();
    let mut value: serde_json::Value = serde_json::from_str(&export_return(&t.r1).unwrap().to_json().unwrap()).unwrap();
    value["version"] = json!(999);
    let err = Bundle::from_json(&value.to_string()).unwrap_err();
    assert!(err.to_string().contains("neueren Version"), "{err}");
}
