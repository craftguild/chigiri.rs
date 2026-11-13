//! Isolate module syntax scopes, then share host declarations in one batched type-check.

use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{HookScript, MAX_GENERATED, Script, ScriptError};
use crate::validation_control::ValidationControl;

struct Target {
    segment: RangeInclusive<usize>,
    source_start: usize,
    source_lines: usize,
    prefix_lines: usize,
    body_lines: usize,
    columns: super::literal_precision::ColumnMap,
    type_sites: Vec<super::literal_precision::TypeSite>,
    error: ScriptError,
}

pub(crate) struct ValidationBatch {
    declarations: Arc<str>,
    source: String,
    next_line: usize,
    targets: Vec<Target>,
}

impl ValidationBatch {
    fn new(declarations: Arc<str>) -> Self {
        let source = checker_prelude();
        Self {
            declarations,
            next_line: source.lines().count() + 1,
            source,
            targets: Vec::new(),
        }
    }

    /// Isolate each original module in a function scope so exported types and locals cannot leak between modules.
    pub(crate) fn add(
        &mut self,
        source: &str,
        prefix_lines: usize,
        body_lines: usize,
        error: ScriptError,
        ir: &crate::ir::MockIr,
    ) -> Result<(), ScriptError> {
        let (source, mut columns, type_sites) = super::literal_precision::narrow(source, ir);
        let source = super::terminal_flow::refine(&source, &mut columns, ir);
        let id = self.targets.len();
        let control = matches!(
            error.hook,
            Some(crate::hooks::HookSlot::RequestBefore | crate::hooks::HookSlot::OperationBefore)
        );
        let header = if control {
            format!("do\nlocal function __chigiri_validate_{id}(...)\n")
        } else {
            format!(
                "do\nlocal function __chigiri_validate_{id}(...)\nlocal timeleap: nil = nil\nlocal _G: ChigiriNoTimeleapGlobalLibrary = _G :: any\n"
            )
        };
        let footer = if source.ends_with('\n') {
            "end\nend\n"
        } else {
            "\nend\nend\n"
        };
        if self.source.len() + header.len() + source.len() + footer.len() > MAX_GENERATED {
            return Err(ScriptError::limit());
        }
        let segment_start = self.next_line;
        self.source.push_str(&header);
        self.next_line += header.lines().count();
        let source_start = self.next_line;
        let source_lines = source.lines().count().max(1);
        self.source.push_str(&source);
        self.source.push_str(footer);
        self.next_line += source_lines + 2;
        self.targets.push(Target {
            segment: segment_start..=self.next_line - 1,
            source_start,
            source_lines,
            prefix_lines,
            body_lines,
            columns,
            type_sites,
            error,
        });
        Ok(())
    }

    fn check(self, control: Option<&ValidationControl>) -> Result<(), ScriptError> {
        if self.targets.is_empty() {
            return Ok(());
        }
        if let Some(control) = control {
            control.script()?;
        }
        match luaur::check_with_definitions(&self.source, &self.declarations) {
            Ok(()) => Ok(()),
            Err(errors) => {
                let diagnostic = errors.iter().find(|item| !item.in_definitions);
                let target = diagnostic
                    .and_then(|item| {
                        let line = item.line as usize;
                        self.targets
                            .iter()
                            .find(|target| target.segment.contains(&line))
                    })
                    .unwrap_or(&self.targets[0]);
                let mut error = target.error.clone();
                if let Some(item) = diagnostic {
                    let line = item.line as usize;
                    let local = if line >= target.source_start
                        && line < target.source_start + target.source_lines
                    {
                        line - target.source_start + 1
                    } else {
                        1
                    };
                    let source_line = local.max(1).min(target.source_lines);
                    let original_column =
                        target.columns.original(source_line, item.column as usize);
                    let original_byte = target.columns.source_byte(source_line, original_column);
                    let end_line = item.end_line as usize;
                    let local_end = if end_line >= target.source_start
                        && end_line < target.source_start + target.source_lines
                    {
                        end_line - target.source_start + 1
                    } else {
                        source_line
                    };
                    let source_end_line = local_end.max(1).min(target.source_lines);
                    let original_end_column = target
                        .columns
                        .original(source_end_line, item.end_column as usize);
                    let original_end_byte = target
                        .columns
                        .source_byte(source_end_line, original_end_column);
                    error.body = (
                        source_line
                            .saturating_sub(target.prefix_lines)
                            .max(1)
                            .min(target.body_lines),
                        original_column,
                    );
                    error.mark_source_position();
                    if let Some(site) = target
                        .type_sites
                        .iter()
                        .find(|site| site.overlaps(original_byte, original_end_byte))
                    {
                        error.cause = "MOCK_TYPE_MISMATCH";
                        error.member = Some(site.member.clone());
                        error.expected.clone_from(&site.expected);
                        error.actual = site.actual.into();
                        error.hint = site.hint.into();
                    } else {
                        if !error.expected.starts_with("closed subscription module")
                            && !error.expected.starts_with("closed consumer module")
                        {
                            error.expected = if error.hook.is_some() {
                                "closed hook module { run: generated function }"
                            } else {
                                "closed resolver module { resolve: generated function }"
                            }
                            .into();
                        }
                        error.actual = "structural type unavailable".into();
                    }
                }
                Err(error)
            }
        }
    }
}

