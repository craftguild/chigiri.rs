//! Shared human-readable version rendering for the unified binary.

use std::io::Write as _;

/// Product artwork shared by root help and human-readable version output.
pub(crate) const BANNER: &str = include_str!("../assets/version-banner.txt");
pub(crate) const BRAND: craftguild_cli::Brand<'static> = craftguild_cli::Brand {
    banner: BANNER,
    project: "chigiri.rs",
};

pub fn human(command: &str, components: &[(&str, &str)]) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let info = craftguild_cli::BuildInfo {
        command,
        ..crate::BUILD_INFO
    };
    craftguild_cli::version(&mut output, BRAND, info)?;
    writeln!(
        output,
        "host: {}",
        chigiri_codegen::generator_info().build.host
    )?;
    for (name, value) in components {
        writeln!(output, "{name}: {value}")?;
    }
    Ok(output)
}

pub fn write(command: &str, components: &[(&str, &str)]) -> u8 {
    match human(command, components) {
        Ok(body) => crate::write_stdout(&body),
        Err(_) => 2,
    }
}
