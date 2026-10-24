//! Porte de `yarr/YarrErrorCode.h` e `YarrErrorCode.cpp`.
//!
//! `errorToThrow` monta um `JSObject` (`createSyntaxError`/`createOutOfMemoryError`) e depende do
//! runtime. Aqui ficam as mensagens e o tipo de erro correspondente; a camada de `runtime` (Error)
//! cria o objeto a partir de `error_to_throw_type`.

/// `enum class ErrorCode : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ErrorCode {
    NoError = 0,

    // Um erro duro significa que, qualquer que seja a string em que a RegExp for avaliada, ela
    // sempre falha. Um SyntaxError é erro duro porque a RegExp nunca terá sucesso em string
    // alguma. Um OOME não é erro duro, pois a RegExp pode ter sucesso em outra string.

    // Os seguintes são erros duros.
    PatternTooLarge,
    QuantifierOutOfOrder,
    QuantifierWithoutAtom,
    QuantifierTooLarge,
    QuantifierIncomplete,
    CantQuantifyAtom,
    MissingParentheses,
    BracketUnmatched,
    ParenthesesUnmatched,
    ParenthesesTypeInvalid,
    InvalidGroupName,
    DuplicateGroupName,
    CharacterClassUnmatched,
    CharacterClassRangeOutOfOrder,
    CharacterClassRangeInvalid,
    ClassStringDisjunctionUnmatched,
    EscapeUnterminated,
    InvalidUnicodeEscape,
    InvalidUnicodeCodePointEscape,
    InvalidBackreference,
    InvalidNamedBackReference,
    InvalidIdentityEscape,
    InvalidOctalEscape,
    InvalidControlLetterEscape,
    InvalidUnicodePropertyExpression,
    OffsetTooLarge,
    InvalidRegularExpressionFlags,
    InvalidClassSetOperation,
    NegatedClassSetMayContainStrings,
    InvalidClassSetCharacter,
    InvalidRegularExpressionModifier,
    TooManyCaptures,
    FrameTooLarge,

    // Os seguintes NÃO são erros duros.
    TooManyDisjunctions, // ficamos sem pilha compilando.
}

/// O tipo de erro que `errorToThrow` instancia para cada código.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorType {
    SyntaxError,
    OutOfMemoryError,
}

/// `errorMessage(ErrorCode)`. `NoError` devolve a string vazia (`ASCIILiteral { }`).
pub fn error_message(error: ErrorCode) -> &'static str {
    // A ordem deste array deve casar com o enum ErrorCode.
    const ERROR_MESSAGES: [&str; 35] = [
        "", // NoError
        // Os seguintes são erros duros.
        "Invalid regular expression: regular expression too large", // PatternTooLarge
        "Invalid regular expression: numbers out of order in {} quantifier", // QuantifierOutOfOrder
        "Invalid regular expression: nothing to repeat", // QuantifierWithoutAtom
        "Invalid regular expression: number too large in {} quantifier", // QuantifierTooLarge
        "Invalid regular expression: incomplete {} quantifier for Unicode pattern", // QuantifierIncomplete
        "Invalid regular expression: invalid quantifier", // CantQuantifyAtom
        "Invalid regular expression: missing )", // MissingParentheses
        "Invalid regular expression: unmatched ] or } bracket for Unicode pattern", // BracketUnmatched
        "Invalid regular expression: unmatched parentheses", // ParenthesesUnmatched
        "Invalid regular expression: unrecognized character after (?", // ParenthesesTypeInvalid
        "Invalid regular expression: invalid group specifier name", // InvalidGroupName
        "Invalid regular expression: duplicate group specifier name", // DuplicateGroupName
        "Invalid regular expression: missing terminating ] for character class", // CharacterClassUnmatched
        "Invalid regular expression: range out of order in character class", // CharacterClassRangeOutOfOrder
        "Invalid regular expression: invalid range in character class for Unicode pattern", // CharacterClassRangeInvalid
        "Invalid regular expression: missing terminating } for class string disjunction", // ClassStringDisjunctionUnmatched
        "Invalid regular expression: \\ at end of pattern", // EscapeUnterminated
        "Invalid regular expression: invalid Unicode \\u escape", // InvalidUnicodeEscape
        "Invalid regular expression: invalid Unicode code point \\u{} escape", // InvalidUnicodeCodePointEscape
        "Invalid regular expression: invalid backreference for Unicode pattern", // InvalidBackreference
        "Invalid regular expression: invalid \\k<> named backreference", // InvalidNamedBackReference
        "Invalid regular expression: invalid escaped character for Unicode pattern", // InvalidIdentityEscape
        "Invalid regular expression: invalid octal escape for Unicode pattern", // InvalidOctalEscape
        "Invalid regular expression: invalid \\c escape for Unicode pattern", // InvalidControlLetterEscape
        "Invalid regular expression: invalid property expression", // InvalidUnicodePropertyExpression
        "Invalid regular expression: pattern exceeds string length limits", // OffsetTooLarge
        "Invalid regular expression: invalid flags", // InvalidRegularExpressionFlags
        "Invalid regular expression: invalid operation in class set", // InvalidClassSetOperation
        "Invalid regular expression: negated class set may contain strings", // NegatedClassSetMayContainStrings
        "Invalid regular expression: invalid class set character", // InvalidClassSetCharacter
        "Invalid regular expression: invalid regular expression modifier", // InvalidRegularExpressionModifier
        "Invalid regular expression: too many captures", // TooManyCaptures
        "Invalid regular expression: too many frame slots for state", // FrameTooLarge
        // Os seguintes NÃO são erros duros.
        "Invalid regular expression: too many nested disjunctions", // TooManyDisjunctions
    ];

    ERROR_MESSAGES[error as usize]
}

