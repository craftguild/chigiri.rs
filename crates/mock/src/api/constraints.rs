//! Shared constraint semantics and regex resource limits used by execution and
//! reference output.

use super::{FailureClass, RuleSpec};

pub(crate) const REGEX_SOURCE_BYTES: usize = 1024;
pub(crate) const REGEX_PATTERNS: usize = 16;
pub(crate) const REGEX_COMPILED_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const REGEX_CACHE_BYTES: usize = 1024 * 1024;
pub(crate) const REGEX_NESTING: u32 = 64;
pub(crate) const REGEX_BUILD_SECONDS: u64 = 5;
pub(crate) const REGEX_MATCHES: usize =
    crate::limits::request::policy(crate::limits::request::LimitKind::RegexMatches).hard;
pub(crate) const REGEX_MATCH_BYTES: usize =
    crate::limits::request::policy(crate::limits::request::LimitKind::RegexInputBytes).hard;
pub(crate) const REGEX_WORK_FACTOR: usize = REGEX_COMPILED_BYTES / (1024 * 1024);

pub(crate) const RULES: &[RuleSpec] = &[
    RuleSpec {
        id: "constraint.locations",
        description: "@constraint applies to FIELD_DEFINITION, INPUT_FIELD_DEFINITION, and ARGUMENT_DEFINITION. The same rules validate seeds, writes, input literals, variables, defaults, and real resolver output. Strings support minLength, maxLength, startsWith, endsWith, contains, notContains, pattern, and format. Numbers support min, max, exclusiveMin, exclusiveMax, and multipleOf. Lists support minItems and maxItems. errorMessage and uniqueTypeName are unsupported. Unknown, contradictory, or inapplicable constraints are rejected before startup.",
        failure: FailureClass::Storage,
        example: "create.record",
    },
    RuleSpec {
        id: "constraint.unicode",
        description: "String length counts Unicode scalar values. Combining characters, variation selectors, and ZWJ each count separately; values are not normalized or case-folded. maxLength:1 accepts 😀 and é, but rejects e plus U+0301 and ✈ plus U+FE0F. maxLength:0 accepts only the empty string. UTF-8 byte resource limits are separate.",
        failure: FailureClass::Storage,
        example: "create.record",
    },
    RuleSpec {
        id: "constraint.wrappers",
        description: "minItems and maxItems apply only to the outermost list. String and number constraints apply to each scalar after recursively unwrapping lists, but do not propagate into object fields. For [[String]], minItems:2 with minLength:1 accepts [[\"a\"],[]] and rejects [[\"\"],[]]. Null bypasses these constraints; SDL enforces non-null. Custom scalar representation constraints are combined with field constraints using AND.",
        failure: FailureClass::Storage,
        example: "create.record",
    },
    RuleSpec {
        id: "constraint.multiple_of",
        description: "A multipleOf divisor must be positive and finite. Validated integers use exact decimal notation; f64 values use their shortest round-trip canonical decimal representation. Divisibility requires an exact zero remainder with no epsilon. multipleOf:0.1 accepts 0.3, -0.3, and 0, but rejects 0.31 and 0.1 + 0.2 (0.30000000000000004). Integer 3 satisfies multipleOf:0.5. min, max, and exclusive bounds also compare finite values exactly.",
        failure: FailureClass::Storage,
        example: "create.record",
    },
    RuleSpec {
        id: "constraint.formats",
        description: "Formats validate raw strings without rewriting or external lookup. byte requires canonical padded RFC 4648 Base64, including unused-bit validation (empty and Zg== are valid; Zh== is not). date requires a real YYYY-MM-DD date in years 0001-9999. date-time adds T/t, HH:mm:ss, optional fractional seconds, and a required Z/z or ±HH:mm timezone; -00:00 is allowed. email permits UTF-8 or quoted local parts with an ASCII DNS domain of at least two labels, limits the local part to 64 bytes and the whole address to 254 bytes, and rejects display names, comments, domain literals, and Unicode domains. ipv4 uses four decimal octets without leading zeroes. ipv6 permits compression and embedded IPv4 but rejects brackets, zones, and CIDR. uri requires an RFC 3986 scheme; mailto: and urn: are valid, while relative URIs, whitespace, non-ASCII text, and port 65536 are invalid. uuid accepts case-insensitive hex in 8-4-4-4-12 form without restricting version or variant, including nil and max UUIDs. Values are never trimmed or repaired, and custom format names are unsupported.",
        failure: FailureClass::Storage,
        example: "create.record",
    },
    RuleSpec {
        id: "constraint.regex_dialect",
        description: r#"Patterns use the Rust regex 1.13.1 dialect rather than JavaScript RegExp. Matching is Unicode-aware and case-sensitive by default; inline flags such as (?i) are supported. Patterns are not implicitly anchored, so abc also matches xabcx; use \Aabc\z for a whole-string match. JavaScript literal syntax such as /abc/i is not accepted. Unsupported features, including lookaround such as (?=x) and backreferences such as (a)\1, are rejected without rewriting or fallback."#,
        failure: FailureClass::Storage,
        example: "create.record",
    },
    RuleSpec {
        id: "constraint.regex_resources",
        description: "Each distinct pattern source is compiled once per schema and shared through Arc. A schema gate serializes matching. Every is_match evaluation counts once, including empty strings, mismatches, and repeated checks, and reserves input bytes plus work equal to 1 + regex.work_factor × (UTF-8 bytes + 1). See regex.* limits. An exceeded limit is sticky; catching it with pcall does not allow the root transaction to commit. Cancellation and deadlines are checked around gate acquisition and synchronous matching. The limits do not guarantee interruption during synchronous build or match work, CPU time, process RSS, or interruption through an async timeout.",
        failure: FailureClass::Resource,
        example: "transaction.caught",
    },
];
