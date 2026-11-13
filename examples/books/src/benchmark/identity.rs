//! Binds each benchmark run to a source revision and explicit toolchain.

use std::fs::File;
use std::io::Read;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::model::{Hardware, SourceIdentity, Toolchain};

pub(super) fn source_identity(version: &str) -> Result<SourceIdentity, Box<dyn std::error::Error>> {
    let revision = required_output("git", &["rev-parse", "HEAD"])?;
    let dirty = !output_allow_empty("git", &["status", "--porcelain"])?.is_empty();
    let executable = std::env::current_exe()?;
    Ok(SourceIdentity {
        revision_unix_seconds: revision_unix_seconds(&revision)?,
        revision,
        dirty,
        binary_sha256: binary_sha256(&executable)?,
        toolchain: toolchain(version)?,
    })
}

fn toolchain(version: &str) -> Result<Toolchain, Box<dyn std::error::Error>> {
    let cargo_path = required_output("rustup", &["which", "--toolchain", version, "cargo"])?;
    let rustc_path = required_output("rustup", &["which", "--toolchain", version, "rustc"])?;
    let cargo = required_output(&cargo_path, &["--version", "--verbose"])?;
    let rustc = required_output(&rustc_path, &["--version", "--verbose"])?;
    let release = format!("release: {version}");
    if !cargo.lines().any(|line| line == release) || !rustc.lines().any(|line| line == release) {
        return Err("Cargo/Rustc identity doesn't match the immutable toolchain version.".into());
    }
    Ok(Toolchain {
        version: version.to_owned(),
        cargo,
        rustc,
    })
}

pub(super) fn hardware() -> Result<Hardware, Box<dyn std::error::Error>> {
    let descriptor = optional_output("sysctl", &["-n", "machdep.cpu.brand_string"])
        .or_else(|| optional_output("env", &["LC_ALL=C", "lscpu"]));
    let descriptor = match descriptor {
        Some(value) => value,
        None => required_output("uname", &["-m"])?,
    };
    let descriptor = if descriptor.contains('\n') {
        stable_lscpu_descriptor(&descriptor)?
    } else {
        descriptor
    };
    if descriptor == std::env::consts::ARCH {
        return Err("The hardware descriptor does not identify the CPU model.".into());
    }
    let os_version = match optional_output("sw_vers", &["-productVersion"]) {
        Some(value) => value,
        None => required_output("uname", &["-r"])?,
    };
    let logical_cpus = logical_cpus()?;
    Ok(Hardware {
        os: std::env::consts::OS.to_owned(),
        os_version,
        arch: std::env::consts::ARCH.to_owned(),
        logical_cpus,
        descriptor,
    })
}

fn logical_cpus() -> Result<usize, Box<dyn std::error::Error>> {
    let value = match optional_output("sysctl", &["-n", "hw.logicalcpu"]) {
        Some(value) => value,
        None => required_output("getconf", &["_NPROCESSORS_ONLN"])?,
    };
    let parsed = value.parse::<usize>()?;
    if parsed == 0 || parsed.to_string() != value {
        return Err("The logical CPU command did not return a positive integer.".into());
    }
    Ok(parsed)
}

fn stable_lscpu_descriptor(output: &str) -> Result<String, Box<dyn std::error::Error>> {
    let labels = ["Vendor ID", "Model name", "CPU family", "Model", "Stepping"];
    let values = labels
        .into_iter()
        .filter_map(|label| {
            output.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                (name.trim() == label && !value.trim().is_empty())
                    .then(|| format!("{label}={}", value.trim()))
            })
        })
        .collect::<Vec<_>>();
    if !values.iter().any(|value| value.starts_with("Model name=")) {
        return Err("lscpu did not return a stable CPU model".into());
    }
    Ok(values.join(";"))
}

pub(super) fn recorded_at_unix_seconds() -> Result<u64, Box<dyn std::error::Error>> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

pub(super) fn revision_unix_seconds(revision: &str) -> Result<u64, Box<dyn std::error::Error>> {
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("The source revision is not 40-character lowercase hexadecimal.".into());
    }
    let value = required_output("git", &["show", "-s", "--format=%ct", revision])?;
    let parsed = value.parse::<u64>()?;
    if parsed == 0 || parsed.to_string() != value {
        return Err("The source revision timestamp is not a canonical positive integer.".into());
    }
    Ok(parsed)
}

pub(super) fn binary_sha256(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut source = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn required_output(
    program: &str,
    arguments: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(program).args(arguments).output()?;
    if !output.status.success() {
        return Err(format!("{program} command failed").into());
    }
    let value = String::from_utf8(output.stdout)?.trim().to_owned();
    if value.is_empty() {
        return Err(format!("{program} command returned empty output").into());
    }
    Ok(value)
}

fn output_allow_empty(
    program: &str,
    arguments: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(program).args(arguments).output()?;
    if !output.status.success() {
        return Err(format!("{program} command failed").into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn optional_output(program: &str, arguments: &[&str]) -> Option<String> {
    required_output(program, arguments).ok()
}

#[cfg(test)]
mod tests {
    use super::stable_lscpu_descriptor;

    #[test]
    fn excludes_dynamic_lscpu_fields_from_the_descriptor() {
        let first = "Vendor ID: GenuineIntel\nModel name: Example CPU\nCPU family: 6\n\
            Model: 1\nStepping: 2\nCPU(s) scaling MHz: 10%";
        let second = first.replace("10%", "90%");
        let expected =
            "Vendor ID=GenuineIntel;Model name=Example CPU;CPU family=6;Model=1;Stepping=2";
        assert_eq!(stable_lscpu_descriptor(first).unwrap(), expected);
        assert_eq!(stable_lscpu_descriptor(&second).unwrap(), expected);
        assert!(stable_lscpu_descriptor("Architecture: x86_64").is_err());
    }
}
