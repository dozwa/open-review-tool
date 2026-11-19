//! Vorlagen, Stationen, Gates und Auswahlregeln. Alles läuft wie im echten Betrieb über Pakete und
//! Rückmeldungen.

mod common;

use std::collections::BTreeMap;

use common::{criteria, person, search, send_package, send_return, wire, Team, SECOND_SOURCE};
use ort_core::error::Error;
use ort_core::event::sha256_hex;
use ort_core::exchange::{export_return, merge};
use ort_core::projection::Verify;
use ort_core::service::{OpenStage, PairDecision, RecordFilter, StageView};
use ort_core::workflow::{builtin_template, StationView};
use ort_core::Review;
use serde_json::json;
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
    // Was die App noch nicht kann, steht in der Leiste, ist aber als nicht verfügbar gekennzeichnet.
    let available: Vec<&str> =
        detail.workflow.stations.iter().filter(|s| s.available).map(|s| s.key.as_str()).collect();
    assert_eq!(available, ["protocol", "search", "import", "pilot_title_abstract", "title_abstract", "fulltext"]);
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
    assert!(matches!(t.owner.override_gate("fulltext_retrieval", "Grund"), Err(Error::Invalid(_))), "sperrt nichts");
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

fn last_payload(review: &Review, event_type: &str) -> serde_json::Value {
    review.events().unwrap().into_iter().rfind(|e| e.event_type == event_type).unwrap().payload
}

#[test]
fn fulltext_exclusion_needs_exactly_one_primary_reason() {
    let t = team_before_fulltext();
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
    let t = team_before_fulltext();
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
    assert!(report.rejected.iter().any(|r| r.contains("ConsensusRecorded") && r.contains("Hauptgrund")), "{report:?}");
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

/// Derselbe Ablauf für jede Vorlage: Stationen der Reihe nach, bis alle abgeschlossen sind, die die
/// App bearbeiten kann. Im Test steht kein Schlüssel einer Station.
fn walk(template: &str) {
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
            other => panic!("{template}: Station der Art {other} gilt als verfügbar"),
        }
    }
    for member in [&t.r1, &t.r2, &t.viewer] {
        send_package(&t.owner, member);
        assert_eq!(states(member), states(&t.owner), "alle sehen denselben Stand");
        assert!(member.verify_chain().unwrap().ok);
    }
    let after = t.owner.stations().unwrap();
    assert!(after.iter().filter(|s| s.blocking).all(|s| s.state == "done"));
    assert!(after.iter().filter(|s| s.kind == "screening").all(|s| s.state == "done"));
}

#[test]
fn same_walk_runs_with_the_scoping_template() {
    walk("jbi_scoping");
}

#[test]
fn same_walk_runs_with_the_free_template() {
    walk("free");
}
