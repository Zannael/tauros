//! Game-independent validation, shared by every connector. Changes are checked in
//! order against a working copy, so later changes see earlier ones (add a map, then
//! an event on it). Connectors add their game-specific rules on top (G3-008).

use crate::change::{Change, ChangeSet, Violation};
use crate::entities::{Event, EventKind, Flag, Trainer};
use crate::game::{Capabilities, ProjectSnapshot};

/// Checks `changes` against `snapshot`; returns every violation found.
pub fn check(snapshot: &ProjectSnapshot, changes: &ChangeSet) -> Vec<Violation> {
    simulate(snapshot, changes).err().unwrap_or_default()
}

/// Applies `changes` to a copy of `snapshot`, or returns all violations.
pub fn simulate(snapshot: &ProjectSnapshot, changes: &ChangeSet) -> Result<ProjectSnapshot, Vec<Violation>> {
    let mut work = snapshot.clone();
    let mut found = Vec::new();
    if changes.summary.trim().is_empty() {
        found.push(Violation::error(None, "changeset.summary", "a change set needs a summary"));
    }
    for (index, change) in changes.changes.iter().enumerate() {
        let i = Some(index);
        match change {
            Change::UpsertTrainer { trainer } => {
                found.extend(check_trainer(&work, &snapshot.game.capabilities, trainer, i));
                work.trainers.insert(trainer.id.clone(), trainer.clone());
            }
            Change::RemoveTrainer { id } => {
                if work.trainers.remove(id).is_none() {
                    found.push(Violation::error(i, "trainer.exists", format!("no trainer {id}")));
                }
            }
            Change::UpsertMap { map } => {
                for event in &map.events {
                    found.extend(check_event(&work, event, i, Some(&map.id)));
                }
                work.maps.insert(map.id.clone(), map.clone());
            }
            Change::AddEvent { map, event } => {
                found.extend(check_event(&work, event, i, Some(map)));
                match work.maps.get_mut(map) {
                    Some(m) => m.events.push(event.clone()),
                    None => found.push(Violation::error(i, "map.exists", format!("no map {map}"))),
                }
            }
            Change::RemoveEvent { map, kind, index: n } => match work.maps.get_mut(map) {
                Some(m) => match position_of(&m.events, *kind, *n) {
                    Some(at) => {
                        m.events.remove(at);
                    }
                    None => found.push(Violation::error(i, "event.exists", format!("{map} has no {kind:?} #{n}"))),
                },
                None => found.push(Violation::error(i, "map.exists", format!("no map {map}"))),
            },
            Change::UpsertScene { scene } => {
                if scene.source.trim().is_empty() {
                    found.push(Violation::error(i, "scene.source", format!("scene {} is empty", scene.label)));
                }
                if let Some(map) = &scene.map {
                    if !work.maps.contains_key(map) {
                        found.push(Violation::error(i, "map.exists", format!("no map {map}")));
                    }
                }
                work.scenes.insert(scene.label.clone(), scene.clone());
            }
            Change::RemoveScene { label } => {
                if work.scenes.remove(label).is_none() {
                    found.push(Violation::error(i, "scene.exists", format!("no scene {label}")));
                }
            }
            Change::AllocateFlag { id } => {
                if work.flags.contains_key(id) {
                    found.push(Violation::error(i, "flag.unique", format!("flag {id} already exists")));
                } else {
                    work.flags.insert(id.clone(), Flag { id: id.clone(), value: None, in_use: true });
                }
            }
        }
    }
    if found.is_empty() { Ok(work) } else { Err(found) }
}

fn position_of(events: &[Event], kind: EventKind, n: usize) -> Option<usize> {
    events.iter().enumerate().filter(|(_, e)| e.kind() == kind).nth(n).map(|(at, _)| at)
}

fn check_trainer(work: &ProjectSnapshot, caps: &Capabilities, trainer: &Trainer, i: Option<usize>) -> Vec<Violation> {
    let mut found = Vec::new();
    let size = trainer.party.members.len();
    if size == 0 || size > caps.max_party_size as usize {
        found.push(Violation::error(
            i,
            "party.size",
            format!("{} has {size} party members; allowed 1..={}", trainer.id, caps.max_party_size),
        ));
    }
    for (slot, member) in trainer.party.members.iter().enumerate() {
        let who = format!("{} slot {}", trainer.id, slot + 1);
        if !work.species.is_empty() && !work.species.contains_key(&member.species) {
            found.push(Violation::error(i, "species.exists", format!("{who}: unknown species {}", member.species)));
        }
        if member.level < caps.min_level || member.level > caps.max_level {
            found.push(Violation::error(
                i,
                "member.level",
                format!("{who}: level {} outside {}..={}", member.level, caps.min_level, caps.max_level),
            ));
        }
        if member.moves.len() > caps.max_moves as usize {
            found.push(Violation::error(i, "member.moves", format!("{who}: {} moves, max {}", member.moves.len(), caps.max_moves)));
        }
        if member.held_item.is_some() && !caps.held_items {
            found.push(Violation::error(i, "member.held_item", format!("{who}: this game has no held items")));
        }
        if member.ability.is_some() && !caps.abilities {
            found.push(Violation::error(i, "member.ability", format!("{who}: this game has no abilities")));
        }
    }
    found
}

fn check_event(work: &ProjectSnapshot, event: &Event, i: Option<usize>, own_map: Option<&crate::MapId>) -> Vec<Violation> {
    match event {
        Event::Warp { destination_map, .. } if Some(destination_map) != own_map && !work.maps.contains_key(destination_map) => {
            vec![Violation::error(i, "warp.destination", format!("warp to unknown map {destination_map}"))]
        }
        _ => Vec::new(),
    }
}
