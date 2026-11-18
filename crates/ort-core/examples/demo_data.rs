//! Beispieldaten zum Ausprobieren der App. Alles läuft über den echten Kern: vier Personen mit
//! eigenen Dateien, Pakete, Rückmeldungen, Entblinden, Konsens. Die Studien sind erfunden
//! (DOI-Präfix 10.5555 ist für Tests reserviert).
//!
//!     cargo run -p ort-core --example demo_data -- /tmp/ort-demo "Dein Name"
//!     cd app && ORT_DATA_DIR=/tmp/ort-demo ORT_KEY_STORE=file npm run tauri dev

use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ort_core::exchange::{create_from_package, export_package, export_return, merge, Bundle};
use ort_core::service::OpenStage;
use ort_core::{Identity, Review};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde_json::json;
use uuid::Uuid;

fn wire(b: &Bundle) -> Bundle {
    Bundle::from_json(&b.to_json().unwrap()).unwrap()
}

/// Studie mit „wahrer“ Entscheidung, damit die Teamvoten realistisch übereinstimmen.
struct Study {
    ris: String,
    title: String,
    include: bool,
}

fn studies(rng: &mut ChaCha8Rng, n: usize) -> Vec<Study> {
    let interventions = [
        ("smartphone app", "app"),
        ("web-based cognitive behavioural therapy", "CBT"),
        ("wearable biofeedback device", "biofeedback"),
        ("telemedicine consultation", "telemedicine"),
        ("headache diary app", "diary"),
        ("virtual reality relaxation programme", "VR"),
        ("text message reminders", "SMS"),
        ("chatbot-guided self-management", "chatbot"),
    ];
    let populations = [
        ("adults with episodic migraine", true),
        ("adolescents with migraine", true),
        ("adults with chronic migraine", true),
        ("patients with tension-type headache", false),
        ("healthy volunteers", false),
    ];
    let designs = [
        ("A randomised controlled trial", "randomised controlled trial", true),
        ("A prospective cohort study", "cohort study", true),
        ("A qualitative interview study", "qualitative study", true),
        ("A pilot feasibility study", "feasibility study", true),
        ("An editorial commentary", "editorial", false),
        ("A study protocol", "protocol", false),
    ];
    let names = [
        "Müller",
        "Nguyen",
        "Okafor",
        "Rossi",
        "Kowalski",
        "Haddad",
        "Johansson",
        "Tanaka",
        "García",
        "Novak",
        "Chen",
        "Silva",
    ];
    let journals =
        ["Cephalalgia Digital", "Journal of Headache Research", "Digital Health Reports", "Neurology and Technology"];
    (0..n)
        .map(|i| {
            let (iv, short) = interventions[rng.gen_range(0..interventions.len())];
            let (pop, pop_ok) = populations[rng.gen_range(0..populations.len())];
            let (design, kw, design_ok) = designs[rng.gen_range(0..designs.len())];
            let year = rng.gen_range(2015..2026);
            let title = if rng.gen_bool(0.35) {
                format!("Effect of a {iv} on headache days in {pop}")
            } else {
                format!("{}{} for {pop}: a {kw}", iv[..1].to_uppercase(), &iv[1..])
            };
            let abstract_text = if i % 13 == 5 {
                String::new()
            } else if design_ok {
                format!(
                    "{design} evaluating a {iv} in {pop}. Primary outcomes were monthly headache days and acute medication use; \
                     secondary outcomes included disability (MIDAS) and quality of life over {} weeks. Adherence was tracked in the app.",
                    [8, 12, 24, 52][rng.gen_range(0..4)]
                )
            } else {
                format!("{design} on the use of {iv} in {pop}. No original data are reported.")
            };
            let mut ris = String::from("TY  - JOUR\n");
            for _ in 0..rng.gen_range(1..4) {
                ris += &format!("AU  - {}, {}.\n", names[rng.gen_range(0..names.len())], (b'A' + rng.gen_range(0..20)) as char);
            }
            ris += &format!("TI  - {title}\nAB  - {abstract_text}\nPY  - {year}\nJO  - {}\n", journals[rng.gen_range(0..journals.len())]);
            ris += &format!("DO  - 10.5555/mig.{year}.{}\nKW  - migraine\nKW  - {short}\nKW  - {kw}\nER  - \n\n", 1000 + i);
            Study { ris, title, include: pop_ok && design_ok }
        })
        .collect()
}

