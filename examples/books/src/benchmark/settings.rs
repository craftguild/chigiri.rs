//! Parses the reproducible benchmark command line with a closed grammar.

use std::path::PathBuf;

pub(super) enum ComparisonMode {
    InitialCandidate,
    PreviousRelease(PathBuf),
}

impl ComparisonMode {
    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::InitialCandidate => "initial-candidate",
            Self::PreviousRelease(_) => "previous-release",
        }
    }
}

pub(super) enum Mode {
    Record,
    Compare {
        baseline: PathBuf,
        comparison: ComparisonMode,
    },
}

pub(super) struct Settings {
    pub(super) toolchain: String,
    pub(super) samples: usize,
    pub(super) iterations: usize,
    pub(super) output: PathBuf,
    pub(super) mode: Mode,
    pub(super) command: Vec<String>,
}

impl Settings {
    pub(super) fn parse(command: Vec<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let arguments = command
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        match super::cli::parse(arguments.clone())? {
            super::cli::Parsed::Run(matches) => Self::from_matches(arguments, matches),
            super::cli::Parsed::Display(_) => {
                Err("Benchmark evidence requires an execution command.".into())
            }
        }
    }

    pub(super) fn from_matches(
        arguments: Vec<std::ffi::OsString>,
        matches: clap::ArgMatches,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Do not record argv[0] as evidence identity or parse it again as a path option.
        if arguments
            .first()
            .and_then(|name| std::path::Path::new(name).file_name())
            .and_then(|name| name.to_str())
            != Some("production-benchmark")
        {
            return Err("The benchmark program name does not match the closed contract.".into());
        }
        let (mode, args) = matches
            .subcommand()
            .ok_or("A benchmark mode is required.")?;
        let text = |id| {
            args.get_one::<String>(id)
                .expect("the benchmark CLI definition must be valid")
                .clone()
        };
        let path = |id| {
            args.get_one::<PathBuf>(id)
                .expect("the benchmark CLI definition must be valid")
                .clone()
        };
        let toolchain = text("toolchain");
        require_immutable_toolchain(&toolchain)?;
        let samples = parse_positive(&text("samples"), "samples")?;
        let iterations = parse_positive(&text("iterations"), "iterations")?;
        if samples < 5 {
            return Err("At least five samples are required.".into());
        }
        let parsed_mode = match mode {
            "record" => Mode::Record,
            "compare" => {
                let binary = args.get_one::<PathBuf>("baseline-binary").cloned();
                let comparison = match text("comparison-mode").as_str() {
                    "initial-candidate" if binary.is_none() => ComparisonMode::InitialCandidate,
                    "previous-release" => ComparisonMode::PreviousRelease(
                        binary.ok_or("A baseline binary is required.")?,
                    ),
                    _ => {
                        return Err("comparison mode does not match the baseline binary".into());
                    }
                };
                Mode::Compare {
                    baseline: path("baseline"),
                    comparison,
                }
            }
            _ => return Err("benchmark mode is not in the closed set.".into()),
        };
        let output = path("output");
        let expected_output = if matches!(parsed_mode, Mode::Record) {
            "benchmark-baseline-unaccepted.json"
        } else {
            "benchmark.json"
        };
        if output.file_name().and_then(|name| name.to_str()) != Some(expected_output) {
            return Err("The benchmark output file name doesn't match the mode.".into());
        }
        let command = canonical_command(&toolchain, samples, iterations, &output, &parsed_mode)?;
        Ok(Self {
            toolchain,
            samples,
            iterations,
            output,
            mode: parsed_mode,
            command,
        })
    }
}

fn require_immutable_toolchain(value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let parts = value.split('.').collect::<Vec<_>>();
    let valid = parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        })
        && parts[0] != "0";
    if valid {
        Ok(())
    } else {
        Err("Specify the performance toolchain as an immutable x.y.z version.".into())
    }
}

fn parse_positive(value: &str, label: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let parsed = value.parse::<usize>()?;
    if parsed == 0 || parsed.to_string() != value {
        return Err(format!("{label} must be a canonical positive integer.").into());
    }
    Ok(parsed)
}

