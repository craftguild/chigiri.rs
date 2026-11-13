//! Tracks generator inputs during normal Cargo builds without generating from SDL.

#![forbid(unsafe_code)]

#[path = "build/identity.rs"]
mod identity;
// When we build, we only use a portion of the common process API.
#[allow(dead_code)]
#[path = "src/process/mod.rs"]
mod process;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let package =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("package")?);
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("output")?);
    // Untracked additions and dirty→clean are also normally re-observed in build.
    // Avoid rewriting unchanged metadata so dependent-crate caches remain valid.
    println!(
        "cargo:rerun-if-changed={}",
        output.join("recapture-every-build").display()
    );
    let root = package
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("root")?;
    let info = identity::capture(root);
    info.validate()?;
    let source = identity::rust_literal(&info);
    let destination = output.join("generator-info.rs");
    if std::fs::read(&destination).ok().as_deref() != Some(source.as_bytes()) {
        std::fs::write(destination, source)?;
    }
    Ok(())
}
