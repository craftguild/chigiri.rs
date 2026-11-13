//! Unified command boundary for chigiri.rs Rust and Mock workflows.

#![forbid(unsafe_code)]

mod grammar;
mod mock;
mod rust;
mod version;

include!(concat!(env!("OUT_DIR"), "/craftguild.rs"));

use std::ffi::OsString;
use std::io::Write as _;

use grammar::Action;

pub fn run(arguments: Vec<OsString>) -> u8 {
    let rust_format = rust::diagnostic_format_hint(&arguments);
    match grammar::parse(arguments) {
        Ok(Action::RootHelp(body)) => terminal(craftguild_cli::help(
            &mut std::io::stdout().lock(),
            version::BRAND,
            &body,
        )),
        Ok(Action::Help(body)) => write_stdout(body.as_bytes()),
        Ok(Action::MockHelp(body)) => write_stdout(body.as_bytes()),
        Ok(Action::RootVersion) => version::write("chigiri", &[]),
        Ok(Action::Sbom) => terminal(craftguild_cli::sbom(&mut std::io::stdout().lock(), SBOM)),
        Ok(Action::RustVersion(format)) => rust::version(format),
        Ok(Action::MockVersion) => mock::version(),
        Ok(Action::Rust(action)) => rust::run(action),
        Ok(Action::Mock { command, args }) => mock::run(&command, &args),
        Ok(Action::Writer) => mock::writer(),
        Err(message) if rust_format.is_some() => {
            rust::parse_error(&message, rust_format.expect("checked format"))
        }
        Err(message) => {
            eprintln!("chigiri: {message}");
            2
        }
    }
}

pub(crate) fn write_stdout(bytes: &[u8]) -> u8 {
    let mut stdout = std::io::stdout().lock();
    terminal(stdout.write_all(bytes).and_then(|()| stdout.flush()))
}

fn terminal(result: std::io::Result<()>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(_) => 2,
    }
}
