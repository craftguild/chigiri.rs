//! Version-controlled payload expectations shared by all real wire clients.

use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Profile {
    ServerSentEvents,
    Multipart,
    WebSocket,
}

impl Profile {
    pub(super) const fn index(self) -> usize {
        match self {
            Self::ServerSentEvents => 0,
            Self::Multipart => 1,
            Self::WebSocket => 2,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Goal {
    Failure,
    Recovery,
}

pub(super) struct StreamReport {
    profile: Profile,
    observations: Vec<Observation>,
    terminal: bool,
}

enum Observation {
    Event { id: String, value: i64 },
    Failure,
}

impl StreamReport {
    pub(super) fn new(profile: Profile) -> Self {
        Self {
            profile,
            observations: Vec::new(),
            terminal: false,
        }
    }

    pub(super) fn observe_event<'a>(&mut self, payload: &'a Value) -> Option<&'a str> {
        assert_eq!(
            payload.pointer("/extensions/chigiri/delivery/droppedTotal"),
            Some(&Value::Null),
            "{:?} delivery metadata",
            self.profile
        );
        if let Some(id) = payload.pointer("/data/changed/id").and_then(Value::as_str) {
            assert_ne!(id, "rollback", "a rolled-back publication reached the wire");
            let value = payload["data"]["changed"]["value"].as_i64().unwrap();
            assert_eq!(
                payload.pointer("/data/changed/related/label"),
                Some(&Value::String(format!("current-{id}"))),
                "{:?} relation must use the committed event snapshot",
                self.profile
            );
            if matches!(id, "first" | "second" | "recovery") {
                self.observations.push(Observation::Event {
                    id: id.to_owned(),
                    value,
                });
            }
            return Some(id);
        }
        if payload.get("errors").is_some() {
            self.assert_safe_error(&payload["errors"]);
            self.observations.push(Observation::Failure);
            return Some("failure");
        }
        None
    }

    pub(super) fn observe_transport_error(&mut self, errors: &Value) {
        self.assert_safe_error(errors);
        self.observations.push(Observation::Failure);
    }

    pub(super) fn mark_terminal(&mut self) {
        assert!(
            matches!(self.observations.last(), Some(Observation::Failure)),
            "{:?} completion must follow its error",
            self.profile
        );
        self.terminal = true;
    }

    pub(super) fn done(&self, goal: Goal) -> bool {
        match goal {
            Goal::Failure => self.terminal,
            Goal::Recovery => matches!(
                self.observations.last(),
                Some(Observation::Event { id, .. }) if id == "recovery"
            ),
        }
    }

    pub(super) fn assert_contract(self, goal: Goal) {
        match goal {
            Goal::Failure => match &self.observations[..] {
                [
                    Observation::Event {
                        id: first,
                        value: 1,
                    },
                    Observation::Event {
                        id: second,
                        value: 2,
                    },
                    Observation::Failure,
                ] => {
                    assert_eq!(first, "first");
                    assert_eq!(second, "second");
                    assert!(self.terminal);
                }
                _ => panic!("unexpected {:?} failure sequence", self.profile),
            },
            Goal::Recovery => match &self.observations[..] {
                [
                    Observation::Event {
                        id: recovery,
                        value: 14,
                    },
                ] => assert_eq!(recovery, "recovery"),
                _ => panic!("unexpected {:?} recovery sequence", self.profile),
            },
        }
    }

    fn assert_safe_error(&self, errors: &Value) {
        assert_safe_error(errors);
    }
}

pub(super) fn assert_safe_error(errors: &Value) {
    assert_eq!(errors[0]["message"], "Mock execution failed");
    assert_eq!(errors[0]["extensions"]["code"], "MOCK_EXECUTION_FAILED");
    let encoded = errors.to_string();
    assert!(!encoded.contains("private event failure"));
    assert!(!encoded.contains("publication staging pressure exceeded"));
}

pub(super) fn assert_safe_source_error(errors: &Value) {
    assert_eq!(errors[0]["message"], "Internal server error");
    assert_eq!(errors[0]["extensions"]["code"], "INTERNAL_SERVER_ERROR");
    let encoded = errors.to_string();
    assert!(!encoded.contains("publication staging pressure exceeded"));
    assert!(!encoded.contains("[source]"));
    assert!(!encoded.contains("resolvers/subscription"));
}
