//! Tradução de `parser/Lexer.h` (inteiro) e de `parser/Lexer.cpp` (linhas 1 a 900).
//!
//! Decisões do porte, todas em cima do que o C++ faz com ponteiros:
//!
//! - `m_code`, `m_codeStart`, `m_codeEnd`, `m_lineStart` e `m_codeStartPlusOffset` são índices
//!   (`usize`) no texto do fonte, que o `Lexer` mantém vivo por um `Rc<StringImpl>`. Como o C++
//!   lê `*m_code` mesmo no fim do buffer (onde o byte vale 0 por convenção do lexer), leitura
//!   fora do texto devolve 0.
//! - `VM&` vira `Rc<VM>`; `IdentifierArena*` vira o `Rc<RefCell<IdentifierArena>>` que o
//!   `ParserArena` entrega; `const SourceCode*` vira uma cópia de `SourceCode` (no C++ é um
//!   `RefPtr<SourceProvider>` mais dois deslocamentos, barato de copiar).
//! - `const Identifier*` vira `Identifier` (um `Rc` por baixo; a arena continua dona do original).
//! - `OptionSet<LexerFlags>` vira `LexerFlagSet`, como o `FlagSet` do yarr.
//! - Especializações por `Latin1Character`/`char16_t` viram o genérico `T: CharType`. Onde a
//!   especialização de `char16_t` só acrescenta um ramo para caracteres acima de 0xFF (que não
//!   existem em `Latin1Character`), a função genérica traz o ramo e o `u8` nunca o alcança.
//!
//! O resto do `Lexer.cpp` (linha 901 em diante: `lex`, strings, números, template, comentários)
//! entra em outros blocos `impl<T: CharType> Lexer<T>` ao fim deste arquivo.
//!
//! `Lexer::verifyLayout` e o `JSC_CACHE_LINE_ALIGNED` não existem: só valem em ARM64 sem
//! asserts e não têm efeito observável.
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::parser::keyword_lookup::{match_keyword, MAX_TOKEN_LENGTH};
use crate::parser::lexer_lut::main_table_entry;
use crate::parser::lexer_unicode_properties::is_non_latin1_white_space;
use crate::parser::parser_arena::{IdentifierArena, ParserArena};
use crate::parser::parser_modes::{JSParserBuiltinMode, JSParserScriptMode};
use crate::parser::parser_tokens::*;
use crate::parser::source_code::SourceCode;
use crate::runtime::identifier::Identifier;
use crate::runtime::options::Options;
use crate::runtime::vm::VM;
use crate::wtf::ascii_ctype::{
    is_ascii_binary_digit, is_ascii_digit, is_ascii_hex_digit, is_ascii_octal_digit, to_ascii_hex_value, AsciiChar,
};
use crate::wtf::math_extras::truncate_double_to_int64;
use crate::wtf::option_set::{OptionSet, OptionSetFlag};
use crate::wtf::text::string_impl::{CharType, StringImpl};
use crate::wtf::text::string_view::StringView;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::unicode::{is_id_continue, is_id_start};

use self::CharacterType::*;

/// `enum class LexerFlags`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexerFlags {
    IgnoreReservedWords = 1 << 0,
    DontBuildStrings = 1 << 1,
    DontBuildKeywords = 1 << 2,
}

impl OptionSetFlag for LexerFlags {
    type Mask = u8;
    const NONE: u8 = 0;
    const ALL: &'static [LexerFlags] =
        &[LexerFlags::IgnoreReservedWords, LexerFlags::DontBuildStrings, LexerFlags::DontBuildKeywords];

    fn bit(self) -> u8 {
        self as u8
    }
}

/// `OptionSet<LexerFlags>`.
pub type LexerFlagSet = OptionSet<LexerFlags>;

/// `Lexer<T>::RawStringsBuildMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawStringsBuildMode {
    BuildRawStrings,
    DontBuildRawStrings,
}

/// `Lexer<T>::StringParseResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringParseResult {
    StringParsedSuccessfully,
    StringUnterminated,
    StringCannotBeParsed,
}

/// `Lexer<T>::NumberParseResult` (`Variant<double, const Identifier*>`).
#[derive(Clone, Debug)]
pub enum NumberParseResult {
    Double(f64),
    Identifier(Identifier),
}

/// `constinit const WTF::BitSet<256> whiteSpaceTable`.
pub use ul_common::js_number::WHITE_SPACE_TABLE;

/// `isLexerKeyword(const Identifier&)`.
pub fn is_lexer_keyword(identifier: &Identifier) -> bool {
    let Some(string) = identifier.string().impl_() else {
        return main_table_entry::<u8>(&[]).is_some();
    };
    if string.is_8bit() {
        main_table_entry(string.span8()).is_some()
    } else {
        main_table_entry(string.span16()).is_some()
    }
}

/// `enum CharacterType : uint8_t` do `Lexer.cpp`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CharacterType {
    // Types for the main switch

    // The first three types are fixed, and also used for identifying
    // ASCII alpha and alphanumeric characters (see isIdentStart and isIdentPart).
    CharacterLatin1IdentifierStart,
    CharacterZero,
    CharacterNumber,

    // For single-byte characters grandfathered into Other_ID_Continue -- namely just U+00B7 MIDDLE DOT.
    // (http://unicode.org/reports/tr31/#Backward_Compatibility)
    //
    // Character types are divided into two groups depending on whether they can be part of an
    // identifier or not. Those whose type value is less or equal than CharacterOtherIdentifierPart can be
    // part of an identifier. (See the CharacterType definition for more details.)
    CharacterOtherIdentifierPart,
    CharacterBackSlash, // Keep the ordering until this. We use this ordering to detect identifier-part or back-slash quickly.

    CharacterInvalid,
    CharacterLineTerminator,
    CharacterExclamationMark,
    CharacterOpenParen,
    CharacterCloseParen,
    CharacterOpenBracket,
    CharacterCloseBracket,
    CharacterComma,
    CharacterColon,
    CharacterQuestion,
    CharacterTilde,
    CharacterQuote,
    CharacterBackQuote,
    CharacterDot,
    CharacterSlash,
    CharacterSemicolon,
    CharacterOpenBrace,
    CharacterCloseBrace,

    CharacterAdd,
    CharacterSub,
    CharacterMultiply,
    CharacterModulo,
    CharacterAnd,
    CharacterXor,
    CharacterOr,
    CharacterLess,
    CharacterGreater,
    CharacterEqual,

    // Other types (only one so far)
    CharacterWhiteSpace,
    CharacterHash,
    CharacterPrivateIdentifierStart,
    CharacterNonLatin1IdentifierStart,
}

