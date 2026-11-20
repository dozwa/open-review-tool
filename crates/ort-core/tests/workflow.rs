//! Vorlagen, Stationen, Gates und Auswahlregeln. Alles läuft wie im echten Betrieb über Pakete und
//! Rückmeldungen.

mod common;

use std::collections::BTreeMap;

use common::{criteria, person, retrieve_fulltexts, search, send_package, send_return, wire, Team, SECOND_SOURCE};
use ort_core::error::Error;
use ort_core::event::sha256_hex;
use ort_core::exchange::{export_return, merge};
use ort_core::fulltext::Assign;
use ort_core::highlights::NewHighlight;
use ort_core::projection::Verify;
use ort_core::registration::RegistrationDraft;
use ort_core::report::{Level, Retrieval};
use ort_core::service::{ImportSource, OpenStage, PairDecision, RecordFilter, StageView};
use ort_core::workflow::{builtin_template, StationView};
use ort_core::Review;
use serde_json::{json, Value};
use uuid::Uuid;

const JBI: Option<&str> = Some("jbi_scoping");

fn station(review: &Review, key: &str) -> StationView {
    review.station(key).unwrap()
}

fn states(review: &Review) -> BTreeMap<String, &'static str> {
    review.stations().unwrap().into_iter().map(|s| (s.key, s.state)).collect()
}

/// Öffnet die nächste Stufe einer Station mit den Vorgaben der Vorlage.
fn open(t: &Team, key: &str, change: impl FnOnce(&mut OpenStage)) -> StageView {
    let mut params = t.owner.stage_defaults(key).unwrap();
    change(&mut params);
    let stage = t.owner.open_stage(&params).unwrap();
    for member in [&t.r1, &t.r2] {
        send_package(&t.owner, member);
    }
    stage
}

/// Die Einträge der Stufe in der Reihenfolge, in der sie vorgelegt werden.
fn order(t: &Team, stage: &Uuid) -> Vec<String> {
    t.owner.export_csv(stage).map(|csv| csv.lines().skip(1).map(|l| l[..36].to_string()).collect()).unwrap()
}

/// Dieselbe Reihenfolge, gelesen aus der Liste; geht auch in einer verblindeten Stufe.
fn order_of(review: &Review, stage: &Uuid) -> Vec<String> {
    review.stage_records(stage, &RecordFilter::default()).unwrap().items.iter().map(|i| i.id.to_string()).collect()
}

fn vote(review: &Review, stage: &Uuid, record: &str, decision: &str) {
    review.cast_vote(stage, record, decision, None, &[], "").unwrap();
}

/// Beide Reviewer stimmen der Reihe nach ab; danach liegen alle Stimmen beim Owner.
fn vote_all(t: &Team, stage: &Uuid, r1: &[&str], r2: &[&str]) {
    for (reviewer, plan) in [(&t.r1, r1), (&t.r2, r2)] {
        for decision in plan {
            let record = reviewer.next_record(stage).unwrap().record.expect("noch ein Eintrag offen");
            vote(reviewer, stage, &record.id.to_string(), decision);
        }
        send_return(reviewer, &t.owner);
    }
}

fn jbi_team_with_protocol() -> Team {
    let t = Team::with_template(JBI);
    t.owner.publish_criteria(&criteria("")).unwrap();
    t.owner.publish_search(&search("MEDLINE", json!({}))).unwrap();
    t
}

#[test]
fn review_carries_its_template_in_the_log() {
    let t = Team::with_template(JBI);
    let detail = t.r1.detail().unwrap();
    assert!(detail.workflow.published);
    assert_eq!((detail.workflow.template_key.as_str(), detail.workflow.version), ("jbi_scoping", 1));
    assert_eq!(detail.workflow.terms["records"], "Einträge");
    assert_eq!(detail.workflow.stations.len(), 12);
    let created = &t.r1.events().unwrap()[0];
    assert_eq!(created.payload["review_type"], "scoping");
    // Alle Stationen der Vorlage kann die App bearbeiten.
    let available: Vec<&str> =
        detail.workflow.stations.iter().filter(|s| s.available).map(|s| s.key.as_str()).collect();
    assert_eq!(
        available,
        [
            "protocol",
            "search",
            "import",
            "pilot_title_abstract",
            "title_abstract",
            "fulltext_retrieval",
            "fulltext",
            "citation_search",
            "bundling",
            "charting_pilot",
            "charting",
            "report"
        ]
    );
    assert!(Review::create_in_memory_with(person("x"), "X", Some("gibtsnicht")).is_err());
}

#[test]
fn protocol_gate_locks_the_pilot() {
    let t = Team::with_template(JBI);
    let s = states(&t.owner);
    assert_eq!((s["protocol"], s["import"], s["pilot_title_abstract"]), ("running", "done", "locked"));
    let protocol = station(&t.owner, "protocol");
    let checks: Vec<(&str, Option<bool>)> = protocol.checks.iter().map(|c| (c.check.as_str(), c.passed)).collect();
    assert_eq!(checks, [("criteria_published", Some(false)), ("min_writers", Some(true))]);
    assert_eq!(protocol.checks[1].detail, "3 vorhanden");

    let params = t.owner.stage_defaults("pilot_title_abstract").unwrap();
    let err = t.owner.open_stage(&params).unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err}");
    assert!(err.to_string().contains("Protokoll und Team"), "{err}");

    t.owner.publish_criteria(&criteria("")).unwrap();
    // Als Nächstes fehlt die Suche; sie lässt sich festhalten oder mit Begründung übersteuern.
    let s = states(&t.owner);
    assert_eq!((s["protocol"], s["search"], s["pilot_title_abstract"]), ("done", "open", "locked"));
    assert!(t.owner.open_stage(&params).unwrap_err().to_string().contains("„Suche“"));
    t.owner.publish_search(&search("MEDLINE", json!({}))).unwrap();
    let s = states(&t.owner);
    assert_eq!((s["search"], s["pilot_title_abstract"]), ("done", "open"));
    assert_eq!(station(&t.owner, "search").checks[0].detail, "1 Suche");
    // Ohne Station geht es in einem Review mit Vorlage nicht, und nicht jede Station hat Stufen.
    assert!(t.owner.open_stage(&OpenStage { name: "x".into(), ..params.clone() }).is_ok());
    let no_station = OpenStage { station: None, ..params.clone() };
    assert!(t.owner.open_stage(&no_station).unwrap_err().to_string().contains("Station"));
    let setup = OpenStage { station: Some("import".into()), ..params };
    assert!(matches!(t.owner.open_stage(&setup), Err(Error::Invalid(_))));
}

#[test]
fn pilot_below_75_percent_locks_title_abstract_and_a_new_round_opens_it() {
    let t = jbi_team_with_protocol();
    let pilot = open(&t, "pilot_title_abstract", |p| p.seed = Some(7));
    assert_eq!((pilot.station.as_deref(), pilot.round, pilot.n_records), (Some("pilot_title_abstract"), 1, 4));
    assert_eq!((pilot.decision_rule.as_str(), pilot.min_reviewers, pilot.blind), ("unanimous", 2, true));
    assert_eq!(states(&t.owner)["pilot_title_abstract"], "running");
    // Solange die Runde läuft, gibt es keine zweite.
    let again = t.owner.stage_defaults("pilot_title_abstract").unwrap();
    assert!(t.owner.open_stage(&again).unwrap_err().to_string().contains("noch eine Stufe offen"));

    // 2 von 4 gleich: 50 %. „Maybe“ zählt in der Vorlage als Include.
    vote_all(&t, &pilot.id, &["include", "include", "include", "include"], &["maybe", "exclude", "exclude", "include"]);
    assert!(matches!(t.owner.gate_preview(&pilot.id), Err(Error::Forbidden(_))), "verblindet kein Einblick");
    t.owner.unblind_stage(&pilot.id).unwrap();
    assert!(matches!(t.r1.gate_preview(&pilot.id), Err(Error::Forbidden(_))));
    let preview = t.owner.gate_preview(&pilot.id).unwrap().unwrap();
    let closed = t.owner.close_stage(&pilot.id).unwrap();
    let gate = closed.gate.unwrap();
    assert_eq!(gate, preview, "die Vorschau zeigt, was ins Log kommt");
    assert!(!gate.passed);
    let c = &gate.checks[0];
    assert_eq!((c.numerator, c.denominator, c.threshold_percent, c.percent_x100), (2, 4, Some(75), Some(5000)));

    let view = station(&t.owner, "pilot_title_abstract");
    assert_eq!(view.state, "open", "eine neue Runde ist möglich");
    assert_eq!(view.checks[0].passed, Some(false));
    assert_eq!(view.checks[0].detail, "50,0 % bei 4 doppelt bewerteten Einträgen");
    assert_eq!(states(&t.owner)["title_abstract"], "locked");
    let ta = t.owner.stage_defaults("title_abstract").unwrap();
    let err = t.owner.open_stage(&ta).unwrap_err();
    assert!(err.to_string().contains("Pilot Titel/Abstract"), "{err}");

    // Runde 2: 3 von 4 gleich, genau die Schwelle.
    let second = open(&t, "pilot_title_abstract", |_| {});
    assert_eq!((second.round, second.name.as_str()), (2, "Pilot Titel/Abstract (Runde 2)"));
    vote_all(&t, &second.id, &["include"; 4], &["include", "include", "exclude", "include"]);
    t.owner.unblind_stage(&second.id).unwrap();
    assert!(t.owner.close_stage(&second.id).unwrap().gate.unwrap().passed);
    let view = station(&t.owner, "pilot_title_abstract");
    assert_eq!((view.state, view.rounds.len()), ("done", 2));
    assert_eq!(view.rounds[0].gate.as_ref().map(|g| g.passed), Some(false), "Runden bleiben nebeneinander sichtbar");
    assert_eq!(states(&t.owner)["title_abstract"], "open");
    t.owner.open_stage(&ta).unwrap();

    // Das Ergebnis des Gates kommt mit dem Paket bei allen an.
    send_package(&t.owner, &t.r1);
    assert_eq!(states(&t.r1)["pilot_title_abstract"], "done");
    assert_eq!(states(&t.r1)["title_abstract"], "running");
    assert!(t.r1.verify_chain().unwrap().ok);
}

#[test]
fn failed_gate_can_be_overridden_with_a_reason() {
    let t = jbi_team_with_protocol();
    let pilot = open(&t, "pilot_title_abstract", |_| {});
    // Niemand hat doppelt bewertet: nichts zu messen, Gate nicht erfüllt.
    assert!(matches!(t.owner.override_gate("pilot_title_abstract", "eilig"), Err(Error::Conflict(_))));
    let gate = t.owner.close_stage(&pilot.id).unwrap().gate.unwrap();
    assert_eq!((gate.passed, gate.checks[0].denominator, gate.checks[0].percent_x100), (false, 0, None));
    assert_eq!(states(&t.owner)["title_abstract"], "locked");

    assert!(matches!(t.owner.override_gate("pilot_title_abstract", "  "), Err(Error::Invalid(_))));
    assert!(matches!(t.r1.override_gate("pilot_title_abstract", "Grund"), Err(Error::Forbidden(_))));
    assert!(matches!(t.owner.override_gate("report", "Grund"), Err(Error::Invalid(_))), "sperrt nichts");
    t.owner.override_gate("pilot_title_abstract", "Team hat sich mündlich abgestimmt").unwrap();
    let view = station(&t.owner, "pilot_title_abstract");
    assert_eq!(view.state, "done");
    assert_eq!(view.overridden.as_deref(), Some("Team hat sich mündlich abgestimmt"));
    assert!(matches!(t.owner.override_gate("pilot_title_abstract", "nochmal"), Err(Error::Conflict(_))));
    let ta = open(&t, "title_abstract", |_| {});
    assert_eq!(ta.n_records, 4);
    assert_eq!(station(&t.r2, "pilot_title_abstract").overridden.as_deref(), Some("Team hat sich mündlich abgestimmt"));

    // Eine neue Runde nach dem Übersteuern muss ihr Gate wieder selbst bestehen.
    let again = open(&t, "pilot_title_abstract", |_| {});
    t.owner.close_stage(&again.id).unwrap();
    assert_eq!(station(&t.owner, "pilot_title_abstract").state, "open");
}

#[test]
fn title_abstract_gate_needs_all_decided_and_no_conflicts() {
    let t = jbi_team_with_protocol();
    t.owner.override_gate("pilot_title_abstract", "Pilot entfällt, eingespieltes Team").unwrap();
    let ta = open(&t, "title_abstract", |_| {});
    vote_all(&t, &ta.id, &["include", "include", "exclude"], &["include", "exclude", "exclude", "include"]);
    t.owner.unblind_stage(&ta.id).unwrap();
    let gate = t.owner.gate_preview(&ta.id).unwrap().unwrap();
    let by: BTreeMap<&str, (bool, i64, i64)> =
        gate.checks.iter().map(|c| (c.check.as_str(), (c.passed, c.numerator, c.denominator))).collect();
    assert_eq!(by["all_decided"], (false, 2, 4), "ein Widerspruch, ein Eintrag mit nur einer Stimme");
    assert_eq!(by["no_open_conflicts"], (false, 1, 4));

    let records = order(&t, &ta.id);
    t.owner.record_consensus(&ta.id, &records[1], "include", "", None, &[], "").unwrap();
    vote(&t.r1, &ta.id, &records[3], "include");
    send_return(&t.r1, &t.owner);
    let closed = t.owner.close_stage(&ta.id).unwrap();
    assert!(closed.gate.unwrap().passed);
    assert_eq!(states(&t.owner)["title_abstract"], "done");

    // Die Volltext-Stufe nimmt die Eingeschlossenen der Station, ohne dass jemand die Stufe wählt.
    retrieve_fulltexts(&t);
    let fulltext = open(&t, "fulltext", |_| {});
    assert_eq!((fulltext.n_records, fulltext.scope.as_deref()), (3, Some("from_stage")));
}

#[test]
fn template_rules_that_are_fixed_cannot_be_changed() {
    let t = jbi_team_with_protocol();
    let mut params = t.owner.stage_defaults("pilot_title_abstract").unwrap();
    params.decision_rule = Some("liberal".into());
    let err = t.owner.open_stage(&params).unwrap_err();
    assert!(err.to_string().contains("legt die Auswahlregel"), "{err}");
    params.decision_rule = Some("würfeln".into());
    assert!(matches!(t.owner.open_stage(&params), Err(Error::Invalid(_))));
}

#[test]
fn liberal_rule_matches_hand_calculation() {
    let t = Team::with_template(Some("free"));
    let stage = open(&t, "screening", |p| {
        p.decision_rule = Some("liberal".into());
        p.min_reviewers = 2;
        p.blind = false;
    });
    let [a, b, c, d]: [String; 4] = order(&t, &stage.id).try_into().unwrap();
    for (record, decision) in [(&a, "include"), (&b, "exclude"), (&c, "exclude"), (&d, "include")] {
        vote(&t.r1, &stage.id, record, decision);
    }
    send_return(&t.r1, &t.owner);
    send_package(&t.owner, &t.r2);
    // r2 sieht die Stimmen von r1: Was schon eingeschlossen ist, braucht keine zweite Stimme.
    let next = t.r2.next_record(&stage.id).unwrap();
    assert_eq!(next.record.unwrap().id.to_string(), b);
    assert_eq!((next.progress.done, next.progress.total), (0, 2));
    vote(&t.r2, &stage.id, &b, "exclude");
    vote(&t.r2, &stage.id, &a, "exclude");
    assert!(t.r2.next_record(&stage.id).unwrap().record.is_some(), "c ist noch offen");
    send_return(&t.r2, &t.owner);

    let finals = t.owner.final_decisions(&stage.id).unwrap();
    let got = |r: &String| (finals[r].decision.as_deref(), finals[r].status);
    assert_eq!(got(&a), (Some("include"), "liberal"), "eine Einschluss-Stimme genügt, auch gegen einen Ausschluss");
    assert_eq!(got(&b), (Some("exclude"), "unanimous"), "zwei Ausschlüsse");
    assert_eq!(got(&c), (None, "pending"), "ein Ausschluss allein reicht nicht");
    assert_eq!(got(&d), (Some("include"), "liberal"));
    assert!(t.owner.conflicts(&stage.id).unwrap().is_empty());
}

#[test]
fn single_verified_rule_matches_hand_calculation() {
    let t = Team::with_template(Some("free"));
    let blind = OpenStage {
        decision_rule: Some("single_verified".into()),
        blind: true,
        ..t.owner.stage_defaults("screening").unwrap()
    };
    assert!(t.owner.open_stage(&blind).unwrap_err().to_string().contains("nicht verblindet"));

    let stage = open(&t, "screening", |p| p.decision_rule = Some("single_verified".into()));
    assert_eq!(stage.verify, Some(Verify { scope: "all".into(), percent: None, seed: None }));
    let [a, b, c, d]: [String; 4] = order(&t, &stage.id).try_into().unwrap();
    for (record, decision) in [(&a, "include"), (&b, "exclude"), (&c, "exclude"), (&d, "maybe")] {
        vote(&t.r1, &stage.id, record, decision);
    }
    send_return(&t.r1, &t.owner);
    let finals = t.owner.final_decisions(&stage.id).unwrap();
    assert_eq!((finals[&a].decision.as_deref(), finals[&a].status), (Some("include"), "single"));
    assert_eq!((finals[&b].decision.as_deref(), finals[&b].status), (None, "pending"), "Ausschluss wartet auf Prüfung");
    assert_eq!(finals[&d].decision.as_deref(), Some("maybe"));

    // Die zweite Person bekommt nur die Ausschlüsse vorgelegt.
    send_package(&t.owner, &t.r2);
    let next = t.r2.next_record(&stage.id).unwrap();
    assert_eq!((next.record.unwrap().id.to_string(), next.progress.total), (b.clone(), 2));
    // Die Liste sagt dasselbe, damit die Screening-Seite nach einer Stimme nicht zu Einschlüssen springt.
    let wanted: Vec<String> =
        t.r2.stage_records(&stage.id, &RecordFilter::default())
            .unwrap()
            .items
            .into_iter()
            .filter(|row| row.wants_my_vote)
            .map(|row| row.id.to_string())
            .collect();
    assert_eq!(wanted, vec![b.clone(), c.clone()]);
    vote(&t.r2, &stage.id, &b, "exclude");
    vote(&t.r2, &stage.id, &c, "include");
    assert!(t.r2.next_record(&stage.id).unwrap().record.is_none());
    send_return(&t.r2, &t.owner);
    let finals = t.owner.final_decisions(&stage.id).unwrap();
    assert_eq!((finals[&b].decision.as_deref(), finals[&b].status), (Some("exclude"), "unanimous"));
    assert_eq!(finals[&c].status, "disagreement");
}

