//! Refuse unprepared or stale distributions without recursively invoking Cargo.

#![forbid(unsafe_code)]

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let package =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("package")?);
    craftguild_cli_build::emit(&package.join("../.."), "chigiri")?;
    Ok(())
}