/// `typesOfLatin1Characters`: 256 Latin-1 codes.
pub static TYPES_OF_LATIN1_CHARACTERS: [CharacterType; 256] = [
    /*   0 - Null               */ CharacterInvalid,
    /*   1 - Start of Heading   */ CharacterInvalid,
    /*   2 - Start of Text      */ CharacterInvalid,
    /*   3 - End of Text        */ CharacterInvalid,
    /*   4 - End of Transm.     */ CharacterInvalid,
    /*   5 - Enquiry            */ CharacterInvalid,
    /*   6 - Acknowledgment     */ CharacterInvalid,
    /*   7 - Bell               */ CharacterInvalid,
    /*   8 - Back Space         */ CharacterInvalid,
    /*   9 - Horizontal Tab     */ CharacterWhiteSpace,
    /*  10 - Line Feed          */ CharacterLineTerminator,
    /*  11 - Vertical Tab       */ CharacterWhiteSpace,
    /*  12 - Form Feed          */ CharacterWhiteSpace,
    /*  13 - Carriage Return    */ CharacterLineTerminator,
    /*  14 - Shift Out          */ CharacterInvalid,
    /*  15 - Shift In           */ CharacterInvalid,
    /*  16 - Data Line Escape   */ CharacterInvalid,
    /*  17 - Device Control 1   */ CharacterInvalid,
    /*  18 - Device Control 2   */ CharacterInvalid,
    /*  19 - Device Control 3   */ CharacterInvalid,
    /*  20 - Device Control 4   */ CharacterInvalid,
    /*  21 - Negative Ack.      */ CharacterInvalid,
    /*  22 - Synchronous Idle   */ CharacterInvalid,
    /*  23 - End of Transmit    */ CharacterInvalid,
    /*  24 - Cancel             */ CharacterInvalid,
    /*  25 - End of Medium      */ CharacterInvalid,
    /*  26 - Substitute         */ CharacterInvalid,
    /*  27 - Escape             */ CharacterInvalid,
    /*  28 - File Separator     */ CharacterInvalid,
    /*  29 - Group Separator    */ CharacterInvalid,
    /*  30 - Record Separator   */ CharacterInvalid,
    /*  31 - Unit Separator     */ CharacterInvalid,
    /*  32 - Space              */ CharacterWhiteSpace,
    /*  33 - !                  */ CharacterExclamationMark,
    /*  34 - "                  */ CharacterQuote,
    /*  35 - #                  */ CharacterHash,
    /*  36 - $                  */ CharacterLatin1IdentifierStart,
    /*  37 - %                  */ CharacterModulo,
    /*  38 - &                  */ CharacterAnd,
    /*  39 - '                  */ CharacterQuote,
    /*  40 - (                  */ CharacterOpenParen,
    /*  41 - )                  */ CharacterCloseParen,
    /*  42 - *                  */ CharacterMultiply,
    /*  43 - +                  */ CharacterAdd,
    /*  44 - ,                  */ CharacterComma,
    /*  45 - -                  */ CharacterSub,
    /*  46 - .                  */ CharacterDot,
    /*  47 - /                  */ CharacterSlash,
    /*  48 - 0                  */ CharacterZero,
    /*  49 - 1                  */ CharacterNumber,
    /*  50 - 2                  */ CharacterNumber,
    /*  51 - 3                  */ CharacterNumber,
    /*  52 - 4                  */ CharacterNumber,
    /*  53 - 5                  */ CharacterNumber,
    /*  54 - 6                  */ CharacterNumber,
    /*  55 - 7                  */ CharacterNumber,
    /*  56 - 8                  */ CharacterNumber,
    /*  57 - 9                  */ CharacterNumber,
    /*  58 - :                  */ CharacterColon,
    /*  59 - ;                  */ CharacterSemicolon,
    /*  60 - <                  */ CharacterLess,
    /*  61 - =                  */ CharacterEqual,
    /*  62 - >                  */ CharacterGreater,
    /*  63 - ?                  */ CharacterQuestion,
    /*  64 - @                  */ CharacterPrivateIdentifierStart,
    /*  65 - A                  */ CharacterLatin1IdentifierStart,
    /*  66 - B                  */ CharacterLatin1IdentifierStart,
    /*  67 - C                  */ CharacterLatin1IdentifierStart,
    /*  68 - D                  */ CharacterLatin1IdentifierStart,
    /*  69 - E                  */ CharacterLatin1IdentifierStart,
    /*  70 - F                  */ CharacterLatin1IdentifierStart,
    /*  71 - G                  */ CharacterLatin1IdentifierStart,
    /*  72 - H                  */ CharacterLatin1IdentifierStart,
    /*  73 - I                  */ CharacterLatin1IdentifierStart,
    /*  74 - J                  */ CharacterLatin1IdentifierStart,
    /*  75 - K                  */ CharacterLatin1IdentifierStart,
    /*  76 - L                  */ CharacterLatin1IdentifierStart,
    /*  77 - M                  */ CharacterLatin1IdentifierStart,
    /*  78 - N                  */ CharacterLatin1IdentifierStart,
    /*  79 - O                  */ CharacterLatin1IdentifierStart,
    /*  80 - P                  */ CharacterLatin1IdentifierStart,
    /*  81 - Q                  */ CharacterLatin1IdentifierStart,
    /*  82 - R                  */ CharacterLatin1IdentifierStart,
    /*  83 - S                  */ CharacterLatin1IdentifierStart,
    /*  84 - T                  */ CharacterLatin1IdentifierStart,
    /*  85 - U                  */ CharacterLatin1IdentifierStart,
    /*  86 - V                  */ CharacterLatin1IdentifierStart,
    /*  87 - W                  */ CharacterLatin1IdentifierStart,
    /*  88 - X                  */ CharacterLatin1IdentifierStart,
    /*  89 - Y                  */ CharacterLatin1IdentifierStart,
    /*  90 - Z                  */ CharacterLatin1IdentifierStart,
    /*  91 - [                  */ CharacterOpenBracket,
    /*  92 - \                  */ CharacterBackSlash,
    /*  93 - ]                  */ CharacterCloseBracket,
    /*  94 - ^                  */ CharacterXor,
    /*  95 - _                  */ CharacterLatin1IdentifierStart,
    /*  96 - `                  */ CharacterBackQuote,
    /*  97 - a                  */ CharacterLatin1IdentifierStart,
    /*  98 - b                  */ CharacterLatin1IdentifierStart,
    /*  99 - c                  */ CharacterLatin1IdentifierStart,
    /* 100 - d                  */ CharacterLatin1IdentifierStart,
    /* 101 - e                  */ CharacterLatin1IdentifierStart,
    /* 102 - f                  */ CharacterLatin1IdentifierStart,
    /* 103 - g                  */ CharacterLatin1IdentifierStart,
    /* 104 - h                  */ CharacterLatin1IdentifierStart,
    /* 105 - i                  */ CharacterLatin1IdentifierStart,
    /* 106 - j                  */ CharacterLatin1IdentifierStart,
    /* 107 - k                  */ CharacterLatin1IdentifierStart,
    /* 108 - l                  */ CharacterLatin1IdentifierStart,
    /* 109 - m                  */ CharacterLatin1IdentifierStart,
    /* 110 - n                  */ CharacterLatin1IdentifierStart,
    /* 111 - o                  */ CharacterLatin1IdentifierStart,
    /* 112 - p                  */ CharacterLatin1IdentifierStart,
    /* 113 - q                  */ CharacterLatin1IdentifierStart,
    /* 114 - r                  */ CharacterLatin1IdentifierStart,
    /* 115 - s                  */ CharacterLatin1IdentifierStart,
    /* 116 - t                  */ CharacterLatin1IdentifierStart,
    /* 117 - u                  */ CharacterLatin1IdentifierStart,
    /* 118 - v                  */ CharacterLatin1IdentifierStart,
    /* 119 - w                  */ CharacterLatin1IdentifierStart,
    /* 120 - x                  */ CharacterLatin1IdentifierStart,
    /* 121 - y                  */ CharacterLatin1IdentifierStart,
    /* 122 - z                  */ CharacterLatin1IdentifierStart,
    /* 123 - {                  */ CharacterOpenBrace,
    /* 124 - |                  */ CharacterOr,
    /* 125 - }                  */ CharacterCloseBrace,
    /* 126 - ~                  */ CharacterTilde,
    /* 127 - Delete             */ CharacterInvalid,
    /* 128 - Cc category        */ CharacterInvalid,
    /* 129 - Cc category        */ CharacterInvalid,
    /* 130 - Cc category        */ CharacterInvalid,
    /* 131 - Cc category        */ CharacterInvalid,
    /* 132 - Cc category        */ CharacterInvalid,
    /* 133 - Cc category        */ CharacterInvalid,
    /* 134 - Cc category        */ CharacterInvalid,
    /* 135 - Cc category        */ CharacterInvalid,
    /* 136 - Cc category        */ CharacterInvalid,
    /* 137 - Cc category        */ CharacterInvalid,
    /* 138 - Cc category        */ CharacterInvalid,
    /* 139 - Cc category        */ CharacterInvalid,
    /* 140 - Cc category        */ CharacterInvalid,
    /* 141 - Cc category        */ CharacterInvalid,
    /* 142 - Cc category        */ CharacterInvalid,
    /* 143 - Cc category        */ CharacterInvalid,
    /* 144 - Cc category        */ CharacterInvalid,
    /* 145 - Cc category        */ CharacterInvalid,
    /* 146 - Cc category        */ CharacterInvalid,
    /* 147 - Cc category        */ CharacterInvalid,
    /* 148 - Cc category        */ CharacterInvalid,
    /* 149 - Cc category        */ CharacterInvalid,
    /* 150 - Cc category        */ CharacterInvalid,
    /* 151 - Cc category        */ CharacterInvalid,
    /* 152 - Cc category        */ CharacterInvalid,
    /* 153 - Cc category        */ CharacterInvalid,
    /* 154 - Cc category        */ CharacterInvalid,
    /* 155 - Cc category        */ CharacterInvalid,
    /* 156 - Cc category        */ CharacterInvalid,
    /* 157 - Cc category        */ CharacterInvalid,
    /* 158 - Cc category        */ CharacterInvalid,
    /* 159 - Cc category        */ CharacterInvalid,
    /* 160 - Zs category (nbsp) */ CharacterWhiteSpace,
    /* 161 - Po category        */ CharacterInvalid,
    /* 162 - Sc category        */ CharacterInvalid,
    /* 163 - Sc category        */ CharacterInvalid,
    /* 164 - Sc category        */ CharacterInvalid,
    /* 165 - Sc category        */ CharacterInvalid,
    /* 166 - So category        */ CharacterInvalid,
    /* 167 - So category        */ CharacterInvalid,
    /* 168 - Sk category        */ CharacterInvalid,
    /* 169 - So category        */ CharacterInvalid,
    /* 170 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 171 - Pi category        */ CharacterInvalid,
    /* 172 - Sm category        */ CharacterInvalid,
    /* 173 - Cf category        */ CharacterInvalid,
    /* 174 - So category        */ CharacterInvalid,
    /* 175 - Sk category        */ CharacterInvalid,
    /* 176 - So category        */ CharacterInvalid,
    /* 177 - Sm category        */ CharacterInvalid,
    /* 178 - No category        */ CharacterInvalid,
    /* 179 - No category        */ CharacterInvalid,
    /* 180 - Sk category        */ CharacterInvalid,
    /* 181 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 182 - So category        */ CharacterInvalid,
    /* 183 - Po category        */ CharacterOtherIdentifierPart,
    /* 184 - Sk category        */ CharacterInvalid,
    /* 185 - No category        */ CharacterInvalid,
    /* 186 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 187 - Pf category        */ CharacterInvalid,
    /* 188 - No category        */ CharacterInvalid,
    /* 189 - No category        */ CharacterInvalid,
    /* 190 - No category        */ CharacterInvalid,
    /* 191 - Po category        */ CharacterInvalid,
    /* 192 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 193 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 194 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 195 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 196 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 197 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 198 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 199 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 200 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 201 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 202 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 203 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 204 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 205 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 206 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 207 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 208 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 209 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 210 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 211 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 212 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 213 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 214 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 215 - Sm category        */ CharacterInvalid,
    /* 216 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 217 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 218 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 219 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 220 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 221 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 222 - Lu category        */ CharacterLatin1IdentifierStart,
    /* 223 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 224 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 225 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 226 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 227 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 228 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 229 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 230 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 231 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 232 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 233 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 234 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 235 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 236 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 237 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 238 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 239 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 240 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 241 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 242 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 243 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 244 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 245 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 246 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 247 - Sm category        */ CharacterInvalid,
    /* 248 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 249 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 250 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 251 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 252 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 253 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 254 - Ll category        */ CharacterLatin1IdentifierStart,
    /* 255 - Ll category        */ CharacterLatin1IdentifierStart,
];

