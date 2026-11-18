//! final_decision und Maybe-Regel, portiert aus eventlog/test_projections.py (Szenarien C und D).

mod common;

use common::person;
use ort_core::agreement::percent_agreement;
use ort_core::projection::{MaybeMode, StageState};
use ort_core::store::Store;
use ort_core::Identity;
use serde_json::{json, Value};
use uuid::Uuid;

struct Log {
    store: Store,
    sys: Identity,
}

impl Log {
    fn new() -> Log {
        let store = Store::in_memory().unwrap();
        store.set_meta("review_id", &Uuid::new_v4().to_string()).unwrap();
        Log { store, sys: person("sys") }
    }
    fn add(&self, who: &Identity, t: &str, stage: Uuid, payload: Value) {
        self.store.append(who, t, Some(stage), payload).unwrap();
    }
    fn stage(&self, id: Uuid) -> StageState {
        StageState::build(&id, &self.store.events().unwrap()).unwrap()
    }
}

#[test]
fn final_decision_scenarios() {
    let log = Log::new();
    let st = Uuid::new_v4();
    let (a, b) = (person("a"), person("b"));
    log.add(&log.sys, "StageInstanceOpened", st, json!({"min_reviewers": 2}));
    for (rec, va, vb) in [
        ("r1", "include", Some("include")),
        ("r2", "include", None),
        ("r3", "include", Some("exclude")),
        ("r4", "include", Some("exclude")),
        ("r5", "include", Some("exclude")),
        ("r6", "maybe", Some("include")),
    ] {
        log.add(&a, "VoteCast", st, json!({"record_id": rec, "decision": va}));
        if let Some(vb) = vb {
            log.add(&b, "VoteCast", st, json!({"record_id": rec, "decision": vb}));
        }
    }
    log.add(&a, "ConsensusRecorded", st, json!({"record_id": "r4", "decision": "exclude", "method": "discussion"}));
    log.add(&b, "VoteRevised", st, json!({"record_id": "r5", "decision": "include"}));
    log.add(&a, "RecordFlaggedUndecidable", st, json!({"record_id": "r7", "reason": "abstract_missing"}));
    log.add(&b, "VoteCast", st, json!({"record_id": "r7", "decision": "include"}));
    log.add(&a, "RecordFlaggedUndecidable", st, json!({"record_id": "r8", "reason": "language"}));
    log.add(&b, "ConsensusRecorded", st, json!({"record_id": "r8", "decision": "exclude", "method": "third_reviewer"}));

    let stage = log.stage(st);
    let got: Vec<(String, Option<String>, &str, usize)> =
        stage.final_decisions().into_iter().map(|(r, d)| (r, d.decision, d.status, d.n_votes)).collect();
    let s = |x: &str| Some(x.to_string());
    assert_eq!(
        got,
        vec![
            ("r1".into(), s("include"), "unanimous", 2),
            ("r2".into(), None, "pending", 1),
            ("r3".into(), None, "disagreement", 2),
            ("r4".into(), s("exclude"), "consensus", 2),
            ("r5".into(), s("include"), "unanimous", 2),
            ("r6".into(), None, "disagreement", 2),
            ("r7".into(), None, "needs_clarification", 1),
            ("r8".into(), s("exclude"), "consensus", 0),
        ]
    );

    let p = percent_agreement(&stage.stage_votes(&MaybeMode::Stage));
    assert_eq!(p.n_items, 5);
    assert!((p.percent_agreement.unwrap() - 0.4).abs() < 1e-12);
    let p = percent_agreement(&stage.stage_votes(&MaybeMode::As("include".into())));
    assert_eq!(p.n_items, 5);
    assert!((p.percent_agreement.unwrap() - 0.6).abs() < 1e-12);
}

#[test]
fn maybe_rule_comes_from_first_opened_event() {
    let log = Log::new();
    let st = Uuid::new_v4();
    let (a, b) = (person("a"), person("b"));
    log.add(&log.sys, "StageInstanceOpened", st, json!({"min_reviewers": 2, "maybe_as": "include"}));
    // ein späteres Opened ändert die Regel nicht
    log.add(&log.sys, "StageInstanceOpened", st, json!({"min_reviewers": 2, "maybe_as": null}));
    for (rec, va, vb) in [("d1", "maybe", "include"), ("d2", "exclude", "exclude"), ("d3", "maybe", "exclude")] {
        log.add(&a, "VoteCast", st, json!({"record_id": rec, "decision": va}));
        log.add(&b, "VoteCast", st, json!({"record_id": rec, "decision": vb}));
    }
    let stage = log.stage(st);
    let got: Vec<(String, Option<String>, &str)> =
        stage.final_decisions().into_iter().map(|(r, d)| (r, d.decision, d.status)).collect();
    assert_eq!(
        got,
        vec![
            ("d1".into(), Some("include".into()), "unanimous"),
            ("d2".into(), Some("exclude".into()), "unanimous"),
            ("d3".into(), None, "disagreement"),
        ]
    );
    let standard = percent_agreement(&stage.stage_votes(&MaybeMode::Stage)).percent_agreement.unwrap();
    let raw = percent_agreement(&stage.stage_votes(&MaybeMode::Raw)).percent_agreement.unwrap();
    assert!((standard - 2.0 / 3.0).abs() < 1e-12, "Standard folgt dem Protokoll");
    assert!((raw - 1.0 / 3.0).abs() < 1e-12, "explizite Abweichung als Sensitivitätsanalyse");
}

#[test]
fn latest_vote_per_person_counts() {
    let log = Log::new();
    let st = Uuid::new_v4();
    let a = person("a");
    log.add(&log.sys, "StageInstanceOpened", st, json!({"min_reviewers": 1}));
    log.add(&a, "VoteCast", st, json!({"record_id": "x", "decision": "include"}));
    log.add(&a, "VoteRevised", st, json!({"record_id": "x", "decision": "exclude"}));
    let d = log.stage(st).final_decisions();
    assert_eq!(d["x"].decision.as_deref(), Some("exclude"));
    assert_eq!(d["x"].n_votes, 1);
}
