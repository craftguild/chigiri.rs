//! Compare base, user, and next candidate bytes without treating a `.dist` proposal as adoption.

use super::digest::Digest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decision {
    Unchanged,
    Replace,
    Adopt,
    Keep,
    Propose,
}

pub(super) fn compare(base: &Digest, user: &Digest, next: &Digest) -> Decision {
    if user == base {
        if user == next {
            Decision::Unchanged
        } else {
            Decision::Replace
        }
    } else if user == next {
        Decision::Adopt
    } else if next == base {
        Decision::Keep
    } else {
        Decision::Propose
    }
}

impl Decision {
    pub fn advances_base(self) -> bool {
        matches!(self, Self::Replace | Self::Adopt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp_09_10_all_equalities_have_one_action_and_only_installed_or_adopted_bytes_advance_base() {
        let a = Digest::bytes(b"a");
        let b = Digest::bytes(b"b");
        let c = Digest::bytes(b"c");
        for (base, user, next, decision, advance) in [
            (&a, &a, &a, Decision::Unchanged, false),
            (&a, &a, &b, Decision::Replace, true),
            (&a, &b, &b, Decision::Adopt, true),
            (&a, &b, &a, Decision::Keep, false),
            (&a, &b, &c, Decision::Propose, false),
        ] {
            let actual = compare(base, user, next);
            assert_eq!(actual, decision);
            assert_eq!(actual.advances_base(), advance);
        }
    }

    #[test]
    fn mp_09_10_repeated_proposals_and_independent_user_migration_never_become_pristine() {
        let mut base = Digest::bytes(b"pristine");
        let user = Digest::bytes(b"user implementation");
        for next in [
            b"candidate one".as_slice(),
            b"candidate one",
            b"candidate two",
        ] {
            let next = Digest::bytes(next);
            let decision = compare(&base, &user, &next);
            assert_eq!(decision, Decision::Propose);
            if decision.advances_base() {
                base = next;
            }
            assert_eq!(base, Digest::bytes(b"pristine"));
        }
        let adopted = Digest::bytes(b"candidate two");
        assert_eq!(compare(&base, &adopted, &adopted), Decision::Adopt);
        base = adopted;
        assert_eq!(compare(&base, &user, &base), Decision::Keep);
    }
}