/// `singleCharacterEscapeValuesForASCII`: o caractere que resulta de `\X`, onde X é o índice na
/// tabela. Valor 0 quer dizer que é preciso mais processamento.
static SINGLE_CHARACTER_ESCAPE_VALUES_FOR_ASCII: [u8; 128] = [
    /*   0 - Null               */ 0,
    /*   1 - Start of Heading   */ 0,
    /*   2 - Start of Text      */ 0,
    /*   3 - End of Text        */ 0,
    /*   4 - End of Transm.     */ 0,
    /*   5 - Enquiry            */ 0,
    /*   6 - Acknowledgment     */ 0,
    /*   7 - Bell               */ 0,
    /*   8 - Back Space         */ 0,
    /*   9 - Horizontal Tab     */ 0,
    /*  10 - Line Feed          */ 0,
    /*  11 - Vertical Tab       */ 0,
    /*  12 - Form Feed          */ 0,
    /*  13 - Carriage Return    */ 0,
    /*  14 - Shift Out          */ 0,
    /*  15 - Shift In           */ 0,
    /*  16 - Data Line Escape   */ 0,
    /*  17 - Device Control 1   */ 0,
    /*  18 - Device Control 2   */ 0,
    /*  19 - Device Control 3   */ 0,
    /*  20 - Device Control 4   */ 0,
    /*  21 - Negative Ack.      */ 0,
    /*  22 - Synchronous Idle   */ 0,
    /*  23 - End of Transmit    */ 0,
    /*  24 - Cancel             */ 0,
    /*  25 - End of Medium      */ 0,
    /*  26 - Substitute         */ 0,
    /*  27 - Escape             */ 0,
    /*  28 - File Separator     */ 0,
    /*  29 - Group Separator    */ 0,
    /*  30 - Record Separator   */ 0,
    /*  31 - Unit Separator     */ 0,
    /*  32 - Space              */ b' ',
    /*  33 - !                  */ b'!',
    /*  34 - "                  */ b'"',
    /*  35 - #                  */ b'#',
    /*  36 - $                  */ b'$',
    /*  37 - %                  */ b'%',
    /*  38 - &                  */ b'&',
    /*  39 - '                  */ b'\'',
    /*  40 - (                  */ b'(',
    /*  41 - )                  */ b')',
    /*  42 - *                  */ b'*',
    /*  43 - +                  */ b'+',
    /*  44 - ,                  */ b',',
    /*  45 - -                  */ b'-',
    /*  46 - .                  */ b'.',
    /*  47 - /                  */ b'/',
    /*  48 - 0                  */ 0,
    /*  49 - 1                  */ 0,
    /*  50 - 2                  */ 0,
    /*  51 - 3                  */ 0,
    /*  52 - 4                  */ 0,
    /*  53 - 5                  */ 0,
    /*  54 - 6                  */ 0,
    /*  55 - 7                  */ 0,
    /*  56 - 8                  */ 0,
    /*  57 - 9                  */ 0,
    /*  58 - :                  */ b':',
    /*  59 - ;                  */ b';',
    /*  60 - <                  */ b'<',
    /*  61 - =                  */ b'=',
    /*  62 - >                  */ b'>',
    /*  63 - ?                  */ b'?',
    /*  64 - @                  */ b'@',
    /*  65 - A                  */ b'A',
    /*  66 - B                  */ b'B',
    /*  67 - C                  */ b'C',
    /*  68 - D                  */ b'D',
    /*  69 - E                  */ b'E',
    /*  70 - F                  */ b'F',
    /*  71 - G                  */ b'G',
    /*  72 - H                  */ b'H',
    /*  73 - I                  */ b'I',
    /*  74 - J                  */ b'J',
    /*  75 - K                  */ b'K',
    /*  76 - L                  */ b'L',
    /*  77 - M                  */ b'M',
    /*  78 - N                  */ b'N',
    /*  79 - O                  */ b'O',
    /*  80 - P                  */ b'P',
    /*  81 - Q                  */ b'Q',
    /*  82 - R                  */ b'R',
    /*  83 - S                  */ b'S',
    /*  84 - T                  */ b'T',
    /*  85 - U                  */ b'U',
    /*  86 - V                  */ b'V',
    /*  87 - W                  */ b'W',
    /*  88 - X                  */ b'X',
    /*  89 - Y                  */ b'Y',
    /*  90 - Z                  */ b'Z',
    /*  91 - [                  */ b'[',
    /*  92 - \                  */ b'\\',
    /*  93 - ]                  */ b']',
    /*  94 - ^                  */ b'^',
    /*  95 - _                  */ b'_',
    /*  96 - `                  */ b'`',
    /*  97 - a                  */ b'a',
    /*  98 - b                  */ 0x08,
    /*  99 - c                  */ b'c',
    /* 100 - d                  */ b'd',
    /* 101 - e                  */ b'e',
    /* 102 - f                  */ 0x0C,
    /* 103 - g                  */ b'g',
    /* 104 - h                  */ b'h',
    /* 105 - i                  */ b'i',
    /* 106 - j                  */ b'j',
    /* 107 - k                  */ b'k',
    /* 108 - l                  */ b'l',
    /* 109 - m                  */ b'm',
    /* 110 - n                  */ 0x0A,
    /* 111 - o                  */ b'o',
    /* 112 - p                  */ b'p',
    /* 113 - q                  */ b'q',
    /* 114 - r                  */ 0x0D,
    /* 115 - s                  */ b's',
    /* 116 - t                  */ 0x09,
    /* 117 - u                  */ 0,
    /* 118 - v                  */ 0x0B,
    /* 119 - w                  */ b'w',
    /* 120 - x                  */ 0,
    /* 121 - y                  */ b'y',
    /* 122 - z                  */ b'z',
    /* 123 - {                  */ b'{',
    /* 124 - |                  */ b'|',
    /* 125 - }                  */ b'}',
    /* 126 - ~                  */ b'~',
    /* 127 - Delete             */ 0,
];

