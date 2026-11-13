//! Capture bounded logical stack locations without retaining VM values or source text.

use luaur::Lua;

use super::state::Host;
use crate::luau::ScriptStack;

impl Host {
    pub(super) fn capture_stack(&mut self, lua: &Lua, level: usize, origin: &'static str) {
        const LIMIT: usize = crate::protected_call::REENTRY_LIMIT;
        if self.cancel_unwinding || self.stack.is_some() || self.stack_captures >= LIMIT {
            return;
        }
        self.stack_captures += 1;
        let mut lines = Vec::new();
        for offset in 0..LIMIT {
            let Some(frame) = lua.inspect_stack(level + offset) else {
                break;
            };
            // `inspect_stack` borrows VM strings, so retain only bounded line numbers.
            if frame.source().and_then(|source| source.strip_prefix('='))
                == Some(self.stack_source.as_ref())
                && let Some(line) = frame
                    .current_line()
                    .and_then(|line| usize::try_from(line).ok())
                && line > 0
            {
                lines.push(line);
            }
        }
        if !lines.is_empty() {
            self.stack = Some(ScriptStack {
                origin,
                lines,
                truncated: lua.inspect_stack(level + LIMIT).is_some(),
            });
        }
    }
}
