//! Austauschdateien: was beim Einlesen abgewiesen werden muss.

mod common;

use common::{person, search, send_package, send_return, wire, Team, SECOND_SOURCE};
use ort_core::error::Error;
use ort_core::exchange::{
    create_from_package, export_package, export_return, identity_file, merge, read_identity_file, Bundle,
};
use ort_core::fulltext::ZoteroLink;
use ort_core::Review;
use serde_json::json;

/// Team mit offener, verblindeter Stufe; r1 hat zwei Records bewertet.
fn screened() -> (Team, uuid::Uuid) {
    let t = Team::new();
    let stage = t.open_stage(|_| {});
    send_package(&t.owner, &t.r1);
    for decision in ["include", "exclude"] {
        let record = t.r1.next_record(&stage.id).unwrap().record.unwrap();
        t.r1.cast_vote(&stage.id, &record.id.to_string(), decision, None, &[], "").unwrap();
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
    t.r1.cast_vote(&stage, &record.id.to_string(), "maybe", None, &[], "").unwrap();
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
    t.r1.cast_vote(&stage, &record, "include", None, &[], "").unwrap();
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

/// Kopie der Datei einer Person, in der sie an der App vorbei Ereignisse anhängt.
fn copy_of(t: &Team, member: &Review) -> (tempfile::TempDir, Review) {
    let dir = tempfile::TempDir::new().unwrap();
    let pkg = wire(&export_package(&t.owner, &member.me().actor_id).unwrap());
    let copy = create_from_package(&dir.path().join("kopie.sqlite"), member.me().clone(), &pkg).unwrap().0;
    (dir, copy)
}

#[test]
fn searches_and_duplicate_decisions_need_the_lead() {
    let t = Team::new();
    let made = t.owner.publish_search(&search("MEDLINE", json!({}))).unwrap();
    let records = t.owner.records(None, false).unwrap();
    let (a, b) = (records[0].id.to_string(), records[1].id.to_string());
    assert!(matches!(t.r1.publish_search(&search("Embase", json!({}))), Err(Error::Forbidden(_))));
    assert!(matches!(t.r1.review_search(&made.id, 1, "2026-11-20", "geprüft", None), Err(Error::Forbidden(_))));

    // r1 umgeht die App: eigene Suche, eigener Prüfvermerk, eigene Dublettenentscheidung.
    let forged = [
        (
            "SearchVersionPublished",
            search("Embase", json!({"search_id": uuid::Uuid::new_v4().to_string(), "version": 1})),
        ),
        (
            "SearchPeerReviewed",
            json!({"search_id": made.id.to_string(), "version": 1, "reviewed_on": "2026-11-20", "note": "passt"}),
        ),
        ("DuplicatesResolved", json!({"pairs": [{"a": a, "b": b, "decision": "duplicate", "keep": a}]})),
    ];
    for (event_type, payload) in forged {
        let (_dir, copy) = copy_of(&t, &t.r1);
        copy.store().append(copy.me(), event_type, None, payload).unwrap();
        let report = merge(&t.owner, &wire(&export_return(&copy).unwrap())).unwrap();
        assert_eq!(report.accepted, 0, "{event_type}");
        assert!(report.rejected[0].contains("Owner und Admins"), "{event_type}: {:?}", report.rejected);
    }
    assert_eq!(t.owner.searches().unwrap().len(), 1);
    assert_eq!(t.owner.records(None, false).unwrap().len(), 4);
}

#[test]
fn bundles_and_source_types_need_the_lead() {
    let t = Team::new();
    let records = t.owner.records(None, false).unwrap();
    let (a, b) = (records[0].id, records[1].id);

    // Richtige Rolle: Die Leitung bündelt und setzt die Quellenart; mit dem Paket kommt beides an.
    let stage = t.open_stage(|p| (p.blind, p.min_reviewers) = (false, 1));
    for record in &records {
        t.owner.cast_vote(&stage.id, &record.id.to_string(), "include", None, &[], "").unwrap();
    }
    let study = t.owner.link_study(None, "Kohorte", &[a, b], &a).unwrap();
    t.owner.set_source_type(&[a], "Primärstudie").unwrap();
    send_package(&t.owner, &t.r1);
    let seen = t.r1.sources().unwrap();
    assert_eq!((seen.n_records, seen.n_sources), (4, 3));
    let bundle = seen.sources.iter().find(|s| s.study_id == Some(study)).unwrap();
    assert_eq!((bundle.id, bundle.source_type.as_str()), (a, "Primärstudie"), "ohne Liste ist die Art freier Text");

    // Falsche Rolle an der App vorbei.
    let forged = [
        ("RecordsLinkedToStudy", json!({"study_id": study.to_string(), "label": "", "records": [], "primary": null})),
        ("SourceTypeSet", json!({"records": [b.to_string()], "source_type": "Review"})),
    ];
    for (event_type, payload) in forged {
        let (_dir, copy) = copy_of(&t, &t.r1);
        copy.store().append(copy.me(), event_type, None, payload).unwrap();
        let report = merge(&t.owner, &wire(&export_return(&copy).unwrap())).unwrap();
        assert_eq!(report.accepted, 0, "{event_type}");
        assert!(report.rejected[0].contains("Owner und Admins"), "{event_type}: {:?}", report.rejected);
    }
    assert_eq!(t.owner.sources().unwrap().n_sources, 3, "das Bündel steht noch");
}

#[test]
fn fulltext_status_needs_write_rights_and_zotero_links_need_the_lead() {
    let t = Team::new();
    let record = t.owner.records(None, false).unwrap()[0].id;
    let sum = "0".repeat(64);
    let status = json!({"record_id": record.to_string(), "status": "unavailable", "reason": "nicht lieferbar"});
    let link = json!({"library_type": "group", "library_id": 7, "name": "Team"});
    let mapped = json!({"items": [{"record_id": record.to_string(), "item_key": "ABCD2345"}]});

    // Richtige Rolle: r1 vermerkt den Status, die Leitung verknüpft Zotero; alle erfahren es mit dem Paket.
    t.r1.set_fulltext_unavailable(&record, "nicht lieferbar").unwrap();
    assert_eq!(send_return(&t.r1, &t.owner).accepted, 1);
    let zotero = ZoteroLink { library_type: "group".into(), library_id: 7, collection_key: None, name: "Team".into() };
    t.owner.link_zotero(&zotero).unwrap();
    send_package(&t.owner, &t.r2);
    let seen = t.r2.fulltext_overview().unwrap();
    assert_eq!(seen.zotero, Some(zotero.clone()));
    assert_eq!(seen.rows[0].fulltext.as_ref().map(|f| f.reason.as_str()), Some("nicht lieferbar"));

    // Falsche Rolle über die App.
    assert!(matches!(t.viewer.set_fulltext_unavailable(&record, "x"), Err(Error::Forbidden(_))));
    assert!(matches!(t.r1.link_zotero(&zotero), Err(Error::Forbidden(_))));
    assert!(matches!(t.r1.confirm_zotero(&[]), Err(Error::Forbidden(_))));

    // Falsche Rolle an der App vorbei, und ein Pfad, der aus dem Ordner herausführt.
    let forged = [
        (&t.viewer, "FullTextStatusSet", status, "nicht abstimmen"),
        (&t.r1, "ZoteroLibraryLinked", link, "Owner und Admins"),
        (&t.r1, "ZoteroItemMapped", mapped, "Owner und Admins"),
        (
            &t.r1,
            "FullTextStatusSet",
            json!({"record_id": record.to_string(), "status": "retrieved", "origin": "folder",
                "path": "../../geheim.pdf", "sha256": sum, "size": 1}),
            "innerhalb des Ordners",
        ),
    ];
    for (member, event_type, payload, reason) in forged {
        let (_dir, copy) = copy_of(&t, member);
        copy.store().append(copy.me(), event_type, None, payload).unwrap();
        let report = merge(&t.owner, &wire(&export_return(&copy).unwrap())).unwrap();
        assert_eq!(report.accepted, 0, "{event_type}");
        assert!(report.rejected[0].contains(reason), "{event_type}: {:?}", report.rejected);
    }
}

#[test]
fn a_second_lead_may_document_searches_but_only_for_known_versions() {
    let t = Team::new();
    let made = t.owner.publish_search(&search("MEDLINE", json!({}))).unwrap();
    t.owner.set_member_role(&t.r1.me().actor_id, "admin").unwrap();
    send_package(&t.owner, &t.r1);

    // Richtige Rolle, über die App: neue Version, Prüfvermerk und eine Dublettenentscheidung.
    let next = search("MEDLINE", json!({"search_id": made.id.to_string(), "change_note": "Begriff ergänzt"}));
    assert_eq!(t.r1.publish_search(&next).unwrap().versions.len(), 2);
    t.r1.review_search(&made.id, 2, "2026-11-21", "Bibliothekarin, PRESS: ohne Befund", None).unwrap();
    let report = send_return(&t.r1, &t.owner);
    assert_eq!((report.accepted, report.rejected.len()), (2, 0), "{:?}", report.rejected);
    let view = &t.owner.searches().unwrap()[0];
    assert_eq!(view.versions.len(), 2);
    assert_eq!(view.versions[1].version.peer_reviews[0].recorded_by, t.r1.me().actor_id);

    // Richtige Rolle, aber an der App vorbei auf eine Suchversion verwiesen, die es nicht gibt.
    let unknown = uuid::Uuid::new_v4().to_string();
    let forged = [
        (
            "RecordsImported",
            json!({
                "batch_id": uuid::Uuid::new_v4().to_string(), "records_sha256": "x", "n_records": 0,
                "origin": "database", "search_id": unknown, "search_version": 1,
            }),
        ),
        (
            "RecordsImported",
            json!({
                "batch_id": uuid::Uuid::new_v4().to_string(), "records_sha256": "x", "n_records": 0,
                "origin": "database", "search_id": made.id.to_string(), "search_version": 7,
            }),
        ),
        (
            "SearchPeerReviewed",
            json!({"search_id": unknown, "version": 1, "reviewed_on": "2026-11-20", "note": "passt"}),
        ),
    ];
    for (event_type, payload) in forged {
        let (_dir, copy) = copy_of(&t, &t.r1);
        copy.store().append(copy.me(), event_type, None, payload).unwrap();
        let report = merge(&t.owner, &wire(&export_return(&copy).unwrap())).unwrap();
        assert_eq!(report.accepted, 0, "{event_type}");
        assert!(report.rejected[0].contains("Suchversion ist hier nicht bekannt"), "{:?}", report.rejected);
    }
}

#[test]
fn duplicate_decisions_reach_everyone_with_the_package() {
    let t = Team::new();
    t.owner.import_file("embase.ris", SECOND_SOURCE, "Embase").unwrap();
    let list = t.owner.duplicate_candidates().unwrap();
    let decisions: Vec<ort_core::service::PairDecision> = list
        .pairs
        .iter()
        .map(|p| ort_core::service::PairDecision {
            a: p.a.id,
            b: p.b.id,
            decision: "duplicate".into(),
            keep: Some(p.keep),
        })
        .collect();
    t.owner.resolve_duplicates(&decisions).unwrap();
    for member in [&t.r1, &t.viewer] {
        let report = send_package(&t.owner, member);
        assert_eq!((report.records_added, report.rejected.len()), (3, 0), "{:?}", report.rejected);
        let detail = member.detail().unwrap();
        assert_eq!((detail.n_records, detail.n_duplicates), (9, 5));
        assert_eq!(member.duplicate_candidates().unwrap().total, 0);
        assert!(matches!(member.resolve_duplicates(&decisions), Err(Error::Forbidden(_))));
    }
}