/// `UCHAR_MAX_VALUE` do ICU.
const UCHAR_MAX_VALUE: u32 = 0x10ffff;

// ---- macros `U16_*` do ICU usadas pelo lexer ----------------------------------------------------

/// `U16_IS_SURROGATE(c)`.
fn u16_is_surrogate(c: u32) -> bool {
    (c & 0xFFFF_F800) == 0xD800
}

/// `U16_IS_LEAD(c)`.
fn u16_is_lead(c: u32) -> bool {
    (c & 0xFFFF_FC00) == 0xD800
}

/// `U16_IS_SURROGATE_TRAIL(c)`.
fn u16_is_surrogate_trail(c: u32) -> bool {
    (c & 0x400) != 0
}

/// `U16_GET_SUPPLEMENTARY(lead, trail)`.
fn u16_get_supplementary(lead: u32, trail: u32) -> u32 {
    (lead << 10).wrapping_add(trail).wrapping_sub((0xD800 << 10) + 0xDC00 - 0x10000)
}

/// `isLatin1(c)`.
#[inline(always)]
fn is_latin1(c: u32) -> bool {
    c <= 0xFF
}

/// `tokenTypeForIntegerLikeToken(double)`.
pub(crate) fn token_type_for_integer_like_token(double_value: f64) -> JSTokenType {
    if (double_value != 0.0 || !double_value.is_sign_negative()) && truncate_double_to_int64(double_value) as f64 == double_value {
        return INTEGER;
    }
    DOUBLE
}

/// `isRestrKeyword(JSTokenType)`.
#[inline(always)]
pub(crate) fn is_restr_keyword(token: JSTokenType) -> bool {
    token == CONTINUE || token == BREAK || token == RETURN || token == THROW
}

/// `isNonLatin1IdentStart(char32_t)`: `u_hasBinaryProperty(c, UCHAR_ID_START)`.
fn is_non_latin1_ident_start(c: u32) -> bool {
    is_id_start(c)
}

