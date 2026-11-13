//! Export independently rerendered, checked bytes into an empty private directory.

#![forbid(unsafe_code)]

use std::{io::Write, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [manifest, output] = args.as_slice() else {
        return Err("Usage: reproduce-owned MANIFEST EMPTY_OUTPUT".into());
    };
    let output = PathBuf::from(output);
    let kind = output.symlink_metadata()?.file_type();
    if !kind.is_dir() || kind.is_symlink() || output.read_dir()?.next().is_some() {
        return Err("Reproduction requires an empty private directory".into());
    }
    let manifest = chigiri_codegen::ResolvedManifest::load(&PathBuf::from(manifest))?;
    let checked = chigiri_codegen::check_contract(&manifest)?;
    let rendered = checked.render(&manifest)?;
    for (name, bytes) in rendered.files() {
        let path = output.join(name);
        std::fs::create_dir_all(path.parent().ok_or("Missing output parent")?)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()?;
    }
    Ok(())
}
