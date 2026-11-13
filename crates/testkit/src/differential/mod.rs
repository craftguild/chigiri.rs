//! Public testkit for strict old/new response comparison, ignoring only object ordering.

mod allowlist;
mod compare;
mod model;

pub use allowlist::{AllowlistEntry, DifferenceClassification, parse_allowlist};
pub use compare::{ComparisonCase, ComparisonError, compare_corpus};
pub use model::ObservedResponse;
