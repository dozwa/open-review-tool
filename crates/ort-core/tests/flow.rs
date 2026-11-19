//! Durchgehender Ablauf mit vier Personen und getrennten Dateien, nur über Austauschdateien.
//! Portiert aus backend/tests/test_flow.py; ersetzt den früheren Smoke-Test durch den Proxy.

mod common;

use common::{criteria, person, send_package, send_return, Team};
use ort_core::error::Error;
use ort_core::event::is_blindable;
use ort_core::exchange::export_package;
use ort_core::service::OpenStage;
use serde_json::json;

#[test]
fn review_detail_records_and_rights() {
    let t = Team::new();
    let detail = t.viewer.detail().unwrap();
    assert_eq!(detail.role, "viewer");
    assert_eq!((detail.n_records, detail.n_duplicates), (6, 2));
    let owner = detail.members.iter().find(|m| m.role == "owner").unwrap();
    assert_eq!(owner.name, "owner");
    assert_eq!(t.r1.records(None, false).unwrap().len(), 4);
    assert_eq!(t.r1.records(Some("migraine"), true).unwrap().len(), 2, "Original und Dublette");
    // Reviewer dürfen weder importieren noch Stufen öffnen.
    assert!(matches!(t.r1.import_file("x.ris", common::RIS, ""), Err(Error::Forbidden(_))));
    assert!(matches!(
        t.r1.open_stage(&OpenStage { name: "x".into(), ..OpenStage::default() }),
        Err(Error::Forbidden(_))
    ));
}

#[test]
fn criteria_versions() {
    let t = Team::new();
    let c = t.owner.publish_criteria(&criteria("")).unwrap();
    assert_eq!(c.version, 1);
    assert_eq!(c.include_keywords, ["headache", "migraine"]);
    assert_eq!(c.reason_codes(), ["E1", "E2"]);
    assert_eq!((c.items[0].code.as_str(), c.items[0].note.as_str()), ("I1", "auch Migräne"));
    // Eine neue Version braucht eine Notiz zur Änderung und mindestens ein Kriterium.
    assert!(matches!(t.owner.publish_criteria(&criteria(" ")), Err(Error::Invalid(_))));
    assert!(matches!(t.owner.publish_criteria(&json!({"items": [], "change_note": "x"})), Err(Error::Invalid(_))));
    assert_eq!(t.owner.publish_criteria(&criteria("E2 geschärft")).unwrap().version, 2);
    assert!(matches!(t.r1.publish_criteria(&criteria("x")), Err(Error::Forbidden(_))));
    send_package(&t.owner, &t.r1);
    let c = t.r1.current_criteria().unwrap();
    assert_eq!((c.version, c.previous_version), (2, Some(1)));
}

