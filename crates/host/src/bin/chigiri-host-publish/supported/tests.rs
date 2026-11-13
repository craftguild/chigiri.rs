use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

fn fixture() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the test clock is after the epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "chigiri-host-publish-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("the fixture root is created");
    let root = root.canonicalize().expect("the fixture root is canonical");
    let private = root.join("private");
    let slot = root.join("slot");
    fs::create_dir(&private).expect("the private fixture is created");
    fs::create_dir(&slot).expect("the slot fixture is created");
    (root, private, slot)
}

fn arguments(
    action: &str,
    private: &Path,
    slot: &Path,
    candidate: Identity,
    previous: Option<Identity>,
) -> Vec<OsString> {
    let mut values: Vec<OsString> = [
        OsString::from(action),
        private.as_os_str().to_owned(),
        slot.as_os_str().to_owned(),
        OsString::from("chigiri"),
        OsString::from(candidate.device.to_string()),
        OsString::from(candidate.inode.to_string()),
    ]
    .into();
    if let Some(previous) = previous {
        values.extend([
            OsString::from(previous.device.to_string()),
            OsString::from(previous.inode.to_string()),
        ]);
    }
    values
}

#[test]
fn atomic_install_and_exchange_retain_the_expected_identity() {
    let (root, private, slot) = fixture();
    fs::write(private.join(CANDIDATE), b"first").expect("candidate is written");
    let opened = Chain::open(&private).expect("private chain opens");
    let first = named(&opened, OsStr::new(CANDIDATE)).expect("candidate is named");
    publish(&arguments("install", &private, &slot, first, None)).expect("install succeeds");
    assert_eq!(fs::read(slot.join("chigiri")).unwrap(), b"first");

    fs::write(private.join(CANDIDATE), b"second").expect("replacement is written");
    let opened = Chain::open(&private).expect("private chain reopens");
    let second = named(&opened, OsStr::new(CANDIDATE)).expect("replacement is named");
    let exchange_arguments = arguments("exchange", &private, &slot, second, Some(first));
    let racing = root.join("racing");
    let failure = publish_with_hook(&exchange_arguments, || {
        fs::write(&racing, b"racing").expect("racing candidate is written");
        fs::rename(&racing, slot.join("chigiri")).expect("public name is replaced");
    });
    assert_eq!(
        failure.unwrap_err(),
        "atomic publication identities changed"
    );
    assert_eq!(fs::read(slot.join("chigiri")).unwrap(), b"racing");
    assert_eq!(fs::read(private.join(CANDIDATE)).unwrap(), b"second");

    let opened = Chain::open(&slot).expect("slot chain reopens");
    let racing = named(&opened, OsStr::new("chigiri")).expect("racing entry is named");
    publish(&arguments(
        "exchange",
        &private,
        &slot,
        second,
        Some(racing),
    ))
    .expect("exchange succeeds");
    assert_eq!(fs::read(slot.join("chigiri")).unwrap(), b"second");
    assert_eq!(fs::read(private.join(CANDIDATE)).unwrap(), b"racing");
    fs::remove_dir_all(root).expect("fixture is removed");
}
