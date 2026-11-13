//! Define shared request-limit names, policies, notices, and failures for execution and reference output.

/// Monotonic resource totals charged to one request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum LimitKind {
    /// Work charged by execution, validation, and conversion.
    Work,
    /// Number of logical value nodes.
    ValueNodes,
    /// Number of logical bytes.
    LogicalBytes,
    /// Number of regex matches attempted.
    RegexMatches,
    /// Number of input bytes passed to regex matching.
    RegexInputBytes,
    /// Milliseconds from receipt.
    ElapsedMs,
}

/// All request resources in stable reporting order.
pub const KINDS: [LimitKind; 6] = [
    LimitKind::Work,
    LimitKind::ValueNodes,
    LimitKind::LogicalBytes,
    LimitKind::RegexMatches,
    LimitKind::RegexInputBytes,
    LimitKind::ElapsedMs,
];

/// Policy whose soft threshold warns and whose hard threshold rejects before reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitPolicy {
    /// Stable resource name used in JSON and reference output.
    pub name: &'static str,
    /// Usage at which the first warning is emitted.
    pub soft: usize,
    /// Maximum permissible use.
    pub hard: usize,
}

/// Return the compiled-in policy for one resource.
pub const fn policy(kind: LimitKind) -> LimitPolicy {
    let (name, hard) = match kind {
        LimitKind::Work => ("work", 1_000_000),
        LimitKind::ValueNodes => ("valueNodes", 100_000),
        LimitKind::LogicalBytes => ("logicalBytes", 16 * 1024 * 1024),
        LimitKind::RegexMatches => ("regexMatches", 4096),
        LimitKind::RegexInputBytes => ("regexInputBytes", 1024 * 1024),
        LimitKind::ElapsedMs => ("elapsedMs", 5000),
    };
    LimitPolicy {
        name,
        soft: hard * 3 / 4,
        hard,
    }
}

/// Limit notice that does not include input values or source text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitNotice {
    /// Resource whose threshold was reached.
    pub kind: LimitKind,
    /// Observed usage, or projected usage for a refused reservation.
    pub observed: usize,
}

/// Copyable failure classification that does not carry a dynamic error string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetFailure {
    /// Shared resource exceeded its hard limit.
    Hard(LimitNotice),
    /// Per-VM hard guard was exceeded.
    LocalHard(LocalLimit),
    /// Request or local-VM deadline expired.
    Deadline,
    /// Request or worker was cancelled.
    Cancelled,
    /// Host failure other than resources.
    Execution,
}

/// Hard guard and allocator limits that apply independently to each VM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalLimit {
    /// VM allocator reported an out-of-memory failure.
    VmMemory,
    /// Host-call count exceeded its per-VM limit.
    HostCalls,
    /// Do not allow `pcall` to suppress this failure before the protected-call boundary handles it.
    PcallReentry,
}

impl LocalLimit {
    /// Fixed name not including input value.
    pub const fn name(self) -> &'static str {
        match self {
            Self::VmMemory => "vmMemoryBytes",
            Self::HostCalls => "hostCalls",
            Self::PcallReentry => "pcallReentry",
        }
    }

    /// Return the hard limit enforced by the corresponding guard.
    pub const fn hard(self) -> usize {
        match self {
            Self::VmMemory => crate::value::MAX_BYTES,
            Self::HostCalls => crate::api::HOST_CALL_LIMIT,
            Self::PcallReentry => crate::protected_call::REENTRY_LIMIT,
        }
    }

    /// Return the first rejected usage count when one can be represented.
    pub const fn observed(self) -> Option<usize> {
        match self {
            Self::VmMemory => None,
            _ => Some(self.hard() + 1),
        }
    }
}
