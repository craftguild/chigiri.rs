#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    std::panic::set_hook(Box::new(|_| eprintln!("chigiri [worker] internal failure")));
    std::process::ExitCode::from(chigiri_cli::run(std::env::args_os().skip(1).collect()))
}
