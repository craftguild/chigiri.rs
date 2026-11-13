#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fs::File;
use std::io::pipe;
use std::process::Stdio;
use std::time::{Duration, Instant};

use chigiri_mock::stdio_writer::{LINE_BYTES, Pipe};

// A freshly linked executable can need several seconds to start on macOS.
// Readiness is separate from the existing transport/shutdown deadlines.
const STARTUP: Duration = Duration::from_secs(30);

fn executable() -> &'static std::path::Path {
    std::path::Path::new(env!("CARGO_BIN_EXE_chigiri"))
}

fn delivery(pipe: &mut Pipe, deadline: Instant) -> Option<bool> {
    while Instant::now() < deadline {
        if let Some(delivered) = pipe.pump() {
            return Some(delivered);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}

#[test]
fn host_06_writer_transport_delivers_and_applies_capacity_one_backpressure() {
    let directory = std::env::temp_dir().join(format!(
        "chigiri-writer-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let output_path = directory.join("output");
    let output = File::create(&output_path).unwrap();
    let mut pipe = Pipe::start(executable(), Ok(Stdio::from(output)));

    assert!(pipe.put(b"first line\n".to_vec()));
    assert!(!pipe.put(b"second line\n".to_vec()));
    let started = Instant::now();
    let receipt = delivery(&mut pipe, started + STARTUP);
    assert_eq!(
        receipt,
        Some(true),
        "writer delivery: elapsed={:?}, child={:?}, sink_bytes={:?}",
        started.elapsed(),
        pipe.process_id(),
        std::fs::metadata(&output_path).map(|metadata| metadata.len())
    );
    assert!(pipe.idle());
    assert!(!pipe.put(vec![b'x'; LINE_BYTES + 1]));
    assert!(pipe.stop(Instant::now() + Duration::from_secs(2)));
    assert_eq!(std::fs::read(&output_path).unwrap(), b"first line\n");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn host_06_blocked_output_unblocks_when_the_owned_child_is_terminated() {
    let (reader, writer) = pipe().unwrap();
    let mut output = Pipe::start(executable(), Ok(Stdio::from(writer)));
    // Observe a completed write before treating an absent short receipt as
    // saturation. Child startup alone does not establish pipe backpressure.
    assert!(output.put(b"ready\n".to_vec()));
    assert_eq!(
        delivery(&mut output, Instant::now() + STARTUP),
        Some(true),
        "writer must deliver before the pressure phase"
    );
    let frame = {
        let mut bytes = vec![b'x'; LINE_BYTES];
        *bytes.last_mut().unwrap() = b'\n';
        bytes
    };

    let saturation_deadline = Instant::now() + Duration::from_secs(2);
    let mut blocked = false;
    while Instant::now() < saturation_deadline {
        if output.idle() {
            assert!(output.put(frame.clone()));
        }
        if delivery(&mut output, Instant::now() + Duration::from_millis(20)).is_none() {
            blocked = true;
            break;
        }
    }
    assert!(blocked, "the unread pipe did not saturate");

    let started = Instant::now();
    assert!(output.stop(started + Duration::from_secs(2)));
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(reader);
}

#[test]
fn host_05_writer_cleanup_finishes_within_the_supplied_deadline() {
    let (reader, writer) = pipe().unwrap();
    let mut output = Pipe::start(executable(), Ok(Stdio::from(writer)));
    let pid = rustix::process::Pid::from_raw(output.process_id().unwrap() as i32).unwrap();
    let mut frame = vec![b'x'; LINE_BYTES];
    *frame.last_mut().unwrap() = b'\n';
    assert!(output.put(frame));

    let started = Instant::now();
    let deadline = started + Duration::from_secs(2);
    assert!(output.stop(deadline));
    assert!(Instant::now() < deadline);
    drop(output);
    assert!(Instant::now() < deadline);
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    );
    assert_eq!(
        rustix::process::test_kill_process_group(pid),
        Err(rustix::io::Errno::SRCH)
    );
    drop(reader);
}
