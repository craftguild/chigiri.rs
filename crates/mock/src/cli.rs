//! Dispatch Clap-validated commands to operations that read only explicit project paths.

mod check;
mod check_navigation;
mod check_report;
mod init;
mod reference;
mod reference_output;
mod schema;
mod schema_warnings;
mod update;

use crate::config::Config;
use crate::diagnostic::Diagnostic;
use std::path::PathBuf;

pub type CommandError = Diagnostic;

/// Writes bounded terminal output using the Mock namespace's pipe contract.
pub fn write_terminal_output(body: &[u8]) -> Result<(), CommandError> {
    reference_output::direct(&mut std::io::stdout().lock(), body)
}

/// Runs one command selected by the unified product command tree.
pub fn run_matches(command: &str, args: &clap::ArgMatches) -> Result<(), CommandError> {
    match command {
        chigiri_mock::stdio_writer::COMMAND => chigiri_mock::stdio_writer::serve_writer(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            std::io::stderr().lock(),
        )
        .map_err(|_| Diagnostic::environment("isolated output writer failed")),
        "init" => init::run(schemas(args)?, path(args, "config-dir")),
        "reference" => reference::run(args),
        "update" => update::run(args, path(args, "config-dir")),
        "codegen" => update::codegen(path(args, "config-dir")),
        "check" => check::run(args),
        "serve" => {
            let config = if let Some(path) = args.get_one::<PathBuf>("config") {
                Config::read(path)?
            } else {
                Config::project(path(args, "config-dir"))?
            };
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|_| Diagnostic::environment("cannot create async runtime"))?;
            runtime.block_on(crate::server::serve(
                config,
                args.get_flag("enable-graphiql"),
            ))
        }
        "export" => {
            chigiri_mock::project::export_project(path(args, "config-dir"), path(args, "output"))
                .map_err(Diagnostic::project)?;
            println!("chigiri mock export: bundle placed; readiness_checked = true");
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn schemas(matches: &clap::ArgMatches) -> Result<Vec<PathBuf>, Diagnostic> {
    let schemas = matches
        .get_many::<PathBuf>("schema")
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    if schemas.len() > crate::limits::SDL_SOURCES {
        return Err(usage());
    }
    Ok(schemas)
}

pub fn usage_error(message: &'static str) -> CommandError {
    Diagnostic::environment(message)
}

fn path<'a>(args: &'a clap::ArgMatches, id: &str) -> &'a std::path::Path {
    args.get_one::<PathBuf>(id)
        .expect("clap must provide the required path")
}

fn usage() -> Diagnostic {
    let mut error = Diagnostic::environment("usage: see --help for explicit command inputs");
    error.phase = "usage";
    error
}
