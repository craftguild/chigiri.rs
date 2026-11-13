//! Validate config-v2 required items and cross-table constraints once.

use toml_edit::{ImDocument, Table};

use super::{Consumer, MANIFEST_SCHEMA, Manifest, Package, RuntimeSource, shape, table::Context};
use crate::CodegenError;

pub(super) fn parse(label: &str, source: &str) -> Result<Manifest, CodegenError> {
    let cx = Context { label, source };
    if source.len() > 64 * 1024 {
        return Err(cx.error(None, "The manifest is above the 64 KiB limit."));
    }
    let document = source.parse::<ImDocument<String>>().map_err(|error| {
        cx.error_at(
            error.span().map_or(0, |span| span.start),
            "SF4011",
            "The manifest contains invalid TOML syntax.",
        )
    })?;
    let root = document.as_table();
    cx.closed(
        root,
        &[
            "schema_version",
            "schema",
            "package",
            "generation",
            "runtime",
            "bindings",
            "consumer",
        ],
    )?;
    cx.require(
        root,
        "schema_version",
        cx.string(root, "schema_version")? == MANIFEST_SCHEMA,
        "Manifest schema must be chigiri/config-v2.",
    )?;
    let schema = cx.table(root, "schema", &["directory"])?;
    let schema_directory = path(&cx, schema, "directory", false)?;
    let table = cx.table(root, "package", &["name", "version", "edition", "license"])?;
    let package = Package {
        name: cx.string(table, "name")?,
        version: cx.string(table, "version")?,
        edition: cx.string(table, "edition")?,
        license: table
            .get("license")
            .map(|_| cx.string(table, "license"))
            .transpose()?,
    };
    cx.require(
        table,
        "license",
        package
            .license
            .as_ref()
            .is_none_or(|value| !value.trim().is_empty()),
        "The package license must contain non-whitespace text.",
    )?;
    cx.require(
        table,
        "name",
        shape::package(&package.name),
        "The Cargo package name is invalid.",
    )?;
    cx.require(
        table,
        "version",
        package.version.len() <= 256 && semver::Version::parse(&package.version).is_ok(),
        "Specify the package version as valid SemVer.",
    )?;
    cx.require(
        table,
        "edition",
        package.edition == "2024",
        "Specify Rust edition 2024.",
    )?;
    let generation = cx.table(root, "generation", &["output"])?;
    let output = path(&cx, generation, "output", false)?;
    cx.require(
        generation,
        "output",
        !shape::overlaps(&schema_directory, &output),
        "Schema and output paths must not overlap.",
    )?;
    let runtime = runtime(&cx, root)?;
    let mut binding_labels = std::collections::BTreeMap::new();
    let domain_enums = if root.contains_key("bindings") {
        let table = cx.table(root, "bindings", &["domain_enums"])?;
        let names = cx.strings(table, "domain_enums", "SF4012")?;
        if let Some(array) = table
            .get("domain_enums")
            .and_then(toml_edit::Item::as_array)
        {
            for (name, value) in names.iter().zip(array.iter()) {
                let offset = value.span().map_or(0, |span| span.start);
                if !shape::name(name) {
                    return Err(cx.error_at(
                        offset,
                        "SF4012",
                        "domain_enums must be an array of GraphQL Name values.",
                    ));
                }
                binding_labels.insert(name.clone(), cx.label(offset));
            }
        }
        names
    } else {
        Vec::new()
    };
    let consumer = if root.contains_key("consumer") {
        Some(consumer(&cx, root, &output)?)
    } else {
        None
    };
    Ok(Manifest {
        schema_directory,
        package,
        output,
        runtime,
        domain_enums,
        binding_labels,
        consumer,
    })
}

fn path(cx: &Context<'_>, table: &Table, key: &str, parents: bool) -> Result<String, CodegenError> {
    let value = cx.string(table, key)?;
    cx.require(
        table,
        key,
        shape::relative(&value, parents),
        "The path must use only permitted relative components.",
    )?;
    Ok(value)
}

fn runtime(cx: &Context<'_>, root: &Table) -> Result<RuntimeSource, CodegenError> {
    let table = cx.table(
        root,
        "runtime",
        &["source", "url", "revision", "version", "root"],
    )?;
    match cx.string(table, "source")?.as_str() {
        "path" => {
            cx.closed(table, &["source", "root"])?;
            Ok(RuntimeSource::Path {
                root: path(cx, table, "root", true)?,
            })
        }
        "git" => {
            cx.closed(table, &["source", "url", "revision", "version"])?;
            let url = cx.string(table, "url")?;
            let revision = cx.string(table, "revision")?;
            cx.require(
                table,
                "url",
                shape::git_url(&url),
                "Specify a credential-free HTTPS or SSH Git URL.",
            )?;
            cx.require(
                table,
                "revision",
                shape::commit(&revision),
                "revision must be a nonzero 40-character lowercase hexadecimal commit ID.",
            )?;
            let version = if table.contains_key("version") {
                let version = cx.string(table, "version")?;
                cx.require(
                    table,
                    "version",
                    version.len() <= 256 && semver::Version::parse(&version).is_ok(),
                    "Specify the runtime package version as valid SemVer, without a requirement operator.",
                )?;
                Some(version)
            } else {
                None
            };
            Ok(RuntimeSource::Git {
                url,
                revision,
                version,
            })
        }
        _ => Err(cx.error(
            table.get("source"),
            "The runtime source must be either `git` or `path`.",
        )),
    }
}

fn consumer(cx: &Context<'_>, root: &Table, output: &str) -> Result<Consumer, CodegenError> {
    let table = cx.table(
        root,
        "consumer",
        &[
            "cargo_manifest",
            "package",
            "generated_dependency",
            "features",
            "all_features",
            "no_default_features",
            "all_targets",
        ],
    )?;
    let consumer = Consumer {
        cargo_manifest: path(cx, table, "cargo_manifest", false)?,
        package: cx.string(table, "package")?,
        generated_dependency: cx.string(table, "generated_dependency")?,
        features: cx.strings(table, "features", "SF4011")?,
        all_features: cx.boolean(table, "all_features", false)?,
        no_default_features: cx.boolean(table, "no_default_features", false)?,
        all_targets: cx.boolean(table, "all_targets", true)?,
    };
    cx.require(
        table,
        "package",
        shape::package(&consumer.package),
        "The consumer package name is invalid.",
    )?;
    cx.require(
        table,
        "generated_dependency",
        shape::alias(&consumer.generated_dependency),
        "generated_dependency must be a Rust dependency alias.",
    )?;
    cx.require(
        table,
        "features",
        consumer
            .features
            .iter()
            .all(|feature| shape::feature(feature)),
        "The feature name is invalid.",
    )?;
    cx.require(
        table,
        "all_features",
        !consumer.all_features || (consumer.features.is_empty() && !consumer.no_default_features),
        "all_features cannot be combined with features or no_default_features.",
    )?;
    cx.require(
        table,
        "cargo_manifest",
        !shape::overlaps(&consumer.cargo_manifest, output),
        "The consumer Cargo manifest and output paths must not overlap.",
    )?;
    Ok(consumer)
}