#[test]
fn verification_sample_can_be_recomputed_from_seed_and_id() {
    let t = Team::with_template(Some("free"));
    let stage = open(&t, "screening", |p| {
        p.decision_rule = Some("single_verified".into());
        p.verify = Some(Verify { scope: "sample".into(), percent: Some(50), seed: Some(11) });
    });
    let verify = stage.verify.clone().unwrap();
    // Nachrechnen ohne die App: SHA-256 über „Seed:ID“, erste acht Hex-Stellen, Rest bei Teilung durch 100.
    let by_hand = |record: &str| {
        let digest = sha256_hex(format!("11:{record}").as_bytes());
        u32::from_str_radix(&digest[..8], 16).unwrap() % 100 < 50
    };
    let records = order(&t, &stage.id);
    for record in &records {
        assert_eq!(verify.covers(record), by_hand(record));
        vote(&t.r1, &stage.id, record, "exclude");
    }
    send_return(&t.r1, &t.owner);
    let finals = t.owner.final_decisions(&stage.id).unwrap();
    for record in &records {
        let expected = if by_hand(record) { (None, "pending") } else { (Some("exclude"), "single") };
        assert_eq!((finals[record].decision.as_deref(), finals[record].status), expected);
    }
    let hits = (0..2000).filter(|_| verify.covers(&Uuid::new_v4().to_string())).count();
    assert!((850..1150).contains(&hits), "etwa die Hälfte, gezählt {hits}");

    // Ohne Seed würfelt die App einen und schreibt ihn in die Stufe; ohne Anteil geht es nicht.
    let mut params = t.owner.stage_defaults("screening").unwrap();
    params.decision_rule = Some("single_verified".into());
    params.verify = Some(Verify { scope: "sample".into(), percent: Some(20), seed: None });
    assert!(t.owner.open_stage(&params).unwrap().verify.unwrap().seed.is_some());
    params.verify = Some(Verify { scope: "sample".into(), percent: None, seed: None });
    assert!(matches!(t.owner.open_stage(&params), Err(Error::Invalid(_))));
}

#[test]
fn third_person_rule_keeps_voters_from_recording_consensus() {
    let t = Team::with_template(Some("free"));
    let stage = open(&t, "screening", |p| {
        p.conflict_rule = Some("third_person".into());
        p.min_reviewers = 2;
    });
    let record = order(&t, &stage.id)[0].clone();
    vote(&t.r1, &stage.id, &record, "include");
    vote(&t.r2, &stage.id, &record, "exclude");
    send_return(&t.r2, &t.owner);

    let err = t.r1.record_consensus(&stage.id, &record, "include", "", None, &[], "").unwrap_err();
    assert!(matches!(err, Error::Forbidden(_)), "{err}");
    // r1 umgeht die App und schreibt den Konsens selbst ins Log: Der Owner weist ihn beim Einlesen ab.
    t.r1.store()
        .append(t.r1.me(), "ConsensusRecorded", Some(stage.id), json!({"record_id": record, "decision": "include"}))
        .unwrap();
    let report = merge(&t.owner, &wire(&export_return(&t.r1).unwrap())).unwrap();
    assert_eq!(report.rejected.len(), 1, "{:?}", report.rejected);
    assert!(report.rejected[0].contains("Dritt-Entscheid"), "{:?}", report.rejected);
    assert_eq!(t.owner.final_decisions(&stage.id).unwrap()[&record].status, "disagreement");

    // Der Owner hat nicht abgestimmt und darf entscheiden.
    t.owner.record_consensus(&stage.id, &record, "exclude", "", None, &[], "").unwrap();
    let event = t.owner.events().unwrap().into_iter().rfind(|e| e.event_type == "ConsensusRecorded").unwrap();
    assert_eq!(event.payload["method"], "third_person");
    assert_eq!(t.owner.final_decisions(&stage.id).unwrap()[&record].decision.as_deref(), Some("exclude"));
}

#[test]
fn only_the_lead_may_write_workflow_events() {
    let t = jbi_team_with_protocol();
    let pilot = open(&t, "pilot_title_abstract", |_| {});
    let template = serde_json::to_value(builtin_template("free").unwrap()).unwrap();
    assert!(matches!(t.r1.publish_workflow(&template, "lockerer"), Err(Error::Forbidden(_))));
    // r1 umgeht die App: eigene Vorlage, eigenes Gate-Ergebnis, eigenes Übersteuern.
    let forged = [
        ("WorkflowVersionPublished", None, json!({"version": 2, "template": template})),
        (
            "GateEvaluated",
            Some(pilot.id),
            json!({"station": "pilot_title_abstract", "round": 1, "passed": true, "checks": []}),
        ),
        ("GateOverridden", None, json!({"station": "pilot_title_abstract", "round": 1, "reason": "passt schon"})),
    ];
    for (event_type, stage, payload) in forged {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("r1.sqlite");
        let pkg = wire(&ort_core::exchange::export_package(&t.owner, &t.r1.me().actor_id).unwrap());
        let copy = ort_core::exchange::create_from_package(&path, t.r1.me().clone(), &pkg).unwrap().0;
        copy.store().append(copy.me(), event_type, stage, payload).unwrap();
        let report = merge(&t.owner, &wire(&export_return(&copy).unwrap())).unwrap();
        assert_eq!(report.accepted, 0, "{event_type}");
        assert!(report.rejected[0].contains("Owner und Admins"), "{event_type}: {:?}", report.rejected);
    }
    assert_eq!(t.owner.detail().unwrap().workflow.version, 1);
    assert_eq!(states(&t.owner)["pilot_title_abstract"], "running");
}

#[test]
fn changed_template_is_a_new_version_with_a_reason() {
    let t = jbi_team_with_protocol();
    let mut template = serde_json::to_value(builtin_template("jbi_scoping").unwrap()).unwrap();
    template["stations"][3]["gate"][0]["percent"] = json!(80);
    assert!(matches!(t.owner.publish_workflow(&template, " "), Err(Error::Invalid(_))));
    assert_eq!(t.owner.publish_workflow(&template, "Schwelle laut Protokoll 80 %").unwrap(), 2);
    assert_eq!(station(&t.owner, "pilot_title_abstract").checks[0].label, "Übereinstimmung mindestens 80 %");
    template["stations"][3]["gate"][0]["check"] = json!("mondphase");
    assert!(matches!(t.owner.publish_workflow(&template, "x"), Err(Error::Invalid(_))));
    send_package(&t.owner, &t.r1);
    assert_eq!(t.r1.detail().unwrap().workflow.version, 2);
}

#[test]
fn reviews_without_a_template_run_freely_as_before() {
    let t = Team::new();
    let workflow = t.owner.detail().unwrap().workflow;
    assert!(!workflow.published);
    assert_eq!((workflow.version, workflow.template_key.as_str()), (0, "free"));
    // Stufen ohne Station, auch mehrere nebeneinander, mit den Regeln von früher.
    let first = t.open_stage(|_| {});
    let second = t.open_stage(|p| p.name = "Zweite".into());
    assert_eq!((first.decision_rule.as_str(), first.min_reviewers, first.blind), ("unanimous", 2, true));
    let screening = station(&t.owner, "screening");
    assert_eq!((screening.state, screening.rounds.len()), ("running", 2));
    assert!(t.owner.close_stage(&first.id).unwrap().gate.is_none());
    t.owner.close_stage(&second.id).unwrap();
    assert_eq!(station(&t.owner, "screening").state, "done");
    assert!(t.owner.events().unwrap().iter().all(|e| !e.event_type.starts_with("Gate")));

    // Eine Vorlage lässt sich nachträglich veröffentlichen; die erste Version braucht keine Begründung.
    let template = serde_json::to_value(builtin_template("free").unwrap()).unwrap();
    assert_eq!(t.owner.publish_workflow(&template, "").unwrap(), 1);
    assert!(t.owner.detail().unwrap().workflow.published);
}

/// Scoping Review bis vor den Volltext: Titel/Abstract ist geschlossen, alle vier Einträge sind eingeschlossen.
fn team_before_fulltext() -> Team {
    let t = jbi_team_with_protocol();
    t.owner.override_gate("pilot_title_abstract", "Pilot entfällt, eingespieltes Team").unwrap();
    let ta = open(&t, "title_abstract", |_| {});
    vote_all(&t, &ta.id, &["include"; 4], &["include"; 4]);
    t.owner.unblind_stage(&ta.id).unwrap();
    assert!(t.owner.close_stage(&ta.id).unwrap().gate.unwrap().passed);
    t
}

/// Dasselbe, und alle vier Volltexte liegen im Ordner.
fn team_with_fulltexts() -> Team {
    let t = team_before_fulltext();
    retrieve_fulltexts(&t);
    t
}

fn last_payload(review: &Review, event_type: &str) -> serde_json::Value {
    review.events().unwrap().into_iter().rfind(|e| e.event_type == event_type).unwrap().payload
}

#[test]
fn fulltext_exclusion_needs_exactly_one_primary_reason() {
    let t = team_with_fulltexts();
    // Die Vorlage verlangt den Grund; abschalten lässt er sich nicht.
    let mut params = t.owner.stage_defaults("fulltext").unwrap();
    assert_eq!(params.reason_required, Some(true));
    params.reason_required = Some(false);
    assert!(t.owner.open_stage(&params).unwrap_err().to_string().contains("Hauptgrund"));
    let stage = open(&t, "fulltext", |_| {});
    assert!(stage.reason_required);
    let records = order_of(&t.r1, &stage.id);
    let (first, second) = (records[0].as_str(), records[1].as_str());
    let reasons = |codes: &[&str]| codes.iter().map(|c| c.to_string()).collect::<Vec<_>>();

    let cases: [(&str, Option<&str>, &[&str], &str); 5] = [
        ("exclude", None, &["E1"], "genau einen Hauptgrund"),
        ("exclude", Some("I1"), &[], "kein Ausschlusskriterium"),
        ("exclude", Some("E9"), &[], "kein Ausschlusskriterium"),
        ("exclude", Some("E1"), &["Falsche Population"], "kein Ausschlusskriterium"),
        ("include", Some("E1"), &[], "nur zu einem Ausschluss"),
    ];
    for (decision, primary, more, expected) in cases {
        let err = t.r1.cast_vote(&stage.id, first, decision, primary, &reasons(more), "").unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{err}");
        assert!(err.to_string().contains(expected), "{err}");
    }
    // Der Hauptgrund steht einmal im Ereignis, nicht noch einmal unter den weiteren Gründen.
    t.r1.cast_vote(&stage.id, first, "exclude", Some("E1"), &reasons(&["E2", "E1", " "]), "").unwrap();
    let payload = last_payload(&t.r1, "VoteCast");
    assert_eq!((&payload["primary_reason"], &payload["reasons"]), (&json!("E1"), &json!(["E2"])));
    t.r1.cast_vote(&stage.id, first, "exclude", Some("E2"), &[], "").unwrap();
    assert_eq!(last_payload(&t.r1, "VoteRevised")["primary_reason"], "E2");
    // Einschluss und Maybe brauchen keinen Grund.
    t.r1.cast_vote(&stage.id, second, "maybe", None, &[], "").unwrap();
    t.r2.cast_vote(&stage.id, first, "include", None, &[], "").unwrap();
    for reviewer in [&t.r1, &t.r2] {
        assert_eq!(send_return(reviewer, &t.owner).rejected, Vec::<String>::new());
    }

    // Klärung: Der Hauptgrund der Stimme ist sichtbar, und auch der Konsens braucht einen.
    t.owner.unblind_stage(&stage.id).unwrap();
    let conflicts = t.owner.conflicts(&stage.id).unwrap();
    let r1_vote = conflicts[0].votes.iter().find(|v| v.name == "r1").unwrap();
    assert_eq!(r1_vote.primary_reason.as_deref(), Some("E2"));
    let err = t.owner.record_consensus(&stage.id, first, "exclude", "", None, &[], "").unwrap_err();
    assert!(err.to_string().contains("genau einen Hauptgrund"), "{err}");
    t.owner.record_consensus(&stage.id, first, "exclude", "", Some("E2"), &[], "").unwrap();
    assert_eq!(last_payload(&t.owner, "ConsensusRecorded")["primary_reason"], "E2");
    assert_eq!(t.owner.final_decisions(&stage.id).unwrap()[first].decision.as_deref(), Some("exclude"));
}

#[test]
fn exclusion_without_primary_reason_is_rejected_on_merge() {
    let t = team_with_fulltexts();
    let stage = open(&t, "fulltext", |_| {});
    let record = order_of(&t.r1, &stage.id)[0].clone();
    // Beide Reviewer umgehen die App: ohne Hauptgrund und mit einem Kürzel, das es nicht gibt.
    let forged = [
        (&t.r1, json!({"record_id": record, "decision": "exclude", "reasons": ["E1"]}), "genau einen Hauptgrund"),
        (
            &t.r2,
            json!({"record_id": record, "decision": "exclude", "primary_reason": "X1"}),
            "kein Ausschlusskriterium",
        ),
    ];
    for (reviewer, payload, expected) in forged {
        reviewer.store().append(reviewer.me(), "VoteCast", Some(stage.id), payload).unwrap();
        let report = merge(&t.owner, &wire(&export_return(reviewer).unwrap())).unwrap();
        assert_eq!(report.accepted, 0, "{:?}", report.rejected);
        assert!(report.rejected[0].contains(expected), "{:?}", report.rejected);
    }

    // Dasselbe gilt für einen Konsens. Er liegt in der Verwaltungskette und wird einzeln abgewiesen.
    t.owner.unblind_stage(&stage.id).unwrap();
    assert!(t.owner.final_decisions(&stage.id).unwrap().is_empty());
    send_package(&t.owner, &t.r1);
    t.r1.store()
        .append(t.r1.me(), "ConsensusRecorded", Some(stage.id), json!({"record_id": record, "decision": "exclude"}))
        .unwrap();
    let report = merge(&t.owner, &wire(&export_return(&t.r1).unwrap())).unwrap();
    assert!(report.rejected.iter().any(|r| r.contains("Konsens Nr.") && r.contains("Hauptgrund")), "{report:?}");
    // Mit Hauptgrund kommt eine Stimme durch die Rückmeldung.
    let other = order(&t, &stage.id)[1].clone();
    send_package(&t.owner, &t.viewer);
    let r2_copy = t.dir.path().join("r2-neu.sqlite");
    let pkg = wire(&ort_core::exchange::export_package(&t.owner, &t.r2.me().actor_id).unwrap());
    let fresh = ort_core::exchange::create_from_package(&r2_copy, t.r2.me().clone(), &pkg).unwrap().0;
    fresh.cast_vote(&stage.id, &other, "exclude", Some("E2"), &[], "").unwrap();
    let report = merge(&t.owner, &wire(&export_return(&fresh).unwrap())).unwrap();
    assert_eq!((report.accepted, report.rejected.len()), (1, 0), "{report:?}");
}

#[test]
fn a_stage_keeps_the_criteria_it_was_opened_with() {
    let t = Team::with_template(Some("free"));
    // Ohne Ausschlusskriterien mit Kürzel gibt es keine Stufe mit Pflichtgrund.
    let mut params = t.owner.stage_defaults("screening").unwrap();
    assert_eq!(params.reason_required, Some(false));
    params.reason_required = Some(true);
    assert!(t.owner.open_stage(&params).unwrap_err().to_string().contains("Kürzel"));

    t.owner.publish_criteria(&criteria("")).unwrap();
    let old = open(&t, "screening", |p| p.reason_required = Some(true));
    let changed = json!({
        "items": [{"code": "I1", "kind": "include", "text": "Erwachsene"}, {"code": "E7", "kind": "exclude", "text": "Tierstudie"}],
        "change_note": "Gründe neu gefasst",
    });
    assert_eq!(t.owner.publish_criteria(&changed).unwrap().version, 2);
    let new = open(&t, "screening", |p| {
        p.name = "Zweite".into();
        p.reason_required = Some(true);
    });
    assert_eq!((old.criteria_version.as_deref(), new.criteria_version.as_deref()), (Some("1"), Some("2")));

    let record = order(&t, &old.id)[0].clone();
    let err = t.r1.cast_vote(&old.id, &record, "exclude", Some("E7"), &[], "").unwrap_err();
    assert!(err.to_string().contains("Kriterienversion 1"), "{err}");
    t.r1.cast_vote(&old.id, &record, "exclude", Some("E2"), &[], "").unwrap();
    assert!(t.r1.cast_vote(&new.id, &record, "exclude", Some("E2"), &[], "").is_err());
    t.r1.cast_vote(&new.id, &record, "exclude", Some("E7"), &[], "").unwrap();
    assert_eq!(send_return(&t.r1, &t.owner).rejected, Vec::<String>::new());

    let detail = t.r1.stage_detail(&old.id).unwrap();
    assert_eq!((detail.criteria.version, detail.newer_criteria), (1, true));
    assert_eq!(detail.criteria.reason_codes(), ["E1", "E2"]);
    let detail = t.r1.stage_detail(&new.id).unwrap();
    assert_eq!((detail.criteria.version, detail.newer_criteria), (2, false));
}

