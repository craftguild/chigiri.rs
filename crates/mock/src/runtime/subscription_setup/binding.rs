//! Own setup binding candidates without retaining Luau values or request state.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use luaur::{Lua, UserData, Value as LuaValue};

use crate::events::{SourceLimits, retention::Retention};
use crate::ir::ScriptId;
use crate::publication::SequenceDefinition;

/// One schema-authorized subscription binding copied out of the setup VM.
pub(crate) struct SubscriptionBinding {
    script: ScriptId,
    selection: SourceSelection,
    overflow: chigiri::SubscriptionOverflow,
}

pub(crate) enum SourceSelection {
    PubSub(Box<str>),
    AllTopics,
    Sequence(Arc<SequenceDefinition>),
}

/// VM-local host data, never a structural Lua table or retained VM owner.
struct SourceToken {
    owner: Rc<()>,
    script: ScriptId,
    index: usize,
}

impl UserData for SourceToken {}

#[derive(Clone)]
pub(in crate::runtime) struct BindingCollector {
    script: ScriptId,
    owner: Rc<()>,
    candidates: Rc<RefCell<Vec<Option<SubscriptionBinding>>>>,
    limits: SourceLimits,
    retention: Retention,
    consumer: bool,
}

impl SubscriptionBinding {
    pub(crate) const fn script(&self) -> ScriptId {
        self.script
    }

    pub(crate) fn topic(&self) -> Option<&str> {
        match &self.selection {
            SourceSelection::PubSub(topic) => Some(topic),
            SourceSelection::Sequence(_) | SourceSelection::AllTopics => None,
        }
    }

    pub(crate) fn sequence(&self) -> Option<&Arc<SequenceDefinition>> {
        match &self.selection {
            SourceSelection::Sequence(definition) => Some(definition),
            SourceSelection::PubSub(_) | SourceSelection::AllTopics => None,
        }
    }

    pub(crate) fn all_topics(&self) -> bool {
        matches!(self.selection, SourceSelection::AllTopics)
    }

    pub(crate) const fn overflow(&self) -> chigiri::SubscriptionOverflow {
        self.overflow
    }
}

impl BindingCollector {
    #[cfg(test)]
    pub(in crate::runtime) fn new(script: ScriptId) -> Self {
        let limits = SourceLimits::default();
        Self::with_policy(script, limits, Retention::new(limits.max_retained_bytes()))
    }

    pub(in crate::runtime) fn with_policy(
        script: ScriptId,
        limits: SourceLimits,
        retention: Retention,
    ) -> Self {
        Self {
            script,
            owner: Rc::new(()),
            candidates: Rc::new(RefCell::new(Vec::new())),
            limits,
            retention,
            consumer: false,
        }
    }

    pub(super) fn for_consumer(mut self) -> Self {
        self.consumer = true;
        self
    }

    pub(super) const fn allows_all_topics(&self) -> bool {
        self.consumer
    }

    pub(super) fn push_all_topics(
        &self,
        lua: &Lua,
        overflow: chigiri::SubscriptionOverflow,
    ) -> luaur::Result<LuaValue> {
        if !self.consumer {
            return Err(luaur::Error::runtime("consumer-only source selection"));
        }
        self.insert(lua, SourceSelection::AllTopics, overflow)
    }

    pub(super) const fn policy(&self) -> (SourceLimits, &Retention) {
        (self.limits, &self.retention)
    }

    pub(in crate::runtime) const fn script(&self) -> ScriptId {
        self.script
    }

    pub(in crate::runtime) fn push(
        &self,
        lua: &Lua,
        topic: Box<str>,
        overflow: chigiri::SubscriptionOverflow,
    ) -> luaur::Result<LuaValue> {
        self.insert(lua, SourceSelection::PubSub(topic), overflow)
    }

    pub(super) fn push_sequence(
        &self,
        lua: &Lua,
        definition: Arc<SequenceDefinition>,
        overflow: chigiri::SubscriptionOverflow,
    ) -> luaur::Result<LuaValue> {
        if !definition.belongs_to(&self.retention) {
            return Err(luaur::Error::runtime("foreign sequence definition"));
        }
        self.insert(lua, SourceSelection::Sequence(definition), overflow)
    }

    fn insert(
        &self,
        lua: &Lua,
        selection: SourceSelection,
        overflow: chigiri::SubscriptionOverflow,
    ) -> luaur::Result<LuaValue> {
        let mut candidates = self.candidates.borrow_mut();
        candidates
            .try_reserve(1)
            .map_err(|_| luaur::Error::runtime("source candidate allocation failed"))?;
        let sentinel = lua.create_userdata(SourceToken {
            owner: self.owner.clone(),
            script: self.script,
            index: candidates.len(),
        })?;
        candidates.push(Some(SubscriptionBinding {
            script: self.script,
            selection,
            overflow,
        }));
        Ok(LuaValue::UserData(sentinel))
    }

    pub(in crate::runtime) fn take(&self, result: &LuaValue) -> Result<SubscriptionBinding, ()> {
        let LuaValue::UserData(value) = result else {
            return Err(());
        };
        let token = value.borrow::<SourceToken>().map_err(|_| ())?;
        if !Rc::ptr_eq(&token.owner, &self.owner) || token.script != self.script {
            return Err(());
        }
        let mut candidates = self.candidates.borrow_mut();
        candidates.get_mut(token.index).ok_or(())?.take().ok_or(())
    }

    /// Prove that every VM callback holding the collector was destroyed.
    pub(in crate::runtime) fn closed(self) -> bool {
        Rc::try_unwrap(self.candidates).is_ok()
    }
}
