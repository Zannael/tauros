//! The connector contract suite. Every connector runs it from its own tests against
//! a fixture project: `contract::assert_contract(|| make_connector_on_fresh_copy())`.
//! `fresh` must return a connector over an unmodified copy of the same fixture each
//! time; the fixture needs at least one trainer with at least one party member.

use crate::change::{Change, ChangeSet, Severity};
use crate::connector::{Connector, ConnectorError};
use crate::ids::TrainerId;
use crate::provenance::Provenance;

type Check<C> = (&'static str, fn(&mut C) -> Result<(), String>);

/// Runs every check; returns the names and reasons of those that failed.
pub fn run_all<C: Connector>(fresh: impl Fn() -> C) -> Vec<String> {
    let checks: [Check<C>; 8] = [
        ("load_is_deterministic", load_is_deterministic),
        ("capabilities_are_sane", capabilities_are_sane),
        ("empty_change_set_is_noop", empty_change_set_is_noop),
        ("unchanged_rewrite_is_lossless", unchanged_rewrite_is_lossless),
        ("edit_is_visible_and_local", edit_is_visible_and_local),
        ("invalid_change_is_rejected_untouched", invalid_change_is_rejected_untouched),
        ("missing_target_is_rejected", missing_target_is_rejected),
        ("change_sets_are_atomic", change_sets_are_atomic),
    ];
    checks
        .iter()
        .filter_map(|(name, check)| check(&mut fresh()).err().map(|why| format!("{name}: {why}")))
        .collect()
}

/// Panics with every failed check.
pub fn assert_contract<C: Connector>(fresh: impl Fn() -> C) {
    let failures = run_all(fresh);
    assert!(failures.is_empty(), "connector contract violated:\n  {}", failures.join("\n  "));
}

fn err(e: ConnectorError) -> String {
    e.to_string()
}

fn first_trainer<C: Connector>(c: &C) -> Result<crate::Trainer, String> {
    c.load()
        .map_err(err)?
        .trainers
        .into_values()
        .find(|t| !t.party.members.is_empty())
        .ok_or_else(|| "fixture has no trainer with a party member".to_owned())
}

fn load_is_deterministic<C: Connector>(c: &mut C) -> Result<(), String> {
    let (a, b) = (c.load().map_err(err)?, c.load().map_err(err)?);
    if a != b {
        return Err("two loads differ".into());
    }
    if c.fingerprint().map_err(err)? != c.fingerprint().map_err(err)? {
        return Err("fingerprint is not stable".into());
    }
    Ok(())
}

fn capabilities_are_sane<C: Connector>(c: &mut C) -> Result<(), String> {
    let caps = c.info().capabilities;
    if caps.max_party_size == 0 || caps.max_moves == 0 || caps.min_level == 0 || caps.min_level > caps.max_level {
        return Err(format!("implausible capabilities {caps:?}"));
    }
    Ok(())
}

fn empty_change_set_is_noop<C: Connector>(c: &mut C) -> Result<(), String> {
    let (before, print) = (c.load().map_err(err)?, c.fingerprint().map_err(err)?);
    c.apply(&ChangeSet::new("contract: empty", Provenance::Human)).map_err(err)?;
    if c.fingerprint().map_err(err)? != print || c.load().map_err(err)? != before {
        return Err("an empty change set modified the project".into());
    }
    Ok(())
}

fn unchanged_rewrite_is_lossless<C: Connector>(c: &mut C) -> Result<(), String> {
    let trainer = first_trainer(c)?;
    let print = c.fingerprint().map_err(err)?;
    let set = ChangeSet::new("contract: rewrite unchanged", Provenance::Human)
        .with(Change::UpsertTrainer { trainer: trainer.clone() });
    c.apply(&set).map_err(err)?;
    if c.fingerprint().map_err(err)? != print {
        return Err(format!("rewriting unchanged {} altered the files", trainer.id));
    }
    Ok(())
}

fn edit_is_visible_and_local<C: Connector>(c: &mut C) -> Result<(), String> {
    let caps = c.info().capabilities;
    let before = c.load().map_err(err)?;
    let mut trainer = first_trainer(c)?;
    let member = &mut trainer.party.members[0];
    member.level = if member.level < caps.max_level { member.level + 1 } else { member.level - 1 };
    let set = ChangeSet::new("contract: edit one level", Provenance::AiAssisted)
        .with(Change::UpsertTrainer { trainer: trainer.clone() });
    c.apply(&set).map_err(err)?;
    let after = c.load().map_err(err)?;
    if after.trainers.get(&trainer.id) != Some(&trainer) {
        return Err(format!("edit to {} not visible after reload", trainer.id));
    }
    let others_changed = before
        .trainers
        .iter()
        .filter(|(id, _)| **id != trainer.id)
        .any(|(id, t)| after.trainers.get(id) != Some(t));
    if others_changed || before.maps != after.maps || before.scenes != after.scenes || before.flags != after.flags {
        return Err("editing one trainer changed other entities".into());
    }
    Ok(())
}

fn rejected_untouched<C: Connector>(c: &mut C, set: &ChangeSet) -> Result<(), String> {
    let print = c.fingerprint().map_err(err)?;
    if !c.validate(set).iter().any(|v| v.severity == Severity::Error) {
        return Err(format!("validate accepted invalid change set '{}'", set.summary));
    }
    match c.apply(set) {
        Err(ConnectorError::Rejected(_)) => {}
        other => return Err(format!("apply of invalid set returned {other:?}")),
    }
    if c.fingerprint().map_err(err)? != print {
        return Err("a rejected change set modified files".into());
    }
    Ok(())
}

fn invalid_change_is_rejected_untouched<C: Connector>(c: &mut C) -> Result<(), String> {
    let caps = c.info().capabilities;
    let mut trainer = first_trainer(c)?;
    let member = trainer.party.members[0].clone();
    trainer.party.members = vec![member; caps.max_party_size as usize + 1];
    let set = ChangeSet::new("contract: oversize party", Provenance::AiGenerated)
        .with(Change::UpsertTrainer { trainer });
    rejected_untouched(c, &set)
}

fn missing_target_is_rejected<C: Connector>(c: &mut C) -> Result<(), String> {
    let set = ChangeSet::new("contract: remove missing", Provenance::Human)
        .with(Change::RemoveTrainer { id: TrainerId::from("TAUROS_CONTRACT_MISSING_TRAINER") });
    rejected_untouched(c, &set)
}

fn change_sets_are_atomic<C: Connector>(c: &mut C) -> Result<(), String> {
    let caps = c.info().capabilities;
    let mut trainer = first_trainer(c)?;
    let member = &mut trainer.party.members[0];
    member.level = if member.level < caps.max_level { member.level + 1 } else { member.level - 1 };
    let set = ChangeSet::new("contract: valid then invalid", Provenance::Human)
        .with(Change::UpsertTrainer { trainer })
        .with(Change::RemoveTrainer { id: TrainerId::from("TAUROS_CONTRACT_MISSING_TRAINER") });
    rejected_untouched(c, &set)
}
