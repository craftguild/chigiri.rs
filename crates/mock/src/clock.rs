//! Server and request apparent-time policies, independent of host deadlines.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use luaur::{Function, Lua, LuaOptions, StdLib, Table, Value};

use crate::value::ValueError;

const SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// One declarative startup clock policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Policy {
    /// Read the host wall clock for every apparent-current-time call.
    #[default]
    System,
    /// Return one admitted Unix timestamp in seconds.
    Fixed(i64),
}

/// Clock configuration retained in project and portable artifacts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ClockSettings {
    policy: Policy,
}

impl ClockSettings {
    /// Use progressing host wall time.
    pub fn system() -> Self {
        Self::default()
    }

    /// Use one native-admitted Unix timestamp in seconds.
    pub fn fixed(value: i64) -> Result<Self, ValueError> {
        validate_timestamp(value)?;
        Ok(Self::fixed_validated(value))
    }

    pub(crate) fn fixed_in(lua: &Lua, value: i64) -> Result<Self, ValueError> {
        validate_timestamp_in(lua, value)?;
        Ok(Self::fixed_validated(value))
    }

    fn fixed_validated(value: i64) -> Self {
        Self {
            policy: Policy::Fixed(value),
        }
    }

    /// Return the declarative mode name.
    pub fn mode(&self) -> &'static str {
        match self.policy {
            Policy::System => "system",
            Policy::Fixed(_) => "fixed",
        }
    }

    /// Return the configured fixed seconds, when present.
    pub fn fixed_value(&self) -> Option<i64> {
        match self.policy {
            Policy::System => None,
            Policy::Fixed(value) => Some(value),
        }
    }

    pub(crate) fn policy(&self) -> Policy {
        self.policy
    }
}

pub(crate) trait TimeSource: Send + Sync {
    fn now(&self) -> Result<i64, ValueError>;
}

#[derive(Debug)]
struct SystemSource;

impl TimeSource for SystemSource {
    fn now(&self) -> Result<i64, ValueError> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ValueError("system clock before Unix epoch"))?;
        i64::try_from(duration.as_secs()).map_err(|_| ValueError("system clock overflow"))
    }
}

/// The immutable startup and atomically captured current policy for one server.
pub(crate) struct ServerClock {
    startup: Policy,
    current: Mutex<Policy>,
    source: Arc<dyn TimeSource>,
}

impl ServerClock {
    pub(crate) fn new(settings: &ClockSettings) -> Arc<Self> {
        Self::with_source(settings.policy(), Arc::new(SystemSource))
    }

    fn with_source(startup: Policy, source: Arc<dyn TimeSource>) -> Arc<Self> {
        Arc::new(Self {
            startup,
            current: Mutex::new(startup),
            source,
        })
    }

    pub(crate) fn capture(&self) -> Arc<RequestClock> {
        let policy = *self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::new(RequestClock {
            captured: policy,
            local: Mutex::new(None),
            sealed: AtomicBool::new(false),
            source: self.source.clone(),
        })
    }

    pub(crate) fn candidate(&self) -> Policy {
        *self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn publish(&self, policy: Policy) {
        *self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = policy;
    }

    pub(crate) fn startup(&self) -> Policy {
        self.startup
    }

    pub(crate) fn read_policy(&self, policy: Policy) -> Result<i64, ValueError> {
        read(policy, self.source.as_ref())
    }

    #[cfg(test)]
    pub(crate) fn with_test_source(
        settings: &ClockSettings,
        source: Arc<dyn TimeSource>,
    ) -> Arc<Self> {
        Self::with_source(settings.policy(), source)
    }
}

/// One request's captured server policy and optional preparation override.
pub(crate) struct RequestClock {
    captured: Policy,
    local: Mutex<Option<i64>>,
    sealed: AtomicBool,
    source: Arc<dyn TimeSource>,
}

impl RequestClock {
    pub(crate) fn now(&self) -> Result<i64, ValueError> {
        let local = *self
            .local
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        read(
            local.map(Policy::Fixed).unwrap_or(self.captured),
            self.source.as_ref(),
        )
    }

