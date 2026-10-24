//! Tradução de `parser/ParserTokens.h`.
//!
//! O `enum JSTokenType` do C++ combina bits de flag com valores pequenos, então os valores numéricos
//! são parte do contrato (o lexer e o parser fazem contas de bits sobre eles). Por isso vira
//! `u32` com uma constante para cada enumerador, com as mesmas contas do C++.

use std::ops::{Add, Sub};

use crate::runtime::identifier::Identifier;

/// `SourceCodeRepresentation` (JSCJSValue.h:91), que o `ParserTokens.h` usa nos literais numéricos.
pub use crate::runtime::js_cjs_value_types::SourceCodeRepresentation;

/// `BINARY_OP_PRECEDENCE(prec)`.
pub const fn binary_op_precedence(prec: u32) -> u32 {
    (prec << BINARY_OP_TOKEN_PRECEDENCE_SHIFT)
        | (prec << (BINARY_OP_TOKEN_PRECEDENCE_SHIFT + BINARY_OP_TOKEN_ALLOWS_IN_PRECEDENCE_ADDITIONAL_SHIFT))
}

/// `IN_OP_PRECEDENCE(prec)`.
pub const fn in_op_precedence(prec: u32) -> u32 {
    prec << (BINARY_OP_TOKEN_PRECEDENCE_SHIFT + BINARY_OP_TOKEN_ALLOWS_IN_PRECEDENCE_ADDITIONAL_SHIFT)
}

/// `enum JSTokenType` do C++.
//
// Token Bitfield: 0b000000000RTE00IIIIPPPPKUXXXXXXXX
// R = right-associative bit
// T = unterminated error flag
// E = error flag
// I = binary operator allows 'in'
// P = binary operator precedence
// K = keyword flag
// U = unary operator flag
//
// Os 8 bits superiores ficam vazios: JSTokenType cabe em 24 bits.
pub type JSTokenType = u32;

pub const UNARY_OP_TOKEN_FLAG: JSTokenType = 1 << 8;
pub const KEYWORD_TOKEN_FLAG: JSTokenType = 1 << 9;
pub const BINARY_OP_TOKEN_PRECEDENCE_SHIFT: JSTokenType = 10;
pub const BINARY_OP_TOKEN_ALLOWS_IN_PRECEDENCE_ADDITIONAL_SHIFT: JSTokenType = 4;
pub const BINARY_OP_TOKEN_PRECEDENCE_MASK: JSTokenType = 15 << BINARY_OP_TOKEN_PRECEDENCE_SHIFT;
pub const CAN_BE_ERROR_TOKEN_FLAG: JSTokenType =
    1 << (BINARY_OP_TOKEN_ALLOWS_IN_PRECEDENCE_ADDITIONAL_SHIFT + BINARY_OP_TOKEN_PRECEDENCE_SHIFT + 6);
pub const UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG: JSTokenType = CAN_BE_ERROR_TOKEN_FLAG << 1;
pub const RIGHT_ASSOCIATIVE_BINARY_OP_TOKEN_FLAG: JSTokenType = UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG << 1;

