//! Canonicalize adopted and project-local SDL with the shared GraphQL lexer and parser.

use chigiri::core::language::{TokenKind, lex, parse_type_system_document};
use chigiri::core::{ExecutionLimits, Source, SourceId};
use serde::{Deserialize, Serialize};

use super::ProjectError;
use super::digest::Digest;

pub(super) const FORMAT: &str = "chigiri-mock-sdl-normalization/v1";

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdoptedSource {
    pub name: String,
    pub sdl: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdoptedSchema {
    pub normalization: String,
    pub sources: Vec<AdoptedSource>,
    pub digest: Digest,
}

impl AdoptedSchema {
    pub fn from_sources(sources: &[Source]) -> Result<Self, ProjectError> {
        check_sources(sources, true)?;
        let mut remaining = crate::schema::MAX_SDL_BYTES;
        let mut adopted = Vec::with_capacity(sources.len());
        for (index, source) in sources.iter().enumerate() {
            let name = logical_name("schema", index);
            let sdl = normalize(&name, source.text(), &mut remaining)?;
            adopted.push(AdoptedSource { name, sdl });
        }
        let digest = public_digest(&adopted)?;
        Ok(Self {
            normalization: FORMAT.into(),
            sources: adopted,
            digest,
        })
    }

    pub fn verify(&self) -> Result<(), ProjectError> {
        if self.normalization != FORMAT
            || self.sources.is_empty()
            || self.sources.len() > crate::limits::SDL_SOURCES
        {
            return Err(ownership_error());
        }
        let stored: Vec<_> = self
            .sources
            .iter()
            .map(|source| Source::new(source.name.clone(), source.sdl.clone()))
            .collect();
        check_sources(&stored, true).map_err(|_| ownership_error())?;
        let mut remaining = crate::schema::MAX_SDL_BYTES;
        for (index, source) in self.sources.iter().enumerate() {
            let canonical = normalize(&source.name, &source.sdl, &mut remaining)
                .map_err(|_| ownership_error())?;
            if source.name != logical_name("schema", index) || canonical != source.sdl {
                return Err(ownership_error());
            }
        }
        if self.digest != public_digest(&self.sources)? {
            return Err(ownership_error());
        }
        Ok(())
    }

    pub fn to_sources(&self) -> Result<Vec<Source>, ProjectError> {
        self.verify()?;
        Ok(self
            .sources
            .iter()
            .map(|source| Source::new(source.name.clone(), source.sdl.clone()))
            .collect())
    }
}

pub(super) fn normalized_sources(
    role: &str,
    sources: &[Source],
) -> Result<Vec<Source>, ProjectError> {
    check_sources(sources, false)?;
    let mut remaining = crate::schema::MAX_SDL_BYTES;
    sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let name = logical_name(role, index);
            normalize(&name, source.text(), &mut remaining).map(|sdl| Source::new(name, sdl))
        })
        .collect()
}

fn public_digest(sources: &[AdoptedSource]) -> Result<Digest, ProjectError> {
    let pairs: Vec<_> = sources
        .iter()
        .map(|source| (source.name.as_str(), source.sdl.as_str()))
        .collect();
    let bytes = serde_json::to_vec(&(FORMAT, pairs))
        .map_err(|_| ProjectError::new("limit", "public schema digest limit"))?;
    Ok(Digest::bytes(&bytes))
}

fn normalize(name: &str, text: &str, remaining: &mut usize) -> Result<String, ProjectError> {
    let limits = ExecutionLimits::production();
    let source_id = SourceId::new(0);
    let parsed = parse_type_system_document(source_id, text, &limits);
    let tokens = lex(source_id, text, limits.max_diagnostics);
    let empty = tokens.is_valid() && tokens.tokens.len() == 1;
    if !parsed.is_valid() && !empty {
        let span = parsed.diagnostics.iter().next().and_then(|item| item.span);
        return Err(source_error(name, text, span.map(|value| value.start)));
    }
    let mut output = String::with_capacity(text.len().min(*remaining));
    for token in tokens.tokens.iter().filter(|token| !token.kind.is_eof()) {
        if !output.is_empty() {
            push(&mut output, " ", *remaining)?;
        }
        write_token(&mut output, &token.kind, *remaining)?;
    }
    if !output.is_empty() {
        push(&mut output, "\n", *remaining)?;
    }
    *remaining -= output.len();
    Ok(output)
}