fn vote_like(
    review: &Review,
    stage: &Uuid,
    truth: &dyn Fn(&str) -> bool,
    accuracy: f64,
    share: f64,
    rng: &mut ChaCha8Rng,
) {
    let reasons = ["Falsche Population", "Keine digitale Intervention", "Kein Originalbeitrag", "Protokoll ohne Daten"];
    let total = review.get_stage(stage).unwrap().n_records as f64;
    let mut done = 0.0;
    while done < total * share {
        let Some(record) = review.next_record(stage).unwrap().record else { break };
        let id = record.id.to_string();
        if record.abstract_text.is_empty() && rng.gen_bool(0.5) {
            // Markierte Records bleiben „offen“ für diese Person; dann einmal abstimmen, damit es weitergeht.
            review.flag_undecidable(stage, &id, "Kein Abstract, Volltext nötig.").unwrap();
        }
        let right = rng.gen_bool(accuracy);
        let include = truth(&record.title) == right;
        if rng.gen_bool(0.07) {
            review.cast_vote(stage, &id, "maybe", &[], "Unsicher, bitte besprechen.").unwrap();
        } else if include {
            review.cast_vote(stage, &id, "include", &[], "").unwrap();
        } else {
            review.cast_vote(stage, &id, "exclude", &[reasons[rng.gen_range(0..4)].to_string()], "").unwrap();
        }
        done += 1.0;
    }
}

fn save_identity(dir: &Path, me: &Identity) {
    std::fs::create_dir_all(dir.join("reviews")).unwrap();
    std::fs::write(dir.join("identity.json"), serde_json::to_string_pretty(&me.public()).unwrap()).unwrap();
    std::fs::write(dir.join("identity.key"), B64.encode(me.secret_bytes())).unwrap();
}