#[test]
fn blind_screening_conflict_consensus_close() {
    let t = Team::new();
    t.owner.publish_criteria(&criteria("")).unwrap();
    let stage = t.open_stage(|_| {});
    let sid = stage.id;
    assert_eq!(stage.n_records, 4);
    assert!(stage.blind_active);
    for member in [&t.r1, &t.r2, &t.viewer] {
        send_package(&t.owner, member);
    }

    // Beide sehen dieselbe Reihenfolge und stimmen alle vier Records ab.
    let plans = [["include", "exclude", "include", "maybe"], ["exclude", "exclude", "include", "include"]];
    let mut seen = Vec::new();
    for (reviewer, plan) in [&t.r1, &t.r2].into_iter().zip(plans) {
        let mut order = Vec::new();
        for decision in plan {
            let next = reviewer.next_record(&sid).unwrap();
            let record = next.record.expect("noch ein Record offen");
            let reasons = if decision == "exclude" { vec!["E1".to_string()] } else { vec![] };
            reviewer.cast_vote(&sid, &record.id.to_string(), decision, None, &reasons, "").unwrap();
            order.push(record.id.to_string());
        }
        let done = reviewer.next_record(&sid).unwrap();
        assert!(done.record.is_none());
        assert_eq!((done.progress.done, done.progress.total), (4, 4));
        seen.push(order);
    }
    assert_eq!(seen[0], seen[1]);
    let [a, b, _c, d] = [&seen[0][0], &seen[0][1], &seen[0][2], &seen[0][3]];

    // Viewer dürfen nicht abstimmen.
    assert!(matches!(t.viewer.cast_vote(&sid, a, "include", None, &[], ""), Err(Error::Forbidden(_))));

    send_return(&t.r1, &t.owner);
    let report = send_return(&t.r2, &t.owner);
    assert_eq!(report.accepted, 4, "{:?}", report.rejected);

    // Verblindet: niemand bekommt fremde Stimmen, auch nicht der Owner.
    for review in [&t.owner, &t.r1, &t.r2, &t.viewer] {
        assert!(matches!(review.conflicts(&sid), Err(Error::Forbidden(_))));
        assert!(matches!(review.export_csv(&sid), Err(Error::Forbidden(_))));
        assert!(review.summary(&sid).unwrap().decisions.is_none());
    }
    assert!(t.r1.summary(&sid).unwrap().reviewers.is_none());
    let counts: Vec<usize> = t.owner.summary(&sid).unwrap().reviewers.unwrap().iter().map(|r| r.n_votes).collect();
    assert_eq!(counts, [4, 4]);

    // Das Paket an r1 enthält in der verblindeten Stufe keine Voten von r2 – die Verblindung ist echt.
    let pkg = export_package(&t.owner, &t.r1.me().actor_id).unwrap();
    let r2_id = t.r2.me().actor_id;
    assert!(pkg.events.iter().filter(|e| is_blindable(&e.event_type)).all(|e| e.actor_id == t.r1.me().actor_id));
    assert!(pkg.events.iter().all(|e| e.actor_id != r2_id), "r2 hat bisher nur Voten geschrieben");
    send_package(&t.owner, &t.r1);
    assert!(t.r1.events().unwrap().iter().all(|e| !(is_blindable(&e.event_type) && e.actor_id == r2_id)));

    // Konsens ist verblindet nicht möglich, Entblinden nur durch Owner oder Admin.
    assert!(matches!(t.r1.record_consensus(&sid, a, "include", "", None, &[], ""), Err(Error::Forbidden(_))));
    assert!(matches!(t.r1.unblind_stage(&sid), Err(Error::Forbidden(_))));
    t.owner.unblind_stage(&sid).unwrap();
    send_package(&t.owner, &t.r1);
    send_package(&t.owner, &t.r2);

    let conflicts = t.r1.conflicts(&sid).unwrap();
    let mut ids: Vec<String> = conflicts.iter().map(|c| c.record.id.to_string()).collect();
    ids.sort();
    let mut expected = vec![a.clone(), d.clone()];
    expected.sort();
    assert_eq!(ids, expected, "include/exclude und maybe/include");
    let conflict_a = conflicts.iter().find(|c| &c.record.id.to_string() == a).unwrap();
    let votes: Vec<(&str, &str)> = conflict_a.votes.iter().map(|v| (v.name.as_str(), v.decision.as_str())).collect();
    assert_eq!(votes, [("r1", "include"), ("r2", "exclude")]);
    assert_eq!(conflict_a.votes[1].reasons, ["E1"]);

    let summary = t.r1.summary(&sid).unwrap();
    let dec = summary.decisions.unwrap();
    assert_eq!(
        (dec.include, dec.exclude, dec.maybe, dec.disagreement, dec.needs_clarification, dec.pending),
        (1, 1, 0, 2, 0, 0)
    );
    let agreement = summary.agreement.unwrap();
    assert_eq!(agreement.percent.n_items, 4);
    assert!((agreement.percent.percent_agreement.unwrap() - 0.5).abs() < 1e-12);
    assert_eq!(agreement.cohen_pairs.len(), 1);
    let pair = &agreement.cohen_pairs[0];
    let mut names = [pair.rater_a.as_str(), pair.rater_b.as_str()];
    names.sort();
    assert_eq!(names, ["r1", "r2"], "Paare tragen Namen, keine IDs");

    // Konsens für A durch r2, Revision der eigenen Stimme für D durch r1.
    t.r2.record_consensus(&sid, a, "include", "discussion", None, &[], "").unwrap();
    t.r1.cast_vote(&sid, d, "include", None, &[], "").unwrap();
    assert_eq!(t.r1.events().unwrap().iter().filter(|e| e.event_type == "VoteRevised").count(), 1);
    send_return(&t.r1, &t.owner);
    send_return(&t.r2, &t.owner);
    assert!(t.owner.conflicts(&sid).unwrap().is_empty());
    let dec = t.owner.summary(&sid).unwrap().decisions.unwrap();
    assert_eq!((dec.include, dec.exclude), (3, 1));

    let csv = t.owner.export_csv(&sid).unwrap();
    assert!(csv.lines().next().unwrap().starts_with("record_id,title"));
    assert_eq!(csv.lines().count(), 5);

    // Abschluss: Snapshot, danach keine Stimmen mehr.
    assert!(matches!(t.r1.close_stage(&sid), Err(Error::Forbidden(_))));
    let snap = t.owner.close_stage(&sid).unwrap();
    assert_eq!(snap.hash.len(), 64);
    // r2 hat den Abschluss noch nicht erhalten und stimmt nach: der Owner weist das ab.
    t.r2.cast_vote(&sid, b, "include", None, &[], "zu spät").unwrap();
    let late = send_return(&t.r2, &t.owner);
    assert_eq!(late.accepted, 0);
    assert!(late.rejected[0].contains("geschlossen"), "{:?}", late.rejected);
    send_package(&t.owner, &t.r1);
    assert!(matches!(t.r1.cast_vote(&sid, b, "include", None, &[], ""), Err(Error::Conflict(_))));

    // Folgestufe: nur die eingeschlossenen Records.
    let fulltext = t.open_stage(|p| {
        p.name = "Volltext".into();
        p.kind = "fulltext".into();
        p.scope = "from_stage".into();
        p.source_stage = Some(sid);
        p.reason_required = Some(true);
    });
    assert_eq!(fulltext.n_records, 3);

    // Im Volltext braucht jeder Ausschluss genau einen Hauptgrund, in der App und beim Einlesen.
    assert!(fulltext.reason_required);
    for member in [&t.r1, &t.r2] {
        send_package(&t.owner, member);
    }
    let ft = t.r1.next_record(&fulltext.id).unwrap().record.unwrap().id.to_string();
    let err = t.r1.cast_vote(&fulltext.id, &ft, "exclude", None, &["E1".to_string()], "").unwrap_err();
    assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("Hauptgrund"), "{err}");
    t.r1.cast_vote(&fulltext.id, &ft, "exclude", Some("E2"), &[], "").unwrap();
    assert!(!send_return(&t.r1, &t.owner).rejected.iter().any(|r| r.contains("Hauptgrund")));
    let forged = json!({"record_id": ft, "decision": "exclude"});
    t.r2.store().append(t.r2.me(), "VoteCast", Some(fulltext.id), forged).unwrap();
    assert!(send_return(&t.r2, &t.owner).rejected.iter().any(|r| r.contains("Hauptgrund")));
    let votes = t.owner.store().stage_events(&fulltext.id).unwrap();
    assert_eq!(votes.iter().filter(|e| e.event_type == "VoteCast").count(), 1);

    // Alle Ketten sind unversehrt, beim Owner und bei den anderen.
    send_package(&t.owner, &t.viewer);
    for review in [&t.owner, &t.r1, &t.viewer] {
        let chain = review.verify_chain().unwrap();
        assert!(chain.ok, "{:?}", chain.problems);
        assert!(chain.n_events > 15);
    }
}

