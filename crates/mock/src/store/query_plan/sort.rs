//! Stable-sort record handles without copying records, propagating comparison failure and cancellation.

use super::{Atom, QueryPlan};
use crate::store::{Record, work::Work};
use crate::value::ValueError;
use std::cmp::Ordering;
use std::sync::Arc;

impl QueryPlan<'_> {
    pub fn sort(&self, rows: &mut Vec<&Arc<Record>>, work: &mut Work) -> Result<(), ValueError> {
        if self.order.is_empty() || rows.len() < 2 {
            return work.charge(1);
        }
        work.charge(rows.len())?;
        let mut scratch = rows.clone();
        let mut width = 1;
        while width < rows.len() {
            for start in (0..rows.len()).step_by(width * 2) {
                let middle = (start + width).min(rows.len());
                let end = (start + width * 2).min(rows.len());
                let (mut left, mut right) = (start, middle);
                for output in &mut scratch[start..end] {
                    work.charge(1)?;
                    if left < middle
                        && (right == end
                            || self.compare(rows[left], rows[right], work)? != Ordering::Greater)
                    {
                        *output = rows[left];
                        left += 1;
                    } else {
                        *output = rows[right];
                        right += 1;
                    }
                }
            }
            std::mem::swap(rows, &mut scratch);
            width *= 2;
        }
        Ok(())
    }

    fn compare(
        &self,
        left: &Record,
        right: &Record,
        work: &mut Work,
    ) -> Result<Ordering, ValueError> {
        for key in &self.order {
            work.charge(1)?;
            let a = left.get(key.slot).filter(|value| !value.is_null());
            let b = right.get(key.slot).filter(|value| !value.is_null());
            let comparison = match (a, b) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => {
                    let ordering = Atom::stored(a, work)?.compare(Atom::stored(b, work)?, work)?;
                    if key.descending {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                }
            };
            if comparison != Ordering::Equal {
                return Ok(comparison);
            }
        }
        // Preserve input order for equal keys through stable merging.
        Ok(Ordering::Equal)
    }
}
