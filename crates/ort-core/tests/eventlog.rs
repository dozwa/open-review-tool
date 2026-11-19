//! Event-Log: Ketten, Unveränderlichkeit, Manipulation erkennen.

mod common;

use common::person;
use ort_core::store::Store;
use ort_core::Review;
use serde_json::json;
use uuid::Uuid;

fn store() -> Store {
    let s = Store::in_memory().unwrap();
    s.set_meta("review_id", &Uuid::new_v4().to_string()).unwrap();
    s
}

#[test]
fn append_builds_one_chain_per_person_and_stream() {
    let s = store();
    let (a, b) = (person("a"), person("b"));
    let stage = Uuid::new_v4();
    let e1 = s.append(&a, "ReviewCreated", None, json!({"title": "Kern"})).unwrap();
    let e2 = s.append(&a, "MemberRoleSet", None, json!({"role": "owner"})).unwrap();
    let v1 = s.append(&a, "VoteCast", Some(stage), json!({"record_id": "r", "decision": "include"})).unwrap();
    let w1 = s.append(&b, "VoteCast", Some(stage), json!({"record_id": "r", "decision": "exclude"})).unwrap();
    assert_eq!((e1.seq, e2.seq, v1.seq, w1.seq), (1, 2, 1, 1));
    assert_eq!(e2.prev_hash.as_deref(), Some(e1.hash.as_str()));
    assert_eq!(v1.prev_hash, None, "Voten haben eine eigene Kette je Stufe");
    assert_eq!(v1.stream, format!("stage:{stage}"));
    assert_eq!(s.events().unwrap().len(), 4);
}

#[test]
fn votes_need_a_stage() {
    assert!(store().append(&person("a"), "VoteCast", None, json!({})).is_err());
}

#[test]
fn events_are_immutable() {
    let s = store();
    s.append(&person("a"), "ReviewCreated", None, json!({"title": "x"})).unwrap();
    for sql in ["UPDATE event SET type = 'X'", "DELETE FROM event"] {
        let err = s.conn().execute(sql, []).unwrap_err();
        assert!(err.to_string().contains("append-only"), "{err}");
    }
    assert_eq!(s.events().unwrap().len(), 1);
}

#[test]
fn tampering_is_detected() {
    let review = Review::create_in_memory(person("owner"), "Kern").unwrap();
    for i in 0..3 {
        review.publish_criteria(&json!({"text": format!("v{i}")})).unwrap();
    }
    assert!(review.verify_chain().unwrap().ok);
    let c = review.store().conn();
    c.execute_batch(
        "DROP TRIGGER event_no_update; \
         UPDATE event SET payload = replace(payload, '\"v1\"', '\"v99\"') WHERE type = 'CriteriaVersionPublished'; \
         CREATE TRIGGER event_no_update BEFORE UPDATE ON event BEGIN SELECT RAISE(ABORT, 'event log is append-only'); END;",
    )
    .unwrap();
    let report = review.verify_chain().unwrap();
    assert!(!report.ok);
    assert_eq!(report.n_bad, 1);
    assert!(report.problems[0].contains("Prüfsumme"), "{:?}", report.problems);
}

#[test]
fn review_file_survives_reopen() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("r.sqlite");
    let me = person("owner");
    let id = Review::create(&path, me.clone(), "Datei").unwrap().id().unwrap();
    let again = Review::open(&path, me).unwrap();
    assert_eq!(again.id().unwrap(), id);
    assert_eq!(again.list_item().unwrap().title, "Datei");
    assert!(again.verify_chain().unwrap().ok);
}

#[test]
fn unknown_event_type_cannot_be_appended() {
    let err = store().append(&person("a"), "Erfunden", None, json!({})).unwrap_err();
    assert!(err.to_string().contains("Unbekannter Ereignistyp"), "{err}");
}

#[test]
fn schema_steps_are_recorded_and_newer_files_are_refused() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("r.sqlite");
    let me = person("owner");
    let review = Review::create(&path, me.clone(), "Datei").unwrap();
    let step = review.store().schema_step().unwrap();
    assert!(step >= 1);
    // Eine Datei der ersten App-Version trägt noch Schritt 0 und wird beim Öffnen nachgezogen.
    review.store().conn().pragma_update(None, "user_version", 0).unwrap();
    drop(review);
    let again = Review::open(&path, me.clone()).unwrap();
    assert_eq!(again.store().schema_step().unwrap(), step);
    assert!(again.verify_chain().unwrap().ok);
    // Eine Datei aus einer neueren App wird nicht angefasst.
    again.store().conn().pragma_update(None, "user_version", step + 1).unwrap();
    drop(again);
    let err = Review::open(&path, me).err().unwrap();
    assert!(err.to_string().contains("neueren Version"), "{err}");
}
