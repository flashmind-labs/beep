use super::*;
use crate::agents::{Agents, adapters::installs::Installs, harnesses::{Latest, Settings}, models::ModelCache};
use crate::occupancy::Occupancy;
use beep_proto::node::agent;
use std::collections::BTreeMap;

fn kind(name: &str) -> AgentKind {
    AgentKind {
        name: name.to_owned(),
        program: name.to_owned(),
        args: Vec::new(),
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

/// Every field is a value of a type below the catalog: building it takes the
/// agents and nothing of `Agents`, and a new field fails here until its type
/// is named.
#[test]
fn a_catalog_is_built_from_values() {
    let Catalog {
        kinds,
        adapters,
        models,
        settings,
        instructions,
        latest,
        settled,
        mcp,
        skills_dir,
    } = Catalog::new(vec![kind("claude")]);
    let _: &watch::Sender<Arc<Vec<AgentKind>>> = &kinds;
    let _: &Installs = &adapters;
    let _: &ModelCache = &models;
    let _: &Settings = &settings;
    let _: &Mutex<Option<String>> = &instructions;
    let _: &Latest = &latest;
    let _: &watch::Sender<bool> = &settled;
    let _: &OnceLock<Arc<Launch>> = &mcp;
    let _: &OnceLock<PathBuf> = &skills_dir;

    assert_eq!(watched::read_sent(&kinds, |kinds| kinds.len()), 1);
}

#[test]
fn the_agents_a_catalog_holds_are_found_by_name_and_protocol() {
    let agents = Agents::new(vec![kind("claude"), kind("codex")], Occupancy::default());

    assert_eq!(agents.named("codex").map(|kind| kind.name), Some("codex".to_owned()));
    assert!(agents.named("pi").is_none());
    assert_eq!(
        agents.kind(&agent::protocol("claude")).map(|kind| kind.name),
        Some("claude".to_owned())
    );
}

#[test]
fn a_new_detection_wakes_the_followers_once_and_only_when_it_differs() {
    let agents = Agents::new(vec![kind("claude")], Occupancy::default());
    let mut changed = agents.kinds_changed();
    watched::read_and_update(&mut changed, |_| ());

    agents.catalog.set_kinds(vec![kind("claude")]);
    assert!(!changed.has_changed().unwrap());

    agents.catalog.set_kinds(vec![kind("claude"), kind("pi")]);
    assert!(changed.has_changed().unwrap());
    assert_eq!(agents.kinds().len(), 2);
}
