//! Phase-aware apparent-time ownership shared by one Luau VM.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::clock::{Policy, RequestClock, ServerClock};
use crate::value::ValueError;

enum Authority {
    Initialization,
    Request {
        clock: Arc<RequestClock>,
        control: bool,
    },
    World {
        server: Arc<ServerClock>,
        candidate: Arc<Mutex<Policy>>,
    },
}

/// Runtime state consulted at call time, so aliases cannot capture stale time.
pub(crate) struct Runtime {
    active: AtomicBool,
    authority: Authority,
}

impl Runtime {
    pub(crate) fn initialization() -> Arc<Self> {
        Arc::new(Self {
            active: AtomicBool::new(false),
            authority: Authority::Initialization,
        })
    }

    pub(crate) fn request(clock: Arc<RequestClock>, control: bool) -> Arc<Self> {
        Arc::new(Self {
            active: AtomicBool::new(false),
            authority: Authority::Request { clock, control },
        })
    }

    pub(crate) fn world(server: Arc<ServerClock>, candidate: Arc<Mutex<Policy>>) -> Arc<Self> {
        Arc::new(Self {
            active: AtomicBool::new(false),
            authority: Authority::World { server, candidate },
        })
    }

    pub(crate) fn activate(&self) {
        self.active.store(true, Ordering::Release);
    }

    pub(super) fn live(&self) -> Result<(), ValueError> {
        if self.active.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(ValueError("live time unavailable during initialization"))
        }
    }

    pub(super) fn now(&self) -> Result<i64, ValueError> {
        self.live()?;
        match &self.authority {
            Authority::Request { clock, .. } => clock.now(),
            Authority::World { server, candidate } => {
                let policy = *candidate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                server.read_policy(policy)
            }
            Authority::Initialization => Err(ValueError("live time unavailable")),
        }
    }

    pub(super) fn travel_to(
        &self,
        lua: &luaur::Lua,
        value: i64,
        world: bool,
    ) -> Result<(), ValueError> {
        self.live()?;
        match (&self.authority, world) {
            (
                Authority::Request {
                    clock,
                    control: true,
                },
                false,
            ) => clock.travel_to_in(lua, value),
            (Authority::World { candidate, .. }, true) => {
                crate::clock::validate_timestamp_in(lua, value)?;
                *candidate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Policy::Fixed(value);
                Ok(())
            }
            _ => Err(ValueError("timeleap authority unavailable")),
        }
    }

    pub(super) fn reset(&self, world: bool) -> Result<(), ValueError> {
        self.live()?;
        match (&self.authority, world) {
            (
                Authority::Request {
                    clock,
                    control: true,
                },
                false,
            ) => clock.reset(),
            (Authority::World { server, candidate }, true) => {
                *candidate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = server.startup();
                Ok(())
            }
            _ => Err(ValueError("timeleap authority unavailable")),
        }
    }
}
