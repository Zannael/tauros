//! G3-009: append-only constants and guarded party creation, including capacity.
mod common;
use std::fs;
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_connector_gen3::{trainers, Gen3Connector};
use tauros_model::*;

#[test]
fn trainers_are_appended_atomically_without_renumbering() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "new-trainers", &["src", "include", "data", "tools/trainerproc"]);
    let mut c = Gen3Connector::open(dir.path(), None).unwrap();
    let snapshot = c.load().unwrap();
    let before = fs::read_to_string(dir.path().join(trainers::HEADER)).unwrap();
    let mut trainer = snapshot.trainers[&TrainerId::from("TRAINER_ROXANNE_1")].clone();
    trainer.id = "TRAINER_TAUROS_NEW".into();
    let changes = ChangeSet::new("create trainer", Provenance::AiAssisted).with(Change::UpsertTrainer { trainer: trainer.clone() });
    let result = c.apply(&changes).unwrap();
    assert_eq!(result.touched, [trainers::HEADER, "src/data/trainers.party"]);
    assert!(result.commit.is_some());
    assert_eq!(c.load().unwrap().trainers[&trainer.id], trainer);
    let after = fs::read_to_string(dir.path().join(trainers::HEADER)).unwrap();
    assert_eq!(
        after.replace("#define TRAINER_TAUROS_NEW 855\n", "").replace("TRAINERS_COUNT_EMERALD     856", "TRAINERS_COUNT_EMERALD     855"),
        before
    );
    assert!(c.apply(&changes).unwrap().touched.is_empty());
    // Fill all remaining reserved slots, then reject the whole over-capacity batch.
    let mut set = ChangeSet::new("exhaust slots", Provenance::AiAssisted);
    for n in 0..9 {
        let mut t = trainer.clone();
        t.id = TrainerId(format!("TRAINER_TAUROS_{n}"));
        set.changes.push(Change::UpsertTrainer { trainer: t });
    }
    let fingerprint = c.fingerprint().unwrap();
    assert!(matches!(c.apply(&set), Err(ConnectorError::Rejected(v)) if v.iter().any(|v| v.rule == "trainer.capacity")));
    assert_eq!(c.fingerprint().unwrap(), fingerprint);
    set.changes.pop();
    c.apply(&set).unwrap();
    assert!(fs::read_to_string(dir.path().join(trainers::HEADER)).unwrap().contains("TRAINERS_COUNT_EMERALD     864"));
    assert_eq!(c.load().unwrap().trainers.len(), snapshot.trainers.len() + 9);
}

#[test]
fn created_trainer_builds_into_generated_header() {
    if std::env::var_os("TAUROS_DOCKER_TESTS").is_none() {
        return;
    }
    let engine = common::engine().expect("pinned engine required");
    let dir = common::built_fixture(&engine, "new-trainer-build").expect("base build required");
    let tc = Toolchain::for_repository(&common::repo_root()).unwrap();
    let mut c = Gen3Connector::open(dir.path(), Some(tc)).unwrap();
    let mut trainer = c.load().unwrap().trainers[&TrainerId::from("TRAINER_ROXANNE_1")].clone();
    trainer.id = "TRAINER_TAUROS_NEW".into();
    c.apply(&ChangeSet::new("create buildable trainer", Provenance::AiAssisted).with(Change::UpsertTrainer { trainer })).unwrap();
    let report = c.build_report().unwrap();
    assert!(report.success, "{}", report.log);
    let generated = common::files(dir.path(), "src/data", |n| n == "trainers.h");
    assert!(
        generated.iter().any(|p| fs::read_to_string(p).unwrap().contains("[TRAINER_TAUROS_NEW]")),
        "trainer must appear in generated trainers.h"
    );
    eprintln!("created TRAINER_TAUROS_NEW=855; generated trainers.h and ROM build passed");
}