/// `hasError(ErrorCode)`.
pub fn has_error(error_code: ErrorCode) -> bool {
    error_code != ErrorCode::NoError
}

/// `hasHardError(ErrorCode)`. Veja o comentário no enum `ErrorCode` para a definição de erro duro.
pub fn has_hard_error(error_code: ErrorCode) -> bool {
    has_error(error_code) && error_code < ErrorCode::TooManyDisjunctions
}

/// A decisão de `errorToThrow(JSGlobalObject*, ErrorCode)` sem criar o objeto: qual tipo de erro
/// é lançado. Devolve `None` para `NoError` (o C++ tem `ASSERT_NOT_REACHED` e devolve `nullptr`).
/// A camada futura de `runtime` (createSyntaxError/createOutOfMemoryError, com a mensagem de
/// `error_message`) constrói o `JSObject`.
pub fn error_to_throw_type(error: ErrorCode) -> Option<ErrorType> {
    match error {
        ErrorCode::NoError => None,
        ErrorCode::TooManyDisjunctions => Some(ErrorType::OutOfMemoryError),
        _ => Some(ErrorType::SyntaxError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        assert_eq!(error_message(ErrorCode::NoError), "");
        assert_eq!(
            error_message(ErrorCode::QuantifierWithoutAtom),
            "Invalid regular expression: nothing to repeat"
        );
        assert_eq!(
            error_message(ErrorCode::InvalidUnicodeEscape),
            "Invalid regular expression: invalid Unicode \\u escape"
        );
        assert_eq!(
            error_message(ErrorCode::TooManyDisjunctions),
            "Invalid regular expression: too many nested disjunctions"
        );
        assert_eq!(
            error_message(ErrorCode::FrameTooLarge),
            "Invalid regular expression: too many frame slots for state"
        );
    }

    #[test]
    fn hard_errors_and_types() {
        assert!(!has_hard_error(ErrorCode::NoError));
        assert!(has_hard_error(ErrorCode::FrameTooLarge));
        assert!(!has_hard_error(ErrorCode::TooManyDisjunctions));
        assert!(has_error(ErrorCode::TooManyDisjunctions));
        assert_eq!(error_to_throw_type(ErrorCode::NoError), None);
        assert_eq!(error_to_throw_type(ErrorCode::MissingParentheses), Some(ErrorType::SyntaxError));
        assert_eq!(error_to_throw_type(ErrorCode::TooManyDisjunctions), Some(ErrorType::OutOfMemoryError));
        assert_eq!(ErrorCode::TooManyDisjunctions as usize, 34);
    }
}