fn check_sources(sources: &[Source], nonempty: bool) -> Result<(), ProjectError> {
    if (nonempty && sources.is_empty()) || sources.len() > crate::limits::SDL_SOURCES {
        return Err(schema_error("SDL source count limit"));
    }
    sources.iter().try_fold(0usize, |bytes, source| {
        bytes
            .checked_add(source.text().len())
            .filter(|value| *value <= crate::schema::MAX_SDL_BYTES)
            .ok_or_else(|| schema_error("SDL byte budget exceeded"))
    })?;
    Ok(())
}

fn write_token(output: &mut String, token: &TokenKind, limit: usize) -> Result<(), ProjectError> {
    match token {
        TokenKind::Name(value) | TokenKind::Int(value) | TokenKind::Float(value) => {
            push(output, value, limit)
        }
        TokenKind::String(value) | TokenKind::BlockString(value) => {
            write_string(output, value, limit)
        }
        TokenKind::Bang => push(output, "!", limit),
        TokenKind::Dollar => push(output, "$", limit),
        TokenKind::Ampersand => push(output, "&", limit),
        TokenKind::ParenLeft => push(output, "(", limit),
        TokenKind::ParenRight => push(output, ")", limit),
        TokenKind::Spread => push(output, "...", limit),
        TokenKind::Colon => push(output, ":", limit),
        TokenKind::Equals => push(output, "=", limit),
        TokenKind::At => push(output, "@", limit),
        TokenKind::BracketLeft => push(output, "[", limit),
        TokenKind::BracketRight => push(output, "]", limit),
        TokenKind::BraceLeft => push(output, "{", limit),
        TokenKind::Pipe => push(output, "|", limit),
        TokenKind::BraceRight => push(output, "}", limit),
        TokenKind::Eof => Ok(()),
    }
}

fn write_string(output: &mut String, value: &str, limit: usize) -> Result<(), ProjectError> {
    push(output, "\"", limit)?;
    for character in value.chars() {
        match character {
            '"' => push(output, "\\\"", limit)?,
            '\\' => push(output, "\\\\", limit)?,
            '\u{0008}' => push(output, "\\b", limit)?,
            '\u{000c}' => push(output, "\\f", limit)?,
            '\n' => push(output, "\\n", limit)?,
            '\r' => push(output, "\\r", limit)?,
            '\t' => push(output, "\\t", limit)?,
            '\u{0000}'..='\u{001f}' => {
                let code = character as u32;
                let hex = b"0123456789abcdef";
                let escape = [
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    hex[((code >> 4) & 0xf) as usize],
                    hex[(code & 0xf) as usize],
                ];
                let escape = std::str::from_utf8(&escape).expect("ASCII escape");
                push(output, escape, limit)?;
            }
            _ => {
                let mut bytes = [0; 4];
                push(output, character.encode_utf8(&mut bytes), limit)?;
            }
        }
    }
    push(output, "\"", limit)
}

fn push(output: &mut String, value: &str, limit: usize) -> Result<(), ProjectError> {
    if value.len() > limit.saturating_sub(output.len()) {
        return Err(schema_error("normalized SDL byte budget exceeded"));
    }
    output.push_str(value);
    Ok(())
}

fn logical_name(role: &str, index: usize) -> String {
    format!("{role}/{:03}.graphql", index + 1)
}

fn source_error(name: &str, text: &str, offset: Option<usize>) -> ProjectError {
    let mut error = schema_error("invalid SDL");
    error.set_file(name);
    if let Some(offset) = offset {
        let offset = text.floor_char_boundary(offset.min(text.len()));
        let prefix = &text[..offset];
        error.set_position((
            prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
            prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
        ));
    }
    error
}

fn schema_error(message: &'static str) -> ProjectError {
    ProjectError::new("schema", message)
}

fn ownership_error() -> ProjectError {
    let mut error = ProjectError::new("ownership", "invalid adopted public schema");
    error.set_file("generated/schema.snapshot.json");
    error
}
