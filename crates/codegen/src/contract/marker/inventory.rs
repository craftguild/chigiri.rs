//! Compares verified marker records with captured trees and classifies ownership differences.

use std::collections::{BTreeMap, BTreeSet};

use crate::CodegenError;
use crate::contract::GENERATED_MARKER_FILE;
use crate::contract::differences::{Differences, Kind};
use crate::filesystem::CapturedTree;
use crate::filesystem::PortableRelativePath;

use super::read::{invalid, validate_record};
use super::{Marker, digest};

pub(super) fn validate_inventory(marker: &Marker, tree: &CapturedTree) -> Result<(), CodegenError> {
    if marker.files.len() >= 65_536 {
        return Err(invalid());
    }
    let mut previous = None;
    let mut directories = BTreeSet::new();
    let mut expected = BTreeMap::new();
    for file in &marker.files {
        validate_record(file, &mut previous)?;
        if file.path.as_str() != "Cargo.toml"
            && !(file.path.starts_with("src/") && file.path.ends_with(".rs"))
        {
            return Err(invalid());
        }
        expected.insert(file.path.as_str(), file);
        let mut path = file.path.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            directories.insert(PortableRelativePath::new(parent).map_err(|_| invalid())?);
            path = parent;
        }
    }
    if [
        "Cargo.toml",
        "src/lib.rs",
        "src/__chigiri/generator_info.rs",
    ]
    .into_iter()
    .any(|path| !expected.contains_key(path))
    {
        return Err(invalid());
    }
    let mut differences = Differences::default();
    for (name, file) in &expected {
        match tree.files.get(*name) {
            None => differences.add(Kind::Missing, name),
            Some(bytes) if file.bytes != bytes.len() as u64 || file.sha256 != digest(bytes) => {
                differences.add(Kind::Changed, name);
            }
            Some(_) => {}
        }
    }
    for name in tree.files.keys() {
        if name.as_str() != GENERATED_MARKER_FILE && !expected.contains_key(name.as_str()) {
            differences.add(Kind::Extra, name.as_str());
        }
    }
    for name in tree.directories.difference(&directories) {
        differences.add(Kind::Extra, &format!("{name}/"));
    }
    for name in directories.difference(&tree.directories) {
        differences.add(Kind::Missing, &format!("{name}/"));
    }
    if let Some(error) = differences.error(
        "SF4020",
        "The tree does not match the generated marker's ownership inventory.",
    ) {
        return Err(error);
    }
    for (name, bytes) in &tree.files {
        if name.ends_with(".rs") && std::str::from_utf8(bytes).is_err() {
            return Err(invalid());
        }
    }
    let cargo = std::str::from_utf8(&tree.files["Cargo.toml"]).map_err(|_| invalid())?;
    let cargo = cargo
        .parse::<toml_edit::ImDocument<_>>()
        .map_err(|_| invalid())?;
    let package = cargo
        .get("package")
        .and_then(toml_edit::Item::as_table)
        .ok_or_else(invalid)?;
    if package.get("name").and_then(toml_edit::Item::as_str) != Some(marker.package.name.as_str())
        || package.get("version").and_then(toml_edit::Item::as_str)
            != Some(marker.package.version.as_str())
        || package.get("publish").and_then(toml_edit::Item::as_bool) != Some(false)
        || package.get("edition").and_then(toml_edit::Item::as_str) != Some("2024")
        || package
            .get("rust-version")
            .and_then(toml_edit::Item::as_str)
            != Some("1.95")
        || !cargo
            .get("workspace")
            .is_some_and(toml_edit::Item::is_table)
    {
        return Err(invalid());
    }
    Ok(())
}
