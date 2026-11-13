//! Test fixtures for deterministic update-plan generation transitions.

use std::fs;

use super::filesystem::Directory;
use super::state::State;
use super::test_support::Fixture;
use super::update_inputs::Inputs;
use super::update_plan::Plan;
use super::{ProjectError, generate_project};

pub(super) const MODULE: &str = "resolvers/query/items.luau";
pub(super) const DIST: &str = "resolvers/query/items.luau.dist";

pub(super) fn schema(argument: &str) -> String {
    format!(
        "type Item {{ id:ID! title:String! }} type Query {{ items({argument}:Int! = 20):[Item!]! }}"
    )
}

pub(super) fn plan(fixture: &Fixture, source: &str) -> Result<Plan, ProjectError> {
    fs::write(fixture.base.join("schema.graphql"), source).unwrap();
    let root = Directory::open(&fixture.project)?;
    let (schema, old) = super::sources::from_external(root)?;
    let next = generate_project(schema.ir().clone())?;
    let inputs = Inputs::new(schema.directory.clone());
    super::update_plan::build(&old, next, inputs)
}

pub(super) fn apply(fixture: &Fixture, plan: &Plan) {
    for (path, bytes) in &plan.files {
        fixture.write(path, bytes);
    }
    assert_eq!(state(fixture).generation, plan.generation);
}

pub(super) fn state(fixture: &Fixture) -> State {
    serde_json::from_str(&fixture.read("generated/update-state.json")).unwrap()
}