fn new_review(dir: &Path, me: &Identity, title: &str) -> (Review, PathBuf) {
    let tmp = dir.join("reviews").join(format!("neu-{}.tmp", Uuid::new_v4()));
    let id = Review::create(&tmp, me.clone(), title).unwrap().id().unwrap();
    let path = dir.join("reviews").join(format!("{id}.ortreview"));
    std::fs::rename(&tmp, &path).unwrap();
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", tmp.display()));
    }
    (Review::open(&path, me.clone()).unwrap(), path)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("Aufruf: demo_data <Datenverzeichnis> [Name]"));
    let name = args.next().unwrap_or_else(|| "Leitung".into());
    if dir.join("identity.json").exists() {
        panic!("{} enthält schon eine Identität; bitte ein leeres Verzeichnis angeben.", dir.display());
    }
    let team_dir = tempfile_dir();
    let mut rng = ChaCha8Rng::seed_from_u64(20261118);
    let me = Identity::generate(&name).unwrap();
    save_identity(&dir, &me);

    // ------------------------------------------------ Review 1: weit fortgeschritten
    let (owner, _) = new_review(&dir, &me, "Digitale Interventionen bei Migräne");
    let people: Vec<Identity> =
        ["Anna Berger", "Ben Krüger", "Clara Weiß"].iter().map(|n| Identity::generate(n).unwrap()).collect();
    let viewer = Identity::generate("Dora Lang").unwrap();
    for p in &people {
        owner.set_member(&p.public(), "reviewer").unwrap();
    }
    owner.set_member(&viewer.public(), "viewer").unwrap();
    owner
        .publish_criteria(&json!({
            "text": "Eingeschlossen werden Originalstudien zu digitalen Interventionen bei Menschen mit Migräne (alle Altersgruppen).\nAusgeschlossen: andere Kopfschmerzarten, Editorials, Protokolle ohne Daten.",
            "include_keywords": "migraine, app, telemedicine, digital", "exclude_keywords": "editorial, protocol, tension-type",
            "exclusion_reasons": "Falsche Population\nKeine digitale Intervention\nKein Originalbeitrag\nProtokoll ohne Daten",
            "change_note": "Erste Fassung"}))
        .unwrap();
    owner
        .publish_criteria(&json!({
            "text": "Eingeschlossen werden Originalstudien zu digitalen Interventionen bei Menschen mit Migräne (alle Altersgruppen), auch qualitative Studien.\nAusgeschlossen: andere Kopfschmerzarten, Editorials, Protokolle ohne Daten, Übersichtsarbeiten.",
            "include_keywords": "migraine, app, telemedicine, digital, qualitative", "exclude_keywords": "editorial, protocol, tension-type",
            "exclusion_reasons": "Falsche Population\nKeine digitale Intervention\nKein Originalbeitrag\nProtokoll ohne Daten",
            "change_note": "Qualitative Studien ausdrücklich eingeschlossen"}))
        .unwrap();
    let list = studies(&mut rng, 46);
    let truth_table: Vec<(String, bool)> = list.iter().map(|s| (s.title.clone(), s.include)).collect();
    let truth = move |title: &str| truth_table.iter().find(|(t, _)| t == title).is_none_or(|(_, inc)| *inc);
    let ris_a: String = list[..34].iter().map(|s| s.ris.as_str()).collect();
    let ris_b: String = list[30..].iter().map(|s| s.ris.as_str()).collect();
    owner.import_file("pubmed_2026-11-02.ris", &ris_a, "PubMed").unwrap();
    owner.import_file("scopus_2026-11-02.ris", &ris_b, "Scopus").unwrap();

    let members: Vec<Review> = people
        .iter()
        .chain([&viewer])
        .map(|p| {
            let pkg = wire(&export_package(&owner, &p.actor_id).unwrap());
            create_from_package(&team_dir.join(format!("{}.sqlite", p.actor_id)), p.clone(), &pkg).unwrap().0
        })
        .collect();
    let sync_out = |owner: &Review, members: &[Review]| {
        for m in members {
            merge(m, &wire(&export_package(owner, &m.me().actor_id).unwrap())).unwrap();
        }
    };
    let collect = |owner: &Review, members: &[Review]| {
        for m in members {
            merge(owner, &wire(&export_return(m).unwrap())).unwrap();
        }
    };

    // Pilot: alle drei, entblindet, Konsens, abgeschlossen
    let pilot = owner
        .open_stage(&OpenStage {
            name: "Pilot Titel/Abstract".into(),
            kind: "pilot".into(),
            scope: "sample".into(),
            sample_size: Some(10),
            seed: Some(42),
            min_reviewers: 3,
            maybe_as: Some("include".into()),
            ..OpenStage::default()
        })
        .unwrap();
    sync_out(&owner, &members);
    for m in &members[..3] {
        vote_like(m, &pilot.id, &truth, 0.82, 1.0, &mut rng);
    }
    collect(&owner, &members[..3]);
    owner.unblind_stage(&pilot.id).unwrap();
    for c in owner.conflicts(&pilot.id).unwrap() {
        let decision = if truth(&c.record.title) { "include" } else { "exclude" };
        owner
            .record_consensus(
                &pilot.id,
                &c.record.id.to_string(),
                decision,
                "discussion",
                &[],
                "Im Pilot-Treffen besprochen.",
            )
            .unwrap();
    }
    owner.close_stage(&pilot.id).unwrap();

    // Titel/Abstract: entblindet, offene Konflikte, die Leitung selbst hat noch nicht gescreent
    let ta = owner.open_stage(&OpenStage { name: "Titel/Abstract".into(), ..OpenStage::default() }).unwrap();
    sync_out(&owner, &members);
    for (m, (accuracy, share)) in members[..3].iter().zip([(0.9, 1.0), (0.84, 0.85), (0.92, 0.4)]) {
        vote_like(m, &ta.id, &truth, accuracy, share, &mut rng);
    }
    collect(&owner, &members[..3]);
    owner.unblind_stage(&ta.id).unwrap();
    sync_out(&owner, &members);
    for c in members[2].conflicts(&ta.id).unwrap().into_iter().take(2) {
        members[2]
            .record_consensus(&ta.id, &c.record.id.to_string(), "include", "third_reviewer", &[], "Drittmeinung")
            .unwrap();
    }
    collect(&owner, &members[2..3]);

    // ------------------------------------------------ Review 2: frisch und verblindet
    let (second, _) = new_review(&dir, &me, "KI-gestützte Triage in der Notaufnahme");
    second.set_member(&people[0].public(), "reviewer").unwrap();
    second.set_member(&people[1].public(), "reviewer").unwrap();
    second
        .publish_criteria(&json!({"text": "Studien zu KI-Systemen, die in der Notaufnahme die Dringlichkeit einschätzen, verglichen mit Triage durch Pflegekräfte.",
            "include_keywords": "triage, emergency department, machine learning", "exclude_keywords": "prehospital, simulation",
            "exclusion_reasons": "Kein Notaufnahme-Setting\nKein Vergleich mit Standardtriage\nNur Simulationsdaten"}))
        .unwrap();
    let mut ris = String::new();
    for i in 0..18 {
        let model = [
            "Gradient boosting",
            "Deep neural network",
            "Logistic regression",
            "Large language model",
            "Random forest",
        ][i % 5];
        let setting = [
            "an urban emergency department",
            "a paediatric emergency department",
            "three tertiary emergency departments",
            "prehospital ambulance services",
        ][rng.gen_range(0..4)];
        let year = rng.gen_range(2018..2027);
        ris += &format!(
            "TY  - JOUR\nAU  - Haddad, S.\nTI  - {model} versus nurse triage in {setting}\nAB  - Retrospective analysis of {} visits in {setting}. The model predicted admission and critical care need and was compared with the Manchester Triage System.\nPY  - {year}\nJO  - Emergency Medicine AI\nDO  - 10.5555/ed.{year}.{}\nER  - \n\n",
            rng.gen_range(2..90) * 1000, 300 + i
        );
    }
    second.import_file("embase_export.ris", &ris, "Embase").unwrap();
    second
        .open_stage(&OpenStage {
            name: "Titel/Abstract".into(),
            maybe_as: Some("include".into()),
            ..OpenStage::default()
        })
        .unwrap();

    for r in [&owner, &second] {
        assert!(r.verify_chain().unwrap().ok);
    }
    let _ = std::fs::remove_dir_all(&team_dir);
    println!("Beispieldaten in {} angelegt (Identität: {name}).", dir.display());
    println!("Start: cd app && ORT_DATA_DIR={} ORT_KEY_STORE=file npm run tauri dev", dir.display());
}

fn tempfile_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ort-demo-team-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