/// `isIdentStart<Latin1Character>` e `isIdentStart<char32_t>` (o `Latin1Character` entra como `u32`).
#[inline(always)]
pub(crate) fn is_ident_start(c: u32) -> bool {
    if !is_latin1(c) {
        return is_non_latin1_ident_start(c);
    }
    TYPES_OF_LATIN1_CHARACTERS[c as usize] == CharacterLatin1IdentifierStart
}

/// `isSingleCharacterIdentStart(char16_t)`.
#[inline(always)]
pub(crate) fn is_single_character_ident_start(c: u16) -> bool {
    if is_latin1(c as u32) {
        return is_ident_start(c as u32);
    }
    !u16_is_surrogate(c as u32) && is_ident_start(c as u32)
}

/// `cannotBeIdentStart(Latin1Character)` e `cannotBeIdentStart(char16_t)`.
#[inline(always)]
pub(crate) fn cannot_be_ident_start<T: CharType>(c: T) -> bool {
    let c = c.to_u16();
    if is_latin1(c as u32) {
        return !is_ident_start(c as u32) && c != b'\\' as u16;
    }
    Lexer::<u16>::is_white_space(c) || Lexer::<u16>::is_line_terminator(c)
}

/// `isNonLatin1IdentPart(char32_t)`.
#[inline(never)]
fn is_non_latin1_ident_part(c: u32) -> bool {
    is_id_continue(c) || c == 0x200C || c == 0x200D
}

/// `isIdentPart<Latin1Character>` e `isIdentPart<char32_t>`.
#[inline(always)]
pub(crate) fn is_ident_part(c: u32) -> bool {
    if !is_latin1(c) {
        return is_non_latin1_ident_part(c);
    }

    // Character types are divided into two groups depending on whether they can be part of an
    // identifier or not. Those whose type value is less or equal than CharacterOtherIdentifierPart can be
    // part of an identifier. (See the CharacterType definition for more details.)
    TYPES_OF_LATIN1_CHARACTERS[c as usize] <= CharacterOtherIdentifierPart
}

/// `isSingleCharacterIdentPart(char16_t)`.
#[inline(always)]
pub(crate) fn is_single_character_ident_part(c: u16) -> bool {
    if is_latin1(c as u32) {
        return is_ident_part(c as u32);
    }
    !u16_is_surrogate(c as u32) && is_ident_part(c as u32)
}

/// `cannotBeIdentPartOrEscapeStart(Latin1Character)` e `cannotBeIdentPartOrEscapeStart(char16_t)`.
///
/// NOTE: This may give give false negatives (for non-ascii) but won't give false posititves.
/// This means it can be used to detect the end of a keyword (all keywords are ascii)
#[inline(always)]
pub(crate) fn cannot_be_ident_part_or_escape_start<T: CharType>(c: T) -> bool {
    let c = c.to_u16();
    if is_latin1(c as u32) {
        return !is_ident_part(c as u32) && c != b'\\' as u16;
    }
    Lexer::<u16>::is_white_space(c) || Lexer::<u16>::is_line_terminator(c)
}

/// `isASCIIDigitOrSeparator`.
#[inline]
pub(crate) fn is_ascii_digit_or_separator<C: AsciiChar>(character: C) -> bool {
    is_ascii_digit(character) || character.to_u32() == b'_' as u32
}

/// `isASCIIHexDigitOrSeparator`.
#[inline]
pub(crate) fn is_ascii_hex_digit_or_separator<C: AsciiChar>(character: C) -> bool {
    is_ascii_hex_digit(character) || character.to_u32() == b'_' as u32
}

/// `isASCIIBinaryDigitOrSeparator`.
#[inline]
pub(crate) fn is_ascii_binary_digit_or_separator<C: AsciiChar>(character: C) -> bool {
    is_ascii_binary_digit(character) || character.to_u32() == b'_' as u32
}

/// `isASCIIOctalDigitOrSeparator`.
#[inline]
pub(crate) fn is_ascii_octal_digit_or_separator<C: AsciiChar>(character: C) -> bool {
    is_ascii_octal_digit(character) || character.to_u32() == b'_' as u32
}

/// `singleEscape(int)`.
#[inline]
pub(crate) fn single_escape(c: i32) -> u8 {
    if c < 128 {
        return usize::try_from(c)
            .ok()
            .and_then(|index| SINGLE_CHARACTER_ESCAPE_VALUES_FOR_ASCII.get(index))
            .copied()
            .unwrap_or(0);
    }
    0
}

/// `struct ParsedUnicodeEscapeValue`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedUnicodeEscapeValue {
    value: u32,
}

/// `ParsedUnicodeEscapeValue::SpecialValueType`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialValueType {
    Incomplete = 0xFFFF_FFFE,
    Invalid = 0xFFFF_FFFF,
}

impl ParsedUnicodeEscapeValue {
    /// `ParsedUnicodeEscapeValue(char32_t)`.
    pub fn new(value: u32) -> ParsedUnicodeEscapeValue {
        let result = ParsedUnicodeEscapeValue { value };
        debug_assert!(result.is_valid());
        result
    }

    /// `ParsedUnicodeEscapeValue(SpecialValueType)`.
    pub fn special(special_type: SpecialValueType) -> ParsedUnicodeEscapeValue {
        ParsedUnicodeEscapeValue { value: special_type as u32 }
    }

    pub fn is_valid(&self) -> bool {
        self.value != SpecialValueType::Incomplete as u32 && self.value != SpecialValueType::Invalid as u32
    }

    pub fn is_incomplete(&self) -> bool {
        self.value == SpecialValueType::Incomplete as u32
    }

    pub fn value(&self) -> u32 {
        debug_assert!(self.is_valid());
        self.value
    }
}

/// `isSafeBuiltinIdentifier(VM&, const Identifier*)`: sem `ASSERT_ENABLED` é sempre verdadeiro.
#[inline(always)]
pub fn is_safe_builtin_identifier(_vm: &VM, _identifier: &Identifier) -> bool {
    true
}

/// `class Lexer<T>`.
pub struct Lexer<T: CharType> {
    // Fields up to m_sourceURLDirective are arranged according to access frequency
    // and affinity; do not rearrange without careful analysis.
    vm: Rc<VM>,
    arena: Option<Rc<RefCell<IdentifierArena>>>,
    /// `m_code`: índice no texto do fonte.
    code: usize,
    /// `m_codeStart`: índice do início do texto (sempre 0; o C++ guarda o ponteiro).
    code_start: usize,
    /// `m_codeEnd`.
    code_end: usize,
    /// `m_lineStart`.
    line_start: usize,
    lex_error_message: WtfString,
    line_number: i32,
    current: T,
    has_line_terminator_before_token: bool,
    at_line_start: bool,
    parsing_builtin_function: bool,

    buffer8: Vec<u8>,
    buffer16: Vec<u16>,
    buffer_for_raw_template_string16: Vec<u16>,
    position_before_last_newline: JSTextPosition,
    is_reparsing_function: bool,
    error: bool,

