//! Compare binary bytes and verify startup without source files or Cargo search paths.

use super::{BINARY, Directory};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::process::Command;

pub(super) fn command(directory: &Directory) -> Command {
    let binary = directory.0.join("chigiri");
    let bytes = fs::copy(BINARY, &binary).unwrap();
    assert_eq!(bytes, fs::metadata(BINARY).unwrap().len());
    let mut source = BufReader::new(File::open(BINARY).unwrap());
    let mut copy = BufReader::new(File::open(&binary).unwrap());
    let mut block = [0; 64 * 1024];
    let mut other = [0; 64 * 1024];
    loop {
        let read = source.read(&mut block).unwrap();
        copy.read_exact(&mut other[..read]).unwrap();
        assert_eq!(block[..read], other[..read]);
        if read == 0 {
            assert_eq!(copy.read(&mut other).unwrap(), 0);
            break;
        }
    }
    let mut command = Command::new(binary);
    command.env_clear().env("PATH", "");
    command
}
