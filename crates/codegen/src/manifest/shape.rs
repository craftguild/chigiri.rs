//! Grammar for configured paths and Cargo identifiers; runtime paths may traverse only through parents.

pub(super) fn relative(value: &str, parents: bool) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\\', ':'])
        && !value.chars().any(char::is_control)
        && value.split('/').all(|part| {
            !part.is_empty() && part.len() <= 255 && (parents || !matches!(part, "." | ".."))
        })
}

pub(super) fn name(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 255
        && bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

pub(super) fn package(value: &str) -> bool {
    name(&value.replace('-', "_")) && value != "_"
}

pub(super) fn alias(value: &str) -> bool {
    name(value) && value != "_" && !matches!(value, "self" | "Self" | "super" | "crate")
}

pub(super) fn feature(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-+./?".contains(&byte))
}

pub(super) fn commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

pub(super) fn git_url(value: &str) -> bool {
    if value.len() > 4096
        || value
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        || value.contains(['?', '#', '\\'])
    {
        return false;
    }
    let (ssh, rest) = if let Some(rest) = value.strip_prefix("https://") {
        (false, rest)
    } else if let Some(rest) = value.strip_prefix("ssh://") {
        (true, rest)
    } else {
        return false;
    };
    let Some((authority, path)) = rest.split_once('/') else {
        return false;
    };
    if path.is_empty()
        || !percent_encoding(path)
        || !path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
    {
        return false;
    }
    let host = if let Some((user, host)) = authority.split_once('@') {
        if !ssh
            || user.is_empty()
            || !user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return false;
        }
        host
    } else {
        authority
    };
    let (host, port) = if host.starts_with('[') {
        let Some((address, suffix)) = host.split_once(']') else {
            return false;
        };
        if address[1..].parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        (
            address,
            suffix
                .strip_prefix(':')
                .or_else(|| suffix.is_empty().then_some("")),
        )
    } else {
        let (host, port) = host.split_once(':').unwrap_or((host, ""));
        if host.is_empty()
            || !host.split('.').all(|part| {
                !part.is_empty()
                    && !part.starts_with('-')
                    && !part.ends_with('-')
                    && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return false;
        }
        (host, Some(port))
    };
    !host.is_empty()
        && port.is_some_and(|port| {
            port.is_empty() || port.parse::<u16>().is_ok_and(|value| value != 0)
        })
}

fn percent_encoding(path: &str) -> bool {
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%'
            && !(0..2).all(|_| bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit()))
        {
            return false;
        }
    }
    true
}

pub(super) fn overlaps(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