#[test]
fn criteria_elements_come_from_the_template() {
    let with_element =
        |element: &str| json!({"items": [{"code": "I1", "element": element, "kind": "include", "text": "Erwachsene"}]});
    let jbi = Team::with_template(JBI);
    assert_eq!(jbi.owner.detail().unwrap().workflow.criteria_schema.len(), 4);
    let err = jbi.owner.publish_criteria(&with_element("wetter")).unwrap_err();
    assert!(err.to_string().contains("Kriterienschema"), "{err}");
    let published = jbi.owner.publish_criteria(&with_element("population")).unwrap();
    assert_eq!(published.items[0].element, "population");
    // Ungültige Kürzel weist das Register ab, auch wenn die Oberfläche sie durchlässt.
    let bad = json!({"items": [{"code": "E 1", "kind": "exclude", "text": "x"}], "change_note": "x"});
    assert!(matches!(jbi.owner.publish_criteria(&bad), Err(Error::Invalid(_))));
    // Der freie Ablauf hat kein Schema; Kriterien stehen dort ohne Element.
    let free = Team::with_template(Some("free"));
    assert!(free.owner.publish_criteria(&with_element("population")).is_err());
    assert!(free.owner.publish_criteria(&with_element("")).is_ok());
}

#[test]
fn list_filters_are_locked_while_the_stage_is_blind() {
    let t = Team::new();
    t.owner.publish_criteria(&criteria("")).unwrap();
    let stage = t.open_stage(|_| {}).id;
    for member in [&t.r1, &t.r2] {
        send_package(&t.owner, member);
    }
    let records = order_of(&t.r1, &stage);
    vote(&t.r1, &stage, &records[0], "include");
    vote(&t.r1, &stage, &records[1], "exclude");
    vote(&t.r2, &stage, &records[0], "exclude");
    for reviewer in [&t.r1, &t.r2] {
        send_return(reviewer, &t.owner);
    }
    let r1 = t.r1.me().actor_id;
    let filter = |change: &dyn Fn(&mut RecordFilter)| {
        let mut f = RecordFilter::default();
        change(&mut f);
        f
    };
    let decision = |d: &str| filter(&|f| f.decision = Some(d.to_string()));

    // Verblindet: nur die eigene Stimme, keine Stimmenzahl, kein Status.
    let all = t.r1.stage_records(&stage, &RecordFilter::default()).unwrap();
    assert_eq!((all.total, all.items.len()), (4, 4));
    assert_eq!(all.locked, ["conflict", "person", "votes"]);
    let mine: Vec<Option<&str>> = all.items.iter().map(|i| i.my_decision.as_deref()).collect();
    assert_eq!(mine, [Some("include"), Some("exclude"), None, None]);
    assert!(all.items.iter().all(|i| i.n_votes.is_none() && i.status.is_none() && i.decision.is_none()));
    let counts: Vec<(&str, &str, usize)> =
        all.keywords.iter().map(|k| (k.keyword.as_str(), k.kind.as_str(), k.n)).collect();
    assert_eq!(counts, [("headache", "include", 1), ("migraine", "include", 1), ("review", "exclude", 1)]);
    assert_eq!(t.r1.stage_records(&stage, &decision("open")).unwrap().total, 2);
    assert_eq!(t.r1.stage_records(&stage, &decision("exclude")).unwrap().items[0].id.to_string(), records[1]);
    assert_eq!(t.r1.stage_records(&stage, &filter(&|f| f.keyword = Some("Migraine".into()))).unwrap().total, 1);
    assert_eq!(t.r1.stage_records(&stage, &filter(&|f| f.query = Some("neurology".into()))).unwrap().total, 1);
    let page = t.r1.stage_records(&stage, &filter(&|f| (f.offset, f.limit) = (1, Some(2)))).unwrap();
    assert_eq!((page.total, page.items.len(), page.items[0].id.to_string()), (4, 2, records[1].clone()));
    assert!(matches!(t.r1.stage_records(&stage, &decision("vielleicht")), Err(Error::Invalid(_))));

    // Die Sperre sitzt im Kern und gilt auch bei der Leitung, die alle Stimmen auf dem Rechner hat.
    let locked = [
        decision("conflict"),
        filter(&|f| f.person = Some(r1)),
        filter(&|f| f.votes_min = Some(1)),
        filter(&|f| f.votes_max = Some(0)),
    ];
    for f in &locked {
        let err = t.owner.stage_records(&stage, f).unwrap_err();
        assert!(matches!(err, Error::Forbidden(_)), "{err}");
        assert!(err.to_string().contains("verblindet"), "{err}");
    }
    let own = t.r1.stage_records(&stage, &filter(&|f| f.person = Some(r1))).unwrap();
    assert_eq!(own.total, 2, "die eigene Person geht immer");

    t.owner.unblind_stage(&stage).unwrap();
    let all = t.owner.stage_records(&stage, &RecordFilter::default()).unwrap();
    assert!(all.locked.is_empty());
    let rows: Vec<(Option<usize>, Option<&str>)> = all.items.iter().map(|i| (i.n_votes, i.status.as_deref())).collect();
    assert_eq!(
        rows,
        [
            (Some(2), Some("disagreement")),
            (Some(1), Some("pending")),
            (Some(0), Some("pending")),
            (Some(0), Some("pending"))
        ]
    );
    let totals: Vec<usize> = locked.iter().map(|f| t.owner.stage_records(&stage, f).unwrap().total).collect();
    assert_eq!(totals, [1, 2, 2, 2], "Konflikt, Stimmen von r1, mindestens eine Stimme, keine Stimme");
    let r1_excluded = filter(&|f| (f.person, f.decision) = (Some(r1), Some("exclude".into())));
    assert_eq!(t.owner.stage_records(&stage, &r1_excluded).unwrap().items[0].id.to_string(), records[1]);

    // Ein Eintrag mit der eigenen Stimme, um sie zu ändern.
    let mine = t.r1.stage_record(&stage, &records[0]).unwrap();
    assert_eq!(mine.my_vote.map(|v| v.decision).as_deref(), Some("include"));
    assert!(t.r1.stage_record(&stage, &records[2]).unwrap().my_vote.is_none());
    assert!(matches!(t.r1.stage_record(&stage, &Uuid::new_v4().to_string()), Err(Error::NotFound(_))));
}

/// Entscheidung über ein vorgeschlagenes Paar: Dublette mit dem vorgeschlagenen Eintrag oder keine.
fn decide(pair: &ort_core::service::DuplicatePair, duplicate: bool) -> PairDecision {
    PairDecision {
        a: pair.a.id,
        b: pair.b.id,
        decision: if duplicate { "duplicate" } else { "distinct" }.into(),
        keep: duplicate.then_some(pair.keep),
    }
}

#[test]
fn open_duplicate_candidates_lock_the_pilot_until_each_pair_is_decided() {
    let t = jbi_team_with_protocol();
    assert_eq!(states(&t.owner)["import"], "done");
    let batch = t.owner.import_file("embase.ris", SECOND_SOURCE, "Embase").unwrap();
    assert_eq!((batch.n_records, batch.n_duplicates), (3, 0), "der exakte Abgleich erkennt nichts");

    let s = states(&t.owner);
    assert_eq!((s["import"], s["pilot_title_abstract"]), ("running", "locked"));
    let check = station(&t.owner, "import").checks.into_iter().find(|c| c.check == "no_unresolved_duplicates").unwrap();
    assert_eq!((check.passed, check.detail.as_str()), (Some(false), "3 offen"));

    let list = t.owner.duplicate_candidates().unwrap();
    assert_eq!((list.total, list.n_exact, list.n_resolved, list.n_distinct), (3, 2, 0, 0));
    let similar = list.pairs.iter().find(|p| p.similarity < 100).unwrap();
    assert!(similar.a.title.starts_with("Large language") && similar.b.title.starts_with("Large language"));
    assert_eq!(list.pairs.iter().filter(|p| p.similarity == 100).count(), 2);
    for pair in &list.pairs {
        // Vorschlag: der Eintrag mit DOI bleibt; er stammt aus dem ersten Import.
        let kept = if pair.keep == pair.a.id { (&pair.a, &pair.a_source) } else { (&pair.b, &pair.b_source) };
        assert!(!kept.0.doi.is_empty() && kept.1 == "Test", "{}", kept.0.title);
    }

    let decisions: Vec<PairDecision> = list.pairs.iter().map(|p| decide(p, p.similarity == 100)).collect();
    assert!(matches!(t.r1.resolve_duplicates(&decisions), Err(Error::Forbidden(_))));
    assert!(matches!(t.owner.resolve_duplicates(&[]), Err(Error::Invalid(_))));
    assert_eq!(t.owner.resolve_duplicates(&decisions).unwrap(), 3);

    let list = t.owner.duplicate_candidates().unwrap();
    assert_eq!((list.total, list.n_exact, list.n_resolved, list.n_distinct), (0, 2, 2, 1));
    let detail = t.owner.detail().unwrap();
    assert_eq!((detail.n_records, detail.n_duplicates), (9, 4));
    assert_eq!(t.owner.records(None, false).unwrap().len(), 5);
    let s = states(&t.owner);
    assert_eq!((s["import"], s["pilot_title_abstract"]), ("done", "open"));

    // Der Pilot enthält die fünf Einträge, die übrig sind: vier von vorher und den ähnlichen, aber eigenen.
    let pilot = open(&t, "pilot_title_abstract", |_| {});
    assert_eq!(pilot.n_records, 5);
    let removed: Vec<Uuid> =
        decisions.iter().filter(|d| d.keep.is_some()).map(|d| if d.keep == Some(d.a) { d.b } else { d.a }).collect();
    let in_stage = order_of(&t.owner, &pilot.id);
    assert!(removed.iter().all(|id| !in_stage.contains(&id.to_string())));

    // Eine spätere Entscheidung zum selben Paar ersetzt die frühere. Die offene Stufe bleibt, wie sie ist.
    let back = decisions.iter().find(|d| d.keep.is_some()).unwrap();
    let back = PairDecision { decision: "distinct".into(), keep: None, ..back.clone() };
    t.owner.resolve_duplicates(&[back]).unwrap();
    let list = t.owner.duplicate_candidates().unwrap();
    assert_eq!((list.total, list.n_resolved, list.n_distinct), (0, 1, 2));
    assert_eq!(order_of(&t.owner, &pilot.id), in_stage);
    send_package(&t.owner, &t.viewer);
    assert_eq!(t.viewer.records(None, false).unwrap().len(), 6);
}

#[test]
fn new_stages_leave_out_decided_duplicates_and_open_stages_stay() {
    let t = Team::new();
    let first = open(&t, "screening", |_| {});
    assert_eq!(first.n_records, 4);
    let before = order_of(&t.owner, &first.id);

    // Erst nach dem Öffnen kommt eine zweite Quelle dazu. Ein Eintrag der offenen Stufe erweist sich
    // als Dublette; hier bleibt ausdrücklich der neue.
    t.owner.import_file("embase.ris", SECOND_SOURCE, "Embase").unwrap();
    let list = t.owner.duplicate_candidates().unwrap();
    let pair = list.pairs.iter().find(|p| p.a.title.starts_with("Knowledge")).unwrap();
    let (old, new) =
        if before.contains(&pair.a.id.to_string()) { (pair.a.id, pair.b.id) } else { (pair.b.id, pair.a.id) };
    let decision = PairDecision { a: pair.a.id, b: pair.b.id, decision: "duplicate".into(), keep: Some(new) };
    // Wer bleibt, muss einer der beiden sein; ein Eintrag ist nicht seine eigene Dublette.
    let wrong = PairDecision { keep: Some(Uuid::new_v4()), ..decision.clone() };
    assert!(matches!(t.owner.resolve_duplicates(&[wrong]), Err(Error::Invalid(_))));
    let same = PairDecision { b: pair.a.id, keep: Some(pair.a.id), ..decision.clone() };
    assert!(matches!(t.owner.resolve_duplicates(&[same]), Err(Error::Invalid(_))));
    t.owner.resolve_duplicates(&[decision]).unwrap();

    assert_eq!(order_of(&t.owner, &first.id), before, "die offene Stufe ändert sich nicht");
    let second = open(&t, "screening", |p| p.name = "Zweiter Durchgang".into());
    let ids = order_of(&t.owner, &second.id);
    assert_eq!(ids.len(), 6, "vier von vorher, drei neue, einer entfernt");
    assert!(ids.contains(&new.to_string()) && !ids.contains(&old.to_string()));

    // Auch die Folgestufe lässt den entfernten Eintrag weg, obwohl er in der Quellstufe eingeschlossen wurde.
    vote_all(&t, &first.id, &["include"; 4], &["include"; 4]);
    if first.blind_active {
        t.owner.unblind_stage(&first.id).unwrap();
    }
    let follow = open(&t, "screening", |p| {
        (p.name, p.scope, p.source_stage) = ("Volltext".into(), "from_stage".into(), Some(first.id));
    });
    let ids = order_of(&t.owner, &follow.id);
    assert_eq!(ids.len(), 3);
    assert!(!ids.contains(&old.to_string()));
    assert_eq!(t.owner.final_decisions(&first.id).unwrap().len(), 4, "die Stimmen der ersten Stufe bleiben");
}

#[test]
fn fulltext_stage_takes_only_retrieved_records_and_latecomers_run_as_a_further_round() {
    let t = team_before_fulltext();
    let needed = t.owner.fulltext_overview().unwrap();
    assert_eq!((needed.needed, needed.retrieved, needed.unavailable, needed.rows.len()), (4, 0, 0, 4));
    assert_eq!(states(&t.owner)["fulltext"], "locked");
    assert!(t.owner.fulltext_suggestions().unwrap_err().to_string().contains("kein Ordner"));

    // Zwei Dateien liegen im Ordner; die App schlägt die Zuordnung über den Titel im Namen vor.
    let folder = t.dir.path().join("volltexte");
    std::fs::create_dir_all(folder.join("unter")).unwrap();
    let rows = &needed.rows;
    std::fs::write(folder.join(format!("Autor - 2020 - {}.pdf", rows[0].title)), "%PDF eins").unwrap();
    std::fs::write(folder.join(format!("unter/{}.pdf", rows[1].title)), "%PDF zwei").unwrap();
    std::fs::write(folder.join("Einkaufszettel.pdf"), "%PDF nichts").unwrap();
    std::fs::write(folder.join("notizen.txt"), "keine PDF").unwrap();
    t.owner.set_fulltext_folder(Some(&folder)).unwrap();
    let found = t.owner.fulltext_suggestions().unwrap();
    let mut matched: Vec<_> = found.iter().map(|s| (s.record_id, s.basis)).collect();
    matched.sort();
    let mut expected = vec![(rows[0].record_id, "title"), (rows[1].record_id, "title")];
    expected.sort();
    assert_eq!(matched, expected);
    // Bestätigen ist ein eigener Schritt; r1 darf das, die Person mit Leserecht nicht.
    assert_eq!(t.owner.fulltext_overview().unwrap().retrieved, 0);
    let list: Vec<_> = found.iter().map(|s| Assign { record_id: s.record_id, path: s.path.clone() }).collect();
    assert!(matches!(t.viewer.set_fulltext_unavailable(&rows[2].record_id, "x"), Err(Error::Forbidden(_))));
    t.owner.assign_fulltexts(&list).unwrap();
    assert!(
        t.owner.fulltext_suggestions().unwrap().is_empty(),
        "zugeordnete Dateien werden nicht erneut vorgeschlagen"
    );
    assert!(matches!(t.owner.set_fulltext_unavailable(&rows[2].record_id, " "), Err(Error::Invalid(_))));
    send_package(&t.owner, &t.r1);
    t.r1.set_fulltext_unavailable(&rows[2].record_id, "Fernleihe erfolglos, Autorin antwortet nicht").unwrap();
    send_return(&t.r1, &t.owner);

    // Ein Eintrag ist offen: Das Gate hält, die Zahlen stehen an der Station.
    let view = station(&t.owner, "fulltext_retrieval");
    assert_eq!(view.state, "open");
    assert_eq!(view.checks[0].passed, Some(false));
    assert_eq!(view.checks[0].detail, "2 von 4 beschafft, 1 nicht beschaffbar");
    assert!(matches!(t.owner.open_stage(&t.owner.stage_defaults("fulltext").unwrap()), Err(Error::Conflict(_))));
    t.owner.override_gate("fulltext_retrieval", "Ein Volltext ist per Fernleihe unterwegs").unwrap();

    // Die Stufe enthält nur die beschafften Einträge.
    let first = open(&t, "fulltext", |_| {});
    assert_eq!((first.n_records, first.round), (2, 1));
    let mut assigned = order_of(&t.owner, &first.id);
    assigned.sort();
    let mut retrieved = vec![rows[0].record_id.to_string(), rows[1].record_id.to_string()];
    retrieved.sort();
    assert_eq!(assigned, retrieved);
    let detail = t.r1.stage_record(&first.id, &assigned[0]).unwrap();
    assert_eq!(detail.fulltext.map(|f| f.status), Some("retrieved".to_string()));
    for reviewer in [&t.r1, &t.r2] {
        for record in &assigned {
            vote(reviewer, &first.id, record, "include");
        }
        send_return(reviewer, &t.owner);
    }
    t.owner.unblind_stage(&first.id).unwrap();
    t.owner.close_stage(&first.id).unwrap();
    assert!(t
        .owner
        .open_stage(&t.owner.stage_defaults("fulltext").unwrap())
        .unwrap_err()
        .to_string()
        .contains("kein weiterer"));

    // Der Nachzügler kommt und läuft als zweite Runde, allein.
    std::fs::write(folder.join("nachzuegler.pdf"), "%PDF vier").unwrap();
    t.owner.assign_fulltexts(&[Assign { record_id: rows[3].record_id, path: "nachzuegler.pdf".into() }]).unwrap();
    let second = open(&t, "fulltext", |_| {});
    assert_eq!((second.n_records, second.round), (1, 2));
    assert_eq!(order_of(&t.owner, &second.id), [rows[3].record_id.to_string()]);
    let after = t.owner.fulltext_overview().unwrap();
    assert_eq!((after.needed, after.retrieved, after.unavailable), (4, 3, 1));
    // Das Paket zur zweiten Runde bringt auch den neuen Stand der Volltexte mit.
    assert_eq!(station(&t.r2, "fulltext_retrieval").checks[0].passed, Some(true));

    // Zurücknehmen öffnet den Eintrag wieder; bestehende Stufen bleiben, wie sie sind.
    t.owner.reset_fulltext(&rows[2].record_id).unwrap();
    assert_eq!(t.owner.fulltext_overview().unwrap().unavailable, 0);
    assert_eq!(order_of(&t.owner, &second.id).len(), 1);
}

