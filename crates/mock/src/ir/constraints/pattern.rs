//! Intern schema-owned regular expressions and bound their compilation and serialized evaluation.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use chigiri::core::Span;
use regex::{Regex, RegexBuilder};

use crate::schema::SchemaError;
use crate::value::ValueError;

use crate::api::constraints::{
    REGEX_BUILD_SECONDS, REGEX_CACHE_BYTES as CACHE_BYTES, REGEX_COMPILED_BYTES as COMPILED_BYTES,
    REGEX_NESTING, REGEX_PATTERNS as PATTERNS, REGEX_SOURCE_BYTES as SOURCE_BYTES,
};

/// Share a compiled regex with its source and charge schema work only when evaluating it.
#[derive(Debug)]
pub struct Pattern {
    source: Arc<str>,
    regex: Regex,
    gate: Arc<Mutex<()>>,
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Pattern {
    /// Return the original regex source for generated reference output.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Check the shared budget before waiting, after acquiring the gate, and after matching.
    pub fn is_match(
        &self,
        text: &str,
        mut guard: impl FnMut() -> Result<(), ValueError>,
    ) -> Result<bool, ValueError> {
        if text.len() > crate::value::MAX_STRING_BYTES {
            return Err(ValueError("regex input limit"));
        }
        let lock = loop {
            guard()?;
            match self.gate.try_lock() {
                Ok(lock) => break lock,
                Err(TryLockError::WouldBlock) => std::thread::yield_now(),
                Err(TryLockError::Poisoned(_)) => return Err(ValueError("regex gate poisoned")),
            }
        };
        guard()?;
        let matched = self.regex.is_match(text);
        drop(lock);
        guard()?;
        Ok(matched)
    }
}

pub(crate) struct PatternRegistry {
    pub(super) work: crate::value::work::Work,
    patterns: BTreeMap<Arc<str>, Arc<Pattern>>,
    gate: Arc<Mutex<()>>,
    source_bytes: usize,
    deadline: Instant,
}

impl Default for PatternRegistry {
    fn default() -> Self {
        Self {
            work: crate::value::work::Work::default(),
            patterns: BTreeMap::new(),
            gate: Arc::new(Mutex::new(())),
            source_bytes: 0,
            deadline: Instant::now() + Duration::from_secs(REGEX_BUILD_SECONDS),
        }
    }
}

impl PatternRegistry {
    pub(crate) fn into_work(self) -> crate::value::work::Work {
        self.work
    }

    pub(super) fn intern(&mut self, source: &str, span: Span) -> Result<Arc<Pattern>, SchemaError> {
        let error = |code| SchemaError::new(code, Some(span));
        if Instant::now() >= self.deadline {
            return Err(error("regex construction deadline"));
        }
        if source.len() > SOURCE_BYTES {
            return Err(error("regex source limit"));
        }
        if let Some(pattern) = self.patterns.get(source) {
            return Ok(pattern.clone());
        }
        // Bound compiled regex memory and cache reservations after construction, when their sizes are available.
        if self.patterns.len() >= PATTERNS
            || source.len() > PATTERNS * SOURCE_BYTES - self.source_bytes
        {
            return Err(error("schema regex reservation limit"));
        }
        self.source_bytes += source.len();
        let regex = RegexBuilder::new(source)
            .size_limit(COMPILED_BYTES)
            .dfa_size_limit(CACHE_BYTES)
            .nest_limit(REGEX_NESTING)
            .build()
            .map_err(|_| error("invalid or oversized regex"))?;
        if Instant::now() >= self.deadline {
            return Err(error("regex construction deadline"));
        }
        let source: Arc<str> = Arc::from(source);
        let pattern = Arc::new(Pattern {
            source: source.clone(),
            regex,
            gate: self.gate.clone(),
        });
        self.patterns.insert(source, pattern.clone());
        Ok(pattern)
    }
}
