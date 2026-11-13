//! Write bounded command output directly and treat a closed pipe as success.

use crate::diagnostic::Diagnostic;
use std::io::{self, Write};

pub(super) fn direct(out: &mut impl Write, body: &[u8]) -> Result<(), Diagnostic> {
    match out.write_all(body).and_then(|()| out.flush()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(_) => Err(Diagnostic::environment("cannot write command output")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Failure {
        kind: io::ErrorKind,
        flush: bool,
    }
    impl Write for Failure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.flush {
                Ok(bytes.len())
            } else {
                Err(self.kind.into())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(self.kind.into())
        }
    }

    #[test]
    fn mr_21_22_broken_pipe_succeeds_but_write_and_flush_io_failures_return_two() {
        for flush in [false, true] {
            assert!(
                direct(
                    &mut Failure {
                        kind: io::ErrorKind::BrokenPipe,
                        flush
                    },
                    b"body"
                )
                .is_ok()
            );
            for kind in [
                io::ErrorKind::StorageFull,
                io::ErrorKind::PermissionDenied,
                io::ErrorKind::Other,
            ] {
                assert_eq!(
                    direct(&mut Failure { kind, flush }, b"body")
                        .unwrap_err()
                        .exit,
                    2
                );
            }
        }
    }
}