#[test]
fn a_swapped_or_missing_file_is_caught_by_its_checksum() {
    let t = team_before_fulltext();
    let folder = retrieve_fulltexts(&t);
    let overview = t.owner.fulltext_overview().unwrap();
    assert!(overview.rows.iter().all(|r| r.file == "ok"));
    let row = &overview.rows[0];
    let path = folder.join(&row.fulltext.as_ref().unwrap().path);
    let data = t.owner.read_fulltext(&row.record_id, None).unwrap();
    assert_eq!(data, std::fs::read(&path).unwrap());
    assert!(t.owner.verify_fulltexts().unwrap().is_empty());

    // Jemand ersetzt die Datei durch eine andere gleichen Namens.
    std::fs::write(&path, "%PDF ein ganz anderer Artikel").unwrap();
    assert!(matches!(t.owner.read_fulltext(&row.record_id, None), Err(Error::Integrity(_))));
    let problems = t.owner.verify_fulltexts().unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!((problems[0].record_id, problems[0].problem), (row.record_id, "changed"));
    // Neu zuordnen ist eine bewusste Entscheidung und steht als neues Ereignis im Log.
    t.owner.assign_fulltexts(&[Assign { record_id: row.record_id, path: path.to_string_lossy().to_string() }]).unwrap();
    assert!(t.owner.read_fulltext(&row.record_id, None).is_ok());

    std::fs::remove_file(&path).unwrap();
    assert!(matches!(t.owner.read_fulltext(&row.record_id, None), Err(Error::NotFound(_))));
    assert_eq!(t.owner.verify_fulltexts().unwrap()[0].problem, "missing");
    assert_eq!(t.owner.fulltext_overview().unwrap().rows[0].file, "missing");

    // Nichts außerhalb des Ordners lässt sich zuordnen oder lesen.
    let outside = t.dir.path().join("draussen.pdf");
    std::fs::write(&outside, "%PDF draußen").unwrap();
    for bad in [outside.to_string_lossy().to_string(), "../draussen.pdf".to_string()] {
        let result = t.owner.assign_fulltexts(&[Assign { record_id: row.record_id, path: bad }]);
        assert!(result.unwrap_err().to_string().contains("nicht im verknüpften Ordner"));
    }

    // Jede Person hat ihren eigenen Ort für den gemeinsamen Ordner; im Log steht nur der relative Pfad.
    send_package(&t.owner, &t.r1);
    let other = &overview.rows[1];
    assert!(t.r1.read_fulltext(&other.record_id, None).unwrap_err().to_string().contains("kein Ordner"));
    let mirror = t.dir.path().join("spiegel");
    std::fs::create_dir_all(&mirror).unwrap();
    let name = &other.fulltext.as_ref().unwrap().path;
    std::fs::copy(folder.join(name), mirror.join(name)).unwrap();
    t.r1.set_fulltext_folder(Some(&mirror)).unwrap();
    assert!(t.r1.read_fulltext(&other.record_id, None).is_ok());
    assert!(!t.r1.events().unwrap().iter().any(|e| e.payload.to_string().contains("spiegel")));
}

/// Zahlen einer Ebene des Flussdiagramms: Eingang, geprüft, eingeschlossen, ausgeschlossen, offen.
fn level_numbers(level: &Level) -> (i64, i64, i64, i64, i64) {
    (level.input, level.assessed, level.included, level.excluded, level.open)
}

#[test]
fn report_adds_up_at_every_step_and_keeps_blind_decisions_out() {
    let t = team_before_fulltext();
    // Vor dem Volltext: Die Ebene steht schon da und zeigt, was auf die Beschaffung wartet.
    let report = t.owner.report().unwrap();
    let flow = &report.flow;
    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!((flow.identified, flow.exact_duplicates, flow.resolved_duplicates, flow.unique), (6, 2, 0, 4));
    assert_eq!(flow.levels.iter().map(|l| l.key.as_str()).collect::<Vec<_>>(), ["title_abstract", "fulltext"]);
    assert_eq!(level_numbers(&flow.levels[0]), (4, 4, 4, 0, 0));
    assert_eq!(level_numbers(&flow.levels[1]), (4, 0, 0, 0, 0));
    assert_eq!(flow.levels[1].retrieval, Some(Retrieval { sought: 4, unavailable: 0, open: 4, waiting: 0 }));
    assert_eq!((flow.included, flow.complete), (Some(0), false));
    assert!(report.svg.contains("Volltext gesucht: 4") && report.svg.contains("Beschaffung offen: 4"));
    assert!(report.svg.contains("vorläufig"));

    // Drei Volltexte liegen vor, einer ist nicht beschaffbar.
    let rows = t.owner.fulltext_overview().unwrap().rows;
    t.owner.set_fulltext_unavailable(&rows[3].record_id, "Fernleihe erfolglos").unwrap();
    retrieve_fulltexts(&t);
    let waiting = t.owner.report().unwrap().flow.levels[1].retrieval.clone();
    assert_eq!(waiting, Some(Retrieval { sought: 4, unavailable: 1, open: 0, waiting: 3 }));

    // Volltext: einmal einig drin, einmal einig draußen mit verschiedenen Gründen, einmal uneins.
    let stage = open(&t, "fulltext", |_| {});
    let records = order_of(&t.owner, &stage.id);
    let (a, b, c) = (records[0].as_str(), records[1].as_str(), records[2].as_str());
    type Votes<'a> = [(&'a str, Option<&'a str>); 3];
    let plan: [(&Review, Votes); 2] = [
        (&t.r1, [("include", None), ("exclude", Some("E2")), ("include", None)]),
        (&t.r2, [("include", None), ("exclude", Some("E1")), ("exclude", Some("E2"))]),
    ];
    for (reviewer, votes) in plan {
        for (record, (decision, reason)) in [a, b, c].into_iter().zip(votes) {
            reviewer.cast_vote(&stage.id, record, decision, reason, &[], "").unwrap();
        }
        send_return(reviewer, &t.owner);
    }
    // Solange die Stufe verblindet ist, zählt alles als offen, auch bei der Leitung.
    for review in [&t.owner, &t.r1] {
        let report = review.report().unwrap();
        let level = &report.flow.levels[1];
        assert_eq!(report.problems, Vec::<String>::new());
        assert_eq!((level_numbers(level), level.blinded), ((4, 3, 0, 0, 3), 3));
        assert!(level.reasons.is_empty());
        assert!(report.methods.contains("noch verblindet"));
        assert!(!report.methods.contains("Ausschlussgründe"));
        assert_eq!(review.report_file("excluded").unwrap().content.lines().count(), 2, "nur der nicht beschaffte");
        assert_eq!(review.report_file("included").unwrap().content, "");
    }

    // Nach dem Entblinden ist ein Eintrag im Konflikt; die Klärung schließt ihn mit Grund aus.
    t.owner.unblind_stage(&stage.id).unwrap();
    let open_conflict = t.owner.report().unwrap();
    assert_eq!(level_numbers(&open_conflict.flow.levels[1]), (4, 3, 1, 1, 1));
    assert_eq!(open_conflict.problems, Vec::<String>::new());
    t.owner.record_consensus(&stage.id, c, "exclude", "", Some("E2"), &[], "").unwrap();
    assert!(t.owner.close_stage(&stage.id).unwrap().gate.unwrap().passed);

    send_package(&t.owner, &t.viewer);
    for review in [&t.owner, &t.viewer] {
        let report = review.report().unwrap();
        let flow = &report.flow;
        assert_eq!(report.problems, Vec::<String>::new());
        let level = &flow.levels[1];
        assert_eq!(level_numbers(level), (4, 3, 1, 2, 0));
        assert_eq!(level.retrieval, Some(Retrieval { sought: 4, unavailable: 1, open: 0, waiting: 0 }));
        // Verschiedene Gründe ohne Klärung: Es gilt der, der in den Kriterien zuerst steht.
        let reasons: Vec<_> = level.reasons.iter().map(|r| (r.code.as_str(), r.text.as_str(), r.n)).collect();
        assert_eq!(reasons, [("E1", "Falsche Population", 1), ("E2", "Keine Primärstudie", 1)]);
        assert_eq!((flow.included, flow.complete), (Some(1), true));
        for text in [
            "Identifiziert: 6 Einträge",
            "Test: 6",
            "Vor der Auswahl entfernt: 2",
            "Titel/Abstract: 4 geprüft",
            "Volltext gesucht: 4",
            "nicht beschaffbar: 1",
            "Volltext-Screening: 3 geprüft",
            "Ausgeschlossen: 2",
            "E1 Falsche Population: 1",
            "Eingeschlossen: 1 Quelle<",
        ] {
            assert!(report.svg.contains(text), "{text} fehlt im Diagramm");
        }
        assert!(!report.svg.contains("vorläufig"));
        assert_eq!(report.n_excluded_rows, 3);
    }

    // Die Anhänge: Ausschlüsse mit Grund, Eingeschlossene als RIS, Suche, Methoden.
    let file = |kind: &str| t.viewer.report_file(kind).unwrap();
    let csv = file("excluded");
    assert_eq!(csv.filename, "ausgeschlossene-volltexte.csv");
    let lines: Vec<&str> = csv.content.lines().collect();
    assert_eq!(lines.len(), 4);
    let line_of = |id: &str| lines.iter().find(|l| l.starts_with(id)).unwrap().to_string();
    assert!(line_of(b).ends_with("Volltext-Screening,excluded,E1,Falsche Population"));
    assert!(line_of(c).ends_with("Volltext-Screening,excluded,E2,Keine Primärstudie"));
    assert!(line_of(&rows[3].record_id.to_string()).ends_with("not_retrieved,,Fernleihe erfolglos"));
    let included = ort_core::import::parse_ris(&file("included").content);
    let title_of_a = rows.iter().find(|r| r.record_id.to_string() == a).unwrap().title.clone();
    assert_eq!(included.iter().map(|i| i.title.clone()).collect::<Vec<_>>(), [title_of_a]);
    let appendix = file("search").content;
    assert!(appendix.contains("## MEDLINE (Ovid)") && appendix.contains("(headache OR migraine) AND adult"));
    assert!(appendix.contains("Tag der Suche: 19.11.2026"));
    assert!(appendix.contains("Importe ohne festgehaltene Suche"), "die Testimporte nennen keine Suche");
    let methods = file("methods").content;
    for text in [
        "Vorlage „JBI Scoping Review“ (Version 1)",
        "## Titel/Abstract",
        "mindestens 2 Personen unabhängig bewertet",
        "Übereinstimmung vor der Klärung: 100,0 % (4 von 4",
        "## Volltext-Screening",
        "Übereinstimmung vor der Klärung: 66,7 % (2 von 3",
        "Über 1 Eintrag wurde in der Klärung entschieden",
        "1 war nicht beschaffbar",
        "- E1 Falsche Population: 1",
        "„Pilot Titel/Abstract“ wurde von der Leitung ohne erfülltes Gate freigegeben: Pilot entfällt",
        "1 Eintrag wurde eingeschlossen.",
    ] {
        assert!(methods.contains(text), "{text} fehlt in\n{methods}");
    }
    assert!(!methods.contains("vorläufig"));
    assert!(file("diagram").content.starts_with("<svg"));
    assert!(matches!(t.owner.report_file("alles"), Err(Error::Invalid(_))));
}

#[test]
fn abbreviated_selection_is_named_as_a_limitation() {
    let t = Team::new();
    let stage = t.open_stage(|p| {
        p.blind = false;
        p.min_reviewers = 1;
        p.decision_rule = Some("single_verified".into());
        p.verify = Some(Verify { scope: "sample".into(), percent: Some(20), seed: Some(7) });
    });
    let report = t.owner.report().unwrap();
    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!(level_numbers(&report.flow.levels[0]), (4, 4, 0, 0, 4));
    assert_eq!(report.flow.levels[0].key, stage.id.to_string(), "im freien Ablauf ist jede Stufe eine Ebene");
    assert!(report.methods.contains("Das Review lief ohne feste Vorlage."));
    assert!(report.methods.contains("## Abweichungen und Limitationen"));
    assert!(report.methods.contains("eine Zufallsstichprobe von 20 % der Ausschlüsse"));
    assert!(report.methods.contains("Einschlüsse wurden nicht doppelt bewertet"));
    assert!(report.search_appendix.contains("Es ist keine Suche festgehalten."));
}

/// Treffer einer Zitationssuche: Der erste ist schon da (gleiche DOI), zwei sind neu.
const CITATIONS: &str = "TY  - JOUR
AU  - Lee, Min
TI  - Large language models for symptom extraction
PY  - 2020
DO  - 10.1000/llm2020
ER  -

TY  - JOUR
AU  - Novak, Petra
TI  - Two-year outcomes after rule-based triage of migraine
PY  - 2022
DO  - 10.1000/followup
ER  -

TY  - RPRT
AU  - Weber, Ida
TI  - Headache services in rural regions
PY  - 2018
ER  -
";

