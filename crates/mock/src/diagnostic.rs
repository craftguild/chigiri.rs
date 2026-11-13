//! Render diagnostics with a fixed vocabulary without echoing user source or values.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::Arc;

use chigiri_host::PortableRelativePath;

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub file: PortableRelativePath,
    pub coordinate: Arc<str>,
    pub toml: (usize, usize),
    pub body: Box<(usize, usize)>,
    pub embedded: Option<Box<(usize, usize)>>,
    pub exit: u8,
    /// Whether the bounded output worker has taken ownership of this diagnostic.
    pub output_owned: bool,
    project: Option<Box<chigiri_mock::project::ProjectError>>,
    details: Box<chigiri_mock::luau::DiagnosticDetails>,
    reference_guidance: Option<Box<ReferenceGuidance>>,
    file_known: bool,
    coordinate_known: bool,
    position_known: bool,
    body_known: bool,
    hook_slot: Option<chigiri_mock::hooks::HookSlot>,
}

#[derive(Clone, Debug)]
struct ReferenceGuidance {
    commands: Vec<String>,
    input_hint: &'static str,
}

impl Diagnostic {
    pub fn exit_code(&self) -> u8 {
        self.exit
    }

    pub fn output_owned(&self) -> bool {
        self.output_owned
    }

    pub fn new(path: &Path, phase: &'static str, message: &'static str) -> Self {
        let (cause, expected, actual, hint) = chigiri_mock::luau::diagnostic_shape(phase);
        let file: String = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config")
            .chars()
            .filter(|c| c.is_alphanumeric() || "._-".contains(*c))
            .take(100)
            .collect();
        Self {
            file: PortableRelativePath::new(&file)
                .unwrap_or_else(|_| PortableRelativePath::new("config").unwrap()),
            coordinate: Arc::from("config"),
            toml: (1, 1),
            body: Box::new((1, 1)),
            embedded: None,
            exit: 1,
            output_owned: false,
            project: None,
            details: Box::new(chigiri_mock::luau::DiagnosticDetails {
                consumer: None,
                phase,
                cause,
                member: None,
                expected: expected.into(),
                actual: actual.into(),
                hint: hint.into(),
                message,
            }),
            reference_guidance: None,
            file_known: true,
            coordinate_known: false,
            position_known: false,
            body_known: false,
            hook_slot: None,
        }
    }

    pub fn environment(message: &'static str) -> Self {
        let mut error = Self::new(Path::new("chigiri-mock"), "environment", message);
        error.exit = 2;
        error.clear_location();
        error
    }

    pub(crate) fn reported_status(exit: u8) -> Self {
        let mut error = Self::environment("command reported a nonzero status");
        error.exit = exit;
        error.output_owned = true;
        error
    }

    pub fn project(error: chigiri_mock::project::ProjectError) -> Self {
        let mut result = Self::new(Path::new("project"), error.phase, error.message);
        result.file = error.file.clone();
        result.coordinate = error.coordinate.clone();
        result.toml = error.position;
        result.body = Box::new(error.position);
        result.cause = error.cause;
        result.member.clone_from(&error.member);
        result.expected.clone_from(&error.expected);
        result.actual.clone_from(&error.actual);
        result.hint.clone_from(&error.hint);
        result.consumer.clone_from(&error.consumer);
        result.exit = error.exit;
        result.file_known = error.file_known();
        result.coordinate_known = error.coordinate_known();
        result.position_known = error.position_known();
        result.body_known = error.position_known();
        result.hook_slot = error.hook_slot();
        result.project = Some(Box::new(error));
        result
    }

    pub fn field(self) -> chigiri::FieldError {
        chigiri::FieldError::public(self.to_string()).code("MOCK_RESOLVER_ERROR")
    }

    pub(crate) fn set_reference_guidance(
        &mut self,
        commands: Vec<String>,
        input_hint: &'static str,
    ) {
        self.reference_guidance = (!commands.is_empty()).then(|| {
            Box::new(ReferenceGuidance {
                commands,
                input_hint,
            })
        });
    }

    pub(crate) fn file_known(&self) -> bool {
        self.file_known
    }

    pub(crate) fn coordinate_known(&self) -> bool {
        self.coordinate_known
    }

    pub(crate) fn position_known(&self) -> bool {
        self.position_known
    }

    pub(crate) fn hook_slot(&self) -> Option<chigiri_mock::hooks::HookSlot> {
        self.hook_slot
    }

    pub(crate) fn set_file(&mut self, file: impl ToString) {
        self.file = PortableRelativePath::new(&file.to_string())
            .unwrap_or_else(|_| PortableRelativePath::new("path").unwrap());
        self.file_known = true;
    }

    pub(crate) fn set_coordinate(&mut self, coordinate: impl Into<Arc<str>>) {
        self.coordinate = coordinate.into();
        self.coordinate_known = true;
    }

    pub(crate) fn clear_coordinate(&mut self) {
        self.coordinate_known = false;
    }

    pub(crate) fn set_position(&mut self, position: (usize, usize)) {
        self.toml = position;
        self.position_known = true;
    }

    pub(crate) fn set_body(&mut self, position: (usize, usize)) {
        *self.body = position;
        self.body_known = true;
    }

    pub(crate) fn set_hook_slot(&mut self, slot: chigiri_mock::hooks::HookSlot) {
        self.hook_slot = Some(slot);
        if self.member.is_none() {
            self.member = Some(slot.name().into());
        }
    }

    pub(crate) fn clear_location(&mut self) {
        self.file_known = false;
        self.coordinate_known = false;
        self.position_known = false;
        self.body_known = false;
    }
}

impl Deref for Diagnostic {
    type Target = chigiri_mock::luau::DiagnosticDetails;

    fn deref(&self) -> &Self::Target {
        &self.details
    }
}

impl DerefMut for Diagnostic {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.details
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(error) = &self.project {
            write!(f, "{error}")?;
        } else if self.file_known && self.position_known && self.coordinate_known {
            write!(
                f,
                "{}:{}:{} {}",
                self.file, self.toml.0, self.toml.1, self.coordinate
            )?;
            if self.body_known {
                write!(f, " Luau {}:{}", self.body.0, self.body.1)?;
            }
            write!(f, " [{}] {}", self.phase, self.message)?;
        } else {
            if self.file_known {
                write!(f, "{}", self.file)?;
                if self.position_known {
                    write!(f, ":{}:{}", self.toml.0, self.toml.1)?;
                }
                if self.coordinate_known {
                    write!(f, " {}", self.coordinate)?;
                } else if let Some(member) = &self.member {
                    write!(f, " {member}")?;
                }
                write!(f, " ")?;
            } else if self.coordinate_known {
                write!(f, "{} ", self.coordinate)?;
            } else if let Some(member) = &self.member {
                write!(f, "{member} ")?;
            }
            write!(f, "[{}] {}", self.phase, self.message)?;
        }
        if let Some(guidance) = &self.reference_guidance {
            for command in &guidance.commands {
                write!(f, "\nreference: {command}")?;
            }
            write!(f, "\nreference input: {}", guidance.input_hint)?;
        }
        Ok(())
    }
}

pub(crate) fn position(source: &str, byte: usize) -> (usize, usize) {
    let prefix = &source[..source.floor_char_boundary(byte.min(source.len()))];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
    )
}