pub const NULLTOKEN: JSTokenType = KEYWORD_TOKEN_FLAG;
pub const TRUETOKEN: JSTokenType = NULLTOKEN + 1;
pub const FALSETOKEN: JSTokenType = TRUETOKEN + 1;
pub const BREAK: JSTokenType = FALSETOKEN + 1;
pub const CASE: JSTokenType = BREAK + 1;
pub const DEFAULT: JSTokenType = CASE + 1;
pub const FOR: JSTokenType = DEFAULT + 1;
pub const NEW: JSTokenType = FOR + 1;
pub const VAR: JSTokenType = NEW + 1;
pub const CONSTTOKEN: JSTokenType = VAR + 1;
pub const CONTINUE: JSTokenType = CONSTTOKEN + 1;
pub const FUNCTION: JSTokenType = CONTINUE + 1;
pub const RETURN: JSTokenType = FUNCTION + 1;
pub const IF: JSTokenType = RETURN + 1;
pub const THISTOKEN: JSTokenType = IF + 1;
pub const DO: JSTokenType = THISTOKEN + 1;
pub const WHILE: JSTokenType = DO + 1;
pub const SWITCH: JSTokenType = WHILE + 1;
pub const WITH: JSTokenType = SWITCH + 1;
pub const RESERVED: JSTokenType = WITH + 1;
pub const RESERVED_IF_STRICT: JSTokenType = RESERVED + 1;
pub const THROW: JSTokenType = RESERVED_IF_STRICT + 1;
pub const TRY: JSTokenType = THROW + 1;
pub const CATCH: JSTokenType = TRY + 1;
pub const FINALLY: JSTokenType = CATCH + 1;
pub const DEBUGGER: JSTokenType = FINALLY + 1;
pub const ELSE: JSTokenType = DEBUGGER + 1;
pub const IMPORT: JSTokenType = ELSE + 1;
pub const EXPORT_: JSTokenType = IMPORT + 1;
pub const CLASSTOKEN: JSTokenType = EXPORT_ + 1;
pub const EXTENDS: JSTokenType = CLASSTOKEN + 1;
pub const SUPER: JSTokenType = EXTENDS + 1;

// Contextual keywords

pub const LET: JSTokenType = SUPER + 1;
pub const YIELD: JSTokenType = LET + 1;
pub const AWAIT: JSTokenType = YIELD + 1;

pub const FIRST_CONTEXTUAL_KEYWORD_TOKEN: JSTokenType = LET;
pub const LAST_CONTEXTUAL_KEYWORD_TOKEN: JSTokenType = AWAIT;

pub const OPENBRACE: JSTokenType = 0;
pub const CLOSEBRACE: JSTokenType = OPENBRACE + 1;
pub const OPENPAREN: JSTokenType = CLOSEBRACE + 1;
pub const CLOSEPAREN: JSTokenType = OPENPAREN + 1;
pub const OPENBRACKET: JSTokenType = CLOSEPAREN + 1;
pub const CLOSEBRACKET: JSTokenType = OPENBRACKET + 1;
pub const COMMA: JSTokenType = CLOSEBRACKET + 1;
pub const QUESTION: JSTokenType = COMMA + 1;
pub const BACKQUOTE: JSTokenType = QUESTION + 1;
pub const INTEGER: JSTokenType = BACKQUOTE + 1;
pub const DOUBLE: JSTokenType = INTEGER + 1;
pub const BIGINT: JSTokenType = DOUBLE + 1;
pub const IDENT: JSTokenType = BIGINT + 1;
pub const PRIVATENAME: JSTokenType = IDENT + 1;
pub const STRING: JSTokenType = PRIVATENAME + 1;
pub const TEMPLATE: JSTokenType = STRING + 1;
pub const REGEXP: JSTokenType = TEMPLATE + 1;
pub const SEMICOLON: JSTokenType = REGEXP + 1;
pub const COLON: JSTokenType = SEMICOLON + 1;
pub const DOT: JSTokenType = COLON + 1;
pub const EOFTOK: JSTokenType = DOT + 1;
pub const EQUAL: JSTokenType = EOFTOK + 1;
pub const PLUSEQUAL: JSTokenType = EQUAL + 1;
pub const MINUSEQUAL: JSTokenType = PLUSEQUAL + 1;
pub const MULTEQUAL: JSTokenType = MINUSEQUAL + 1;
pub const DIVEQUAL: JSTokenType = MULTEQUAL + 1;
pub const LSHIFTEQUAL: JSTokenType = DIVEQUAL + 1;
pub const RSHIFTEQUAL: JSTokenType = LSHIFTEQUAL + 1;
pub const URSHIFTEQUAL: JSTokenType = RSHIFTEQUAL + 1;
pub const MODEQUAL: JSTokenType = URSHIFTEQUAL + 1;
pub const POWEQUAL: JSTokenType = MODEQUAL + 1;
pub const BITANDEQUAL: JSTokenType = POWEQUAL + 1;
pub const BITXOREQUAL: JSTokenType = BITANDEQUAL + 1;
pub const BITOREQUAL: JSTokenType = BITXOREQUAL + 1;
pub const COALESCEEQUAL: JSTokenType = BITOREQUAL + 1;
pub const OREQUAL: JSTokenType = COALESCEEQUAL + 1;
pub const ANDEQUAL: JSTokenType = OREQUAL + 1;
pub const DOTDOTDOT: JSTokenType = ANDEQUAL + 1;
pub const ARROWFUNCTION: JSTokenType = DOTDOTDOT + 1;
pub const QUESTIONDOT: JSTokenType = ARROWFUNCTION + 1;
pub const LAST_UNTAGGED_TOKEN: JSTokenType = QUESTIONDOT + 1;

