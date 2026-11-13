//! Runs the ADR-0013 relative benchmark in the release profile.

fn main() {
    std::process::exit(chigiri_books::benchmark::main_entry(
        std::env::args_os().collect(),
    ));
}