#[test]
fn citation_search_runs_as_a_further_round_and_bundles_count_once() {
    let t = team_with_fulltexts();
    let close = |stage: &StageView| {
        t.owner.unblind_stage(&stage.id).unwrap();
        assert!(t.owner.close_stage(&stage.id).unwrap().gate.unwrap().passed);
    };
    let ft = open(&t, "fulltext", |_| {});
    vote_all(&t, &ft.id, &["include"; 4], &["include"; 4]);
    close(&ft);
    assert_eq!(states(&t.owner)["bundling"], "locked", "erst die Zitationssuche");
    let check = |station_key: &str, key: &str| {
        station(&t.owner, station_key).checks.into_iter().find(|c| c.check == key).unwrap()
    };
    assert_eq!(check("citation_search", "citation_search_documented").passed, Some(false));

    // Die Zitationssuche wird wie jede Suche festgehalten; ihr Import bringt zwei neue Einträge.
    let found =
        search("Zitationssuche", json!({"origin": "citation", "platform": "", "query": "Literaturverzeichnisse"}));
    let made = t.owner.publish_search(&found).unwrap();
    let from = ImportSource { search_id: Some(made.id), ..ImportSource::default() };
    let batch = t.owner.import_file_with("zitationen.ris", CITATIONS, &from).unwrap();
    assert_eq!((batch.n_records, batch.n_duplicates), (3, 1), "{}", batch.error);
    assert_eq!(check("citation_search", "citation_search_documented").passed, Some(true));
    let unassigned = check("citation_search", "all_records_assigned");
    assert_eq!((unassigned.passed, unassigned.detail.as_str()), (Some(false), "2 ohne Runde"));
    let report = t.owner.report().unwrap();
    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!((report.flow.other_identified, report.flow.levels[0].not_assigned, report.flow.complete), (3, 2, false));

    // Zweiter Durchlauf: Jede Station bekommt nur, was neu ist.
    let ta = open(&t, "title_abstract", |_| {});
    assert_eq!((ta.round, ta.n_records), (2, 2));
    vote_all(&t, &ta.id, &["include"; 2], &["include"; 2]);
    close(&ta);
    assert!(t
        .owner
        .open_stage(&t.owner.stage_defaults("title_abstract").unwrap())
        .unwrap_err()
        .to_string()
        .contains("nichts Neues"));
    assert_eq!(check("fulltext_retrieval", "all_fulltext_status").detail, "4 von 6 beschafft, 0 nicht beschaffbar");
    retrieve_fulltexts(&t);
    let ft2 = open(&t, "fulltext", |_| {});
    assert_eq!((ft2.round, ft2.n_records), (2, 2), "die vier aus Runde 1 liegen nicht noch einmal vor");
    let new = order_of(&t.owner, &ft2.id);
    for reviewer in [&t.r1, &t.r2] {
        reviewer.cast_vote(&ft2.id, &new[0], "include", None, &[], "").unwrap();
        reviewer.cast_vote(&ft2.id, &new[1], "exclude", Some("E1"), &[], "").unwrap();
        send_return(reviewer, &t.owner);
    }
    close(&ft2);
    assert_eq!(states(&t.owner)["citation_search"], "done");

    // Der Zweig „andere Quellen“ im Diagramm.
    let report = t.owner.report().unwrap();
    let flow = &report.flow;
    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!((flow.identified, flow.exact_duplicates, flow.unique, flow.other_identified), (9, 3, 6, 3));
    assert_eq!((level_numbers(&flow.levels[0]), flow.levels[0].rounds), ((6, 6, 6, 0, 0), 2));
    assert_eq!((level_numbers(&flow.levels[1]), flow.levels[1].rounds), ((6, 6, 5, 1, 0), 2));
    assert_eq!((flow.levels[1].other_assessed, flow.levels[1].other_included), (2, 1));
    assert!(flow.complete);
    for text in [
        "Datenbanken und Register: 6",
        "Andere Quellen: 3",
        "Zitationssuche: 3",
        "Identifiziert: 9 Einträge",
        "aus anderen Quellen: 2",
        "Einträge aus anderen Quellen: 1",
        "<polyline",
    ] {
        assert!(report.svg.contains(text), "{text} fehlt im Diagramm");
    }
    assert!(report.methods.contains("Davon stammen 3 aus der Zitationssuche"));

    // Bündeln: Zwei Einträge derselben Studie zählen als eine Quelle.
    let list = t.owner.sources().unwrap();
    assert_eq!((list.n_records, list.n_sources, list.n_without_type, list.complete), (5, 5, 5, true));
    assert_eq!(list.level.as_deref(), Some("Volltext-Screening"));
    let ids: Vec<Uuid> = list.sources.iter().map(|s| s.id).collect();
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    assert!(matches!(t.r1.link_study(None, "", &[a, b], &a), Err(Error::Forbidden(_))));
    assert!(matches!(t.owner.link_study(None, "", &[a], &a), Err(Error::Invalid(_))));
    assert!(matches!(t.owner.link_study(None, "", &[a, b], &c), Err(Error::Invalid(_))));
    let study = t.owner.link_study(None, "Kohorte Prag", &[a, b], &b).unwrap();
    assert!(matches!(t.owner.link_study(None, "", &[b, c], &c), Err(Error::Conflict(_))), "b ist schon gebündelt");
    let list = t.owner.sources().unwrap();
    assert_eq!((list.n_records, list.n_sources), (5, 4));
    let bundle = list.sources.iter().find(|s| s.study_id == Some(study)).unwrap();
    assert_eq!((bundle.id, bundle.label.as_str(), bundle.records.len()), (b, "Kohorte Prag", 2));
    assert!(bundle.records[0].primary && !bundle.records[1].primary);

    // Quellenart: Das Gate verlangt sie für jede Quelle, die Vorlage nennt die Auswahl.
    assert_eq!(check("bundling", "source_types_set").detail, "4 von 4 ohne Quellenart");
    assert!(matches!(t.owner.set_source_type(&ids, "roman"), Err(Error::Invalid(_))));
    assert!(matches!(t.r1.set_source_type(&ids, "review"), Err(Error::Forbidden(_))));
    t.owner.set_source_type(&ids, "primary_study").unwrap();
    t.owner.set_source_type(&[c], "review").unwrap();
    assert_eq!(states(&t.owner)["bundling"], "done");

    // Bericht und Dateien zählen Quellen; alle im Team sehen dasselbe.
    send_package(&t.owner, &t.viewer);
    let report = t.viewer.report().unwrap();
    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!((report.flow.included, report.flow.included_sources), (Some(5), Some(4)));
    let kinds: Vec<(&str, i64)> = report.flow.source_types.iter().map(|k| (k.label.as_str(), k.n)).collect();
    assert_eq!(kinds, [("Primärstudie", 3), ("Review oder Evidenzsynthese", 1)]);
    for text in ["Eingeschlossen: 4 Quellen", "Einträge darin: 5", "Primärstudie: 3"] {
        assert!(report.svg.contains(text), "{text} fehlt im Diagramm");
    }
    assert!(report.methods.contains("Gezählt werden 4 Quellen."));
    let csv = t.viewer.report_file("sources").unwrap();
    assert_eq!(csv.filename, "eingeschlossene-quellen.csv");
    let lines: Vec<&str> = csv.content.lines().skip(1).collect();
    assert_eq!(lines.len(), 5, "je Eintrag eine Zeile");
    let bundled: Vec<&&str> = lines.iter().filter(|l| l.contains("Kohorte Prag")).collect();
    assert_eq!(bundled.len(), 2);
    assert_eq!(bundled[0].split(',').next(), bundled[1].split(',').next(), "dieselbe Nummer der Quelle");
    assert!(bundled[0].contains(",primary,yes,") && bundled[1].contains(",companion,yes,"));
    assert_eq!(t.viewer.report_file("included").unwrap().content.matches("TY  - ").count(), 5);

    // Aufgelöst zählt wieder jeder Eintrag für sich.
    assert!(matches!(t.r1.dissolve_study(&study), Err(Error::Forbidden(_))));
    t.owner.dissolve_study(&study).unwrap();
    assert_eq!(t.owner.sources().unwrap().n_sources, 5);
    assert!(matches!(t.owner.dissolve_study(&study), Err(Error::NotFound(_))));
    assert!(t.owner.verify_chain().unwrap().ok);
}

/// Auswahl abgeschlossen, Zitationssuche übersteuert, jede der vier Quellen hat eine Quellenart:
/// drei Primärstudien und ein Review.
fn team_before_charting() -> Team {
    let t = team_with_fulltexts();
    let ft = open(&t, "fulltext", |_| {});
    vote_all(&t, &ft.id, &["include"; 4], &["include"; 4]);
    t.owner.unblind_stage(&ft.id).unwrap();
    assert!(t.owner.close_stage(&ft.id).unwrap().gate.unwrap().passed);
    t.owner.override_gate("citation_search", "Keine Zitationssuche in diesem Review").unwrap();
    let ids: Vec<Uuid> = t.owner.sources().unwrap().sources.iter().map(|s| s.id).collect();
    t.owner.set_source_type(&ids[..3], "primary_study").unwrap();
    t.owner.set_source_type(&ids[3..], "review").unwrap();
    assert_eq!(states(&t.owner)["bundling"], "done");
    t
}

fn charting_form(extra: serde_json::Value) -> serde_json::Value {
    let mut data = json!({
        "purpose": "charting", "name": "Charting", "change_note": "",
        "fields": [
            {"key": "country", "label": "Land", "type": "text", "required": true},
            {"key": "design", "label": "Design", "type": "choice", "required": true, "options": ["Kohorte", "RCT"]},
            {"key": "n", "label": "Stichprobe", "type": "integer"},
            {"key": "themes", "label": "Themen", "type": "multi_choice", "options": ["Diagnose", "Therapie"]},
        ],
    });
    for (key, value) in extra.as_object().into_iter().flatten() {
        data[key] = value.clone();
    }
    data
}

fn sources_of(review: &Review, stage: &Uuid) -> Vec<String> {
    review.form_stage(stage).unwrap().rows.iter().map(|r| r.record_id.to_string()).collect()
}

fn chart(review: &Review, stage: &Uuid, record: &str, country: &str, design: &str) {
    review.record_form_value(stage, record, "country", &json!(country)).unwrap();
    review.record_form_value(stage, record, "design", &json!(design)).unwrap();
    review.complete_form_entry(stage, record, Some(120)).unwrap();
}

#[test]
fn charting_runs_blind_and_differences_show_only_after_unblinding() {
    let t = team_before_charting();
    // Ohne Formular keine Stufe; nur die Leitung veröffentlicht eines.
    let no_form = t.owner.open_stage(&t.owner.stage_defaults("charting_pilot").unwrap()).unwrap_err();
    assert!(no_form.to_string().contains("kein Formular"), "{no_form}");
    assert_eq!(t.owner.forms().unwrap().starters.len(), 1, "die Vorlage bringt ein Startformular mit");
    assert!(matches!(t.r1.publish_form(&charting_form(json!({}))), Err(Error::Forbidden(_))));
    let form = t.owner.publish_form(&charting_form(json!({}))).unwrap();
    assert_eq!((form.current().version, form.confirmed), (1, false));
    assert!(t.owner.forms().unwrap().starters.is_empty());

    // Pilot: je Quellenart höchstens zwei Quellen, verblindet.
    let pilot = open(&t, "charting_pilot", |_| {});
    assert_eq!((pilot.n_records, pilot.blind_active, pilot.form_version), (3, true, Some(1)));
    assert!(pilot.seed.is_some());
    let records = sources_of(&t.r1, &pilot.id);
    let (a, b, c) = (&records[0], &records[1], &records[2]);
    assert!(matches!(t.r1.cast_vote(&pilot.id, a, "include", None, &[], ""), Err(Error::Invalid(_))));
    assert!(matches!(t.viewer.record_form_value(&pilot.id, a, "country", &json!("CZ")), Err(Error::Forbidden(_))));

    // Werte müssen zum Feld passen; der Abschluss verlangt die Pflichtfelder.
    assert!(matches!(t.r1.record_form_value(&pilot.id, a, "n", &json!("viele")), Err(Error::Invalid(_))));
    assert!(matches!(t.r1.record_form_value(&pilot.id, a, "design", &json!("Fallserie")), Err(Error::Invalid(_))));
    assert!(matches!(t.r1.record_form_value(&pilot.id, a, "gibtsnicht", &json!("x")), Err(Error::Invalid(_))));
    assert!(t.r1.record_form_value(&pilot.id, a, "country", &json!(" Tschechien ")).unwrap());
    assert!(!t.r1.record_form_value(&pilot.id, a, "country", &json!("Tschechien")).unwrap(), "unverändert");
    let missing = t.r1.complete_form_entry(&pilot.id, a, None).unwrap_err();
    assert!(missing.to_string().contains("Pflichtfelder: Design"), "{missing}");
    assert_eq!(t.r1.form_stage(&pilot.id).unwrap().rows[0].mine, "draft");
    t.r1.record_form_value(&pilot.id, a, "n", &json!(120)).unwrap();
    t.r1.record_form_value(&pilot.id, a, "themes", &json!(["Therapie", "Diagnose"])).unwrap();
    chart(&t.r1, &pilot.id, a, "Tschechien", "Kohorte");
    chart(&t.r1, &pilot.id, b, "Polen", "RCT");
    chart(&t.r1, &pilot.id, c, "Ungarn", "RCT");
    // r2 weicht bei einer Quelle in einem Feld ab.
    t.r2.record_form_value(&pilot.id, a, "n", &json!(120)).unwrap();
    t.r2.record_form_value(&pilot.id, a, "themes", &json!(["Diagnose", "Therapie"])).unwrap();
    chart(&t.r2, &pilot.id, a, "Tschechien", "Kohorte");
    chart(&t.r2, &pilot.id, b, "Polen", "Kohorte");
    chart(&t.r2, &pilot.id, c, "Ungarn", "RCT");
    send_return(&t.r1, &t.owner);
    send_return(&t.r2, &t.owner);

    // Verblindet: kein Abgleich, kein Export, keine Klärung; das Paket trägt keine fremden Werte.
    let view = t.owner.form_stage(&pilot.id).unwrap();
    assert!(view.counts.is_none() && view.rows.iter().all(|r| r.status.is_none() && r.n_completed.is_none()));
    assert!(
        view.rows.iter().all(|r| r.cells.iter().all(|cell| cell.value.is_null())),
        "die Leitung hat nichts eingetragen"
    );
    assert!(t.owner.form_entry(&pilot.id, b).unwrap().compared.is_none());
    assert!(matches!(t.owner.form_file(&pilot.id, "table"), Err(Error::Forbidden(_))));
    assert!(matches!(t.owner.form_frequencies(&pilot.id), Err(Error::Forbidden(_))));
    assert!(matches!(t.r1.record_form_consensus(&pilot.id, b, "design", &json!("RCT"), ""), Err(Error::Forbidden(_))));
    send_package(&t.owner, &t.r2);
    let foreign = |review: &Review| {
        let me = review.me().actor_id;
        review
            .events()
            .unwrap()
            .iter()
            .filter(|e| e.event_type.starts_with("Form") && e.stream != "main" && e.actor_id != me)
            .count()
    };
    assert_eq!(foreign(&t.r2), 0);
    assert_eq!(t.r2.form_stage(&pilot.id).unwrap().mine_done, 3);

    // Entblindet: Die Abweichung erscheint mit beiden Werten.
    t.owner.unblind_stage(&pilot.id).unwrap();
    send_package(&t.owner, &t.r1);
    send_package(&t.owner, &t.r2);
    assert!(foreign(&t.r2) > 0);
    let view = t.r2.form_stage(&pilot.id).unwrap();
    let counts = view.counts.unwrap();
    assert_eq!((counts.conflict, counts.done, counts.open), (1, 2, 0));
    assert_eq!(
        view.times.iter().map(|x| (x.source_type.as_str(), x.n, x.seconds)).collect::<Vec<_>>(),
        [("primary_study", 4, 120), ("review", 2, 120)]
    );
    let entry = t.r2.form_entry(&pilot.id, b).unwrap();
    let compared = entry.compared.unwrap();
    let design = compared.cells.iter().find(|cell| cell.field == "design").unwrap();
    assert_eq!((compared.status, design.status), ("conflict", "differs"));
    let mut given: Vec<(&str, &str)> =
        design.values.iter().map(|g| (g.name.as_str(), g.value.as_str().unwrap())).collect();
    given.sort();
    assert_eq!(given, [("r1", "RCT"), ("r2", "Kohorte")]);
    let same = t.r2.form_entry(&pilot.id, a).unwrap().compared.unwrap();
    assert!(same.cells.iter().all(|cell| cell.status == "agreed"), "Listen zählen ohne Reihenfolge");

    // Klärung je Feld.
    t.r1.record_form_consensus(&pilot.id, b, "design", &json!("RCT"), "im Volltext nachgesehen").unwrap();
    send_return(&t.r1, &t.owner);
    let counts = t.owner.form_stage(&pilot.id).unwrap().counts.unwrap();
    assert_eq!((counts.conflict, counts.done), (0, 3));

    // Gate des Pilots ist die Bestätigung des Formulars durch die Leitung.
    assert!(!t.owner.close_stage(&pilot.id).unwrap().gate.unwrap().passed);
    assert_eq!((states(&t.owner)["charting_pilot"], states(&t.owner)["charting"]), ("open", "locked"));
    assert!(matches!(t.r1.confirm_form(&form.id), Err(Error::Forbidden(_))));
    t.owner.confirm_form(&form.id).unwrap();
    assert!(matches!(t.owner.confirm_form(&form.id), Err(Error::Conflict(_))));
    assert_eq!((states(&t.owner)["charting_pilot"], states(&t.owner)["charting"]), ("done", "open"));
    send_package(&t.owner, &t.r1);
    assert!(matches!(t.r1.record_form_value(&pilot.id, a, "country", &json!("CZ")), Err(Error::Conflict(_))));

    // Charting: alle vier Quellen, auch die aus dem Pilot.
    let charting = open(&t, "charting", |_| {});
    assert_eq!((charting.n_records, charting.round), (4, 1));
    let records = sources_of(&t.r1, &charting.id);
    for reviewer in [&t.r1, &t.r2] {
        for record in &records {
            chart(reviewer, &charting.id, record, "Polen", "RCT");
        }
        send_return(reviewer, &t.owner);
    }
    t.owner.unblind_stage(&charting.id).unwrap();
    assert_eq!(t.owner.form_stage(&charting.id).unwrap().counts.unwrap().done, 4);

    // Änderung nach dem Pilot: Begründung Pflicht, der Typ eines Feldes bleibt.
    let with_setting = |note: &str| {
        let mut data = charting_form(json!({"form_id": form.id, "change_note": note}));
        let field = json!({"key": "setting", "label": "Setting", "type": "text", "required": true});
        data["fields"].as_array_mut().unwrap().push(field);
        data
    };
    assert!(matches!(t.owner.publish_form(&with_setting("")), Err(Error::Invalid(_))));
    let mut retyped = with_setting("n als Text");
    retyped["fields"][2]["type"] = json!("text");
    assert!(matches!(t.owner.publish_form(&retyped), Err(Error::Invalid(_))));
    let changed = t.owner.publish_form(&with_setting("Setting fehlte im Pilot")).unwrap();
    let current = changed.current();
    assert_eq!((current.version, current.after_confirmation, current.confirmed), (2, true, false));
    assert_eq!(current.change_note, "Setting fehlte im Pilot");
    assert!(matches!(t.owner.publish_form(&with_setting("noch einmal")), Err(Error::Conflict(_))));

    // Fertige Quellen sind nachzutragen; das Gate hält die Station offen.
    let view = t.owner.form_stage(&charting.id).unwrap();
    assert_eq!(view.counts.unwrap().incomplete, 4);
    assert_eq!(t.owner.gate_preview(&charting.id).unwrap().unwrap().checks[0].numerator, 0);
    for reviewer in [&t.r1, &t.r2] {
        send_package(&t.owner, reviewer);
        let view = reviewer.form_stage(&charting.id).unwrap();
        assert_eq!((view.todo, view.form.version, view.rows[0].mine), (4, 2, "todo"));
        assert_eq!(reviewer.form_entry(&charting.id, &records[0]).unwrap().missing, ["Setting"]);
        for record in &records {
            reviewer.record_form_value(&charting.id, record, "setting", &json!("Hausarztpraxis")).unwrap();
        }
        assert_eq!(reviewer.form_stage(&charting.id).unwrap().todo, 0);
        let report = send_return(reviewer, &t.owner);
        assert_eq!((report.accepted, report.rejected.len()), (4, 0), "{:?}", report.rejected);
    }

    // Tabelle und Häufigkeiten.
    let frequencies = t.owner.form_frequencies(&charting.id).unwrap();
    let design = &frequencies[0];
    assert_eq!((design.label.as_str(), design.n_empty, design.n_open), ("Design", 0, 0));
    assert_eq!(
        design.counts.iter().map(|x| (x.option.as_str(), x.n)).collect::<Vec<_>>(),
        [("Kohorte", 0), ("RCT", 4)]
    );
    assert_eq!(frequencies[1].n_empty, 4, "Themen hat niemand gewählt");
    let table = t.owner.form_file(&charting.id, "table").unwrap();
    assert_eq!(table.filename, "extraktion-tabelle.csv");
    assert!(table.content.lines().next().unwrap().ends_with("status,Land,Design,Stichprobe,Themen,Setting"));
    assert_eq!(table.content.lines().filter(|l| l.contains(",done,Polen,RCT,,,Hausarztpraxis")).count(), 4);
    assert!(t.owner.form_file(&charting.id, "frequencies").unwrap().content.contains("design,Design,RCT,4"));
    // Dasselbe als JSON: Werte unter dem Schlüssel des Feldes, mit ihrem Typ.
    let file = t.owner.form_file(&charting.id, "json").unwrap();
    let data: Value = serde_json::from_str(&file.content).unwrap();
    assert_eq!(file.filename, "extraktion.json");
    assert_eq!(data["form"]["fields"].as_array().unwrap().len(), 5);
    let done: Vec<&Value> = data["sources"].as_array().unwrap().iter().filter(|s| s["status"] == "done").collect();
    assert_eq!(done.len(), 4);
    assert!(done.iter().all(|s| s["values"]["design"] == "RCT" && s["differences"].as_object().unwrap().is_empty()));
    assert_eq!(data["frequencies"][0]["field"], "design");
    assert!(matches!(t.owner.stage_file(&charting.id, "csv"), Err(Error::Invalid(_))), "keine Stimmen im Formular");

    let closed = t.owner.close_stage(&charting.id).unwrap();
    assert!(closed.gate.unwrap().passed);
    assert_eq!((states(&t.owner)["charting"], states(&t.owner)["report"]), ("done", "open"));
    assert_eq!(t.owner.report().unwrap().problems, Vec::<String>::new(), "Formular-Stufen sind keine Ebene");
    for member in [&t.r1, &t.r2, &t.viewer] {
        send_package(&t.owner, member);
        assert_eq!(states(member), states(&t.owner));
        assert!(member.verify_chain().unwrap().ok);
    }
    assert_eq!(t.viewer.form_stage(&charting.id).unwrap().counts.unwrap().done, 4);
}

