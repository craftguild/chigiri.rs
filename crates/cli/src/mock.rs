//! Mock command adapter and same-executable writer boundary.

use clap::ArgMatches;

pub fn run(command: &str, args: &ArgMatches) -> u8 {
    finish(chigiri_mock::cli::run_matches(command, args))
}

pub fn writer() -> u8 {
    finish(
        chigiri_mock::stdio_writer::serve_writer(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            std::io::stderr().lock(),
        )
        .map_err(|_| chigiri_mock::cli::usage_error("isolated output writer failed")),
    )
}

pub fn version() -> u8 {
    crate::version::write(
        "chigiri mock",
        &[("luaur", chigiri_mock::info::LUAUR_VERSION)],
    )
}

fn finish(result: Result<(), chigiri_mock::cli::CommandError>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            if !error.output_owned() {
                eprintln!("{error}");
            }
            error.exit_code()
        }
    }
}