    source_url_directive: WtfString,
    source_mapping_url_directive: WtfString,
    script_mode: JSParserScriptMode,
    source: Option<SourceCode>,
    source_offset: u32,
    /// `m_codeStartPlusOffset`.
    code_start_plus_offset: usize,
    /// O texto que `m_codeStart` apontava; mantém o fonte vivo e indexável.
    source_text: Option<Rc<StringImpl>>,
}

impl<T: CharType> Lexer<T> {
    pub const ERROR_CODE_POINT: u32 = 0xFFFF_FFFF;
    pub const INITIAL_READ_BUFFER_CAPACITY: usize = 32;

    /// `Lexer(VM&, JSParserBuiltinMode, JSParserScriptMode)`.
    pub fn new(vm: Rc<VM>, builtin_mode: JSParserBuiltinMode, script_mode: JSParserScriptMode) -> Lexer<T> {
        Lexer {
            vm,
            arena: None,
            code: 0,
            code_start: 0,
            code_end: 0,
            line_start: 0,
            lex_error_message: WtfString::default(),
            line_number: 0,
            current: T::from_u16(0),
            has_line_terminator_before_token: false,
            at_line_start: false,
            parsing_builtin_function: builtin_mode == JSParserBuiltinMode::Builtin || Options::expose_private_identifiers(),
            buffer8: Vec::new(),
            buffer16: Vec::new(),
            buffer_for_raw_template_string16: Vec::new(),
            position_before_last_newline: JSTextPosition::new(0, 0, 0),
            is_reparsing_function: false,
            error: false,
            source_url_directive: WtfString::default(),
            source_mapping_url_directive: WtfString::default(),
            script_mode,
            source: None,
            source_offset: 0,
            code_start_plus_offset: 0,
            source_text: None,
        }
    }

    // Character manipulation functions.

    /// `Lexer<Latin1Character>::isWhiteSpace` e `Lexer<char16_t>::isWhiteSpace`.
    #[inline(always)]
    pub fn is_white_space(character: T) -> bool {
        let c = character.to_u16();
        if is_latin1(c as u32) {
            return WHITE_SPACE_TABLE[c as usize];
        }

        // Non-Latin1 Zs category (Space_Separator) + BOM
        // Generated from UnicodeData.txt by generateLexerUnicodePropertyTables.py
        is_non_latin1_white_space(c)
    }

    /// `Lexer<Latin1Character>::isLineTerminator` e `Lexer<char16_t>::isLineTerminator`.
    #[inline(always)]
    pub fn is_line_terminator(character: T) -> bool {
        let c = character.to_u16();
        c == b'\r' as u16 || c == b'\n' as u16 || (c & !1) == 0x2028
    }

    #[inline]
    pub fn convert_hex(c1: i32, c2: i32) -> u8 {
        (to_ascii_hex_value(c1 as u32) << 4) | to_ascii_hex_value(c2 as u32)
    }

    #[inline]
    pub fn convert_unicode(c1: i32, c2: i32, c3: i32, c4: i32) -> u16 {
        ((Self::convert_hex(c1, c2) as u16) << 8) | (Self::convert_hex(c3, c4) as u16)
    }

    // Functions to set up parsing.

    /// `setCode(const SourceCode&, ParserArena*)`.
    pub fn set_code(&mut self, source: &SourceCode, arena: &mut ParserArena) {
        self.arena = Some(arena.identifier_arena());

        self.line_number = source.first_line().one_based_int();

        let source_string = source.provider().expect("Lexer::set_code: SourceCode sem provider").source();

        if !source_string.is_null() {
            self.set_code_start(&source_string);
        } else {
            self.code_start = 0;
            self.source_text = None;
        }

        self.source = Some(source.clone());
        self.source_offset = source.start_offset() as u32;
        self.code_start_plus_offset = self.code_start + source.start_offset() as usize;
        self.code = self.code_start_plus_offset;
        self.code_end = self.code_start + source.end_offset() as usize;
        self.error = false;
        self.at_line_start = true;
        self.line_start = self.code;
        self.lex_error_message = WtfString::default();
        self.source_url_directive = WtfString::default();
        self.source_mapping_url_directive = WtfString::default();

        self.buffer8.reserve(Self::INITIAL_READ_BUFFER_CAPACITY);
        self.buffer16.reserve(Self::INITIAL_READ_BUFFER_CAPACITY);
        self.buffer_for_raw_template_string16.reserve(Self::INITIAL_READ_BUFFER_CAPACITY);

        if self.code < self.code_end {
            self.current = self.char_at(self.code);
        } else {
            self.current = T::from_u16(0);
        }
        debug_assert!(self.current_offset() as i64 == source.start_offset() as i64);
    }

    pub fn set_is_reparsing_function(&mut self) {
        self.is_reparsing_function = true;
    }

    pub fn is_reparsing_function(&self) -> bool {
        self.is_reparsing_function
    }

    /// `lex(JSToken*, OptionSet<LexerFlags>, bool)`.
    #[inline(always)]
    pub fn lex(&mut self, token_record: &mut JSToken, lexer_flags: LexerFlagSet, strict_mode: bool) -> JSTokenType {
        self.has_line_terminator_before_token = false;
        self.lex_without_clearing_line_terminator(token_record, lexer_flags, strict_mode)
    }

    pub fn line_number(&self) -> i32 {
        self.line_number
    }

    #[inline(always)]
    pub fn current_offset(&self) -> i32 {
        self.offset_from_source_ptr(self.code)
    }

    #[inline(always)]
    pub fn current_line_start_offset(&self) -> i32 {
        self.offset_from_source_ptr(self.line_start)
    }

    #[inline(always)]
    pub fn current_position(&self) -> JSTextPosition {
        JSTextPosition::new(self.line_number, self.current_offset(), self.current_line_start_offset())
    }

    pub fn position_before_last_newline(&self) -> JSTextPosition {
        self.position_before_last_newline
    }

    pub fn has_line_terminator_before_token(&self) -> bool {
        self.has_line_terminator_before_token
    }

    // Functions for use after parsing.

    pub fn saw_error(&self) -> bool {
        self.error
    }

    pub fn set_saw_error(&mut self, saw_error: bool) {
        self.error = saw_error;
    }

    pub fn get_error_message(&self) -> WtfString {
        self.lex_error_message.clone()
    }

    pub fn set_error_message(&mut self, error_message: &WtfString) {
        self.lex_error_message = error_message.clone();
    }

    pub fn source_url_directive(&self) -> WtfString {
        self.source_url_directive.clone()
    }

    pub fn source_mapping_url_directive(&self) -> WtfString {
        self.source_mapping_url_directive.clone()
    }

    pub fn clear_error_code_and_buffers(&mut self) {
        self.error = false;
        self.lex_error_message = WtfString::default();

        self.buffer8.clear();
        self.buffer16.clear();
    }