#[test]
fn update_runs_as_further_rounds_and_the_report_names_protocol_and_deviations() {
    let t = team_before_charting();
    let item = |review: &Review, number: i64| {
        review.report().unwrap().checklist.unwrap().items.into_iter().find(|i| i.number == number).unwrap()
    };
    let kinds = |review: &Review| review.protocol().unwrap().deviations.iter().map(|d| d.kind).collect::<Vec<_>>();

    // Ohne Registrierung stehen nur die übersteuerten Gates in der Liste.
    assert!(t.owner.protocol().unwrap().registration.is_none());
    assert_eq!(kinds(&t.owner), ["gate", "gate"]);
    assert_eq!(
        (item(&t.owner, 5).status, item(&t.owner, 6).status, item(&t.owner, 1).status),
        ("open", "ready", "manual")
    );
    assert_eq!((item(&t.owner, 12).status, item(&t.owner, 15).status), ("optional", "open"));
    assert!(matches!(t.r1.register_protocol("2026-01-10", "OSF", "osf.io/abc12", ""), Err(Error::Forbidden(_))));
    assert!(matches!(t.owner.register_protocol("10.01.2026", "OSF", "", ""), Err(Error::Invalid(_))));
    assert!(matches!(t.owner.register_protocol("2026-01-10", " ", "", ""), Err(Error::Invalid(_))));
    let registered = t.owner.register_protocol("2026-01-10", "OSF", "osf.io/abc12", "").unwrap();
    let first = registered.registration.unwrap();
    assert_eq!((first.registry.as_str(), first.corrected), ("OSF", false));
    assert_eq!(item(&t.owner, 5).status, "ready");

    // Charting ohne Pilot: Formular bestätigen, Pilot mit Begründung übergehen.
    let form = t.owner.publish_form(&charting_form(json!({}))).unwrap();
    t.owner.confirm_form(&form.id).unwrap();
    t.owner.override_gate("charting_pilot", "Formular aus dem Vorprojekt übernommen").unwrap();
    let assigned = |review: &Review| {
        station(review, "charting").checks.into_iter().find(|c| c.check == "all_sources_assigned").unwrap()
    };
    assert_eq!((assigned(&t.owner).passed, assigned(&t.owner).detail.as_str()), (Some(false), "4 ohne Runde"));
    let charting = open(&t, "charting", |_| {});
    let chart_all = |stage: &StageView, country: &str| {
        for reviewer in [&t.r1, &t.r2] {
            for record in sources_of(reviewer, &stage.id) {
                chart(reviewer, &stage.id, &record, country, "RCT");
            }
            send_return(reviewer, &t.owner);
        }
        t.owner.unblind_stage(&stage.id).unwrap();
        assert!(t.owner.close_stage(&stage.id).unwrap().gate.unwrap().passed);
    };
    assert_eq!(
        t.owner.report().unwrap().table,
        ort_core::report::TableInfo { n_sources: 4, n_charted: 0, n_fields: 4 }
    );
    assert!(t.owner.report_file("table").unwrap().content.lines().skip(1).all(|l| l.contains(",blind")));
    chart_all(&charting, "Polen");
    assert_eq!(states(&t.owner)["charting"], "done");
    assert_eq!(t.owner.report().unwrap().table.n_charted, 4);

    // Update: Kriterien bleiben, die Suche wird wiederholt und bringt zwei neue Einträge.
    let medline = t.owner.searches().unwrap()[0].id;
    let again = search(
        "MEDLINE",
        json!({"search_id": medline.to_string(), "searched_on": "2027-03-01", "change_note": "Update nach einem Jahr"}),
    );
    t.owner.publish_search(&again).unwrap();
    let from = ImportSource { search_id: Some(medline), ..ImportSource::default() };
    let batch = t.owner.import_file_with("update.ris", CITATIONS, &from).unwrap();
    assert_eq!((batch.n_records, batch.n_duplicates), (3, 1), "{}", batch.error);
    let close = |stage: &StageView| {
        t.owner.unblind_stage(&stage.id).unwrap();
        assert!(t.owner.close_stage(&stage.id).unwrap().gate.unwrap().passed);
    };
    let ta = open(&t, "title_abstract", |_| {});
    assert_eq!((ta.round, ta.n_records), (2, 2));
    vote_all(&t, &ta.id, &["include"; 2], &["include"; 2]);
    close(&ta);
    retrieve_fulltexts(&t);
    let ft = open(&t, "fulltext", |_| {});
    assert_eq!((ft.round, ft.n_records), (2, 2));
    let new = order_of(&t.owner, &ft.id);
    for reviewer in [&t.r1, &t.r2] {
        reviewer.cast_vote(&ft.id, &new[0], "include", None, &[], "").unwrap();
        reviewer.cast_vote(&ft.id, &new[1], "exclude", Some("E1"), &[], "").unwrap();
        send_return(reviewer, &t.owner);
    }
    close(&ft);

    // Die neue Quelle hält das Charting wieder offen, bis sie in einer Runde liegt.
    t.owner.set_source_type(&[Uuid::parse_str(&new[0]).unwrap()], "primary_study").unwrap();
    assert_eq!((assigned(&t.owner).passed, assigned(&t.owner).detail.as_str()), (Some(false), "1 ohne Runde"));
    assert_ne!(states(&t.owner)["charting"], "done");
    let table = t.owner.report_file("table").unwrap();
    assert_eq!(table.filename, "tabelle-quellen.csv");
    assert_eq!(table.content.lines().filter(|l| l.contains(",not_assigned")).count(), 1);
    let round2 = open(&t, "charting", |_| {});
    assert_eq!((round2.round, round2.n_records), (2, 1));
    chart_all(&round2, "Ungarn");
    assert_eq!((assigned(&t.owner).passed, states(&t.owner)["charting"]), (Some(true), "done"));

    // Tabelle: eine Zeile je Quelle, Werte aus der Runde, in der die Quelle lag.
    let report = t.owner.report().unwrap();
    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!(report.table, ort_core::report::TableInfo { n_sources: 5, n_charted: 5, n_fields: 4 });
    let table = t.owner.report_file("table").unwrap().content;
    assert!(table.lines().next().unwrap().ends_with("doi,status,Land,Design,Stichprobe,Themen"), "{table}");
    assert_eq!(table.lines().filter(|l| l.ends_with(",done,Polen,RCT,,")).count(), 4, "{table}");
    assert_eq!(table.lines().filter(|l| l.ends_with(",done,Ungarn,RCT,,")).count(), 1, "{table}");

    // Abweichungen: was nach der Registrierung neu versioniert wurde, in der Reihenfolge des Logs.
    t.owner.publish_criteria(&criteria("E2 genauer gefasst")).unwrap();
    assert_eq!(kinds(&t.owner), ["gate", "gate", "gate", "search", "criteria"]);
    let corrected = t.owner.register_protocol("2026-01-12", "OSF", "osf.io/abc12", "Datum berichtigt").unwrap();
    let registration = corrected.registration.unwrap();
    assert_eq!((registration.registered_on.as_str(), registration.corrected), ("2026-01-12", true));
    assert_eq!(registration.recorded_at, first.recorded_at, "der Bezugspunkt bleibt der erste Eintrag");
    assert_eq!(corrected.deviations.len(), 5);
    let deviations = t.owner.report_file("deviations").unwrap().content;
    assert!(deviations.contains("Protokoll registriert am 12.01.2026: OSF (osf.io/abc12)."), "{deviations}");
    assert!(deviations
        .contains("Suche in MEDLINE geändert oder wiederholt (Version 2). Begründung: Update nach einem Jahr"));
    assert!(deviations.contains("Kriterien geändert (Version 2). Begründung: E2 genauer gefasst"));

    // Methodentext und Checkliste.
    let methods = t.owner.report_file("methods").unwrap().content;
    for part in [
        "Das Protokoll wurde am 12.01.2026 registriert: OSF (osf.io/abc12).",
        "Die Suche in MEDLINE wurde nach dem 19.11.2026 geändert oder wiederholt, zuletzt am 01.03.2027 (Version 2).",
        "## Charting\n\nExtrahiert wurde mit dem Formular „Charting“ (Version 1).",
        "von mindestens 2 Personen unabhängig extrahiert",
        "Erhoben wurden 4 Angaben:",
        "- Design (Kohorte, RCT)",
        "Runde 1: 4 Quellen, 2 Personen beteiligt. Abgeglichen: 4.",
        "Runde 2: 1 Quelle, 2 Personen beteiligt. Abgeglichen: 1.",
        "Kriterien geändert (Version 2): E2 genauer gefasst",
        "„Charting-Pilot“ wurde von der Leitung ohne erfülltes Gate freigegeben",
    ] {
        assert!(methods.contains(part), "fehlt: {part}\n{methods}");
    }
    assert_eq!(
        (item(&t.owner, 10).status, item(&t.owner, 15).status, item(&t.owner, 20).status),
        ("ready", "ready", "ready")
    );
    let checklist = t.owner.report_file("checklist").unwrap().content;
    assert!(checklist.starts_with("# Checkliste PRISMA-ScR"));
    assert_eq!(checklist.lines().filter(|l| l.starts_with("| ")).count(), 24, "Kopf, Trennzeile und 22 Punkte");

    // Alle sehen dasselbe; eine Registrierung von r1 an der App vorbei wird beim Einlesen abgewiesen.
    for member in [&t.r1, &t.r2, &t.viewer] {
        send_package(&t.owner, member);
        assert_eq!(member.protocol().unwrap(), t.owner.protocol().unwrap());
        assert_eq!(member.report().unwrap().table.n_charted, 5);
    }
    let forged = json!({"registered_on": "2025-01-01", "registry": "PROSPERO", "identifier": "", "note": ""});
    t.r1.store().append(t.r1.me(), "ProtocolRegistered", None, forged).unwrap();
    let report = merge(&t.owner, &wire(&export_return(&t.r1).unwrap())).unwrap();
    assert!(report.rejected.iter().any(|r| r.contains("Owner und Admins")), "{:?}", report.rejected);
    assert_eq!(t.owner.protocol().unwrap().registration.unwrap().registry, "OSF");
}

#[test]
fn registration_is_prepared_from_the_review_and_own_texts_win() {
    let t = jbi_team_with_protocol();
    let find = |draft: &RegistrationDraft, key: &str| draft.items.iter().find(|i| i.item.key == key).unwrap().clone();
    let draft = t.owner.registration_draft().unwrap();
    assert_eq!((draft.items.len(), draft.sections.len(), draft.n_answered), (65, 6, 0));
    assert_eq!(draft.items.iter().map(|i| i.item.number).collect::<Vec<_>>(), (1..=65).collect::<Vec<_>>());
    assert!(draft.can_edit && draft.registration.is_none());

    // Was das Review schon weiß, steht als Vorschlag da; der Rest bleibt offen.
    let title = find(&draft, "title");
    assert_eq!((title.status, title.text()), ("suggested", "Testreview"));
    assert!(find(&draft, "criteria").suggestion.contains("Ausschluss:\n- E1 Falsche Population"));
    assert_eq!(find(&draft, "databases").suggestion, "MEDLINE");
    assert_eq!(find(&draft, "interfaces").suggestion, "- MEDLINE über Ovid");
    assert!(find(&draft, "query_strings").suggestion.contains("(headache OR migraine) AND adult"));
    assert!(find(&draft, "review_type").suggestion.contains("JBI Manual for Evidence Synthesis"));
    assert!(find(&draft, "review_type").suggestion.contains("PRISMA-ScR"));
    assert!(find(&draft, "review_stages").suggestion.starts_with("Protokoll und Team; Suche; Import und Dubletten"));
    let reliability = find(&draft, "screening_reliability").suggestion;
    assert!(reliability.contains("Pilot Titel/Abstract: 2 Personen bewerten jeden Eintrag unabhängig; weiter ab 75 %"));
    assert!(find(&draft, "screening_blinding").suggestion.contains("Titel/Abstract: verblindet"));
    assert!(find(&draft, "exclusion_criteria").suggestion.contains("In „Volltext-Screening“ braucht jeder Ausschluss"));
    assert!(find(&draft, "entities").suggestion.contains("- Land: Land, in dem die Arbeit entstand"));
    assert!(find(&draft, "synthesis_plan").suggestion.contains("- Land: Häufigkeit je Land"));
    assert!(find(&draft, "other_search").suggestion.contains("Zitationssuche"));
    for key in ["background", "primary_questions", "funding", "start_date"] {
        assert_eq!(find(&draft, key).status, "open", "{key}");
    }
    assert_eq!(draft.n_suggested + draft.n_open, 65);

    // Eigene Texte schreibt nur die Leitung; je Feld gilt der letzte, ein leerer nimmt ihn zurück.
    assert!(matches!(t.r1.draft_registration("background", "x"), Err(Error::Forbidden(_))));
    assert!(matches!(t.owner.draft_registration("lieblingsfarbe", "blau"), Err(Error::Invalid(_))));
    let draft = t.owner.draft_registration("background", "  Warum es das Review braucht  ").unwrap();
    assert_eq!(find(&draft, "background").answer.as_deref(), Some("Warum es das Review braucht"));
    t.owner.draft_registration("background", "Warum es das Review braucht").unwrap();
    let drafted = |review: &Review| review.store().events_of_types(&["ProtocolDrafted"]).unwrap().len();
    assert_eq!(drafted(&t.owner), 1, "derselbe Text ist kein neues Ereignis");
    let draft = t.owner.draft_registration("title", "Formalisierung von Kopfschmerz: ein Scoping Review").unwrap();
    let title = find(&draft, "title");
    assert_eq!((title.status, title.suggestion.as_str()), ("answered", "Testreview"));
    assert_eq!((draft.n_answered, draft.updated_at.is_some()), (2, true));
    let markdown = t.owner.registration_markdown().unwrap();
    assert!(markdown.contains("### 1 Title · Titel\n\nFormalisierung von Kopfschmerz: ein Scoping Review\n"));
    assert!(markdown
        .contains("### 21 Databases · Datenbanken\n\n*Vorschlag aus dem Review, noch nicht geprüft:*\n\nMEDLINE\n"));
    assert!(markdown.contains("### 18 Funding · Finanzierung\n\n*Noch offen.*\n"));
    let draft = t.owner.draft_registration("title", "").unwrap();
    assert_eq!((find(&draft, "title").status, draft.n_answered), ("suggested", 1));

    // Das Team liest mit; ein untergeschobener Eintrag eines Reviewers wird beim Einlesen abgewiesen.
    send_package(&t.owner, &t.r1);
    let seen = t.r1.registration_draft().unwrap();
    assert_eq!((find(&seen, "background").status, seen.can_edit), ("answered", false));
    let forged = json!({"form": "osf_gsrrf", "answers": {"background": "Anders"}});
    t.r1.store().append(t.r1.me(), "ProtocolDrafted", None, forged).unwrap();
    let report = send_return(&t.r1, &t.owner);
    assert_eq!(report.rejected.len(), 1, "{:?}", report.rejected);
    assert_eq!(find(&t.owner.registration_draft().unwrap(), "background").text(), "Warum es das Review braucht");

    // Nach dem Einreichen trägt die Leitung die Registrierung ein; der Dialog zeigt sie.
    t.owner.register_protocol("2026-11-20", "OSF", "osf.io/abc12", "").unwrap();
    assert_eq!(t.owner.registration_draft().unwrap().registration.unwrap().registry, "OSF");
    assert!(t.owner.verify_chain().unwrap().ok);
}