pub(crate) fn checker_prelude() -> String {
    let mut source = String::from("--!strict\n");
    for function in crate::api::runtime::GLOBALS {
        if function.checker_intrinsic() {
            continue;
        }
        source.push_str("local ");
        source.push_str(function.name);
        source.push_str(": ");
        source.push_str(function.function_type());
        source.push_str(" = ");
        source.push_str(function.name);
        source.push_str(" :: any\n");
    }
    for library in crate::api::runtime::LIBRARIES {
        source.push_str("local ");
        source.push_str(library.name);
        source.push_str(": ");
        source.push_str(library.type_name);
        source.push_str(" = ");
        source.push_str(if library.name == "timeleap" {
            "nil"
        } else {
            library.name
        });
        source.push_str(" :: any\n");
    }
    source.push_str("local _G: ChigiriGlobalLibrary = _G :: any\n");
    for name in crate::api::runtime::UNAVAILABLE_GLOBALS {
        source.push_str("local ");
        source.push_str(name);
        source.push_str(": nil = nil\n");
    }
    source
}

pub(crate) fn validate(
    declarations: Arc<str>,
    scripts: &mut [Script],
    hooks: &mut [HookScript],
    control: Option<&ValidationControl>,
) -> Result<(), ScriptError> {
    validate_with_consumers(declarations, scripts, hooks, &mut [], control)
}

pub(crate) fn validate_with_consumers(
    declarations: Arc<str>,
    scripts: &mut [Script],
    hooks: &mut [HookScript],
    consumers: &mut [super::ConsumerScript],
    control: Option<&ValidationControl>,
) -> Result<(), ScriptError> {
    if let Some(control) = control {
        control.script()?;
    }
    let admission = Duration::from_secs(5);
    let deadline = control.map_or_else(|| Instant::now() + admission, |c| c.limit(admission));
    let mut admission_error = scripts
        .first()
        .map(Script::type_admission_error)
        .or_else(|| hooks.first().map(HookScript::type_admission_error))
        .or_else(|| {
            consumers
                .first()
                .map(|consumer| consumer.script().type_admission_error())
        })
        .unwrap_or_else(|| ScriptError::new("environment", "type checker admission expired"));
    let lease = crate::vm_gate::enter(deadline, || {
        control.is_some_and(|c| c.cancellation.cancelled())
    })
    .map_err(|_| {
        admission_error.cancelled = control.is_some_and(|c| c.cancellation.cancelled());
        admission_error
    })?;
    let mut batch = ValidationBatch::new(declarations);
    for script in scripts.iter() {
        script.queue_validation(control, &mut batch)?;
    }
    for hook in hooks.iter() {
        hook.queue_validation(control, &mut batch)?;
    }
    for consumer in consumers.iter() {
        consumer.script().queue_validation(control, &mut batch)?;
    }
    batch.check(control)?;
    if let Some(control) = control {
        control.script()?;
    }
    drop(lease);

    for script in scripts {
        script.initialize(control)?;
    }
    for hook in hooks {
        hook.initialize(control)?;
    }
    for consumer in consumers {
        consumer.initialize(control)?;
    }
    Ok(())
}
