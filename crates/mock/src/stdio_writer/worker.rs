//! Deliver length-prefixed records to one output stream and acknowledge each completed flush.

use std::io::{self, Read, Write};

/// Serve the private writer protocol used by the CLI's child process.
/// The output is a copied stdout or stderr handle; acknowledgments use a separate parent-only pipe.
pub fn serve_writer(
    mut input: impl Read,
    mut output: impl Write,
    mut ack: impl Write,
) -> io::Result<()> {
    loop {
        let mut header = [0u8; 4];
        match input.read(&mut header[..1]) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
        input.read_exact(&mut header[1..])?;
        let length = u32::from_be_bytes(header) as usize;
        if length == 0 || length > super::LINE_BYTES {
            return Err(io::Error::other("output frame limit"));
        }
        let mut bytes = vec![0u8; length];
        input.read_exact(&mut bytes)?;
        if bytes.last() != Some(&b'\n')
            || std::str::from_utf8(&bytes).is_err()
            || bytes[..bytes.len() - 1].iter().any(|byte| *byte < 32)
        {
            return Err(io::Error::other("invalid output line"));
        }
        let delivered = write_record(&mut output, &bytes);
        ack.write_all(&[delivered])?;
        ack.flush()?;
        if delivered == b'2' {
            // Close this sink after a partial write or flush failure; no later line can repair it.
            return Ok(());
        }
    }
}

fn write_record(output: &mut impl Write, bytes: &[u8]) -> u8 {
    let mut offset = 0;
    while offset < bytes.len() {
        match output.write(&bytes[offset..]) {
            Ok(0) => break,
            Ok(length) => offset += length,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    if offset == bytes.len() && output.flush().is_ok() {
        b'1'
    } else if offset == 0 {
        b'0'
    } else {
        b'2'
    }
}