#[test]
fn reviews_without_a_checklist_in_the_template_get_none() {
    let t = Team::with_template(Some("free"));
    let report = t.owner.report().unwrap();
    assert!(report.checklist.is_none() && report.protocol.deviations.is_empty());
    assert!(matches!(t.owner.report_file("checklist"), Err(Error::Invalid(_))));
    assert_eq!(report.table, ort_core::report::TableInfo::default());
}

#[test]
fn single_extraction_is_checked_by_a_second_person() {
    let t = team_before_charting();
    t.owner.publish_form(&charting_form(json!({}))).unwrap();
    t.owner.override_gate("charting_pilot", "Formular aus einem früheren Review").unwrap();
    assert!(t
        .owner
        .open_stage(&OpenStage { decision_rule: Some("liberal".into()), ..t.owner.stage_defaults("charting").unwrap() })
        .is_err());
    let stage = open(&t, "charting", |p| {
        p.decision_rule = Some("single_verified".into());
        p.blind = false;
    });
    let record = &sources_of(&t.r1, &stage.id)[0];
    chart(&t.r1, &stage.id, record, "Polen", "RCT");
    send_return(&t.r1, &t.owner);
    assert_eq!(t.owner.form_stage(&stage.id).unwrap().counts.unwrap().extracted, 1);

    // Die zweite Person sieht die Werte, trägt nichts Eigenes ein und korrigiert über geklärte Werte.
    send_package(&t.owner, &t.r2);
    let entry = t.r2.form_entry(&stage.id, record).unwrap();
    assert_eq!((entry.mode, t.r2.form_stage(&stage.id).unwrap().rows[0].mine), ("verify", "to_verify"));
    assert!(matches!(t.r2.record_form_value(&stage.id, record, "country", &json!("CZ")), Err(Error::Conflict(_))));
    t.r2.record_form_consensus(&stage.id, record, "country", &json!("Tschechien"), "Tabelle 1").unwrap();
    t.r2.complete_form_entry(&stage.id, record, None).unwrap();
    // Wer extrahiert hat, kann die eigene Arbeit nicht selbst prüfen.
    t.r1.complete_form_entry(&stage.id, record, None).unwrap();
    assert_eq!(t.r1.form_entry(&stage.id, record).unwrap().compared.unwrap().status, "extracted");
    send_return(&t.r2, &t.owner);
    send_return(&t.r1, &t.owner);
    let compared = t.owner.form_entry(&stage.id, record).unwrap().compared.unwrap();
    assert_eq!(compared.status, "done");
    let country = compared.cells.iter().find(|cell| cell.field == "country").unwrap();
    assert_eq!((country.status, country.value.as_str()), ("consensus", Some("Tschechien")));
    let counts = t.owner.form_stage(&stage.id).unwrap().counts.unwrap();
    assert_eq!((counts.done, counts.open), (1, 3));
    assert!(!t.owner.close_stage(&stage.id).unwrap().gate.unwrap().passed, "drei Quellen fehlen noch");
}

#[test]
fn form_events_are_checked_when_a_file_is_read() {
    let t = team_before_charting();
    let form = t.owner.publish_form(&charting_form(json!({}))).unwrap();
    let pilot = open(&t, "charting_pilot", |_| {});
    send_package(&t.owner, &t.viewer);
    let record = sources_of(&t.r1, &pilot.id)[0].clone();
    let value = |field: &str, value: serde_json::Value| json!({"record_id": record, "field": field, "group_index": 0, "value": value, "form_version": 1});
    let rejected = |from: &Review, event_type: &str, stage: Option<Uuid>, payload: serde_json::Value| {
        from.store().append(from.me(), event_type, stage, payload).unwrap();
        let report = merge(&t.owner, &wire(&export_return(from).unwrap())).unwrap();
        // Eine abgewiesene Kette bleibt stehen und wird bei jedem Einlesen wieder gemeldet.
        report
            .rejected
            .iter()
            .find(|r| r.contains(&ort_core::catalog::label(event_type)))
            .expect("das Ereignis wird abgewiesen")
            .clone()
    };
    // richtige Rolle, passender Wert
    t.r1.record_form_value(&pilot.id, &record, "country", &json!("Polen")).unwrap();
    assert_eq!(send_return(&t.r1, &t.owner).rejected, Vec::<String>::new());
    // falsche Rolle
    assert!(rejected(&t.viewer, "FormValueRecorded", Some(pilot.id), value("country", json!("Polen")))
        .contains("darf nicht abstimmen"));
    let published = charting_form(json!({"form_id": form.id, "version": 2}));
    assert!(rejected(&t.r1, "FormVersionPublished", None, published).contains("Owner und Admins"));
    // Wert passt nicht zum Feld, Stimme in einer Formular-Stufe, Klärung in einer verblindeten Stufe
    assert!(rejected(&t.r1, "FormValueRecorded", Some(pilot.id), value("n", json!("viele"))).contains("ganze Zahl"));
    let vote = json!({"record_id": record, "decision": "include"});
    assert!(rejected(&t.r2, "VoteCast", Some(pilot.id), vote).contains("Formular"));
    assert!(rejected(&t.r2, "FormConsensusRecorded", Some(pilot.id), value("country", json!("Polen")))
        .contains("verblindet"));
    // geschlossene Stufe
    let late = person("spät");
    t.owner.set_member(&late.public(), "reviewer").unwrap();
    let pkg = wire(&ort_core::exchange::export_package(&t.owner, &late.actor_id).unwrap());
    let late = ort_core::exchange::create_from_package(&t.dir.path().join("late.sqlite"), late, &pkg).unwrap().0;
    late.record_form_value(&pilot.id, &record, "country", &json!("Polen")).unwrap();
    t.owner.unblind_stage(&pilot.id).unwrap();
    t.owner.close_stage(&pilot.id).unwrap();
    let report = send_return(&late, &t.owner);
    assert!(report.rejected[0].contains("bereits geschlossen"), "{:?}", report.rejected);
    assert!(t.owner.verify_chain().unwrap().ok);
}

/// Derselbe Ablauf für jede Vorlage: Stationen der Reihe nach, bis alle abgeschlossen sind, die die
/// App bearbeiten kann. Im Test steht kein Schlüssel einer Station.
fn walk(template: &str) -> Team {
    let t = Team::with_template(Some(template));
    let stations = t.owner.stations().unwrap();
    for planned in stations.iter().filter(|s| s.available) {
        let now = station(&t.owner, &planned.key);
        assert_ne!(now.state, "locked", "{template}: {} ist gesperrt durch {:?}", now.label, now.blocked_by);
        match now.kind.as_str() {
            "setup" => {
                match now.tool.as_deref() {
                    Some("protocol") => drop(t.owner.publish_criteria(&criteria("")).unwrap()),
                    Some("search") => drop(t.owner.publish_search(&search("MEDLINE", json!({}))).unwrap()),
                    Some("fulltext") => drop(retrieve_fulltexts(&t)),
                    Some("citation") => {
                        let found = search("Zitationssuche", json!({"origin": "citation", "platform": ""}));
                        t.owner.publish_search(&found).unwrap();
                    }
                    Some("bundling") => {
                        let list = t.owner.sources().unwrap();
                        let ids: Vec<Uuid> = list.sources.iter().map(|s| s.id).collect();
                        t.owner.set_source_type(&ids, &list.source_types[0].key).unwrap();
                    }
                    _ => {}
                }
                if now.blocking {
                    assert_eq!(station(&t.owner, &now.key).state, "done", "{template}: {}", now.label);
                }
            }
            "screening" => {
                let stage = open(&t, &now.key, |_| {});
                assert_eq!(stage.station.as_deref(), Some(now.key.as_str()));
                let n = stage.n_records as usize;
                vote_all(&t, &stage.id, &vec!["include"; n], &vec!["include"; n]);
                if stage.blind_active {
                    t.owner.unblind_stage(&stage.id).unwrap();
                }
                let closed = t.owner.close_stage(&stage.id).unwrap();
                assert_eq!(closed.gate.is_some(), now.blocking);
                assert!(closed.gate.is_none_or(|g| g.passed), "{template}: {}", now.label);
                assert_eq!(station(&t.owner, &now.key).state, "done", "{template}: {}", now.label);
            }
            "form" => {
                if let Some(starter) = t.owner.forms().unwrap().starters.first() {
                    t.owner.publish_form(&serde_json::to_value(starter).unwrap()).unwrap();
                }
                let stage = open(&t, &now.key, |_| {});
                let view = t.owner.form_stage(&stage.id).unwrap();
                for reviewer in [&t.r1, &t.r2] {
                    // Bei einfacher Extraktion mit Prüfung sieht die zweite Person die Arbeit der ersten
                    // und schließt nur ab; verblindet kommt davon nichts an.
                    send_package(&t.owner, reviewer);
                    for row in &view.rows {
                        let record = row.record_id.to_string();
                        let extracts = reviewer.form_entry(&stage.id, &record).unwrap().mode == "extract";
                        for field in view.form.fields.iter().filter(|f| f.required && extracts) {
                            let value = match field.kind.as_str() {
                                "integer" => json!(1),
                                "decimal" => json!("1,5"),
                                "choice" => json!(field.options[0]),
                                "multi_choice" => json!([field.options[0]]),
                                _ => json!("x"),
                            };
                            reviewer.record_form_value(&stage.id, &record, &field.key, &value).unwrap();
                        }
                        reviewer.complete_form_entry(&stage.id, &record, extracts.then_some(60)).unwrap();
                    }
                    send_return(reviewer, &t.owner);
                }
                if stage.blind_active {
                    t.owner.unblind_stage(&stage.id).unwrap();
                }
                if !view.form.confirmed {
                    t.owner.confirm_form(&view.form.id).unwrap();
                }
                let closed = t.owner.close_stage(&stage.id).unwrap();
                assert!(closed.gate.is_none_or(|g| g.passed), "{template}: {}", now.label);
                assert_eq!(station(&t.owner, &now.key).state, "done", "{template}: {}", now.label);
            }
            "report" => {}
            other => panic!("{template}: Station der Art {other} gilt als verfügbar"),
        }
    }
    // Am Ende geht das Flussdiagramm auf, und nichts ist mehr offen.
    let report = t.owner.report().unwrap();
    assert_eq!(report.problems, Vec::<String>::new(), "{template}");
    assert!(report.flow.complete, "{template}: {:?}", report.flow.levels);
    assert_eq!(report.flow.included, Some(4), "{template}");
    for member in [&t.r1, &t.r2, &t.viewer] {
        send_package(&t.owner, member);
        assert_eq!(states(member), states(&t.owner), "alle sehen denselben Stand");
        assert!(member.verify_chain().unwrap().ok);
    }
    let after = t.owner.stations().unwrap();
    assert!(after.iter().filter(|s| s.blocking).all(|s| s.state == "done"));
    assert!(after.iter().filter(|s| s.kind == "screening").all(|s| s.state == "done"));
    t
}

#[test]
fn same_walk_runs_with_the_scoping_template() {
    walk("jbi_scoping");
}

#[test]
fn same_walk_runs_with_the_free_template() {
    walk("free");
}

#[test]
fn rapid_review_runs_with_the_same_parts_and_names_its_shortcuts() {
    let t = walk("rapid_review");
    let stations = t.owner.stations().unwrap();
    assert!(stations.iter().all(|s| s.available), "die Vorlage braucht keinen neuen Baustein");
    assert!(stations.iter().filter(|s| s.kind != "report").all(|s| s.blocking && s.state == "done"));

    // Der Pilot lief mit zwei unabhängigen Stimmen, alles danach mit einer Stimme und Prüfung.
    let stages = t.owner.detail().unwrap().stages;
    let rule = |key: &str| {
        let stage = stages.iter().find(|s| s.station.as_deref() == Some(key)).unwrap();
        (stage.decision_rule.clone(), stage.blind)
    };
    assert_eq!(rule("pilot_title_abstract"), ("unanimous".to_string(), true));
    assert_eq!(rule("extraction_pilot"), ("unanimous".to_string(), true));
    for key in ["title_abstract", "fulltext", "extraction"] {
        assert_eq!(rule(key), ("single_verified".to_string(), false), "{key}");
    }
    let shortcut = OpenStage {
        decision_rule: Some("single_verified".into()),
        blind: false,
        ..t.owner.stage_defaults("pilot_title_abstract").unwrap()
    };
    assert!(matches!(t.owner.open_stage(&shortcut), Err(Error::Invalid(_))), "der Pilot bleibt doppelt");

    // Jede Abkürzung steht bei den Limitationen; die Begriffe kommen aus der Vorlage.
    let report = t.owner.report().unwrap();
    assert!(report.methods.contains("Der Ablauf folgte der Vorlage „Rapid Review“"));
    assert!(report.methods.contains("Für den Übergang waren mindestens 80 % Übereinstimmung verlangt."));
    let limitations = report.methods.split("## Abweichungen und Limitationen").nth(1).unwrap();
    for label in ["Titel/Abstract", "Volltext-Screening", "Datenextraktion und Bewertung"] {
        assert!(limitations.contains(&format!("- In „{label}“")), "{label} fehlt in:\n{limitations}");
    }
    assert!(!limitations.contains("- In „Pilot und doppelte Bewertung“"));
    assert!(limitations.contains("einzelne passende Studien können dadurch fehlen"));
    assert!(limitations.contains("extrahierte nur eine Person jede Studie"));
    assert_eq!((report.flow.sources_term.as_str(), report.table.n_charted), ("Studien", 4));
    assert!(report.checklist.is_none(), "PRISMA-ScR passt nicht; eine eigene Checkliste gibt es noch nicht");
}

#[test]
fn state_of_the_art_review_blocks_nothing_and_one_vote_is_enough() {
    let t = Team::with_template(Some("state_of_the_art"));
    let stations = t.owner.stations().unwrap();
    assert!(stations.iter().all(|s| s.available && !s.blocking && s.state != "locked"));
    // Ohne Kriterien und ohne Suche lässt sich sofort auswählen, mit einer Stimme je Eintrag.
    let stage = open(&t, "title_abstract", |_| {});
    assert_eq!((stage.min_reviewers, stage.blind, stage.decision_rule.as_str()), (1, false, "unanimous"));
    for _ in 0..stage.n_records {
        let record = t.r1.next_record(&stage.id).unwrap().record.unwrap();
        vote(&t.r1, &stage.id, &record.id.to_string(), "include");
    }
    send_return(&t.r1, &t.owner);
    assert!(t.owner.close_stage(&stage.id).unwrap().gate.is_none());
    let report = t.owner.report().unwrap();
    assert_eq!(report.flow.levels[0].included, 4);
    assert!(report.methods.contains("- In „Titel/Abstract“ bewertete nur eine Person jeden Eintrag, ohne Prüfung"));

    // Derselbe Ablauf wie bei den anderen Vorlagen geht bis zur Auswertung durch.
    let t = walk("state_of_the_art");
    let report = t.owner.report().unwrap();
    assert_eq!((report.table.n_charted, report.checklist.is_none()), (4, true));
    assert!(report.methods.contains("Auswertungsformular"));
}

/// Die Sprache gehört der Person, nicht dem Review: Im Log steht die Vorlage immer in der deutschen
/// Fassung, angezeigt wird sie in der Sprache, die gerade gilt.
#[test]
fn language_changes_what_the_app_says_and_never_the_log() {
    use ort_core::i18n;

    // Auf Englisch angelegt: dasselbe Log wie auf Deutsch.
    let t = i18n::with("en", || Team::with_template(Some("jbi_scoping")));
    let logged = t.owner.store().events_of_types(&["WorkflowVersionPublished"]).unwrap();
    let logged = serde_json::to_string(&logged[0].payload).unwrap();
    assert!(logged.contains("Protokoll und Team") && !logged.contains("Protocol and team"), "{logged}");

    let labels = |review: &Review| -> Vec<String> {
        review.detail().unwrap().workflow.stations.iter().map(|s| s.label.clone()).collect()
    };
    let german = labels(&t.owner);
    let english = i18n::with("en", || labels(&t.owner));
    assert_eq!(german[0], "Protokoll und Team");
    assert_eq!(english.len(), german.len());
    // „Charting“ heißt in beiden Sprachen gleich; alles andere ist übersetzt.
    let same: Vec<&String> = german.iter().zip(&english).filter(|(de, en)| de == en).map(|(de, _)| de).collect();
    assert_eq!(same, ["Charting"]);
    let detail = i18n::with("en", || t.owner.detail().unwrap());
    assert_eq!(detail.workflow.terms["records"], "records");

    // Fehlermeldungen, Bericht, Leitlinie und Registrierung folgen der Sprache des Aufrufs.
    let message = |review: &Review| review.draft_registration("lieblingsfarbe", "blau").unwrap_err().to_string();
    let (de, en) = (message(&t.owner), i18n::with("en", || message(&t.owner)));
    assert_ne!(de, en);
    assert!(!en.contains("Feld") && !en.contains('„'), "{en}");

    let report = i18n::with("en", || t.owner.report().unwrap());
    assert!(report.methods.starts_with("# Methods"), "{}", report.methods);
    assert!(report.svg.contains("Identified") && !report.svg.contains("Identifiziert"));
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert!(t.owner.report().unwrap().methods.starts_with("# Methoden"));

    let draft = i18n::with("en", || t.owner.registration_draft().unwrap());
    let stages = draft.items.iter().find(|i| i.item.key == "review_stages").unwrap();
    assert!(stages.suggestion.starts_with(&english[0]), "{}", stages.suggestion);
    assert_eq!(draft.items.len(), t.owner.registration_draft().unwrap().items.len());

    let guideline = |code: &str| i18n::with(code, || ort_core::guidance::guideline("jbi_scoping").unwrap());
    assert_ne!(guideline("en").entries[0].title, guideline("de").entries[0].title);

    // Wer eine Stufe öffnet, bekommt den Namen in der eigenen Sprache vorgeschlagen.
    let name = |code: &str| i18n::with(code, || t.owner.stage_defaults("title_abstract").unwrap().name);
    assert_ne!(name("en"), name("de"));
    assert_eq!(name("de"), "Titel/Abstract");
}