fn canonical_command(
    toolchain: &str,
    samples: usize,
    iterations: usize,
    output: &std::path::Path,
    mode: &Mode,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut command = vec![
        "production-benchmark".into(),
        if matches!(mode, Mode::Record) {
            "record"
        } else {
            "compare"
        }
        .into(),
        "--toolchain".into(),
        toolchain.into(),
        "--samples".into(),
        samples.to_string(),
        "--iterations".into(),
        iterations.to_string(),
    ];
    // The path is selected as an OsString and only rejects what cannot be expressed in UTF-8 evidence at this boundary.
    let utf8 = |path: &std::path::Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or("The benchmark path cannot be recorded as UTF-8 evidence.")
    };
    if let Mode::Compare {
        baseline,
        comparison,
    } = mode
    {
        command.extend([
            "--comparison-mode".into(),
            comparison.name().into(),
            "--baseline".into(),
            utf8(baseline)?,
        ]);
        if let ComparisonMode::PreviousRelease(binary) = comparison {
            command.extend(["--baseline-binary".into(), utf8(binary)?]);
        }
    }
    command.extend(["--output".into(), utf8(output)?]);
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::{ComparisonMode, Mode, Settings};

    #[test]
    fn decodes_dx3504_equals_and_normalizes_array_syntax_into_one_evidence_command() {
        let values = [
            "production-benchmark",
            "record",
            "--output=benchmark-baseline-unaccepted.json",
            "--iterations=100",
            "--toolchain=1.95.0",
            "--samples=31",
        ]
        .map(str::to_owned)
        .to_vec();
        let settings = Settings::parse(values).unwrap();
        assert_eq!(
            settings.command,
            [
                "production-benchmark",
                "record",
                "--toolchain",
                "1.95.0",
                "--samples",
                "31",
                "--iterations",
                "100",
                "--output",
                "benchmark-baseline-unaccepted.json"
            ]
        );
        let round_trip = Settings::parse(settings.command.clone()).unwrap();
        assert_eq!(round_trip.command, settings.command);
    }

    #[test]
    fn accepts_only_immutable_versions_and_closed_options() {
        let valid = vec![
            "production-benchmark",
            "record",
            "--toolchain",
            "1.95.0",
            "--samples",
            "31",
            "--iterations",
            "100",
            "--output",
            "benchmark-baseline-unaccepted.json",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert!(matches!(Settings::parse(valid).unwrap().mode, Mode::Record));
        for invalid in ["stable", "1.95", "01.95.0", "1.095.0"] {
            let values = vec![
                "production-benchmark",
                "record",
                "--toolchain",
                invalid,
                "--samples",
                "31",
                "--iterations",
                "100",
                "--output",
                "benchmark-baseline-unaccepted.json",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            assert!(Settings::parse(values).is_err());
        }
    }

    #[test]
    fn rejects_changes_to_program_name_and_output_path() {
        let command = [
            "production-benchmark",
            "record",
            "--toolchain",
            "1.95.0",
            "--samples",
            "31",
            "--iterations",
            "100",
            "--output",
            "benchmark-baseline-unaccepted.json",
        ];
        for (index, value) in [(0, "different-program"), (9, "different.json")] {
            let mut modified = command.map(str::to_owned).to_vec();
            modified[index] = value.to_owned();
            assert!(Settings::parse(modified).is_err());
        }
    }

    #[test]
    fn previous_release_requires_immutable_binary() {
        let values = vec![
            "production-benchmark",
            "compare",
            "--toolchain",
            "1.95.0",
            "--samples",
            "31",
            "--iterations",
            "100",
            "--comparison-mode",
            "previous-release",
            "--baseline",
            "baseline.json",
            "--baseline-binary",
            "old-binary",
            "--output",
            "benchmark.json",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let settings = Settings::parse(values).unwrap();
        assert!(matches!(
            settings.mode,
            Mode::Compare {
                comparison: ComparisonMode::PreviousRelease(_),
                ..
            }
        ));
    }
}