    pub fn set_offset(&mut self, offset: i32, line_start_offset: i32) {
        self.error = false;
        self.lex_error_message = WtfString::default();

        self.code = self.source_ptr_from_offset(offset);
        self.line_start = self.source_ptr_from_offset(line_start_offset);
        debug_assert!(self.current_offset() >= self.current_line_start_offset());

        self.buffer8.clear();
        self.buffer16.clear();
        if self.code < self.code_end {
            self.current = self.char_at(self.code);
        } else {
            self.current = T::from_u16(0);
        }
    }

    pub fn set_line_number(&mut self, line: i32) {
        debug_assert!(line >= 0);
        self.line_number = line;
    }

    pub fn set_has_line_terminator_before_token(&mut self, terminator: bool) {
        self.has_line_terminator_before_token = terminator;
    }

    /// `getToken(const JSToken&)`.
    #[inline(always)]
    pub fn get_token(&self, token: &JSToken) -> WtfString {
        let source_provider = match &self.source {
            Some(source) => source.provider().expect("Lexer::get_token: SourceCode sem provider"),
            None => unreachable!("Lexer::get_token chamado antes de set_code"),
        };
        debug_assert!(token.start_position.offset <= token.end_position.offset, "Calling this function with the baked token.");
        source_provider.get_range(token.start_position.offset, token.end_position.offset)
    }

    pub fn code_length(&self) -> usize {
        self.code_end - self.code_start
    }

    // ---- privados --------------------------------------------------------------------------

    /// `*ptr`: o caractere no índice; fora do texto lê 0, como o byte de terminação do C++.
    #[inline(always)]
    fn char_at(&self, index: usize) -> T {
        match &self.source_text {
            Some(text) => T::span_of(text).get(index).copied().unwrap_or_else(|| T::from_u16(0)),
            None => T::from_u16(0),
        }
    }

    /// `m_current` como `u32`, para comparar com literais de caractere.
    #[inline(always)]
    fn cur(&self) -> u32 {
        self.current.into()
    }

    /// `record8(int)`.
    #[inline]
    fn record8(&mut self, c: i32) {
        debug_assert!(is_latin1(c as u32));
        self.buffer8.push(c as u8);
    }

    /// `append8(std::span<const T>)`.
    #[inline]
    fn append8(&mut self, span: &[T]) {
        self.buffer8.reserve(span.len());
        for &c in span {
            debug_assert!(is_latin1(c.to_u16() as u32));
            self.buffer8.push(c.to_u16() as u8);
        }
    }

    /// `append16(std::span<const Latin1Character>)`.
    #[inline]
    fn append16_latin1(&mut self, span: &[u8]) {
        self.buffer16.reserve(span.len());
        for &c in span {
            self.buffer16.push(c as u16);
        }
    }

    /// `append16(std::span<const char16_t>)`.
    #[inline]
    fn append16(&mut self, characters: &[u16]) {
        self.buffer16.extend_from_slice(characters);
    }

    /// `currentCodePoint()` (`Lexer<Latin1Character>` devolve `m_current`; `Lexer<char16_t>` junta o par
    /// substituto).
    #[inline(always)]
    fn current_code_point(&self) -> u32 {
        if T::SIZE == 1 {
            return self.cur();
        }
        debug_assert!(!is_ident_start(Self::ERROR_CODE_POINT), "error values shouldn't appear as a valid identifier start code point");
        if !u16_is_surrogate(self.cur()) {
            return self.cur();
        }

        let trail: u32 = self.peek(1).into();
        if !u16_is_lead(self.cur()) || !u16_is_surrogate_trail(trail) {
            return Self::ERROR_CODE_POINT;
        }

        u16_get_supplementary(self.cur(), trail)
    }

    /// `shift()`.
    #[inline(always)]
    fn shift(&mut self) {
        self.code += 1;
        if self.code < self.code_end {
            self.current = self.char_at(self.code);
        } else {
            self.current = T::from_u16(0);
        }
    }

    /// `atEnd()`.
    #[inline(always)]
    fn at_end(&self) -> bool {
        debug_assert!(self.cur() == 0 || self.code < self.code_end);
        if self.cur() != 0 {
            return false;
        }
        if self.code == self.code_end {
            return true;
        }
        false
    }

    /// `peek(int)`.
    #[inline(always)]
    fn peek(&self, offset: i32) -> T {
        debug_assert!(offset > 0 && offset < 5);
        let code = self.code + offset as usize;
        if code < self.code_end {
            self.char_at(code)
        } else {
            T::from_u16(0)
        }
    }

    /// `parseUnicodeEscape()`.
    fn parse_unicode_escape(&mut self) -> ParsedUnicodeEscapeValue {
        if self.cur() == b'{' as u32 {
            self.shift();
            let mut code_point: u32 = 0;
            loop {
                if !is_ascii_hex_digit(self.cur()) {
                    return if self.cur() != 0 {
                        ParsedUnicodeEscapeValue::special(SpecialValueType::Invalid)
                    } else {
                        ParsedUnicodeEscapeValue::special(SpecialValueType::Incomplete)
                    };
                }
                code_point = (code_point << 4) | to_ascii_hex_value(self.cur()) as u32;
                if code_point > UCHAR_MAX_VALUE {
                    // For raw template literal syntax, we consume `NotEscapeSequence`.
                    // Here, we consume NotCodePoint's HexDigits.
                    //
                    // NotEscapeSequence ::
                    //     u { [lookahread not one of HexDigit]
                    //     u { NotCodePoint
                    //     u { CodePoint [lookahead != }]
                    //
                    // NotCodePoint ::
                    //     HexDigits but not if MV of HexDigits <= 0x10FFFF
                    //
                    // CodePoint ::
                    //     HexDigits but not if MV of HexDigits > 0x10FFFF
                    self.shift();
                    while is_ascii_hex_digit(self.cur()) {
                        self.shift();
                    }

                    return if self.at_end() {
                        ParsedUnicodeEscapeValue::special(SpecialValueType::Incomplete)
                    } else {
                        ParsedUnicodeEscapeValue::special(SpecialValueType::Invalid)
                    };
                }
                self.shift();
                if self.cur() == b'}' as u32 {
                    break;
                }
            }
            self.shift();
            return ParsedUnicodeEscapeValue::new(code_point);
        }

        let character2: u32 = self.peek(1).into();
        let character3: u32 = self.peek(2).into();
        let character4: u32 = self.peek(3).into();
        if !is_ascii_hex_digit(self.cur())
            || !is_ascii_hex_digit(character2)
            || !is_ascii_hex_digit(character3)
            || !is_ascii_hex_digit(character4)
        {
            let result = if (self.code + 4) >= self.code_end {
                ParsedUnicodeEscapeValue::special(SpecialValueType::Incomplete)
            } else {
                ParsedUnicodeEscapeValue::special(SpecialValueType::Invalid)
            };

            // For raw template literal syntax, we consume `NotEscapeSequence`.
            //
            // NotEscapeSequence ::
            //     u [lookahead not one of HexDigit][lookahead != {]
            //     u HexDigit [lookahead not one of HexDigit]
            //     u HexDigit HexDigit [lookahead not one of HexDigit]
            //     u HexDigit HexDigit HexDigit [lookahead not one of HexDigit]
            while is_ascii_hex_digit(self.cur()) {
                self.shift();
            }

            return result;
        }

        let result = Self::convert_unicode(self.cur() as i32, character2 as i32, character3 as i32, character4 as i32);
        self.shift();
        self.shift();
        self.shift();
        self.shift();
        ParsedUnicodeEscapeValue::new(result as u32)
    }

