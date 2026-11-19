//! Messung mit einem großen Review. Läuft nicht mit den normalen Tests:
//!
//! ```bash
//! cargo test -p ort-core --release --test bench -- --ignored --nocapture
//! ```
//!
//! Projektionen werden bei jedem Aufruf neu aus dem Log berechnet. Die Messung zeigt, was das bei
//! 20.000 Einträgen und zwei Stimmen je Eintrag kostet.

mod common;

use std::fmt::Write as _;
use std::time::Instant;

use common::person;
use ort_core::exchange::export_package;
use ort_core::service::OpenStage;
use ort_core::Review;
use serde_json::json;

const N: usize = 20_000;

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    println!("{label:<44} {:>9.1} ms", start.elapsed().as_secs_f64() * 1000.0);
    out
}

#[test]
#[ignore = "Messung, dauert lange"]
fn large_review() {
    let dir = tempfile::TempDir::new().unwrap();
    let owner = Review::create(&dir.path().join("gross.sqlite"), person("owner"), "Großes Review").unwrap();
    let (a, b) = (person("a"), person("b"));
    owner.set_member(&a.public(), "reviewer").unwrap();
    owner.set_member(&b.public(), "reviewer").unwrap();

    let mut ris = String::new();
    for i in 0..N {
        write!(ris, "TY  - JOUR\nAU  - Autor, {i}\nTI  - Titel Nummer {i} über ein Thema\nAB  - Zusammenfassung {i} mit etwas Text, damit der Eintrag eine übliche Länge hat.\nPY  - 2020\nDO  - 10.1000/x{i}\nER  - \n\n").unwrap();
    }
    println!("{N} Einträge, zwei Stimmen je Eintrag");
    timed("Import", || owner.import_file("gross.ris", &ris, "Test").unwrap());
    let stage = timed("Stufe öffnen", || {
        owner.open_stage(&OpenStage { name: "Titel/Abstract".into(), ..OpenStage::default() }).unwrap()
    });
    let records = owner.records(None, false).unwrap();

    // Stimmen direkt ins Log, wie sie nach dem Einlesen der Rückmeldungen dort stünden.
    timed("40.000 Stimmen anhängen", || {
        let tx = owner.store().conn().unchecked_transaction().unwrap();
        for (i, r) in records.iter().enumerate() {
            let id = r.id.to_string();
            let second = if i % 10 == 0 { "exclude" } else { "include" };
            owner
                .store()
                .append(&a, "VoteCast", Some(stage.id), json!({"record_id": id, "decision": "include"}))
                .unwrap();
            owner.store().append(&b, "VoteCast", Some(stage.id), json!({"record_id": id, "decision": second})).unwrap();
        }
        tx.commit().unwrap();
    });

    timed("Review-Übersicht (detail)", || owner.detail().unwrap());
    timed("Stufe (stage_detail)", || owner.stage_detail(&stage.id).unwrap());
    timed("Nächster Eintrag (next_record)", || owner.next_record(&stage.id).unwrap());
    let first = records[0].id.to_string();
    timed("Eine Stimme abgeben (cast_vote)", || owner.cast_vote(&stage.id, &first, "include", &[], "").unwrap());
    timed("Paket für eine Person, verblindet", || {
        export_package(&owner, &a.actor_id).unwrap().to_json().unwrap().len()
    });
    timed("Entblinden", || owner.unblind_stage(&stage.id).unwrap());
    timed("Auswertung mit Bootstrap (summary)", || owner.summary(&stage.id).unwrap());
    let conflicts = timed("Konflikte (conflicts)", || owner.conflicts(&stage.id).unwrap());
    assert_eq!(conflicts.len(), N / 10);
    timed("CSV-Export", || owner.export_csv(&stage.id).unwrap().len());
    let bytes = timed("Paket für eine Person, entblindet", || {
        export_package(&owner, &a.actor_id).unwrap().to_json().unwrap().len()
    });
    println!("Paketgröße: {:.1} MB", bytes as f64 / 1_048_576.0);
    timed("Ketten prüfen (verify_chain)", || assert!(owner.verify_chain().unwrap().ok));
}
