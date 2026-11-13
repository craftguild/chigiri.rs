//! Closed root grammar and terminal routing for the unified binary.

mod mock;
mod rust;

use std::ffi::OsStr;
use std::ffi::OsString;

use chigiri_codegen::DiagnosticFormat;
use clap::{Arg, ArgAction, ArgMatches, Command};

pub use rust::{ContractCommand, RustAction};

pub enum Action {
    RootHelp(String),
    Help(String),
    MockHelp(String),
    RootVersion,
    Sbom,
    RustVersion(DiagnosticFormat),
    MockVersion,
    Rust(RustAction),
    Mock { command: String, args: ArgMatches },
    Writer,
}

pub fn parse(arguments: Vec<OsString>) -> Result<Action, String> {
    reject_repeated_globals(&arguments)?;
    let mut definition = command();
    let matches = definition
        .clone()
        .try_get_matches_from(arguments)
        .map_err(|error| message(error.kind()))?;
    let (help, version) = terminal_flags(&matches);
    if matches.subcommand_name() == Some(chigiri_mock::stdio_writer::COMMAND) {
        return if help || version {
            Err(usage())
        } else {
            Ok(Action::Writer)
        };
    }
    let path = command_path(&matches);
    if version {
        return match path.first().copied() {
            Some("rust") => Ok(Action::RustVersion(rust::format(&matches))),
            Some("mock") => Ok(Action::MockVersion),
            _ => Ok(Action::RootVersion),
        };
    }
    if help || is_help(&matches) || path.is_empty() || (path.len() < 2 && path[0] != "sbom") {
        let help_path = help_path(&matches);
        let mock_help = help_path.first() == Some(&"mock");
        let root_help = help_path.is_empty();
        let body = render_help(&mut definition, help_path)?;
        return if root_help {
            Ok(Action::RootHelp(body))
        } else if mock_help {
            Ok(Action::MockHelp(body))
        } else {
            Ok(Action::Help(body))
        };
    }
    let (namespace, namespace_args) = matches.subcommand().ok_or_else(usage)?;
    match namespace {
        "sbom" => Ok(Action::Sbom),
        "rust" => rust::parse(namespace_args).map(Action::Rust),
        "mock" => {
            let (command, args) = mock::parse(namespace_args)?;
            Ok(Action::Mock { command, args })
        }
        _ => Err(usage()),
    }
}

fn reject_repeated_globals(arguments: &[OsString]) -> Result<(), String> {
    for (long, short) in [("help", Some("-h")), ("version", Some("-V"))] {
        if occurrences(arguments, long, short) > 1 {
            return Err("usage: duplicate or conflicting arguments; see --help".into());
        }
    }
    if arguments.iter().any(|argument| argument == "rust") {
        for long in ["manifest", "format", "deny-warnings"] {
            if occurrences(arguments, long, None) > 1 {
                return Err("usage: duplicate or conflicting arguments; see --help".into());
            }
        }
    }
    Ok(())
}

fn occurrences(arguments: &[OsString], long: &str, short: Option<&str>) -> usize {
    let exact = format!("--{long}");
    let prefix = format!("--{long}=");
    arguments
        .iter()
        .filter(|argument| {
            argument.as_os_str() == OsStr::new(&exact)
                || argument
                    .to_str()
                    .is_some_and(|value| value.starts_with(&prefix))
                || short.is_some_and(|value| argument.as_os_str() == OsStr::new(value))
        })
        .count()
}

fn command() -> Command {
    closed(
        Command::new("chigiri")
            .no_binary_name(true)
            .about("chigiri.rs schema-first Rust and local Mock workflows")
            .arg(
                flag("help", "Print help")
                    .short('h')
                    .global(true)
                    .conflicts_with("version"),
            )
            .arg(
                flag("version", "Print build or namespace version")
                    .short('V')
                    .global(true),
            )
            .subcommand(rust::command())
            .subcommand(mock::command())
            .subcommand(closed(
                Command::new("sbom").about("Print the embedded CycloneDX 1.5 SBOM"),
            ))
            .subcommand(help_command())
            .subcommand(closed(
                Command::new(chigiri_mock::stdio_writer::COMMAND).hide(true),
            )),
    )
}

fn help_command() -> Command {
    closed(
        Command::new("help")
            .about("Print help for the root, a namespace, or a command")
            .arg(Arg::new("command").num_args(0..=2).value_parser([
                "rust",
                "mock",
                "init",
                "codegen",
                "update",
                "check",
                "doctor",
                "serve",
                "export",
                "reference",
                "sbom",
            ])),
    )
}

pub(super) fn closed(command: Command) -> Command {
    command
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .args_override_self(false)
        .infer_subcommands(false)
        .infer_long_args(false)
}

pub(super) fn flag(id: &'static str, help: &'static str) -> Arg {
    Arg::new(id).long(id).help(help).action(ArgAction::SetTrue)
}

fn terminal_flags(mut matches: &ArgMatches) -> (bool, bool) {
    let mut selected = (false, false);
    loop {
        selected.0 |= matches.get_flag("help");
        selected.1 |= matches.get_flag("version");
        match matches.subcommand() {
            Some((_, child)) => matches = child,
            None => return selected,
        }
    }
}

fn is_help(matches: &ArgMatches) -> bool {
    let path = command_path(matches);
    path.contains(&"help")
}

fn help_path(matches: &ArgMatches) -> Vec<&str> {
    let path = command_path(matches);
    if path.first() == Some(&"help") {
        return matches
            .subcommand_matches("help")
            .and_then(|args| args.get_many::<String>("command"))
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
    }
    if path.get(1) == Some(&"help") {
        let namespace = path[0];
        let args = matches
            .subcommand_matches(namespace)
            .and_then(|args| args.subcommand_matches("help"));
        let mut result = vec![namespace];
        result.extend(
            args.and_then(|args| args.get_one::<String>("command"))
                .map(String::as_str),
        );
        return result;
    }
    path
}

fn command_path(mut matches: &ArgMatches) -> Vec<&str> {
    let mut path = Vec::new();
    while let Some((name, child)) = matches.subcommand() {
        path.push(name);
        matches = child;
    }
    path
}

fn render_help(definition: &mut Command, path: Vec<&str>) -> Result<String, String> {
    definition.build();
    let mut page = definition;
    for name in path {
        page = page.find_subcommand_mut(name).ok_or_else(usage)?;
    }
    let help = page.render_long_help().to_string();
    Ok(help)
}

fn message(kind: clap::error::ErrorKind) -> String {
    use clap::error::ErrorKind;
    match kind {
        ErrorKind::ArgumentConflict => {
            "usage: duplicate or conflicting arguments; see --help".into()
        }
        ErrorKind::MissingRequiredArgument | ErrorKind::MissingSubcommand => {
            "usage: required command or value missing; see --help".into()
        }
        _ => usage(),
    }
}

fn usage() -> String {
    "usage: invalid command, option or value; see --help".into()
}
