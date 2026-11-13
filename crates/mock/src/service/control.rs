//! Host-owned, per-server serialized world-clock control.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use luaur::{Lua, LuaOptions, StdLib};

use crate::cancellation::Cancellation;
#[cfg(test)]
use crate::clock::Policy;
use crate::clock::ServerClock;
use crate::luau::ScriptError;

/// A host-side handle for bounded control of one mock-server world.
#[derive(Clone)]
pub struct ServerControl {
    clock: Arc<ServerClock>,
    serial: Arc<Mutex<()>>,
    shutdown: Cancellation,
}

impl ServerControl {
    #[cfg(test)]
    pub(super) fn new(clock: Arc<ServerClock>, shutdown: Cancellation) -> Self {
        Self {
            clock,
            serial: Arc::new(Mutex::new(())),
            shutdown,
        }
    }

    pub(super) fn shared(
        clock: Arc<ServerClock>,
        serial: Arc<Mutex<()>>,
        shutdown: Cancellation,
    ) -> Self {
        Self {
            clock,
            serial,
            shutdown,
        }
    }

    /// Run a bounded Luau control chunk and publish its candidate exactly once.
    pub fn run_luau(&self, source: &str) -> Result<(), ScriptError> {
        self.run_with(source, &Cancellation::default())
    }

    fn run_with(&self, source: &str, cancel: &Cancellation) -> Result<(), ScriptError> {
        if source.len() > crate::luau::MAX_BODY {
            return Err(error("control source exceeds byte limit"));
        }
        let _serial = self
            .serial
            .lock()
            .map_err(|_| error("control serialization unavailable"))?;
        let deadline = Instant::now() + crate::runtime::VM_TIME;
        let _lease =
            crate::vm_gate::enter(deadline, || cancel.cancelled() || self.shutdown.cancelled())
                .map_err(|_| error("control VM admission failed"))?;
        let candidate = Arc::new(Mutex::new(self.clock.candidate()));
        let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default())
            .map_err(|_| error("control VM initialization failed"))?;
        lua.set_memory_limit(crate::value::MAX_BYTES)
            .map_err(|_| error("control VM memory limit unavailable"))?;
        let evaluation = crate::sandbox::install_sticky_control(&lua, deadline, cancel.clone())
            .map_err(|_| error("control execution setup failed"))?;
        let time = crate::sandbox::TimeRuntime::world(self.clock.clone(), candidate.clone());
        crate::sandbox::seal_with_time_evaluation(&lua, evaluation.clone(), time.clone())
            .map_err(|_| error("control sandbox setup failed"))?;
        time.activate();
        let execution = lua.load(source).exec();
        if execution.is_err() || evaluation.borrow().failure().is_some() {
            return Err(error("control execution failed"));
        }
        let policy = *candidate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cancel
            .commit(&self.shutdown, deadline, || self.clock.publish(policy))
            .ok_or_else(|| error("control cancelled before publication"))?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn policy(&self) -> Policy {
        self.clock.candidate()
    }

    #[cfg(test)]
    pub(crate) fn run_cancelled(
        &self,
        source: &str,
        cancellation: &Cancellation,
    ) -> Result<(), ScriptError> {
        self.run_with(source, cancellation)
    }
}

