//! Standard-library-only bounded metadata strings.

pub(super) fn commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

pub(super) fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

pub(super) fn semver(value: &str) -> bool {
    if value.is_empty() || value.len() > 256 {
        return false;
    }
    let base = match value.split_once('+') {
        Some((base, build)) if identifiers(build, false) => base,
        Some(_) => return false,
        None => value,
    };
    let numbers = match base.split_once('-') {
        Some((numbers, pre)) if identifiers(pre, true) => numbers,
        Some(_) => return false,
        None => base,
    };
    let mut parts = numbers.split('.');
    (0..3).all(|_| parts.next().is_some_and(number)) && parts.next().is_none()
}

fn number(value: &str) -> bool {
    !value.is_empty()
        && (value.len() == 1 || !value.starts_with('0'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

fn identifiers(value: &str, prerelease: bool) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && (!prerelease
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || part.len() == 1
                || !part.starts_with('0'))
    })
}

pub(super) fn rustc(value: &str) -> bool {
    if value == "unknown" {
        return true;
    }
    if value.len() > 256 {
        return false;
    }
    let Some(banner) = value.strip_prefix("rustc ") else {
        return false;
    };
    let Some((version, details)) = banner.split_once(" (") else {
        return false;
    };
    let Some(details) = details.strip_suffix(')') else {
        return false;
    };
    let Some((hash, date)) = details.split_once(' ') else {
        return false;
    };
    semver(version)
        && (7..=40).contains(&hash.len())
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        && date.len() == 10
        && date.bytes().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}
