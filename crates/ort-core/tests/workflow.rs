//! Vorlagen, Stationen, Gates und Auswahlregeln. Alles läuft wie im echten Betrieb über Pakete und
//! Rückmeldungen.

mod common;

use std::collections::BTreeMap;

use common::{person, send_package, send_return, wire, Team};
use ort_core::error::Error;
use ort_core::event::sha256_hex;
use ort_core::exchange::{export_return, merge};
use ort_core::projection::Verify;
use ort_core::service::{OpenStage, StageView};
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

fn vote(review: &Review, stage: &Uuid, record: &str, decision: &str) {
    review.cast_vote(stage, record, decision, &[], "").unwrap();
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
    t.owner.publish_criteria(&json!({"text": "Population, Concept, Context"})).unwrap();
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
    assert_eq!(available, ["protocol", "import", "pilot_title_abstract", "title_abstract", "fulltext"]);
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

    t.owner.publish_criteria(&json!({"text": "PCC"})).unwrap();
    assert_eq!(states(&t.owner)["pilot_title_abstract"], "open");
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
    assert!(matches!(t.owner.override_gate("search", "Grund"), Err(Error::Invalid(_))), "sperrt nichts");
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
    t.owner.record_consensus(&ta.id, &records[1], "include", "", &[], "").unwrap();
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

    let err = t.r1.record_consensus(&stage.id, &record, "include", "", &[], "").unwrap_err();
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
    t.owner.record_consensus(&stage.id, &record, "exclude", "", &[], "").unwrap();
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
                if now.tool.as_deref() == Some("protocol") {
                    t.owner.publish_criteria(&json!({"text": "Kriterien"})).unwrap();
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