fn error(message: &'static str) -> ScriptError {
    ScriptError::new("control", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ClockSettings;

    #[test]
    fn tl_11_control_publishes_once_and_discards_failed_candidates() {
        let _case = crate::vm_gate::test_case();
        let clock = ServerClock::new(&ClockSettings::fixed(10).unwrap());
        let control = ServerControl::new(clock, Cancellation::default());
        control
            .run_luau("timeleap.travel_to(20, {world=true})")
            .unwrap();
        assert_eq!(control.policy(), Policy::Fixed(20));

        assert!(
            control
                .run_luau("pcall(function() timeleap.travel_to(30, {world=true}) end); error('x')")
                .is_err()
        );
        assert_eq!(control.policy(), Policy::Fixed(20));
        assert!(
            control
                .run_luau(
                    "timeleap.travel_to(30, {world=true}); \
                     pcall(function() error('caught') end)"
                )
                .is_err()
        );
        assert_eq!(control.policy(), Policy::Fixed(20));
        assert!(
            control
                .run_luau(
                    "timeleap.reset({world=true}); \
                     pcall(function() assert(false) end)"
                )
                .is_err()
        );
        assert_eq!(control.policy(), Policy::Fixed(20));
        control.run_luau("timeleap.reset({world=true})").unwrap();
        assert_eq!(control.policy(), Policy::Fixed(10));
        control.run_luau("timeleap.reset({world=true})").unwrap();
        assert_eq!(control.policy(), Policy::Fixed(10));
    }

    #[test]
    fn tl_03_world_control_is_explicit_and_instance_local() {
        let _case = crate::vm_gate::test_case();
        let first = ServerControl::new(
            ServerClock::new(&ClockSettings::system()),
            Cancellation::default(),
        );
        let second = ServerControl::new(
            ServerClock::new(&ClockSettings::fixed(11).unwrap()),
            Cancellation::default(),
        );
        assert!(first.run_luau("timeleap.travel_to(20)").is_err());
        first
            .run_luau("timeleap.travel_to(20, {world=true})")
            .unwrap();
        assert_eq!(first.policy(), Policy::Fixed(20));
        assert_eq!(second.policy(), Policy::Fixed(11));
    }

    #[test]
    fn tl_11_cancelled_control_does_not_publish() {
        let _case = crate::vm_gate::test_case();
        let control = ServerControl::new(
            ServerClock::new(&ClockSettings::fixed(10).unwrap()),
            Cancellation::default(),
        );
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(
            control
                .run_cancelled("timeleap.travel_to(20, {world=true})", &cancellation,)
                .is_err()
        );
        assert_eq!(control.policy(), Policy::Fixed(10));
    }

    #[test]
    fn tl_04_invalid_world_calls_are_sticky_and_leave_policy_unchanged() {
        let _case = crate::vm_gate::test_case();
        let control = ServerControl::new(
            ServerClock::new(&ClockSettings::fixed(10).unwrap()),
            Cancellation::default(),
        );
        for call in [
            "timeleap.travel_to(20)",
            "timeleap.travel_to(20,nil)",
            "timeleap.travel_to(20,{})",
            "timeleap.travel_to(20,{world=false})",
            "timeleap.reset()",
            "timeleap.reset(nil)",
            "timeleap.reset({})",
            "timeleap.reset({world=false})",
            "timeleap.travel_to()",
            "timeleap.travel_to(20,{world=true},1)",
            "timeleap.reset({world=true},1)",
            "timeleap.travel_to(('20' :: any),{world=true})",
            "timeleap.travel_to(20.5,{world=true})",
            "timeleap.travel_to(-1,{world=true})",
            "timeleap.travel_to(1/0,{world=true})",
            "timeleap.travel_to(9007199254740992,{world=true})",
            "timeleap.travel_to(20,({world=1} :: any))",
            "timeleap.travel_to(20,({global=false} :: any))",
            "timeleap.travel_to(20,({world=true,global=false} :: any))",
            "timeleap.reset(({scope='world'} :: any))",
        ] {
            let source = format!("pcall(function() {call} end)");
            assert!(control.run_luau(&source).is_err(), "{call}");
            assert_eq!(control.policy(), Policy::Fixed(10), "{call}");
        }
    }

    #[test]
    fn tl_11_concurrent_controls_observe_one_serial_order() {
        let _case = crate::vm_gate::test_case();
        let control = ServerControl::new(
            ServerClock::new(&ClockSettings::fixed(10).unwrap()),
            Cancellation::default(),
        );
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut threads = Vec::new();
        for _ in 0..2 {
            let control = control.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                control
                    .run_luau(
                        "local current=assert(os.time()); timeleap.travel_to(current+1,{world=true})",
                    )
                    .unwrap();
            }));
        }
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(control.policy(), Policy::Fixed(12));
    }
}
