//! Format only generated candidates in-process; isolate untrusted author input in a worker.

use full_moon::tokenizer::{Lexer, LexerResult, Token, TokenType};
use stylua_lib::{
    BlockNewlineGaps, CallParenType, CollapseSimpleStatement, Config, IndentType, LineEndings,
    LuaVersion, OutputVerification, QuoteStyle, SortRequiresConfig, SpaceAfterFunctionNames,
};

use super::ScriptError;

/// Stable identifier for the formatter policy used to produce generated candidates.
pub const STYLE: &str = "chigiri-mock-luau-format/v1";
/// Fixed formatter version embedded in the library.
pub const VERSION: &str = "2.5.2";

// Iterate to a fixed point because removing cast parentheses can change later formatting.
const MAX_PASSES: usize = 3;

#[allow(deprecated)]
fn policy() -> Config {
    Config {
        syntax: LuaVersion::Luau,
        column_width: 100,
        line_endings: LineEndings::Unix,
        indent_type: IndentType::Spaces,
        indent_width: 4,
        quote_style: QuoteStyle::AutoPreferDouble,
        no_call_parentheses: false,
        call_parentheses: CallParenType::Always,
        collapse_simple_statement: CollapseSimpleStatement::Never,
        block_newline_gaps: BlockNewlineGaps::Never,
        sort_requires: SortRequiresConfig { enabled: false },
        space_after_function_names: SpaceAfterFunctionNames::Never,
    }
}

/// Format source in memory without I/O, configuration discovery, or Luau execution.
/// Enforce input/output capacity and verify that formatting preserves full-source semantics.
pub fn source(source: &str, maximum: usize) -> Result<String, ScriptError> {
    if source.len() > maximum {
        return Err(error("Luau format input byte limit"));
    }
    reject_controls(source)?;
    let mut current = source.to_owned();
    for _ in 0..MAX_PASSES {
        let output = stylua_lib::format_code(&current, policy(), None, OutputVerification::Full)
            .map_err(diagnostic)?;
        if output.len() > maximum {
            return Err(error("Luau format output byte limit"));
        }
        if output == current {
            return Ok(output);
        }
        current = output;
    }
    Err(error("Luau format did not converge within pass limit"))
}

/// Require source to match the current formatter output exactly.
pub fn check(source: &str, maximum: usize) -> Result<(), ScriptError> {
    if self::source(source, maximum)? != source {
        return Err(error("saved candidate is not in the current Luau format"));
    }
    Ok(())
}

fn reject_controls(source: &str) -> Result<(), ScriptError> {
    // Use the parser's lexer so control bytes inside strings are not mistaken for comment trivia.
    let mut lexer = Lexer::new(source, full_moon::LuaVersion::luau());
    while let Some(result) = lexer.consume() {
        let reference = match result {
            LexerResult::Ok(reference) => reference,
            LexerResult::Recovered(_, errors) | LexerResult::Fatal(errors) => {
                let mut failure = error("Luau format syntax rejected");
                if let Some(first) = errors.first() {
                    failure.body = (first.position().line(), first.position().character());
                }
                return Err(failure);
            }
        };
        for token in reference
            .leading_trivia()
            .chain(std::iter::once(reference.token()))
            .chain(reference.trailing_trivia())
        {
            reject_control(token)?;
        }
    }
    Ok(())
}

fn reject_control(token: &Token) -> Result<(), ScriptError> {
    let comment = match token.token_type() {
        TokenType::SingleLineComment { comment } | TokenType::MultiLineComment { comment, .. } => {
            comment
        }
        _ => return Ok(()),
    };
    for (line, text) in comment.lines().enumerate() {
        if matches!(
            text.trim(),
            "stylua: ignore" | "stylua: ignore start" | "stylua: ignore end"
        ) {
            let mut failure = error("Luau formatter control comment is not allowed");
            failure.body = (token.start_position().line() + line, 1);
            return Err(failure);
        }
    }
    Ok(())
}

fn diagnostic(cause: stylua_lib::Error) -> ScriptError {
    match cause {
        stylua_lib::Error::ParseError(errors) => {
            let mut failure = error("Luau format syntax rejected");
            if let Some(first) = errors.first() {
                let start = first.range().0;
                failure.body = (start.line(), start.character());
            }
            failure
        }
        stylua_lib::Error::VerificationAstError(_)
        | stylua_lib::Error::VerificationAstDifference => error("Luau format verification failed"),
    }
}

fn error(message: &'static str) -> ScriptError {
    ScriptError::new("format", message)
}