// Begin tagged tokens
pub const PLUSPLUS: JSTokenType = UNARY_OP_TOKEN_FLAG;
pub const MINUSMINUS: JSTokenType = 1 | UNARY_OP_TOKEN_FLAG;
pub const AUTOPLUSPLUS: JSTokenType = 2 | UNARY_OP_TOKEN_FLAG;
pub const AUTOMINUSMINUS: JSTokenType = 3 | UNARY_OP_TOKEN_FLAG;
pub const EXCLAMATION: JSTokenType = 4 | UNARY_OP_TOKEN_FLAG;
pub const TILDE: JSTokenType = 5 | UNARY_OP_TOKEN_FLAG;
pub const TYPEOF: JSTokenType = 6 | UNARY_OP_TOKEN_FLAG | KEYWORD_TOKEN_FLAG;
pub const VOIDTOKEN: JSTokenType = 7 | UNARY_OP_TOKEN_FLAG | KEYWORD_TOKEN_FLAG;
pub const DELETETOKEN: JSTokenType = 8 | UNARY_OP_TOKEN_FLAG | KEYWORD_TOKEN_FLAG;
pub const COALESCE: JSTokenType = binary_op_precedence(1);
pub const OR: JSTokenType = binary_op_precedence(2);
pub const AND: JSTokenType = binary_op_precedence(3);
pub const BITOR: JSTokenType = binary_op_precedence(4);
pub const BITXOR: JSTokenType = binary_op_precedence(5);
pub const BITAND: JSTokenType = binary_op_precedence(6);
pub const EQEQ: JSTokenType = binary_op_precedence(7);
pub const NE: JSTokenType = 1 | binary_op_precedence(7);
pub const STREQ: JSTokenType = 2 | binary_op_precedence(7);
pub const STRNEQ: JSTokenType = 3 | binary_op_precedence(7);
pub const LT: JSTokenType = binary_op_precedence(8);
pub const GT: JSTokenType = 1 | binary_op_precedence(8);
pub const LE: JSTokenType = 2 | binary_op_precedence(8);
pub const GE: JSTokenType = 3 | binary_op_precedence(8);
pub const INSTANCEOF: JSTokenType = 4 | binary_op_precedence(8) | KEYWORD_TOKEN_FLAG;
pub const INTOKEN: JSTokenType = 5 | in_op_precedence(8) | KEYWORD_TOKEN_FLAG;
pub const LSHIFT: JSTokenType = binary_op_precedence(9);
pub const RSHIFT: JSTokenType = 1 | binary_op_precedence(9);
pub const URSHIFT: JSTokenType = 2 | binary_op_precedence(9);
pub const PLUS: JSTokenType = binary_op_precedence(10) | UNARY_OP_TOKEN_FLAG;
pub const MINUS: JSTokenType = 1 | binary_op_precedence(10) | UNARY_OP_TOKEN_FLAG;
pub const TIMES: JSTokenType = binary_op_precedence(11);
pub const DIVIDE: JSTokenType = 1 | binary_op_precedence(11);
pub const MOD: JSTokenType = 2 | binary_op_precedence(11);
/// Garante que POW tem a maior precedência de operador.
pub const POW: JSTokenType = binary_op_precedence(12) | RIGHT_ASSOCIATIVE_BINARY_OP_TOKEN_FLAG;
pub const ERRORTOK: JSTokenType = CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_IDENTIFIER_ESCAPE_ERRORTOK: JSTokenType =
    CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const INVALID_IDENTIFIER_ESCAPE_ERRORTOK: JSTokenType = 1 | CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_IDENTIFIER_UNICODE_ESCAPE_ERRORTOK: JSTokenType =
    2 | CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const INVALID_IDENTIFIER_UNICODE_ESCAPE_ERRORTOK: JSTokenType = 3 | CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_MULTILINE_COMMENT_ERRORTOK: JSTokenType =
    4 | CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_NUMERIC_LITERAL_ERRORTOK: JSTokenType =
    5 | CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_OCTAL_NUMBER_ERRORTOK: JSTokenType =
    6 | CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const INVALID_NUMERIC_LITERAL_ERRORTOK: JSTokenType = 7 | CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_STRING_LITERAL_ERRORTOK: JSTokenType =
    8 | CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const INVALID_STRING_LITERAL_ERRORTOK: JSTokenType = 9 | CAN_BE_ERROR_TOKEN_FLAG;