    #[cfg(test)]
    pub(crate) fn travel_to(&self, value: i64) -> Result<(), ValueError> {
        if self.sealed.load(Ordering::Acquire) {
            return Err(ValueError("request clock is sealed"));
        }
        validate_timestamp(value)?;
        self.travel_to_validated(value);
        Ok(())
    }

    pub(crate) fn travel_to_in(&self, lua: &Lua, value: i64) -> Result<(), ValueError> {
        if self.sealed.load(Ordering::Acquire) {
            return Err(ValueError("request clock is sealed"));
        }
        validate_timestamp_in(lua, value)?;
        self.travel_to_validated(value);
        Ok(())
    }

    fn travel_to_validated(&self, value: i64) {
        *self
            .local
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(value);
    }

    pub(crate) fn reset(&self) -> Result<(), ValueError> {
        if self.sealed.load(Ordering::Acquire) {
            return Err(ValueError("request clock is sealed"));
        }
        *self
            .local
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }

    pub(crate) fn seal(&self) {
        self.sealed.store(true, Ordering::Release);
    }
}

fn read(policy: Policy, source: &dyn TimeSource) -> Result<i64, ValueError> {
    match policy {
        // The host source is already an integer Unix clock. Keep live reads
        // cheap; the native round trip belongs at fixed-policy admission.
        Policy::System => source.now().and_then(validate_exact_range),
        Policy::Fixed(value) => Ok(value),
    }
}

fn validate_exact_range(value: i64) -> Result<i64, ValueError> {
    if (0..=SAFE_INTEGER).contains(&value) {
        Ok(value)
    } else {
        Err(ValueError("timestamp outside exact native range"))
    }
}

/// Admit a timestamp with the pinned native UTC decomposition and round trip.
pub(crate) fn validate_timestamp(value: i64) -> Result<i64, ValueError> {
    validate_exact_range(value)?;
    let deadline = Instant::now() + crate::runtime::VM_TIME;
    let _lease = crate::vm_gate::enter(deadline, || false)
        .map_err(|_| ValueError("timestamp validation unavailable"))?;
    let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default())
        .map_err(|_| ValueError("timestamp validation unavailable"))?;
    lua.set_memory_limit(crate::value::MAX_BYTES)
        .map_err(|_| ValueError("timestamp validation unavailable"))?;
    let control = crate::sandbox::install_budget(&lua, deadline)
        .map_err(|_| ValueError("timestamp validation unavailable"))?;
    let result = validate_timestamp_in(&lua, value);
    control
        .borrow_mut()
        .check()
        .map_err(|_| ValueError("timestamp validation unavailable"))?;
    result
}

