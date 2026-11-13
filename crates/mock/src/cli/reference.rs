//! Read only schema and configuration, then write the complete reference text to the requested output.

use crate::{
    config::{CONFIG_BYTES, Envelope},
    diagnostic::Diagnostic,
};
use chigiri_mock::reference::{Format, InputKind, ReferenceDocument, Selection, Topic};
use std::path::{Path, PathBuf};

struct Arguments {
    schemas: Vec<PathBuf>,
    config: Option<PathBuf>,
    project: Option<PathBuf>,
    format: Format,
    selection: Selection,
}

pub(super) fn run(arguments: &clap::ArgMatches) -> Result<(), Diagnostic> {
    let args = parse(arguments)?;
    let project = args
        .project
        .as_ref()
        .map(|path| chigiri_mock::project::read_project_schema(path))
        .transpose()
        .map_err(Diagnostic::project)?;
    let (kind, ir, configuration) = if let Some(project) = &project {
        (
            InputKind::Project,
            Some(project.ir().clone()),
            Some((
                project.settings().state.clone(),
                project.settings().hooks,
                project.settings().events.clone(),
            )),
        )
    } else if let Some(path) = &args.config {
        let source = crate::files::read(path, CONFIG_BYTES)?;
        let envelope = Envelope::parse(path, source.source())?;
        source.verify()?;
        (
            InputKind::Bundle,
            Some(envelope.ir),
            Some((envelope.state, envelope.hook_settings, envelope.events)),
        )
    } else if !args.schemas.is_empty() {
        (
            InputKind::Schema,
            Some(super::schema::read(&args.schemas)?),
            None,
        )
    } else {
        (InputKind::Common, None, None)
    };
    let document = if let Some((state, settings, events)) = &configuration {
        ReferenceDocument::from_configuration(
            kind,
            ir.as_deref().expect("configured reference IR"),
            state,
            settings,
            events,
        )
    } else {
        ReferenceDocument::from_input(kind, ir.as_deref())
    };
    let document = document
        .map_err(|error| Diagnostic::new(Path::new("reference"), "reference", error.message()))?;
    if !document.accepts(&args.selection) {
        return Err(selection_usage(args.selection.topic));
    }
    let body = document
        .render_selection(args.format, &args.selection)
        .map_err(|error| Diagnostic::new(Path::new("reference"), "reference", error.message()))?;
    if let Some(project) = project {
        project.verify_unchanged().map_err(Diagnostic::project)?;
    }
    super::reference_output::direct(&mut std::io::stdout().lock(), &body)
}

fn selection_usage(topic: Option<Topic>) -> Diagnostic {
    let message = match topic {
        Some(Topic::Directives) => {
            "usage: unknown reference name; run `chigiri mock reference directives`"
        }
        Some(Topic::Methods) => {
            "usage: unknown reference name; run `chigiri mock reference methods`"
        }
        Some(Topic::Hooks) => "usage: unknown reference name; run `chigiri mock reference hooks`",
        Some(Topic::Models) => "usage: unknown reference name; run `chigiri mock reference models`",
        Some(Topic::Types) => "usage: unknown reference name; run `chigiri mock reference types`",
        Some(Topic::Resolvers) => {
            "usage: unknown reference name; run `chigiri mock reference resolvers`"
        }
        Some(Topic::Limits) => {
            "usage: reference limits does not accept a name; run `chigiri mock reference limits`"
        }
        Some(Topic::Consumers) => {
            "usage: unknown reference name; run `chigiri mock reference consumers`"
        }
        Some(Topic::Runtime) => {
            "usage: unknown reference name; run `chigiri mock reference runtime`"
        }
        Some(Topic::All) => {
            "usage: reference all does not accept a name; run `chigiri mock reference all`"
        }
        None => "usage: run `chigiri mock reference` to list valid topics",
    };
    let mut error = Diagnostic::environment(message);
    error.phase = "usage";
    error
}

fn parse(matches: &clap::ArgMatches) -> Result<Arguments, Diagnostic> {
    let topic = matches
        .get_one::<String>("topic")
        .map(|value| Topic::parse(value).expect("Clap must validate the reference topic"));
    let selection = Selection::new(topic, matches.get_one::<String>("name").cloned())
        .ok_or_else(|| selection_usage(topic))?;
    Ok(Arguments {
        schemas: super::schemas(matches)?,
        config: matches.get_one::<PathBuf>("config").cloned(),
        project: matches.get_one::<PathBuf>("config-dir").cloned(),
        format: if matches.get_one::<String>("format").map(String::as_str) == Some("json") {
            Format::Json
        } else {
            Format::Text
        },
        selection,
    })
}