pub const INVALID_PRIVATE_NAME_ERRORTOK: JSTokenType = 10 | CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_HEX_NUMBER_ERRORTOK: JSTokenType =
    11 | CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_BINARY_NUMBER_ERRORTOK: JSTokenType =
    12 | CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_TEMPLATE_LITERAL_ERRORTOK: JSTokenType =
    13 | CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const UNTERMINATED_REGEXP_LITERAL_ERRORTOK: JSTokenType =
    14 | CAN_BE_ERROR_TOKEN_FLAG | UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG;
pub const INVALID_TEMPLATE_LITERAL_ERRORTOK: JSTokenType = 15 | CAN_BE_ERROR_TOKEN_FLAG;
pub const ESCAPED_KEYWORD: JSTokenType = 16 | CAN_BE_ERROR_TOKEN_FLAG;
pub const INVALID_UNICODE_ENCODING_ERRORTOK: JSTokenType = 17 | CAN_BE_ERROR_TOKEN_FLAG;
pub const INVALID_IDENTIFIER_UNICODE_ERRORTOK: JSTokenType = 18 | CAN_BE_ERROR_TOKEN_FLAG;

const _: () = assert!(POW <= 0x00ff_ffff, "JSTokenType must be 24bits.");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JSTextPosition {
    // FIXME do C++: deveriam ser unsigned.
    pub line: i32,
    pub offset: i32,
    pub line_start_offset: i32,
}

impl Default for JSTextPosition {
    fn default() -> Self {
        JSTextPosition { line: -1, offset: -1, line_start_offset: -1 }
    }
}

impl JSTextPosition {
    pub fn new(line: i32, offset: i32, line_start_offset: i32) -> Self {
        JSTextPosition { line, offset, line_start_offset }
    }

    /// `operator int()`.
    pub fn as_int(&self) -> i32 {
        self.offset
    }

    /// `explicit operator bool()`.
    pub fn is_set(&self) -> bool {
        *self != JSTextPosition::default()
    }

    pub fn column(&self) -> i32 {
        self.offset - self.line_start_offset
    }
}

impl Add<i32> for JSTextPosition {
    type Output = JSTextPosition;
    fn add(self, adjustment: i32) -> JSTextPosition {
        JSTextPosition::new(self.line, self.offset.wrapping_add(adjustment), self.line_start_offset)
    }
}

impl Add<u32> for JSTextPosition {
    type Output = JSTextPosition;
    fn add(self, adjustment: u32) -> JSTextPosition {
        self + (adjustment as i32)
    }
}

impl Sub<i32> for JSTextPosition {
    type Output = JSTextPosition;
    fn sub(self, adjustment: i32) -> JSTextPosition {
        self + adjustment.wrapping_neg()
    }
}

impl Sub<u32> for JSTextPosition {
    type Output = JSTextPosition;
    fn sub(self, adjustment: u32) -> JSTextPosition {
        self + (adjustment as i32).wrapping_neg()
    }
}

