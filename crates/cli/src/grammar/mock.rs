//! Mock namespace grammar preserved under the unified root.

use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command};

use super::{closed, flag};

pub fn command() -> Command {
    let commands = [
        Command::new("init")
            .about("Create a new editable project; never overwrite an existing directory")
            .arg(
                schema()
                    .help("Required SDL source; repeat for multiple files")
                    .required_unless_present_any(["help", "version"]),
            )
            .arg(required_path("config-dir", "DIR", "New project directory")),
        Command::new("update")
            .about("Adopt public SDL and generate while preserving edits and recovery materials")
            .arg(required_path(
                "config-dir",
                "DIR",
                "Existing project directory",
            ))
            .arg(flag(
                "check",
                "Compare public SDL freshness without writing",
            ))
            .arg(
                Arg::new("format")
                    .long("format")
                    .value_parser(["text", "json"])
                    .requires("check")
                    .help("Freshness report format (default: text)"),
            ),
        Command::new("codegen")
            .about("Generate from adopted public SDL and current project-local inputs")
            .arg(required_path(
                "config-dir",
                "DIR",
                "Existing project directory",
            )),
        input_command(
            "check",
            "Check project or portable bundle readiness without rewriting source",
        )
        .arg(
            Arg::new("format")
                .long("format")
                .value_parser(["text", "json"])
                .default_value("text")
                .help("Diagnostic output format"),
        )
        .arg(
            Arg::new("target")
                .long("target")
                .value_name("PATH")
                .action(ArgAction::Append)
                .requires("config-dir")
                .conflicts_with("config")
                .help("Exact resolver, seed, enabled-hook, or registered-consumer path; repeat as needed"),
        ),
        input_command(
            "serve",
            "Serve on loopback after readiness checks; restart resets initial data",
        )
        .arg(flag(
            "enable-graphiql",
            "Publish GraphiQL UI and schema introspection for this process (default: off)",
        )),
        Command::new("export")
            .about("Export a checked project as a new portable TOML bundle")
            .arg(required_path(
                "config-dir",
                "DIR",
                "Existing project directory",
            ))
            .arg(required_path("output", "FILE", "New bundle file")),
        reference_command(),
    ];
    let names = commands
        .iter()
        .map(|command| command.get_name().to_owned())
        .collect::<Vec<_>>();
    closed(
        Command::new("mock")
            .about("Run experimental local schema Mock workflows for chigiri.rs")
            .subcommands(commands.into_iter().map(closed))
            .subcommand(closed(
                Command::new("help")
                    .about("Print Mock command help")
                    .arg(Arg::new("command").value_parser(names)),
            )),
    )
}

pub fn parse(matches: &ArgMatches) -> Result<(String, ArgMatches), String> {
    let (name, args) = matches
        .subcommand()
        .ok_or_else(|| "usage: Mock command required; see --help".to_owned())?;
    Ok((name.to_owned(), args.clone()))
}

fn reference_command() -> Command {
    Command::new("reference")
        .about("Read contracts; project mode verifies generation but not readiness")
        .after_help(
            "Topics: directives, methods, hooks, models, types, resolvers, consumers, limits, runtime, all.\nNames are accepted for directives, methods, hooks, models, types, resolvers, consumers, and runtime.",
        )
        .arg(Arg::new("topic").value_name("TOPIC").value_parser([
            "directives",
            "methods",
            "hooks",
            "models",
            "types",
            "resolvers",
            "consumers",
            "limits",
            "runtime",
            "all",
        ]))
        .arg(Arg::new("name").value_name("NAME").requires("topic"))
        .arg(schema().conflicts_with_all(["config", "config-dir"]))
        .arg(path("config-dir", "DIR", "Adopted project contract").conflicts_with("config"))
        .arg(path("config", "FILE", "Portable bundle schema"))
        .arg(
            Arg::new("format")
                .long("format")
                .value_parser(["text", "json"])
                .default_value("text"),
        )
}

fn input_command(name: &'static str, about: &'static str) -> Command {
    Command::new(name)
        .about(about)
        .after_help("Specify exactly one of --config or --config-dir.")
        .arg(
            path("config", "FILE", "Portable bundle file")
                .conflicts_with("config-dir")
                .required_unless_present_any(["config-dir", "help", "version"]),
        )
        .arg(path("config-dir", "DIR", "Project directory"))
}

fn path(id: &'static str, name: &'static str, help: &'static str) -> Arg {
    Arg::new(id)
        .long(id)
        .value_name(name)
        .help(help)
        .value_parser(clap::value_parser!(PathBuf))
}

fn required_path(id: &'static str, name: &'static str, help: &'static str) -> Arg {
    path(id, name, help)
        .help(format!("{help} (required)"))
        .required_unless_present_any(["help", "version"])
}

fn schema() -> Arg {
    path("schema", "SDL", "SDL source; repeat for multiple files").action(ArgAction::Append)
}