/// Validate through the native functions of an already admitted and bounded VM.
pub(crate) fn validate_timestamp_in(lua: &Lua, value: i64) -> Result<i64, ValueError> {
    validate_exact_range(value)?;
    let native = |name: &str| {
        lua.named_registry_value::<Function>(&format!("mock.native.os.{name}"))
            .or_else(|_| {
                let os: Table = lua.globals().raw_get("os")?;
                os.raw_get(name)
            })
            .map_err(|_| ValueError("timestamp validation unavailable"))
    };
    let date = native("date")?;
    let fields: Value = date
        .call(("!*t", value as f64))
        .map_err(|_| ValueError("timestamp native decomposition failed"))?;
    let Value::Table(fields) = fields else {
        return Err(ValueError("timestamp native decomposition failed"));
    };
    let time = native("time")?;
    let roundtrip: Option<f64> = time
        .call(fields)
        .map_err(|_| ValueError("timestamp native round trip failed"))?;
    if roundtrip == Some(value as f64) {
        Ok(value)
    } else {
        Err(ValueError("timestamp native round trip failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicI64;
    use std::time::Duration;

    struct Manual(AtomicI64);
    impl TimeSource for Manual {
        fn now(&self) -> Result<i64, ValueError> {
            Ok(self.0.fetch_add(1, Ordering::Relaxed))
        }
    }

    #[test]
    fn tl_02_system_progresses_and_fixed_stays_fixed() {
        let _case = crate::vm_gate::test_case();
        let source = Arc::new(Manual(AtomicI64::new(1_700_000_000)));
        let system = ServerClock::with_test_source(&ClockSettings::system(), source.clone());
        let request = system.capture();
        assert_eq!(request.now().unwrap(), 1_700_000_000);
        assert_eq!(request.now().unwrap(), 1_700_000_001);
        request.travel_to(50).unwrap();
        assert_eq!(request.now().unwrap(), 50);
        request.reset().unwrap();
        assert_eq!(request.now().unwrap(), 1_700_000_002);

        let fixed = ClockSettings::fixed(1_789_894_800).unwrap();
        let request = ServerClock::with_test_source(&fixed, source).capture();
        assert_eq!(request.now().unwrap(), 1_789_894_800);
        assert_eq!(request.now().unwrap(), 1_789_894_800);
    }

    #[test]
    fn tl_04_native_admission_and_request_transitions_are_checked() {
        let _case = crate::vm_gate::test_case();
        for valid in [0, 1, 1_789_894_800, SAFE_INTEGER] {
            assert_eq!(validate_timestamp(valid).unwrap(), valid);
        }
        for invalid in [-1, SAFE_INTEGER + 1, i64::MAX] {
            assert!(validate_timestamp(invalid).is_err(), "{invalid}");
        }

        let fixed = ClockSettings::fixed(10).unwrap();
        let server = ServerClock::new(&fixed);
        let request = server.capture();
        request.travel_to(20).unwrap();
        server.publish(Policy::Fixed(30));
        assert_eq!(request.now().unwrap(), 20);
        request.reset().unwrap();
        assert_eq!(request.now().unwrap(), 10);
        request.reset().unwrap();
        assert_eq!(request.now().unwrap(), 10);
        request.seal();
        assert!(request.travel_to(40).is_err());
        assert!(request.reset().is_err());
        assert_eq!(server.capture().now().unwrap(), 30);
        server.publish(server.startup());
        assert_eq!(server.capture().now().unwrap(), 10);
    }

    #[test]
    fn tl_04_standalone_validation_uses_the_gate_and_admitted_vms_validate_in_place() {
        let _case = crate::vm_gate::test_case();
        let lease = crate::vm_gate::test_guard();
        let (started, ready) = std::sync::mpsc::channel();
        let (finished, result) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            finished.send(ClockSettings::fixed(10)).unwrap();
        });
        ready.recv().unwrap();
        assert!(result.recv_timeout(Duration::from_millis(20)).is_err());
        drop(lease);
        assert_eq!(
            result
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap()
                .fixed_value(),
            Some(10)
        );
        worker.join().unwrap();

        let _lease = crate::vm_gate::test_guard();
        let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default()).unwrap();
        lua.set_memory_limit(crate::value::MAX_BYTES).unwrap();
        assert_eq!(
            ClockSettings::fixed_in(&lua, 20).unwrap().fixed_value(),
            Some(20)
        );
    }

    #[test]
    fn tl_07_world_changes_only_contexts_captured_after_publication() {
        let source = Arc::new(Manual(AtomicI64::new(100)));
        let server = ServerClock::with_test_source(&ClockSettings::system(), source);
        let existing = server.capture();
        let (proceed, release) = std::sync::mpsc::channel();
        let (captured, result) = std::sync::mpsc::channel();
        let worker = {
            let server = server.clone();
            std::thread::spawn(move || {
                release.recv().unwrap();
                let budget = crate::runtime::ExecutionBudget::default();
                budget.set_clock(&server);
                captured.send(budget.clock().now().unwrap()).unwrap();
            })
        };
        server.publish(Policy::Fixed(20));
        proceed.send(()).unwrap();
        assert_eq!(result.recv().unwrap(), 20);
        worker.join().unwrap();
        assert_eq!(existing.now().unwrap(), 100);
        server.publish(server.startup());
        assert_eq!(server.capture().now().unwrap(), 101);
        assert_eq!(server.capture().now().unwrap(), 102);
    }
}