/// `union JSTokenData`: no C++ os membros compartilham memória e cada tipo de token usa o seu
/// grupo. Aqui são campos separados; quem lê um grupo só o faz para os tokens que o escreveram
/// (o grupo `cooked/raw/is_tail` é de TEMPLATE, `line/offset/line_start_offset` de posição,
/// `double_value` de DOUBLE e INTEGER, `ident/escaped` de IDENT e STRING, `big_int_string/radix`
/// de BIGINT, `pattern/flags` de REGEXP).
#[derive(Clone, Debug, Default)]
pub struct JSTokenData {
    pub cooked: Option<Identifier>,
    pub raw: Option<Identifier>,
    pub is_tail: bool,
    pub line: u32,
    pub offset: u32,
    pub line_start_offset: u32,
    pub double_value: f64,
    pub ident: Option<Identifier>,
    pub escaped: bool,
    pub big_int_string: Option<Identifier>,
    pub radix: u8,
    pub pattern: Option<Identifier>,
    pub flags: Option<Identifier>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JSTokenLocation {
    pub line: i32,
    pub line_start_offset: u32,
    pub start_offset: u32,
    pub end_offset: u32,
}

#[derive(Clone, Debug)]
pub struct JSToken {
    pub type_: JSTokenType,
    pub data: JSTokenData,
    pub start_position: JSTextPosition,
    pub end_position: JSTextPosition,
}

impl Default for JSToken {
    fn default() -> Self {
        JSToken {
            type_: ERRORTOK,
            data: JSTokenData::default(),
            start_position: JSTextPosition::default(),
            end_position: JSTextPosition::default(),
        }
    }
}

impl JSToken {
    pub fn location(&self) -> JSTokenLocation {
        JSTokenLocation {
            line: self.start_position.line,
            line_start_offset: self.start_position.line_start_offset as u32,
            start_offset: self.start_position.offset as u32,
            end_offset: self.end_position.offset as u32,
        }
    }

    // void dump(WTF::PrintStream&) const;
    // (definido em ParserTokens.cpp/Lexer.cpp; só serve a dump de depuração.)
}

#[inline(always)]
pub fn is_update_op(token: JSTokenType) -> bool {
    token >= PLUSPLUS && token <= AUTOMINUSMINUS
}

#[inline(always)]
pub fn is_unary_op(token: JSTokenType) -> bool {
    token & UNARY_OP_TOKEN_FLAG != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_and_precedence_bits() {
        assert_eq!(UNARY_OP_TOKEN_FLAG, 0x100);
        assert_eq!(KEYWORD_TOKEN_FLAG, 0x200);
        assert_eq!(CAN_BE_ERROR_TOKEN_FLAG, 1 << 20);
        assert_eq!(UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG, 1 << 21);
        assert_eq!(RIGHT_ASSOCIATIVE_BINARY_OP_TOKEN_FLAG, 1 << 22);
        assert_eq!(COALESCE, (1 << 10) | (1 << 14));
        assert_eq!(INTOKEN, 5 | (8 << 14) | 0x200);
        assert_eq!(POW, (12 << 10) | (12 << 14) | (1 << 22));
        assert_eq!(PLUS, (10 << 10) | (10 << 14) | 0x100);
        assert!(POW <= 0x00ff_ffff);
    }

    #[test]
    fn sequential_values() {
        assert_eq!(NULLTOKEN, 512);
        assert_eq!(LET, 512 + 32);
        assert_eq!(AWAIT, 512 + 34);
        assert_eq!(OPENBRACE, 0);
        assert_eq!(EOFTOK, 20);
        assert_eq!(LAST_UNTAGGED_TOKEN, 40);
        assert_eq!(ERRORTOK, 1 << 20);
        assert_eq!(UNTERMINATED_IDENTIFIER_ESCAPE_ERRORTOK, (1 << 20) | (1 << 21));
    }

    #[test]
    fn predicates() {
        assert!(is_update_op(PLUSPLUS) && is_update_op(AUTOMINUSMINUS));
        assert!(!is_update_op(EXCLAMATION));
        assert!(is_unary_op(TYPEOF) && is_unary_op(PLUS) && is_unary_op(MINUS));
        assert!(!is_unary_op(TIMES));
    }

    #[test]
    fn text_position() {
        let p = JSTextPosition::new(2, 10, 4);
        assert_eq!(p.column(), 6);
        assert_eq!((p + 3i32).offset, 13);
        assert_eq!((p - 3u32).offset, 7);
        assert!(p.is_set());
        assert!(!JSTextPosition::default().is_set());
    }
}
