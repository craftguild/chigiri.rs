//! Defines benchmark CLI behavior so help and version resolve before source observation or measurement.

use super::settings::Settings;
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::{ffi::OsString, io::Write, path::PathBuf};

pub(super) enum Parsed {
    Display(String),
    Run(ArgMatches),
}

pub(super) fn parse(args: Vec<OsString>) -> Result<Parsed, String> {
    let mut definition = command();
    let matches = definition
        .clone()
        .try_get_matches_from(args)
        .map_err(|_| "benchmark usage: invalid command, option or value; see --help")?;
    let child_help = matches
        .subcommand()
        .is_some_and(|(_, child)| child.get_flag("help"));
    if (matches.get_flag("version") && matches.subcommand().is_some())
        || (matches.get_flag("help") && child_help)
    {
        return Err("benchmark usage: duplicate or conflicting terminal options".into());
    }
    if matches.get_flag("version") {
        return Ok(Parsed::Display(format!(
            "production-benchmark {}\n",
            env!("CARGO_PKG_VERSION")
        )));
    }
    let subcommand = matches.subcommand();
    if matches.get_flag("help")
        || subcommand.is_some_and(|(name, child)| name == "help" || child.get_flag("help"))
    {
        let selected = match subcommand {
            Some(("help", child)) => child.get_one::<String>("command").map(String::as_str),
            Some((name, _)) => Some(name),
            None => None,
        };
        definition.build();
        let body = if let Some(name) = selected {
            definition
                .find_subcommand_mut(name)
                .ok_or("benchmark usage: unknown help command")?
                .render_long_help()
        } else {
            definition.render_long_help()
        };
        return Ok(Parsed::Display(body.to_string()));
    }
    if subcommand.is_none() {
        return Err("benchmark usage: command required; see --help".into());
    }
    Ok(Parsed::Run(matches))
}

fn command() -> Command {
    let record = common(
        Command::new("record")
            .about("Record an unaccepted baseline with source and hardware evidence"),
    );
    let compare = common(
        Command::new("compare").about("Compare measurements with an explicitly accepted baseline"),
    )
    .arg(required(
        Arg::new("comparison-mode")
            .long("comparison-mode")
            .value_parser(["initial-candidate", "previous-release"]),
    ))
    .arg(required(path("baseline")))
    .arg(path("baseline-binary").required_if_eq("comparison-mode", "previous-release"));
    Command::new("production-benchmark")
        .about("Measure production profiles using an immutable toolchain")
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .arg(help().conflicts_with("version"))
        .arg(
            Arg::new("version")
                .long("version")
                .short('V')
                .action(ArgAction::SetTrue),
        )
        .subcommands([record, compare])
        .subcommand(
            Command::new("help")
                .disable_help_flag(true)
                .arg(help())
                .arg(Arg::new("command").value_parser(["record", "compare"])),
        )
}

fn common(command: Command) -> Command {
    command
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .arg(help())
        .arg(required(
            Arg::new("toolchain")
                .long("toolchain")
                .help("Immutable x.y.z Rust version"),
        ))
        .arg(required(
            Arg::new("samples")
                .long("samples")
                .help("Canonical positive integer; at least 5 samples"),
        ))
        .arg(required(
            Arg::new("iterations")
                .long("iterations")
                .help("Canonical positive iteration count"),
        ))
        .arg(required(
            path("output").help("Mode-specific benchmark evidence filename"),
        ))
}

fn required(arg: Arg) -> Arg {
    let help = format!(
        "{} (required)",
        arg.get_help().map(ToString::to_string).unwrap_or_default()
    );
    arg.help(help).required_unless_present("help")
}
fn path(id: &'static str) -> Arg {
    Arg::new(id)
        .long(id)
        .value_parser(clap::value_parser!(PathBuf))
}
fn help() -> Arg {
    Arg::new("help")
        .long("help")
        .short('h')
        .action(ArgAction::SetTrue)
}

pub(super) fn main_entry(arguments: Vec<OsString>) -> i32 {
    let settings = match parse(arguments.clone()) {
        Ok(Parsed::Display(body)) => {
            return if std::io::stdout().lock().write_all(body.as_bytes()).is_ok() {
                0
            } else {
                2
            };
        }
        Ok(Parsed::Run(matches)) => match Settings::from_matches(arguments, matches) {
            Ok(settings) => settings,
            Err(_) => {
                eprintln!("benchmark usage: invalid settings or evidence path; see --help");
                return 2;
            }
        },
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("benchmark: cannot create runtime");
            return 2;
        }
    };
    match runtime.block_on(super::run(settings)) {
        Ok(()) => 0,
        Err(_) => {
            eprintln!("benchmark: measurement or evidence validation failed");
            1
        }
    }
}
