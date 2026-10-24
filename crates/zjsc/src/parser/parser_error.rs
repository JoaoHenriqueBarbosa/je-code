//! Tradução de `parser/ParserError.h`.

use crate::parser::parser_tokens::JSToken;
use crate::wtf::text::wtf_string::String;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxErrorType {
    SyntaxErrorNone = 0,
    SyntaxErrorIrrecoverable = 1,
    SyntaxErrorUnterminatedLiteral = 2,
    SyntaxErrorRecoverable = 3,
}

impl SyntaxErrorType {
    /// `printInternal(PrintStream&, ParserError::SyntaxErrorType)`.
    pub fn print_internal(&self) -> &'static str {
        match self {
            SyntaxErrorType::SyntaxErrorNone => "SyntaxErrorNone",
            SyntaxErrorType::SyntaxErrorIrrecoverable => "SyntaxErrorIrrecoverable",
            SyntaxErrorType::SyntaxErrorUnterminatedLiteral => "SyntaxErrorUnterminatedLiteral",
            SyntaxErrorType::SyntaxErrorRecoverable => "SyntaxErrorRecoverable",
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorType {
    ErrorNone = 0,
    StackOverflow = 1,
    EvalError = 2,
    OutOfMemory = 3,
    SyntaxError = 4,
}

impl ErrorType {
    /// `printInternal(PrintStream&, ParserError::ErrorType)`.
    pub fn print_internal(&self) -> &'static str {
        match self {
            ErrorType::ErrorNone => "ErrorNone",
            ErrorType::StackOverflow => "StackOverflow",
            ErrorType::EvalError => "EvalError",
            ErrorType::OutOfMemory => "OutOfMemory",
            ErrorType::SyntaxError => "SyntaxError",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ParserError {
    token: JSToken,
    message: String,
    line: i32,
    type_: ErrorType,
    syntax_error_type: SyntaxErrorType,
}

impl Default for ParserError {
    fn default() -> Self {
        ParserError {
            token: JSToken::default(),
            message: String::default(),
            line: -1,
            type_: ErrorType::ErrorNone,
            syntax_error_type: SyntaxErrorType::SyntaxErrorNone,
        }
    }
}

impl ParserError {
    /// `ParserError()`.
    pub fn new() -> Self {
        ParserError::default()
    }

    /// `explicit ParserError(ErrorType type)`.
    pub fn with_type(type_: ErrorType) -> Self {
        ParserError { type_, ..ParserError::default() }
    }

    /// `ParserError(ErrorType, SyntaxErrorType, JSToken)`.
    pub fn with_token(type_: ErrorType, syntax_error: SyntaxErrorType, token: JSToken) -> Self {
        ParserError { token, type_, syntax_error_type: syntax_error, ..ParserError::default() }
    }

    /// `ParserError(ErrorType, SyntaxErrorType, JSToken, const String&, int)`.
    pub fn with_message(
        type_: ErrorType,
        syntax_error: SyntaxErrorType,
        token: JSToken,
        msg: &String,
        line: i32,
    ) -> Self {
        ParserError { token, message: msg.clone(), line, type_, syntax_error_type: syntax_error }
    }

    pub fn is_valid(&self) -> bool {
        self.type_ != ErrorType::ErrorNone
    }

    pub fn syntax_error_type(&self) -> SyntaxErrorType {
        self.syntax_error_type
    }

    pub fn token(&self) -> &JSToken {
        &self.token
    }

    pub fn message(&self) -> &String {
        &self.message
    }

    pub fn line(&self) -> i32 {
        self.line
    }

    pub fn type_(&self) -> ErrorType {
        self.type_
    }

    // JSObject* toErrorObject(
    //     JSGlobalObject* globalObject,
    //     SourceCode source, // Note: We must copy the source here, since the objects that pass in their SourceCode field may be destroyed in addErrorInfo.
    //     int overrideLineNumber = -1)
    // {
    //     JSObject* error = nullptr;
    //     switch (m_type) {
    //     case ErrorNone:
    //         return nullptr;
    //     case SyntaxError:
    //         error = addErrorInfo(
    //             globalObject->vm(),
    //             createSyntaxError(globalObject, m_message),
    //             overrideLineNumber == -1 ? m_line : overrideLineNumber, source);
    //         break;
    //     case EvalError:
    //         error = createSyntaxError(globalObject, m_message);
    //         break;
    //     case StackOverflow: {
    //         ErrorHandlingScope errorScope(getVM(globalObject));
    //         error = createStackOverflowError(globalObject);
    //         break;
    //     }
    //     case OutOfMemory:
    //         error = createOutOfMemoryError(globalObject);
    //         break;
    //     }
    //     downcast<ErrorInstance>(*error).setParseError();
    //     return error;
    // }
    //
    // Fica para a camada de runtime (depende de JSGlobalObject, ErrorInstance e SourceCode).
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_names() {
        let e = ParserError::new();
        assert!(!e.is_valid());
        assert_eq!(e.line(), -1);
        assert_eq!(e.syntax_error_type(), SyntaxErrorType::SyntaxErrorNone);
        assert!(ParserError::with_type(ErrorType::OutOfMemory).is_valid());
        assert_eq!(ErrorType::StackOverflow as u8, 1);
        assert_eq!(SyntaxErrorType::SyntaxErrorRecoverable as u8, 3);
        assert_eq!(ErrorType::SyntaxError.print_internal(), "SyntaxError");
        assert_eq!(SyntaxErrorType::SyntaxErrorUnterminatedLiteral.print_internal(), "SyntaxErrorUnterminatedLiteral");
    }
}
