//! Warn about valid schemas whose required relations cannot form a finite Mock value.

use std::io::Write;

use chigiri_mock::ir::RequiredReferenceCycle;

pub(super) fn emit_cycles(cycles: &[RequiredReferenceCycle]) {
    let mut stderr = std::io::stderr().lock();
    for cycle in cycles {
        let path = bounded_path(cycle.path());
        let _ = writeln!(
            stderr,
            "warning: required model reference cycle: {path}; finite Mock values cannot satisfy this non-null singular cycle"
        );
    }
}

pub(super) fn bounded_path(path: &[String]) -> String {
    let mut out = String::new();
    for (index, coordinate) in path.iter().enumerate() {
        if index != 0 {
            out.push_str(" -> ");
        }
        let remaining = 4096usize.saturating_sub(out.len());
        if coordinate.len() <= remaining {
            out.push_str(coordinate);
        } else {
            if remaining <= 3 {
                let end = out.floor_char_boundary(4093);
                out.truncate(end);
            } else {
                let end = coordinate.floor_char_boundary(remaining - 3);
                out.push_str(&coordinate[..end]);
            }
            out.push_str("...");
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::bounded_path;

    #[test]
    fn warning_path_is_bounded_at_a_utf8_boundary() {
        let path = vec!["Résumé.link".repeat(1024), "Résumé.owner".into()];
        let result = bounded_path(&path);
        assert!(result.len() <= 4096);
        assert!(result.ends_with("..."));
    }
}
