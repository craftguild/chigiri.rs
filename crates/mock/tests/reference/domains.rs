//! Verify a second domain and its renamed form preserve target types, defaults, and configuration.

use super::harness::Directory;
use serde_json::Value;
use std::{collections::BTreeMap, fs};

#[test]
fn mr_17_19_20_two_source_domains_and_rename_preserve_targets_graph_and_files() {
    let source = include_str!("../../fixtures/projects.graphql");
    for renamed in [false, true] {
        let directory = Directory::new();
        let source = if renamed {
            source
                .replace("Member", "Contributor")
                .replace("Project", "Workspace")
                .replace("Task", "Ticket")
        } else {
            source.into()
        };
        let split = source.find("type Query").unwrap();
        let paths = ["models.graphql", "operations.graphql"];
        for (path, bytes) in paths.iter().zip([&source[..split], &source[split..]]) {
            fs::write(directory.0.join(path), bytes).unwrap();
        }
        let args = [
            "reference",
            "--schema",
            paths[0],
            "--schema",
            paths[1],
            "--format",
            "json",
        ];
        let output = directory.run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let doc: Value = serde_json::from_slice(&output.stdout).unwrap();
        let schema = &doc["schema"];
        let types: BTreeMap<_, _> = schema["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|ty| (ty["id"].as_str().unwrap(), ty))
            .collect();
        let fields: BTreeMap<_, _> = schema["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| (field["id"].as_str().unwrap(), field))
            .collect();
        let models: BTreeMap<_, _> = schema["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| {
                let ty = types[model["type_id"].as_str().unwrap()];
                (model["id"].as_str().unwrap(), ty["name"].as_str().unwrap())
            })
            .collect();
        assert_eq!(models.len(), 4);
        let (member, project, task, packet) = if renamed {
            ("Contributor", "Workspace", "Ticket", "Packet")
        } else {
            ("Member", "Project", "Task", "Packet")
        };
        assert_eq!(
            models.values().copied().collect::<Vec<_>>(),
            [member, project, task, packet]
        );
        let relations: BTreeMap<_, _> = schema["relations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|relation| {
                let owner = models[relation["owner_model_id"].as_str().unwrap()];
                let field = fields[relation["owner_field_id"].as_str().unwrap()]["name"]
                    .as_str()
                    .unwrap();
                ((owner, field), relation)
            })
            .collect();
        for (owner, field, target) in [
            (member, "manager", member),
            (project, "lead", member),
            (project, "members", member),
            (task, "project", project),
            (task, "assignee", member),
        ] {
            assert_eq!(
                models[relations[&(owner, field)]["target_model_id"]
                    .as_str()
                    .unwrap()],
                target
            );
        }
        assert_eq!(relations[&(project, "lead")]["unique"], true);
        assert_eq!(relations[&(task, "project")]["nullable"], false);
        assert_eq!(relations[&(project, "members")]["many"], true);
        assert_eq!(relations[&(project, "members")]["nullable_items"], false);
        let pick = types.values().find(|ty| ty["name"] == "Pick").unwrap();
        assert_eq!(pick["one_of"], true);
        let packet = types.values().find(|ty| ty["name"] == "Packet").unwrap();
        let child = fields
            .values()
            .find(|field| field["parent_type_id"] == packet["id"] && field["name"] == "child")
            .unwrap();
        assert_eq!(child["value_type"]["type_id"], packet["id"]);
        assert!(
            schema["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|input| input["default"]["kind"] == "integer"
                    && input["default"]["value"] == "9007199254740994")
        );
        let initialized = directory.run(&[
            "init",
            "--schema",
            paths[0],
            "--schema",
            paths[1],
            "--config-dir",
            "unimplemented",
        ]);
        assert!(
            initialized.status.success(),
            "{}",
            String::from_utf8_lossy(&initialized.stderr)
        );
        let before = fs::read(directory.0.join("unimplemented/mock.luau")).unwrap();
        let config = directory.run(&[
            "reference",
            "--config-dir",
            "unimplemented",
            "--format",
            "json",
        ]);
        assert!(config.status.success());
        let project: Value = serde_json::from_slice(&config.stdout).unwrap();
        assert_ne!(project["schema"]["digest"], doc["schema"]["digest"]);
        super::contract::same_contract(&config.stdout, &output.stdout);
        assert_eq!(project["input_kind"], "project");
        assert_eq!(project["config_executed"], true);
        assert_eq!(project["mock_format"], "chigiri-mock-project/v10");
        assert_eq!(
            before,
            fs::read(directory.0.join("unimplemented/mock.luau")).unwrap()
        );
        assert_eq!(
            fs::read_to_string(directory.0.join(paths[0])).unwrap()
                + &fs::read_to_string(directory.0.join(paths[1])).unwrap(),
            source
        );
        assert_eq!(directory.run(&args).stdout, output.stdout);
    }
}