    /// `shiftLineTerminator()`.
    fn shift_line_terminator(&mut self) {
        debug_assert!(Self::is_line_terminator(self.current));

        self.position_before_last_newline = self.current_position();
        let prev = self.cur();
        self.shift();

        if prev == b'\r' as u32 && self.cur() == b'\n' as u32 {
            self.shift();
        }

        self.line_number += 1;
        self.line_start = self.code;
    }

    #[inline(always)]
    fn offset_from_source_ptr(&self, ptr: usize) -> i32 {
        (ptr as isize - self.code_start as isize) as i32
    }

    #[inline(always)]
    fn source_ptr_from_offset(&self, offset: i32) -> usize {
        (self.code_start as isize + offset as isize) as usize
    }

    /// `invalidCharacterMessage()`.
    fn invalid_character_message(&self) -> WtfString {
        let message: String = match self.cur() {
            0 => "Invalid character: '\\0'".to_string(),
            10 => "Invalid character: '\\n'".to_string(),
            11 => "Invalid character: '\\v'".to_string(),
            13 => "Invalid character: '\\r'".to_string(),
            35 => "Invalid character: '#'".to_string(),
            64 => "Invalid character: '@'".to_string(),
            96 => "Invalid character: '`'".to_string(),
            _ => format!("Invalid character '\\u{:04x}'", self.cur()),
        };
        WtfString::from_latin1(message.as_bytes())
    }

    /// `currentSourcePtr()`.
    #[inline(always)]
    fn current_source_ptr(&self) -> usize {
        debug_assert!(self.code <= self.code_end);
        self.code
    }

    /// `setCodeStart(StringView)`.
    #[inline(always)]
    fn set_code_start(&mut self, source_string: &WtfString) {
        debug_assert!(source_string.is_8bit() == (T::SIZE == 1));
        self.source_text = source_string.impl_().cloned();
        self.code_start = 0;
    }

    /// `makeIdentifier<CharacterType>(std::span<const CharacterType>)`.
    #[inline(always)]
    fn make_identifier<C: CharType>(&mut self, characters: &[C]) -> Identifier {
        self.shared_arena().borrow_mut().make_identifier(&self.vm, characters)
    }

    /// `makeLatin1Identifier(std::span<const Latin1Character>)`: o C++ chama o `makeIdentifier` genérico.
    #[inline(always)]
    fn make_latin1_identifier_8bit(&mut self, characters: &[u8]) -> Identifier {
        self.shared_arena().borrow_mut().make_identifier(&self.vm, characters)
    }

    /// `makeLatin1Identifier(std::span<const char16_t>)`.
    #[inline(always)]
    fn make_latin1_identifier(&mut self, characters: &[u16]) -> Identifier {
        self.shared_arena().borrow_mut().make_latin1_identifier(&self.vm, characters)
    }

    /// `makeRightSizedIdentifier(std::span<const char16_t>, char16_t orAllChars)`
    /// (`Lexer<Latin1Character>` ignora `orAllChars`).
    #[inline(always)]
    fn make_right_sized_identifier(&mut self, characters: &[u16], or_all_chars: u16) -> Identifier {
        if T::SIZE == 1 || (or_all_chars & !0xff) == 0 {
            return self.shared_arena().borrow_mut().make_latin1_identifier(&self.vm, characters);
        }

        self.shared_arena().borrow_mut().make_identifier(&self.vm, characters)
    }

    /// `makeEmptyIdentifier()`.
    #[inline(always)]
    fn make_empty_identifier(&mut self) -> Identifier {
        self.shared_arena().borrow().make_empty_identifier(&self.vm)
    }

    /// `m_arena`: o `Rc` da arena, que só é nulo antes do `set_code` (o C++ também desreferencia nulo
    /// nesse caso, então chamar antes do `set_code` é violação de invariante).
    fn shared_arena(&self) -> Rc<RefCell<IdentifierArena>> {
        match &self.arena {
            Some(arena) => Rc::clone(arena),
            None => unreachable!("Lexer usado antes de set_code"),
        }
    }

    /// `skipWhitespace()`.
    #[inline(always)]
    fn skip_whitespace(&mut self) {
        while Self::is_white_space(self.current) {
            self.shift();
        }
    }

    /// `parseKeyword<shouldCreateIdentifier>(JSTokenData*)` (o corpo vem do `KeywordLookup.h` gerado,
    /// aqui por `match_keyword`). `data.ident` recebe o identificador da palavra quando
    /// `SHOULD_CREATE_IDENTIFIER`.
    #[inline(always)]
    fn parse_keyword<const SHOULD_CREATE_IDENTIFIER: bool>(&mut self, data: &mut JSTokenData) -> JSTokenType {
        if self.code_end.saturating_sub(self.code) < MAX_TOKEN_LENGTH {
            return IDENT;
        }
        let found = match &self.source_text {
            Some(text) => {
                let code = &T::span_of(text)[self.code..self.code_end];
                match_keyword(code, cannot_be_ident_part_or_escape_start::<T>)
            }
            None => None,
        };
        let Some(keyword) = found else {
            return IDENT;
        };
        self.internal_shift_by(keyword.word.len());
        if SHOULD_CREATE_IDENTIFIER {
            data.ident = Some(Identifier::from_span(&self.vm, keyword.word));
        }
        keyword.token
    }

    /// `internalShift<shiftAmount>()` com a quantidade só conhecida em tempo de execução (o
    /// `parseKeyword` gerado a instancia por palavra).
    fn internal_shift_by(&mut self, shift_amount: usize) {
        self.code += shift_amount;
        debug_assert!(self.current_offset() >= self.current_line_start_offset());
        self.current = self.char_at(self.code);
    }

    /// `internalShift<shiftAmount>()`.
    fn internal_shift<const SHIFT_AMOUNT: i32>(&mut self) {
        self.code = (self.code as isize + SHIFT_AMOUNT as isize) as usize;
        debug_assert!(self.current_offset() >= self.current_line_start_offset());
        self.current = self.char_at(self.code);
    }
}

// Linhas 900 a 1674 do Lexer.cpp.
include!("lexer_part2.rs");
// Linhas 1675 em diante do Lexer.cpp.
include!("lexer_part3.rs");
include!("lexer_part4.rs");
