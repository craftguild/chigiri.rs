//! Enforce the nesting ceiling immediately before recursive descent.

use super::{PARSE_NESTING_LIMIT, Parser};

impl Parser {
    pub(super) fn nested<T>(&mut self, parse: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        if self.nesting >= self.max_nesting {
            let span = self.peek().span;
            self.report(
                PARSE_NESTING_LIMIT,
                "GraphQL syntax nesting exceeds the configured parser limit",
                span,
            );
            // Stop parsing this document once the recursion limit is reached.
            self.fatal_error = true;
            return None;
        }
        self.nesting += 1;
        let result = parse(self);
        self.nesting -= 1;
        result
    }
}