#[test]
fn pilot_sample_is_reproducible() {
    let t = Team::new();
    let sample = |name: &str| {
        t.open_stage(|p| {
            p.name = name.into();
            p.kind = "pilot".into();
            p.scope = "sample".into();
            p.sample_size = Some(2);
            p.seed = Some(7);
        })
    };
    let (pilot, again) = (sample("Pilot"), sample("Pilot 2"));
    assert_eq!((pilot.n_records, pilot.seed), (2, Some(7)));
    send_package(&t.owner, &t.r1);
    let first = t.r1.next_record(&pilot.id).unwrap().record.unwrap().id;
    let second = t.r1.next_record(&again.id).unwrap().record.unwrap().id;
    assert_eq!(first, second);
}

#[test]
fn undecidable_flag_goes_to_clarification() {
    let t = Team::new();
    let stage = t.open_stage(|p| p.blind = false);
    send_package(&t.owner, &t.r1);
    let record = t.r1.next_record(&stage.id).unwrap().record.unwrap().id.to_string();
    t.r1.flag_undecidable(&stage.id, &record, "Abstract fehlt").unwrap();
    let conflicts = t.r1.conflicts(&stage.id).unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!((conflicts[0].record.id.to_string(), conflicts[0].status.as_str()), (record, "needs_clarification"));
}

#[test]
fn failed_import_is_reported() {
    let t = Team::new();
    let b = t.owner.import_file("leer.ris", "Diese Datei enthält keine Literaturangaben.\n", "").unwrap();
    assert_eq!(b.status, "failed");
    assert!(!b.error.is_empty());
    assert!(matches!(t.owner.import_file("bild.png", "kein Literaturformat", ""), Err(Error::Invalid(_))));
}

#[test]
fn last_owner_cannot_be_demoted() {
    let t = Team::new();
    let me = t.owner.me().actor_id;
    assert!(matches!(t.owner.set_member_role(&me, "reviewer"), Err(Error::Invalid(_))));
    let other = person("zweiter Owner");
    t.owner.set_member(&other.public(), "owner").unwrap();
    t.owner.set_member_role(&me, "admin").unwrap();
}
