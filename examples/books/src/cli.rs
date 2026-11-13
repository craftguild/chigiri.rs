//! Defines the Books CLI so `listen` uses the same Clap command before and after help output.

use clap::{Arg, ArgAction, Command};
use std::{ffi::OsString, io::Write, net::SocketAddr};

pub(super) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<SocketAddr>, ()> {
    let mut definition = Command::new("chigiri_books")
        .about("Application-owned books GraphQL server")
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .arg(
            Arg::new("address")
                .value_name("ADDRESS")
                .default_value("127.0.0.1:8000")
                .value_parser(clap::value_parser!(SocketAddr))
                .help("Numeric listen address and port"),
        )
        .arg(
            Arg::new("help")
                .long("help")
                .short('h')
                .action(ArgAction::SetTrue)
                .conflicts_with("version"),
        )
        .arg(
            Arg::new("version")
                .long("version")
                .short('V')
                .action(ArgAction::SetTrue),
        )
        .subcommand(
            Command::new("help")
                .about("Print help")
                .disable_help_flag(true),
        );
    let matches = definition.clone().try_get_matches_from(args).map_err(|_| {
        eprintln!("books usage: invalid address or arguments; see --help");
    })?;
    let display = if matches.get_flag("help") || matches.subcommand_name() == Some("help") {
        Some(definition.render_long_help().to_string())
    } else if matches.get_flag("version") {
        Some(format!("chigiri_books {}\n", env!("CARGO_PKG_VERSION")))
    } else {
        None
    };
    if let Some(body) = display {
        std::io::stdout()
            .lock()
            .write_all(body.as_bytes())
            .map_err(|_| {
                eprintln!("books: cannot write help or version");
            })?;
        return Ok(None);
    }
    Ok(matches.get_one::<SocketAddr>("address").copied())
}