#[test]
fn ai_suggestions_are_noted_stay_blind_and_never_count_as_votes() {
    let t = Team::new();
    t.owner.publish_criteria(&criteria("")).unwrap();
    t.owner.draft_registration("primary_questions", "Welche Formen von Kopfschmerz sind beschrieben?").unwrap();
    let stage = t.open_stage(|_| {}).id;
    for member in [&t.r1, &t.r2] {
        send_package(&t.owner, member);
    }
    let records = order_of(&t.r1, &stage);
    let record = &records[0];
    let title = t.r1.stage_record(&stage, record).unwrap().record.title;
    let ai_events =
        |review: &Review| review.events().unwrap().iter().filter(|e| e.event_type.starts_with("AiSuggestion")).count();

    // Der Prompt entsteht aus den Daten des Reviews: Titel, Fragestellung, Kriterien der Stufe, Eintrag.
    let options = ort_core::ai::Options::default();
    let request = t.r1.ai_request(&stage, record, &options).unwrap();
    let prompt = request.prompt();
    for part in ["Testreview", "Welche Formen von Kopfschmerz", "- E1 Falsche Population", "E1, E2", title.as_str()] {
        assert!(prompt.contains(part), "{part} fehlt in:\n{prompt}");
    }
    assert_eq!(request.prompt_sha256, sha256_hex(prompt.as_bytes()));
    let with_publication = ort_core::ai::Options { publication: true, ..options.clone() };
    assert_ne!(t.r1.ai_request(&stage, record, &with_publication).unwrap().prompt_sha256, request.prompt_sha256);
    let own = ort_core::ai::Options { template: "Kurz: {{record}}\n{{format}}".into(), publication: false };
    let short = t.r1.ai_request(&stage, record, &own).unwrap().prompt();
    assert!(short.starts_with("Kurz: ") && !short.contains("Falsche Population"), "{short}");
    assert_eq!(ai_events(&t.r1), 0, "die Anfrage allein schreibt nichts");

    // Eine unlesbare Antwort wird nicht vermerkt.
    let unreadable = t.r1.record_ai_suggestion(&stage, record, "llama3", &request.prompt_sha256, "Schwer zu sagen.");
    assert!(matches!(unreadable, Err(Error::Invalid(_))));
    assert_eq!(ai_events(&t.r1), 0);

    // Der Vorschlag steht im Log und ist keine Stimme.
    let progress = t.r1.next_record(&stage).unwrap().progress;
    let answer = r#"{"decision": "exclude", "reason": "E1", "rationale": "Es geht um Kinder."}"#;
    let suggestion = t.r1.record_ai_suggestion(&stage, record, "llama3", &request.prompt_sha256, answer).unwrap();
    assert_eq!((suggestion.suggested.as_str(), suggestion.reason.as_deref()), ("exclude", Some("E1")));
    assert_eq!(t.r1.next_record(&stage).unwrap().progress, progress);
    let detail = t.r1.stage_record(&stage, record).unwrap();
    assert_eq!(detail.my_vote, None);
    assert_eq!(detail.my_suggestion, Some(suggestion.clone()));
    assert!(t.r1.my_stage_votes(&stage).unwrap().is_empty());

    // Übernehmen ist ein Vermerk; die Stimme gibt die Person selbst ab.
    let other = Uuid::new_v4();
    assert!(matches!(t.r1.resolve_ai_suggestion(&stage, record, &other, "accepted"), Err(Error::NotFound(_))));
    assert!(matches!(t.r1.resolve_ai_suggestion(&stage, record, &suggestion.id, "vielleicht"), Err(Error::Invalid(_))));
    t.r1.resolve_ai_suggestion(&stage, record, &suggestion.id, "accepted").unwrap();
    assert!(matches!(t.r1.resolve_ai_suggestion(&stage, record, &suggestion.id, "declined"), Err(Error::Conflict(_))));
    t.r1.cast_vote(&stage, record, "exclude", None, &["E1".to_string()], "").unwrap();
    let detail = t.r1.stage_record(&stage, record).unwrap();
    assert_eq!(detail.my_suggestion.unwrap().outcome.as_deref(), Some("accepted"));
    assert_eq!(detail.my_vote.unwrap().decision, "exclude");

    // Ein zweiter Vorschlag zum selben Eintrag ersetzt den ersten in der Ansicht; abgelehnt bleibt er ohne Stimme.
    let second = t.r1.record_ai_suggestion(&stage, &records[1], "llama3", "ab", r#"{"decision": "include"}"#).unwrap();
    t.r1.resolve_ai_suggestion(&stage, &records[1], &second.id, "declined").unwrap();
    assert_eq!(t.r1.stage_record(&stage, &records[1]).unwrap().my_vote, None);
    assert_eq!(ai_events(&t.r1), 4);

    // Wer nur liest, bekommt keinen Vorschlag; ein Eintrag außerhalb der Stufe auch nicht.
    assert!(matches!(t.viewer.ai_request(&stage, record, &options), Err(Error::Forbidden(_))));
    let outside = Uuid::new_v4().to_string();
    assert!(matches!(t.r1.ai_request(&stage, &outside, &options), Err(Error::NotFound(_))));

    // Verblindet: Die Leitung bekommt die Vorschläge mit der Rückmeldung, die zweite Person nicht.
    assert!(send_return(&t.r1, &t.owner).rejected.is_empty());
    assert_eq!(ai_events(&t.owner), 4);
    vote(&t.r2, &stage, record, "include");
    send_return(&t.r2, &t.owner);
    send_package(&t.owner, &t.r2);
    assert_eq!(ai_events(&t.r2), 0);

    // Entblindet: Alle sehen, dass der Stimme ein Vorschlag vorausging.
    t.owner.unblind_stage(&stage).unwrap();
    send_package(&t.owner, &t.r2);
    assert_eq!(ai_events(&t.r2), 4);
    let conflicts = t.r2.conflicts(&stage).unwrap();
    let votes = &conflicts.iter().find(|c| c.record.id.to_string() == *record).unwrap().votes;
    let ai: Vec<(&str, Option<&str>)> = votes.iter().map(|v| (v.name.as_str(), v.ai.as_deref())).collect();
    assert_eq!(ai, [("r1", Some("exclude")), ("r2", None)]);

    // In einer geschlossenen Stufe gibt es keine Vorschläge mehr.
    t.owner.close_stage(&stage).unwrap();
    assert!(matches!(t.owner.ai_request(&stage, record, &options), Err(Error::Conflict(_))));
}

#[test]
fn the_lead_switches_ai_off_and_the_methods_text_names_its_use() {
    let t = Team::new();
    t.owner.publish_criteria(&criteria("")).unwrap();
    let stage = t.open_stage(|_| {}).id;
    for member in [&t.r1, &t.r2] {
        send_package(&t.owner, member);
    }
    let records = order_of(&t.r1, &stage);
    let record = &records[0];
    let options = ort_core::ai::Options::default();
    let methods = |review: &Review| review.report_file("methods").unwrap().content;
    let switches = |review: &Review| review.events().unwrap().iter().filter(|e| e.event_type == "AiPolicySet").count();

    // Ohne Entscheidung der Leitung sind Vorschläge erlaubt. Eine Person holt einen ein und übernimmt ihn.
    assert!(t.owner.detail().unwrap().ai_allowed);
    let answer = r#"{"decision": "exclude", "reason": "E1", "rationale": "Es geht um Kinder."}"#;
    let suggestion = t.r1.record_ai_suggestion(&stage, record, "llama3", "ab", answer).unwrap();
    t.r1.resolve_ai_suggestion(&stage, record, &suggestion.id, "accepted").unwrap();
    t.r1.cast_vote(&stage, record, "exclude", None, &["E1".to_string()], "").unwrap();
    send_return(&t.r1, &t.owner);
    assert!(!methods(&t.owner).contains("KI-Unterstützung"), "verblindet steht nichts im Bericht");

    // Abschalten darf nur die Leitung; dasselbe noch einmal schreibt nichts.
    assert!(matches!(t.r1.set_ai_policy(false), Err(Error::Forbidden(_))));
    t.owner.set_ai_policy(false).unwrap();
    t.owner.set_ai_policy(false).unwrap();
    assert_eq!(switches(&t.owner), 1);
    assert!(!t.owner.detail().unwrap().ai_allowed);
    assert!(matches!(t.owner.ai_request(&stage, record, &options), Err(Error::Forbidden(_))));
    assert!(matches!(t.owner.record_ai_suggestion(&stage, record, "llama3", "ab", answer), Err(Error::Forbidden(_))));

    // Bei der zweiten Person ist die Abschaltung noch nicht angekommen. Ihr Vorschlag wird beim Einlesen
    // nicht abgewiesen, damit ihre Stimmen nicht verloren gehen; der Bericht nennt ihn.
    let late = t.r2.record_ai_suggestion(&stage, record, "qwen", "cd", r#"{"decision": "include"}"#).unwrap();
    t.r2.resolve_ai_suggestion(&stage, record, &late.id, "declined").unwrap();
    vote(&t.r2, &stage, record, "exclude");
    assert!(send_return(&t.r2, &t.owner).rejected.is_empty());

    // Mit dem nächsten Paket gilt die Abschaltung auch dort.
    send_package(&t.owner, &t.r2);
    assert!(!t.r2.detail().unwrap().ai_allowed);
    assert!(matches!(t.r2.ai_request(&stage, record, &options), Err(Error::Forbidden(_))));

    // Entblindet nennt der Methodentext Umfang, Modelle und Ausgang, und was nach dem Abschalten entstand.
    t.owner.unblind_stage(&stage).unwrap();
    let text = methods(&t.owner);
    for part in [
        "KI-Vorschläge hat die Leitung für dieses Review am",
        "Vorschläge einer KI zu 1 Eintrag, eingeholt von 2 Personen (Modell: llama3, qwen)",
        "Übernommen: 1, abgelehnt: 1, ohne Vermerk: 0",
        "Sie können die Entscheidungen beeinflusst haben",
        "nachdem die Leitung sie abgeschaltet hatte: 1",
    ] {
        assert!(text.contains(part), "{part} fehlt in:\n{text}");
    }

    // Wieder erlaubt: Vorschläge gehen wieder, der Satz zur Abschaltung entfällt.
    t.owner.set_ai_policy(true).unwrap();
    assert!(t.owner.ai_request(&stage, record, &options).is_ok());
    assert!(!methods(&t.owner).contains("KI-Vorschläge hat die Leitung"));
}

fn highlight(color: &str, note: &str) -> NewHighlight {
    NewHighlight {
        page: 2,
        rects: vec![[1000, 2000, 5000, 150], [1000, 2150, 3000, 150]],
        text: "Erwachsene mit Migräne".into(),
        color: color.into(),
        note: note.into(),
    }
}

#[test]
fn highlights_in_the_fulltext_stay_blind_and_belong_to_their_author() {
    let t = team_with_fulltexts();
    let stage = open(&t, "fulltext", |_| {}).id;
    let record = Uuid::parse_str(&order_of(&t.r1, &stage)[0]).unwrap();

    // Setzen darf, wer Schreibrecht hat, auf einer Seite, in einer der Farben, an einem beschafften Volltext.
    assert!(matches!(t.viewer.add_highlight(&stage, &record, &highlight("yellow", "")), Err(Error::Forbidden(_))));
    assert!(matches!(t.r1.add_highlight(&stage, &record, &highlight("black", "")), Err(Error::Invalid(_))));
    let outside = NewHighlight { rects: vec![[9000, 0, 2000, 100]], ..highlight("yellow", "") };
    assert!(matches!(t.r1.add_highlight(&stage, &record, &outside), Err(Error::Invalid(_))));
    assert!(matches!(t.r1.add_highlight(&stage, &Uuid::new_v4(), &highlight("yellow", "")), Err(Error::NotFound(_))));
    assert!(t.r1.highlights(&record).unwrap().is_empty());

    let mine = t.r1.add_highlight(&stage, &record, &highlight("yellow", "")).unwrap();
    assert_eq!((mine.mine, mine.editable, mine.stale, mine.info.page), (true, true, false, 2));
    let payload = last_payload(&t.r1, "FullTextHighlightAdded");
    assert_eq!(payload["rects"], json!([[1000, 2000, 5000, 150], [1000, 2150, 3000, 150]]));
    assert_eq!(payload["sha256"], t.r1.fulltext_of(&record).unwrap().unwrap().sha256);

    // Farbe und Kommentar lassen sich ändern; es bleibt eine Markierung mit ihrem ersten Zeitpunkt.
    let changed = t.r1.update_highlight(&stage, &record, &mine.id, "green", " Population passt ").unwrap();
    assert_eq!((changed.info.color.as_str(), changed.info.note.as_str()), ("green", "Population passt"));
    let seen = t.r1.highlights(&record).unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].id, &seen[0].info.at, seen[0].name.as_str()), (mine.id, &mine.info.at, "r1"));
    assert_eq!(seen[0].info.text, "Erwachsene mit Migräne");

    // Verblindet: Bei der Leitung liegen beide Ketten, zu sehen ist für jede Person nur die eigene.
    let other =
        t.r2.add_highlight(&stage, &record, &NewHighlight { page: 1, ..highlight("pink", "Design unklar") }).unwrap();
    for reviewer in [&t.r1, &t.r2] {
        assert_eq!(send_return(reviewer, &t.owner).rejected, Vec::<String>::new());
    }
    assert!(t.owner.highlights(&record).unwrap().is_empty());
    send_package(&t.owner, &t.r1);
    assert_eq!(t.r1.highlights(&record).unwrap().len(), 1);
    // Fremde Markierungen ändert und entfernt niemand.
    assert!(matches!(t.r2.remove_highlight(&stage, &record, &mine.id), Err(Error::NotFound(_))));
    assert!(matches!(t.r2.update_highlight(&stage, &record, &mine.id, "blue", ""), Err(Error::NotFound(_))));

    // Entblindet: alle sehen alle, geordnet nach Seite; bearbeiten lässt sich nur die eigene.
    t.owner.unblind_stage(&stage).unwrap();
    send_package(&t.owner, &t.r1);
    let all = t.r1.highlights(&record).unwrap();
    assert_eq!(
        all.iter().map(|h| (h.id, h.mine, h.editable)).collect::<Vec<_>>(),
        [(other.id, false, false), (mine.id, true, true)]
    );
    assert_eq!(all[0].name, "r2");
    assert_eq!(t.viewer.highlights(&record).unwrap().len(), 0, "die Person mit Leserecht hat noch kein Paket");
    send_package(&t.owner, &t.viewer);
    assert!(t.viewer.highlights(&record).unwrap().iter().all(|h| !h.editable));

    // Eine andere Datei zum selben Eintrag: Die Markierungen gelten als veraltet.
    std::fs::write(t.dir.path().join("volltexte/neu.pdf"), "%PDF-1.4 andere Fassung").unwrap();
    t.owner.assign_fulltexts(&[Assign { record_id: record, path: "neu.pdf".into() }]).unwrap();
    assert!(t.owner.highlights(&record).unwrap().iter().all(|h| h.stale));
    let fresh = t.owner.add_highlight(&stage, &record, &highlight("blue", "")).unwrap();
    assert_eq!(
        t.owner.highlights(&record).unwrap().iter().filter(|h| !h.stale).map(|h| h.id).collect::<Vec<_>>(),
        [fresh.id]
    );

    // Entfernen nimmt sie aus der Ansicht; in einer geschlossenen Stufe ändert sich nichts mehr.
    t.r1.remove_highlight(&stage, &record, &mine.id).unwrap();
    assert!(t.r1.highlights(&record).unwrap().iter().all(|h| h.id != mine.id));
    assert_eq!(send_return(&t.r1, &t.owner).rejected, Vec::<String>::new());
    assert!(t.owner.highlights(&record).unwrap().iter().all(|h| h.id != mine.id));
    vote_all(&t, &stage, &["include"; 4], &["include"; 4]);
    t.owner.close_stage(&stage).unwrap();
    assert!(matches!(t.owner.add_highlight(&stage, &record, &highlight("blue", "")), Err(Error::Conflict(_))));
    assert!(matches!(t.owner.remove_highlight(&stage, &record, &fresh.id), Err(Error::Conflict(_))));
    assert!(t.owner.highlights(&record).unwrap().iter().all(|h| !h.editable));
}

#[test]
fn highlights_also_live_in_form_stages() {
    let t = team_before_charting();
    t.owner.publish_form(&charting_form(json!({}))).unwrap();
    let pilot = open(&t, "charting_pilot", |_| {});
    let record = Uuid::parse_str(&sources_of(&t.r1, &pilot.id)[0]).unwrap();
    let mine = t.r1.add_highlight(&pilot.id, &record, &highlight("orange", "Land steht hier")).unwrap();
    // Beim Einlesen gilt die Regel „keine Stimmen in Formular-Stufen“ für Markierungen nicht.
    assert_eq!(send_return(&t.r1, &t.owner).rejected, Vec::<String>::new());
    assert!(t.owner.highlights(&record).unwrap().is_empty(), "die Stufe ist verblindet");
    t.owner.unblind_stage(&pilot.id).unwrap();
    let seen = t.owner.highlights(&record).unwrap();
    assert_eq!(seen.iter().map(|h| (h.id, h.stage_id)).collect::<Vec<_>>(), [(mine.id, pilot.id)]);
    // Die Markierungen aus dem Volltext-Screening und aus der Extraktion stehen nebeneinander.
    assert_eq!(seen[0].stage_name, pilot.name);
}
