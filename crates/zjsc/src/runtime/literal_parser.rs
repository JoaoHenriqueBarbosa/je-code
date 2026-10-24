//! Porte de `runtime/LiteralParser.h` e `LiteralParser.cpp`: o lexer e o parser de JSON estrito
//! (`JSON.parse`, `JSON.rawJSON`) e de literal sloppy (o `eval` de um literal), com as mesmas
//! mensagens de `SyntaxError` do JSC (`"JSON Parse error: ..."`).
//!
//! Também mora aqui o contrato com o motor que o parser e o `JSON.stringify` precisam: `JsonHost`,
//! `JsonKey` e `JsonError`. `json_object.rs` importa daqui (o parser é a camada de baixo).
//!
//! LIGAÇÃO PENDENTE: nada daqui está registrado no global. Falta a `NativeFunction` final (outro
//! agente redesenha a assinatura) para ligar `jsonProtoFuncParse`, `jsonProtoFuncStringify`,
//! `jsonProtoFuncIsRawJSON` e `jsonProtoFuncRawJSON` ao `JSON` do `JSGlobalObject`, e falta o
//! `impl JsonHost for JSGlobalObject` (chamar função, enumerar chaves próprias, `JSRawJSONObject`,
//! caixas de primitivo). Quem ligar converte `JsonError` em exceção com `json_object::throw_json_error`.
//!
//! DIVERGÊNCIAS:
//!
//! - O texto de entrada vira `&[u16]` (o C++ instancia `Latin1Character` e `char16_t`); o resultado
//!   observável é o mesmo, porque a única diferença entre as duas instâncias é o tipo do caractere.
//! - O caminho recursivo (`parseRecursively`, ligado por `Options::useRecursiveJSONParse`) e o
//!   iterativo (`parse`) têm os mesmos resultados e mensagens; aqui só existe o iterativo (a pilha de
//!   estados do `parse`), então não há limite de aninhamento por pilha nativa. `tryEval` reproduz as
//!   mensagens do ramo recursivo (`evalRecursivelyEntry`), o que vale com a opção ligada, o padrão.
//!   Os estados `StartParseStatement` e `StartParseStatementEndStatement` só servem ao `eval`
//!   iterativo (opção desligada) e não foram portados.
//! - Arrays são acumulados em um `Vec` e criados pelo host quando fecham (o `materializeArray` do
//!   ramo recursivo); nenhum código de usuário roda durante o parse, então a ordem é inobservável.
//! - `JSONRanges::record` e o `MarkedArgumentBuffer` somem: o registro de células não coleta.
//! - O modo `JSONP` (`tryJSONPParse`, `JSONPData`, `JSONPPathEntry`) está portado; falta ligá-lo ao
//!   `Interpreter::executeProgram`. `JSONPData::m_value` é um `JSValue` (o `Strong<Unknown>` some,
//!   o registro de células não coleta).
//! - Fora desta fatia: `tryStreamingParse` (acréscimo do Bun) e os caches `jsonAtomStringCache` e
//!   `Structure::trySingleTransition` (otimizações de forma de objeto, sem efeito observável).

use std::collections::HashMap;

use crate::parser::lexer::is_lexer_keyword;
use crate::runtime::identifier::{parse_index, Identifier};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::js_promise_host::Thrown;
use crate::runtime::vm::VM;
use crate::wtf::fast_float::parse_number::parse_json_double;
use crate::wtf::math_extras::truncate_double_to_int32;
use crate::wtf::text::wtf_string::String as WtfString;

/// `maximumRangesStackRecursion`.
const MAXIMUM_RANGES_STACK_RECURSION: usize = 4500;

// ---------------------------------------------------------------------------------------------
// Contrato com o motor
// ---------------------------------------------------------------------------------------------

/// Como um erro interrompe o parse, o `stringify` ou o reviver.
#[derive(Clone, Debug)]
pub enum JsonError {
    /// Exceção que o código de usuário (ou uma operação do host) já lançou.
    Thrown(Thrown),
    /// `throwSyntaxError(globalObject, scope, message)`.
    Syntax(WtfString),
    /// `throwTypeError(globalObject, scope, message)`.
    Type(WtfString),
    /// `throwOutOfMemoryError`.
    OutOfMemory,
    /// `throwStackOverflowError`.
    StackOverflow,
}

impl From<Thrown> for JsonError {
    fn from(thrown: Thrown) -> JsonError {
        JsonError::Thrown(thrown)
    }
}

/// O nome de uma propriedade como o JSON a vê: índice de array ou nome (`PropertyName`).
#[derive(Clone, Debug)]
pub enum JsonKey {
    Index(u32),
    Name(WtfString),
}

impl JsonKey {
    /// `parseIndex(ident)` seguido de `putDirectIndex` ou `putDirect`.
    pub fn from_units(units: &[u16]) -> JsonKey {
        match parse_index(units) {
            Some(index) => JsonKey::Index(index),
            None => JsonKey::Name(units_to_wtf_string(units)),
        }
    }

    /// A chave de um texto (`Identifier::fromString` mais `PropertyName`).
    pub fn from_wtf_string(string: &WtfString) -> JsonKey {
        JsonKey::from_units(&wtf_string_to_units(string))
    }

    /// O texto da chave (o `uid` do `PropertyName`).
    pub fn to_wtf_string(&self) -> WtfString {
        match self {
            JsonKey::Index(index) => WtfString::number_u32(*index),
            JsonKey::Name(name) => name.clone(),
        }
    }

    /// O texto da chave em unidades de código (chave dos `JSONRanges::Object`).
    pub fn units(&self) -> Vec<u16> {
        wtf_string_to_units(&self.to_wtf_string())
    }
}

/// `JSValue::isObject()` e `inherits<...>()` sobre as caixas de primitivo que o `JSON` desembrulha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxedPrimitiveKind {
    /// Não é caixa (inclui `SymbolObject`, que o `JSON.stringify` não desembrulha).
    None,
    Number,
    String,
    Boolean,
    BigInt,
}

/// O que o parser, o `Stringifier` e o `Walker` pedem do `JSGlobalObject` e do `VM` que ainda não
/// existem no porte (mesmo padrão de `PromiseHost`). Cada método diz o que ele é no C++.
pub trait JsonHost {
    /// `globalObject->vm()`.
    fn vm(&self) -> &VM;

    /// `constructEmptyObject(globalObject)`.
    fn new_object(&self) -> Result<JSValue, Thrown>;

    /// `constructArray(globalObject, nullptr, values)` (o array já vem na forma de indexação certa).
    fn new_array(&self, elements: &[JSValue]) -> Result<JSValue, Thrown>;

    /// `putDirect` / `putDirectIndex` / `createDataProperty(globalObject, key, value, shouldThrow=false)`:
    /// define a propriedade de dados própria; `false` se a definição foi recusada sem lançar.
    fn create_data_property(&self, object: JSValue, key: &JsonKey, value: JSValue) -> Result<bool, Thrown>;

    /// `JSValue(object).put(globalObject, "__proto__", value, slot)` (o acessor de `Object.prototype`).
    fn set_underscore_proto(&self, object: JSValue, value: JSValue) -> Result<(), Thrown>;

    /// `JSValue::isObject()`.
    fn is_object(&self, value: JSValue) -> bool;

    /// `JSValue::isCallable()` (`getCallData(value).type != None`).
    fn is_callable(&self, value: JSValue) -> bool;

    /// `call(globalObject, function, callData, thisValue, args)`.
    fn call(&self, function: JSValue, this_value: JSValue, arguments: &[JSValue]) -> Result<JSValue, Thrown>;

    /// `baseValue.getPropertySlot(...)` mais `slot.getValue(...)`: o `get` de uma propriedade de
    /// qualquer valor (inclusive primitivo, como o `BigInt.prototype.toJSON`); `undefined` se não há.
    fn get(&self, base: JSValue, key: &JsonKey) -> Result<JSValue, Thrown>;

    /// `JSC::isArray(globalObject, value)` (enxerga através de `Proxy`).
    fn is_array(&self, value: JSValue) -> Result<bool, Thrown>;

    /// `toLength(globalObject, object)`.
    fn length_of_array_like(&self, object: JSValue) -> Result<u64, Thrown>;

    /// `getOwnPropertyNames(DontEnumPropertiesMode::Exclude)` com `PropertyNameMode::Strings` e
    /// `PrivateSymbolMode::Exclude`: as chaves próprias enumeráveis em ordem de propriedade
    /// (índices crescentes, depois nomes na ordem de inserção).
    fn own_enumerable_string_keys(&self, object: JSValue) -> Result<Vec<JsonKey>, Thrown>;

    /// `JSCell::deleteProperty` / `deletePropertyByIndex`.
    fn delete_property(&self, object: JSValue, key: &JsonKey) -> Result<(), Thrown>;

    /// `object->inherits<NumberObject>()` e as demais caixas (sem efeito colateral).
    fn boxed_primitive_kind(&self, object: JSValue) -> BoxedPrimitiveKind;

    /// O ramo de `unwrapBoxedPrimitive` para uma caixa: `jsNumber(object->toNumber(globalObject))`,
    /// `object->toString(globalObject)` ou `internalValue()`. Só é chamado se `boxed_primitive_kind`
    /// não deu `None`.
    fn unwrap_boxed_primitive(&self, object: JSValue) -> Result<JSValue, Thrown>;

    /// `object->inherits<JSRawJSONObject>()` seguido de `rawJSON(vm)->value(globalObject)`.
    fn raw_json_text(&self, object: JSValue) -> Option<WtfString>;

    /// `value.toString(globalObject)->value(globalObject)` (o `ToString` completo, que pode chamar
    /// código de usuário).
    fn to_string(&self, value: JSValue) -> Result<WtfString, Thrown>;
}

// ---------------------------------------------------------------------------------------------
// Texto
// ---------------------------------------------------------------------------------------------

/// O texto como unidades de código de 16 bits (a string nula vira vazia).
pub fn wtf_string_to_units(string: &WtfString) -> Vec<u16> {
    if string.is_8bit() {
        string.span8().iter().map(|&byte| byte as u16).collect()
    } else {
        string.span16().to_vec()
    }
}

/// O contrário: Latin1 quando tudo cabe em 8 bits, UTF-16 no resto.
pub fn units_to_wtf_string(units: &[u16]) -> WtfString {
    if units.iter().all(|&unit| unit <= 0xFF) {
        let bytes: Vec<u8> = units.iter().map(|&unit| unit as u8).collect();
        WtfString::from_latin1(&bytes)
    } else {
        WtfString::from_utf16(units)
    }
}

/// Um texto ASCII em unidades de código.
fn ascii_units(text: &str) -> Vec<u16> {
    text.bytes().map(|byte| byte as u16).collect()
}

/// Concatena pedaços de mensagem (`makeString`).
fn concat_units(parts: &[&[u16]]) -> Vec<u16> {
    parts.iter().flat_map(|part| part.iter().copied()).collect()
}

// ---------------------------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------------------------

/// `enum TokenType` (`TokLBracket` ... `TokErrorSpace`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenType {
    TokLBracket,
    TokRBracket,
    TokLBrace,
    TokRBrace,
    TokString,
    TokIdentifier,
    TokNumber,
    TokNumberInt32,
    TokColon,
    TokLParen,
    TokRParen,
    TokComma,
    TokTrue,
    TokFalse,
    TokNull,
    TokEnd,
    TokDot,
    TokAssign,
    TokSemi,
    TokError,
    TokErrorSpace,
}

use TokenType::*;

/// `enum ParserMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParserMode {
    StrictJSON,
    SloppyJSON,
    JSONP,
}

/// `enum JSONPPathEntryType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonpPathEntryType {
    /// `var pathEntryName = JSON`.
    DeclareVar,
    /// `<prior entries>.pathEntryName = JSON`.
    Dot,
    /// `<prior entries>[pathIndex] = JSON`.
    Lookup,
    /// `<prior entries>(JSON)`.
    Call,
}

/// `struct JSONPPathEntry`.
#[derive(Clone, Debug)]
pub struct JsonpPathEntry {
    pub path_entry_name: Identifier,
    pub path_index: i32,
    pub entry_type: JsonpPathEntryType,
}

/// `struct JSONPData`.
#[derive(Clone, Debug)]
pub struct JsonpData {
    pub path: Vec<JsonpPathEntry>,
    pub value: JSValue,
}

/// `tokenTypesOfLatin1Characters[c]`, calculado em vez de tabelado: o `switch` abaixo cobre cada
/// linha da tabela do C++ (todo o resto, inclusive 128..255, é `TokError`).
fn token_type_of_latin1(character: u8) -> TokenType {
    match character {
        b'\t' | b'\n' | b'\r' | b' ' => TokErrorSpace,
        b'"' | b'\'' => TokString,
        b'$' | b'_' | b'A'..=b'Z' | b'a'..=b'z' => TokIdentifier,
        b'(' => TokLParen,
        b')' => TokRParen,
        b',' => TokComma,
        b'-' | b'0'..=b'9' => TokNumber,
        b'.' => TokDot,
        b':' => TokColon,
        b';' => TokSemi,
        b'=' => TokAssign,
        b'[' => TokLBracket,
        b']' => TokRBracket,
        b'{' => TokLBrace,
        b'}' => TokRBrace,
        _ => TokError,
    }
}

/// `isJSONWhiteSpace`: tab, LF, CR e espaço.
fn is_json_white_space(character: u16) -> bool {
    matches!(character, 0x09 | 0x0A | 0x0D | 0x20)
}

/// `isValidIdentifierCharacter` do `char16_t` (a versão de 8 bits é a mesma sem os dois joiners).
fn is_valid_identifier_character(character: u16) -> bool {
    matches!(character, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A | 0x5F | 0x24 | 0x200C | 0x200D)
}

/// `isASCIIDigit`.
fn is_ascii_digit(character: u16) -> bool {
    (0x30..=0x39).contains(&character)
}

/// `isASCIIHexDigit` com o valor.
fn hex_digit_value(character: u16) -> Option<u16> {
    match character {
        0x30..=0x39 => Some(character - 0x30),
        0x41..=0x46 => Some(character - 0x41 + 10),
        0x61..=0x66 => Some(character - 0x61 + 10),
        _ => None,
    }
}

/// `LiteralParserToken`: o texto da string ou do identificador mora no fonte (`start`, `len`) ou, se
/// teve escape, no `builder` do lexer.
struct Token {
    kind: TokenType,
    number: f64,
    int32: i32,
    start: usize,
    len: usize,
    owned: bool,
}

// ---------------------------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------------------------

/// `LiteralParser::Lexer`.
struct Lexer<'a> {
    mode: ParserMode,
    chars: &'a [u16],
    ptr: usize,
    /// `m_lexErrorMessage`.
    error: Vec<u16>,
    token: Token,
    /// `m_builder`: o texto da string com escape.
    builder: Vec<u16>,
    /// `m_currentTokenStart` e `m_currentTokenEnd`.
    token_start: usize,
    token_end: usize,
}

impl<'a> Lexer<'a> {
    fn new(chars: &'a [u16], mode: ParserMode) -> Lexer<'a> {
        Lexer {
            mode,
            chars,
            ptr: 0,
            error: Vec::new(),
            token: Token { kind: TokError, number: 0.0, int32: 0, start: 0, len: 0, owned: false },
            builder: Vec::new(),
            token_start: 0,
            token_end: 0,
        }
    }

    /// O texto do token de string ou de identificador.
    fn token_text(&self) -> &[u16] {
        if self.token.owned {
            &self.builder
        } else {
            &self.chars[self.token.start..self.token.start + self.token.len]
        }
    }

    /// `m_end - m_ptr >= literal.len()` e os caracteres batem.
    fn matches_literal(&self, literal: &[u8]) -> bool {
        let remaining = &self.chars[self.ptr..];
        remaining.len() >= literal.len() && remaining.iter().zip(literal).all(|(&unit, &byte)| unit == byte as u16)
    }

    /// `Lexer::next()`.
    fn next(&mut self) -> TokenType {
        self.lex()
    }

    /// `Lexer::lex<JSONIdentifierHint::Unknown>`.
    fn lex(&mut self) -> TokenType {
        while self.ptr < self.chars.len() && is_json_white_space(self.chars[self.ptr]) {
            self.ptr += 1;
        }

        self.token_start = self.ptr;
        self.token_end = self.ptr;

        if self.ptr == self.chars.len() {
            self.token.kind = TokEnd;
            return TokEnd;
        }
        self.token.kind = TokError;
        let character = self.chars[self.ptr];
        if character <= 0xFF {
            let token_type = token_type_of_latin1(character as u8);
            match token_type {
                TokString => {
                    if character == '\'' as u16 && self.mode == ParserMode::StrictJSON {
                        self.error = ascii_units("Single quotes (') are not allowed in JSON");
                        self.token_end = self.ptr;
                        return TokError;
                    }
                    let result = self.lex_string(character);
                    self.token_end = self.ptr;
                    return result;
                }
                TokIdentifier => {
                    let keyword = match character as u8 {
                        b't' if self.matches_literal(b"true") => Some((TokTrue, 4)),
                        b'f' if self.matches_literal(b"false") => Some((TokFalse, 5)),
                        b'n' if self.matches_literal(b"null") => Some((TokNull, 4)),
                        _ => None,
                    };
                    if let Some((keyword_type, length)) = keyword {
                        self.ptr += length;
                        self.token.kind = keyword_type;
                        self.token_end = self.ptr;
                        return keyword_type;
                    }
                    let result = self.lex_identifier();
                    self.token_end = self.ptr;
                    return result;
                }
                TokNumber => {
                    let result = self.lex_number();
                    self.token_end = self.ptr;
                    return result;
                }
                TokError | TokErrorSpace => {}
                _ => {
                    self.token.kind = token_type;
                    self.ptr += 1;
                    self.token_end = self.ptr;
                    return token_type;
                }
            }
        }
        self.error = concat_units(&[&ascii_units("Unrecognized token '"), &[character], &ascii_units("'")]);
        self.token_end = self.ptr;
        TokError
    }

    /// `lexIdentifier`.
    fn lex_identifier(&mut self) -> TokenType {
        let start = self.ptr;
        while self.ptr < self.chars.len() && is_valid_identifier_character(self.chars[self.ptr]) {
            self.ptr += 1;
        }
        self.token.start = start;
        self.token.len = self.ptr - start;
        self.token.owned = false;
        self.token.kind = TokIdentifier;
        TokIdentifier
    }

    /// `isSafeStringCharacter<Strict|Sloppy>` (o caractere fora do Latin1 é sempre seguro).
    fn is_safe_string_character(&self, character: u16, terminator: u16) -> bool {
        if character > 0xFF {
            return true;
        }
        match self.mode {
            ParserMode::StrictJSON => !(character < 0x20 || character == '"' as u16 || character == '\\' as u16),
            ParserMode::SloppyJSON | ParserMode::JSONP => {
                (character >= 0x20 && character != '\\' as u16 && character != terminator) || character == '\t' as u16
            }
        }
    }

    /// `lexString`.
    fn lex_string(&mut self, terminator: u16) -> TokenType {
        self.ptr += 1;
        let run_start = self.ptr;
        while self.ptr < self.chars.len() && self.is_safe_string_character(self.chars[self.ptr], terminator) {
            self.ptr += 1;
        }

        if self.ptr < self.chars.len() && self.chars[self.ptr] == terminator {
            self.token.start = run_start;
            self.token.len = self.ptr - run_start;
            self.token.owned = false;
            self.token.kind = TokString;
            self.ptr += 1;
            return TokString;
        }
        self.lex_string_slow(run_start, terminator)
    }

    /// `lexStringSlow`: o caminho com escape (só o modo estrito processa `\`; no sloppy a barra
    /// invertida cai em `Unterminated string`).
    fn lex_string_slow(&mut self, mut run_start: usize, terminator: u16) -> TokenType {
        self.builder.clear();
        let end = self.chars.len();
        // `goto slowPathBegin`: a primeira volta pula a varredura do começo do laço.
        let mut first_pass = true;
        loop {
            if !first_pass {
                run_start = self.ptr;
                while self.ptr < end && self.is_safe_string_character(self.chars[self.ptr], terminator) {
                    self.ptr += 1;
                }
                if !self.builder.is_empty() {
                    self.builder.extend_from_slice(&self.chars[run_start..self.ptr]);
                }
            }
            first_pass = false;

            // slowPathBegin:
            if self.mode != ParserMode::SloppyJSON && self.ptr < end && self.chars[self.ptr] == '\\' as u16 {
                if self.builder.is_empty() && run_start < self.ptr {
                    self.builder.extend_from_slice(&self.chars[run_start..self.ptr]);
                }
                self.ptr += 1;
                if self.ptr >= end {
                    self.error = ascii_units("Unterminated string");
                    return TokError;
                }
                match self.chars[self.ptr] {
                    0x22 => {
                        self.builder.push('"' as u16);
                        self.ptr += 1;
                    }
                    0x5C => {
                        self.builder.push('\\' as u16);
                        self.ptr += 1;
                    }
                    0x2F => {
                        self.builder.push('/' as u16);
                        self.ptr += 1;
                    }
                    0x62 => {
                        self.builder.push(0x08);
                        self.ptr += 1;
                    }
                    0x66 => {
                        self.builder.push(0x0C);
                        self.ptr += 1;
                    }
                    0x6E => {
                        self.builder.push(0x0A);
                        self.ptr += 1;
                    }
                    0x72 => {
                        self.builder.push(0x0D);
                        self.ptr += 1;
                    }
                    0x74 => {
                        self.builder.push(0x09);
                        self.ptr += 1;
                    }
                    0x75 => {
                        // uNNNN == 5 caracteres.
                        if end - self.ptr < 5 {
                            self.error = ascii_units("\\u must be followed by 4 hex digits");
                            return TokError;
                        }
                        let mut value: u16 = 0;
                        for i in 1..5 {
                            match hex_digit_value(self.chars[self.ptr + i]) {
                                Some(digit) => value = (value << 4) | digit,
                                None => {
                                    self.error = concat_units(&[
                                        &ascii_units("\"\\"),
                                        &self.chars[self.ptr..self.ptr + 5],
                                        &ascii_units("\" is not a valid unicode escape"),
                                    ]);
                                    return TokError;
                                }
                            }
                        }
                        self.builder.push(value);
                        self.ptr += 5;
                    }
                    0x27 if self.mode != ParserMode::StrictJSON => {
                        self.builder.push('\'' as u16);
                        self.ptr += 1;
                    }
                    other => {
                        self.error = concat_units(&[&ascii_units("Invalid escape character "), &[other]]);
                        return TokError;
                    }
                }
            }

            if !(self.mode != ParserMode::SloppyJSON
                && self.ptr != run_start
                && self.ptr < end
                && self.chars[self.ptr] != terminator)
            {
                break;
            }
        }

        if self.ptr >= end || self.chars[self.ptr] != terminator {
            self.error = ascii_units("Unterminated string");
            return TokError;
        }

        if self.builder.is_empty() {
            self.token.start = run_start;
            self.token.len = self.ptr - run_start;
            self.token.owned = false;
        } else {
            self.token.len = self.builder.len();
            self.token.owned = true;
        }
        self.token.kind = TokString;
        self.ptr += 1;
        TokString
    }

    /// `lexNumber`: `-?(0 | [1-9][0-9]*) ('.' [0-9]+)? ([eE][+-]? [0-9]+)?`.
    fn lex_number(&mut self) -> TokenType {
        let end = self.chars.len();
        let initial = self.ptr;
        let mut negative = false;
        if self.ptr < end && self.chars[self.ptr] == '-' as u16 {
            negative = true;
            self.ptr += 1;
        }
        let start = self.ptr; // Sem o '-'.

        // (0 | [1-9][0-9]*)
        let mut accumulated: u32 = 0;
        if self.ptr < end && is_ascii_digit(self.chars[self.ptr]) {
            let character = self.chars[self.ptr];
            self.ptr += 1;
            accumulated = (character - 0x30) as u32;
            if character != '0' as u16 {
                while self.ptr < end && is_ascii_digit(self.chars[self.ptr]) {
                    accumulated = accumulated.wrapping_mul(10).wrapping_add((self.chars[self.ptr] - 0x30) as u32);
                    self.ptr += 1;
                }
            }
        } else {
            self.error = ascii_units("Invalid number");
            return TokError;
        }

        // Os números de -999999999 a 999999999 sempre cabem em int32.
        const NUMBER_OF_DIGITS_FOR_SAFE_INT32: usize = 9;
        if self.ptr < end
            && self.chars[self.ptr] != '.' as u16
            && self.chars[self.ptr] != 'e' as u16
            && self.chars[self.ptr] != 'E' as u16
            && self.ptr - start <= NUMBER_OF_DIGITS_FOR_SAFE_INT32
        {
            let result = accumulated as i32;
            if !negative {
                self.token.kind = TokNumberInt32;
                self.token.int32 = result;
                return TokNumberInt32;
            }
            if result == 0 {
                self.token.kind = TokNumber;
                self.token.number = -0.0;
                return TokNumber;
            }
            self.token.kind = TokNumberInt32;
            self.token.int32 = -result;
            return TokNumberInt32;
        }

        let mut parsed_length: usize = 0;
        if let Some(value) = parse_json_double(&self.chars[initial..], &mut parsed_length) {
            self.ptr = initial + parsed_length;
            self.token.kind = TokNumber;
            self.token.number = value;
            return TokNumber;
        }

        self.lex_number_error()
    }

    /// `lexNumberError`.
    fn lex_number_error(&mut self) -> TokenType {
        let end = self.chars.len();
        // ('.' [0-9]+)?
        if self.ptr < end && self.chars[self.ptr] == '.' as u16 {
            self.ptr += 1;
            // [0-9]+
            if self.ptr >= end || !is_ascii_digit(self.chars[self.ptr]) {
                self.error = ascii_units("Invalid digits after decimal point");
                return TokError;
            }
            self.ptr += 1;
            while self.ptr < end && is_ascii_digit(self.chars[self.ptr]) {
                self.ptr += 1;
            }
        }

        // ([eE][+-]? [0-9]+)?
        if self.ptr < end && (self.chars[self.ptr] == 'e' as u16 || self.chars[self.ptr] == 'E' as u16) {
            self.ptr += 1;
            // [-+]?
            if self.ptr < end && (self.chars[self.ptr] == '-' as u16 || self.chars[self.ptr] == '+' as u16) {
                self.ptr += 1;
            }
            // [0-9]+
            if self.ptr >= end || !is_ascii_digit(self.chars[self.ptr]) {
                self.error = ascii_units(
                    "Exponent symbols should be followed by an optional '+' or '-' and then by at least one number",
                );
                return TokError;
            }
            self.ptr += 1;
            while self.ptr < end && is_ascii_digit(self.chars[self.ptr]) {
                self.ptr += 1;
            }
        }

        // `ASSERT_NOT_REACHED()` no C++: o `parseJSONDouble` já aceita tudo o que passa daqui.
        self.error = ascii_units("Invalid number");
        TokError
    }
}

// ---------------------------------------------------------------------------------------------
// Intervalos de fonte (JSON.parse source text access)
// ---------------------------------------------------------------------------------------------

/// `JSONRanges::Entry::properties` (`Variant<std::monostate, Object, Array>`). As chaves do objeto são
/// o texto da propriedade em unidades de código (o C++ usa a identidade do `UniquedStringImpl`).
#[derive(Clone, Debug, Default)]
pub enum JsonRangeProperties {
    #[default]
    None,
    Object(HashMap<Vec<u16>, JsonRangeEntry>),
    Array(Vec<JsonRangeEntry>),
}

/// `JSONRanges::Entry`: o valor, o intervalo `[begin, end)` do fonte e os filhos.
#[derive(Clone, Debug)]
pub struct JsonRangeEntry {
    pub value: JSValue,
    pub begin: u32,
    pub end: u32,
    pub properties: JsonRangeProperties,
}

impl Default for JsonRangeEntry {
    fn default() -> JsonRangeEntry {
        JsonRangeEntry { value: JSValue::Empty, begin: 0, end: 0, properties: JsonRangeProperties::None }
    }
}

/// `class JSONRanges`.
#[derive(Clone, Debug, Default)]
pub struct JsonRanges {
    root: JsonRangeEntry,
}

impl JsonRanges {
    /// `root()`.
    pub fn root(&self) -> &JsonRangeEntry {
        &self.root
    }

    /// `setRoot`.
    fn set_root(&mut self, root: JsonRangeEntry) {
        self.root = root;
    }
}

// ---------------------------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------------------------

/// `enum ParserState`, sem os dois estados de statement (veja o cabeçalho).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParserState {
    StartParseObject,
    StartParseArray,
    StartParseExpression,
    DoParseObjectStartExpression,
    DoParseObjectEndExpression,
    DoParseArrayStartExpression,
    DoParseArrayEndExpression,
}

/// O que `m_objectStack` guarda: o array ainda em construção (os elementos) ou o objeto já criado.
enum Container {
    Array(Vec<JSValue>),
    Object(JSValue),
}

/// `JSValue()` vazio do C++ é `Ok(None)` (falha de parse, com a mensagem no parser); a exceção
/// pendente é `Err`.
pub type ParseResult = Result<Option<JSValue>, JsonError>;

/// `LiteralParser<CharType, reviverMode>`: a presença de `JsonRanges` no `parse` faz o papel do
/// `JSONReviverMode::Enabled`.
pub struct LiteralParser<'a, H: JsonHost + ?Sized> {
    host: &'a H,
    lexer: Lexer<'a>,
    mode: ParserMode,
    /// `m_parseErrorMessage`.
    parse_error: Vec<u16>,
    /// `m_visitedUnderscoreProto`: o `cell_id` dos objetos que já receberam `__proto__`.
    visited_underscore_proto: Vec<usize>,
    /// `m_objectStack`.
    containers: Vec<Container>,
    /// `m_stateStack`.
    state_stack: Vec<ParserState>,
    /// `m_identifierStack`.
    identifier_stack: Vec<Vec<u16>>,
    /// `m_rangesStack`.
    ranges_stack: Vec<JsonRangeEntry>,
}

impl<'a, H: JsonHost + ?Sized> LiteralParser<'a, H> {
    pub fn new(host: &'a H, characters: &'a [u16], mode: ParserMode) -> LiteralParser<'a, H> {
        LiteralParser {
            host,
            lexer: Lexer::new(characters, mode),
            mode,
            parse_error: Vec::new(),
            visited_underscore_proto: Vec::new(),
            containers: Vec::new(),
            state_stack: Vec::new(),
            identifier_stack: Vec::new(),
            ranges_stack: Vec::new(),
        }
    }

    /// `getErrorMessage()`.
    pub fn get_error_message(&self) -> WtfString {
        let prefix = ascii_units("JSON Parse error: ");
        if !self.lexer.error.is_empty() {
            return units_to_wtf_string(&concat_units(&[&prefix, &self.lexer.error]));
        }
        if !self.parse_error.is_empty() {
            return units_to_wtf_string(&concat_units(&[&prefix, &self.parse_error]));
        }
        units_to_wtf_string(&concat_units(&[&prefix, &ascii_units("Unable to parse JSON string")]))
    }

    /// `tryLiteralParse()` e `tryLiteralParse(JSONRanges*)`: com `ranges` o parse registra os
    /// intervalos de fonte. `Ok(None)` é o `JSValue()` vazio (sem mensagem própria se sobrou token).
    pub fn try_literal_parse(&mut self, ranges: Option<&mut JsonRanges>) -> ParseResult {
        debug_assert!(self.mode == ParserMode::StrictJSON);
        self.lexer.next();
        let result = self.parse(ParserState::StartParseExpression, ranges)?;
        if self.lexer.token.kind != TokEnd {
            return Ok(None);
        }
        Ok(result)
    }

    /// `tryEval()`.
    pub fn try_eval(&mut self) -> ParseResult {
        debug_assert!(self.mode == ParserMode::SloppyJSON);
        self.lexer.next();
        let result = self.eval_recursively_entry()?;
        if self.lexer.token.kind == TokSemi {
            self.lexer.next();
        }
        if self.lexer.token.kind != TokEnd {
            return Ok(None);
        }
        Ok(result)
    }

    /// `tryLiteralParsePrimitiveValue()`.
    pub fn try_literal_parse_primitive_value(&mut self) -> ParseResult {
        debug_assert!(self.mode == ParserMode::StrictJSON);
        self.lexer.next();
        let result = self.parse_primitive_value()?;
        if result.is_some() && self.lexer.token.kind != TokEnd {
            self.parse_error = ascii_units("Unexpected content at end of JSON literal");
            return Ok(None);
        }
        Ok(result)
    }

    /// `Identifier::fromString(vm, m_lexer.currentToken()->identifier())`.
    fn current_identifier(&self) -> Identifier {
        Identifier::from_string(self.host.vm(), &units_to_wtf_string(self.lexer.token_text()))
    }

    /// `tryJSONPParse(results, needsFullSourceInfo)`: `Ok(false)` é a recusa (qualquer coisa fora de
    /// `var x = literal`, `a.b = literal`, `a[n] = literal` e `f(literal)`, separados por `;`).
    pub fn try_jsonp_parse(&mut self, results: &mut Vec<JsonpData>, needs_full_source_info: bool) -> Result<bool, JsonError> {
        debug_assert!(self.mode == ParserMode::JSONP);
        if self.lexer.next() != TokIdentifier {
            return Ok(false);
        }
        loop {
            let mut path: Vec<JsonpPathEntry> = Vec::new();
            // Unguarded next to start off the lexer.
            let name = self.current_identifier();
            let mut entry = JsonpPathEntry { path_entry_name: Identifier::default(), path_index: 0, entry_type: JsonpPathEntryType::Dot };
            if name == self.host.vm().property_names.var_keyword {
                if self.lexer.next() != TokIdentifier {
                    return Ok(false);
                }
                entry.entry_type = JsonpPathEntryType::DeclareVar;
                entry.path_entry_name = self.current_identifier();
                path.push(entry.clone());
            } else {
                entry.entry_type = JsonpPathEntryType::Dot;
                entry.path_entry_name = name;
                path.push(entry.clone());
            }
            if is_lexer_keyword(&entry.path_entry_name) {
                return Ok(false);
            }
            let mut token_type = self.lexer.next();
            if entry.entry_type == JsonpPathEntryType::DeclareVar && token_type != TokAssign {
                return Ok(false);
            }
            while token_type != TokAssign {
                match token_type {
                    TokLBracket => {
                        entry.entry_type = JsonpPathEntryType::Lookup;
                        let number_type = self.lexer.next();
                        if number_type != TokNumber && number_type != TokNumberInt32 {
                            return Ok(false);
                        }
                        let index = if self.lexer.token.kind == TokNumberInt32 {
                            self.lexer.token.int32
                        } else {
                            let double_index = self.lexer.token.number;
                            let index = truncate_double_to_int32(double_index);
                            if index as f64 != double_index {
                                return Ok(false);
                            }
                            index
                        };
                        if index < 0 {
                            return Ok(false);
                        }
                        entry.path_index = index;
                        if self.lexer.next() != TokRBracket {
                            return Ok(false);
                        }
                    }
                    TokDot => {
                        entry.entry_type = JsonpPathEntryType::Dot;
                        if self.lexer.next() != TokIdentifier {
                            return Ok(false);
                        }
                        entry.path_entry_name = self.current_identifier();
                    }
                    TokLParen => {
                        let last = path.last_mut().expect("path vazio");
                        if last.entry_type != JsonpPathEntryType::Dot || needs_full_source_info {
                            return Ok(false);
                        }
                        last.entry_type = JsonpPathEntryType::Call;
                        entry = last.clone();
                        // `goto startJSON`.
                        break;
                    }
                    _ => return Ok(false),
                }
                path.push(entry.clone());
                token_type = self.lexer.next();
            }
            // startJSON:
            self.lexer.next();
            results.push(JsonpData { path: Vec::new(), value: JSValue::Empty });
            let Some(value) = self.parse(ParserState::StartParseExpression, None)? else {
                return Ok(false);
            };
            let last = results.last_mut().expect("results vazio");
            last.value = value;
            last.path = path;
            if entry.entry_type == JsonpPathEntryType::Call {
                if self.lexer.token.kind != TokRParen {
                    return Ok(false);
                }
                self.lexer.next();
            }
            if self.lexer.token.kind != TokSemi {
                break;
            }
            self.lexer.next();
            if self.lexer.token.kind != TokIdentifier {
                break;
            }
        }
        Ok(self.lexer.token.kind == TokEnd)
    }

    /// `evalRecursivelyEntry` (o ramo com `useRecursiveJSONParse`): note que, dentro de `( ... )`,
    /// uma falha do valor não interrompe a checagem do `)`, que pode trocar a mensagem.
    fn eval_recursively_entry(&mut self) -> ParseResult {
        let mut token_type = self.lexer.token.kind;
        if token_type == TokLParen {
            token_type = self.lexer.next();
            let result = if token_type == TokLBrace || token_type == TokLBracket {
                self.parse(ParserState::StartParseExpression, None)?
            } else {
                self.parse_primitive_value()?
            };
            if self.lexer.token.kind != TokRParen {
                self.parse_error = ascii_units("Unexpected content at end of JSON literal");
                return Ok(None);
            }
            self.lexer.next();
            return Ok(result);
        }

        if token_type == TokLBrace {
            self.parse_error = ascii_units("Unexpected token '{'");
            return Ok(None);
        }

        if token_type == TokLBracket {
            return self.parse(ParserState::StartParseExpression, None);
        }
        self.parse_primitive_value()
    }

    /// `setErrorMessageForToken`.
    fn set_error_message_for_token(&mut self, token_type: TokenType) {
        let message = match token_type {
            TokRBrace => "Expected '}'",
            TokRBracket => "Expected ']'",
            TokColon => "Expected ':' before value in object property definition",
            _ => unreachable!("setErrorMessageForToken com token que o C++ não trata"),
        };
        self.parse_error = ascii_units(message);
    }

    /// `parsePrimitiveValue`.
    fn parse_primitive_value(&mut self) -> ParseResult {
        let message = match self.lexer.token.kind {
            TokString => {
                let string = units_to_wtf_string(self.lexer.token_text());
                let value = JSValue::from_js_string(js_string(self.host.vm(), &string));
                self.lexer.next();
                return Ok(Some(value));
            }
            TokNumberInt32 => {
                let value = JSValue::Int32(self.lexer.token.int32);
                self.lexer.next();
                return Ok(Some(value));
            }
            TokNumber => {
                let value = js_number(self.lexer.token.number);
                self.lexer.next();
                return Ok(Some(value));
            }
            TokNull => {
                self.lexer.next();
                return Ok(Some(JSValue::Null));
            }
            TokTrue => {
                self.lexer.next();
                return Ok(Some(JSValue::Bool(true)));
            }
            TokFalse => {
                self.lexer.next();
                return Ok(Some(JSValue::Bool(false)));
            }
            TokRBracket => ascii_units("Unexpected token ']'"),
            TokRBrace => ascii_units("Unexpected token '}'"),
            TokIdentifier => {
                const MAX_LENGTH: usize = 200;
                let text = self.lexer.token_text();
                let shown = text.len().min(MAX_LENGTH);
                let ellipsis = if shown != text.len() { ascii_units("...") } else { Vec::new() };
                concat_units(&[&ascii_units("Unexpected identifier \""), &text[..shown], &ellipsis, &ascii_units("\"")])
            }
            TokColon => ascii_units("Unexpected token ':'"),
            TokLParen => ascii_units("Unexpected token '('"),
            TokRParen => ascii_units("Unexpected token ')'"),
            TokComma => ascii_units("Unexpected token ','"),
            TokDot => ascii_units("Unexpected token '.'"),
            TokAssign => ascii_units("Unexpected token '='"),
            TokSemi => ascii_units("Unexpected token ';'"),
            TokEnd => ascii_units("Unexpected EOF"),
            TokLBracket | TokLBrace | TokError | TokErrorSpace => ascii_units("Could not parse value expression"),
        };
        self.parse_error = message;
        Ok(None)
    }

    /// Guarda `key: value` no objeto, com o tratamento de `__proto__` do modo sloppy. `Ok(false)` é
    /// falha de parse (mensagem em `parse_error`).
    fn store_property(&mut self, object: JSValue, key: &[u16], value: JSValue) -> Result<bool, JsonError> {
        if self.mode != ParserMode::StrictJSON && key == ascii_units("__proto__").as_slice() {
            let object_id = object.as_cell();
            if self.visited_underscore_proto.contains(&object_id) {
                self.parse_error = ascii_units("Attempted to redefine __proto__ property");
                return Ok(false);
            }
            self.visited_underscore_proto.push(object_id);
            self.host.set_underscore_proto(object, value)?;
            return Ok(true);
        }
        self.host.create_data_property(object, &JsonKey::from_units(key), value)?;
        Ok(true)
    }

    /// Fecha o array do topo e pede ao host o `JSArray` (`materializeArray`).
    fn finish_array(&mut self) -> Result<JSValue, JsonError> {
        let Some(Container::Array(elements)) = self.containers.pop() else {
            unreachable!("o topo de m_objectStack não é um array em DoParseArrayEndExpression");
        };
        Ok(self.host.new_array(&elements)?)
    }

    /// `m_rangesStack.takeLast()` com o fim do intervalo no fim do token atual.
    fn close_range(&mut self) -> JsonRangeEntry {
        let mut entry = self.ranges_stack.pop().expect("m_rangesStack vazia");
        entry.end = self.lexer.token_end as u32;
        entry
    }

    /// Guarda `entry` como filho `key` do objeto cujo intervalo está no topo da pilha.
    fn record_object_child(&mut self, key: &[u16], entry: JsonRangeEntry) {
        match &mut self.ranges_stack.last_mut().expect("m_rangesStack vazia").properties {
            JsonRangeProperties::Object(map) => {
                map.insert(key.to_vec(), entry);
            }
            _ => unreachable!("o intervalo do topo não é de objeto"),
        }
    }

    /// `parse(vm, initialState, sourceRanges)`.
    fn parse(&mut self, initial_state: ParserState, ranges: Option<&mut JsonRanges>) -> ParseResult {
        let tracking = ranges.is_some();
        let mut state = initial_state;
        let mut last_value = JSValue::Empty;
        let mut last_value_range = JsonRangeEntry::default();

        'machine: loop {
            match state {
                ParserState::StartParseArray => {
                    self.containers.push(Container::Array(Vec::new()));
                    if tracking {
                        if self.ranges_stack.len() >= MAXIMUM_RANGES_STACK_RECURSION {
                            return Err(JsonError::StackOverflow);
                        }
                        let start = self.lexer.token_start as u32;
                        self.ranges_stack.push(JsonRangeEntry {
                            value: JSValue::Empty,
                            begin: start,
                            end: start,
                            properties: JsonRangeProperties::Array(Vec::new()),
                        });
                    }
                    state = ParserState::DoParseArrayStartExpression;
                    continue 'machine;
                }
                ParserState::DoParseArrayStartExpression => {
                    let last_token = self.lexer.token.kind;
                    if self.lexer.next() == TokRBracket {
                        if last_token == TokComma {
                            self.parse_error = ascii_units("Unexpected comma at the end of array expression");
                            return Ok(None);
                        }
                        if tracking {
                            last_value_range = self.close_range();
                        }
                        self.lexer.next();
                        last_value = self.finish_array()?;
                        if tracking {
                            last_value_range.value = last_value;
                        }
                    } else {
                        self.state_stack.push(ParserState::DoParseArrayEndExpression);
                        state = ParserState::StartParseExpression;
                        continue 'machine;
                    }
                }
                ParserState::DoParseArrayEndExpression => {
                    match self.containers.last_mut() {
                        Some(Container::Array(elements)) => elements.push(last_value),
                        _ => unreachable!("o topo de m_objectStack não é um array em DoParseArrayEndExpression"),
                    }
                    if tracking {
                        match &mut self.ranges_stack.last_mut().expect("m_rangesStack vazia").properties {
                            JsonRangeProperties::Array(children) => children.push(std::mem::take(&mut last_value_range)),
                            _ => unreachable!("o intervalo do topo não é de array"),
                        }
                    }

                    if self.lexer.token.kind == TokComma {
                        state = ParserState::DoParseArrayStartExpression;
                        continue 'machine;
                    }

                    if self.lexer.token.kind != TokRBracket {
                        self.set_error_message_for_token(TokRBracket);
                        return Ok(None);
                    }

                    if tracking {
                        last_value_range = self.close_range();
                    }
                    self.lexer.next();
                    last_value = self.finish_array()?;
                    if tracking {
                        last_value_range.value = last_value;
                    }
                }
                ParserState::StartParseObject => {
                    let object = self.host.new_object()?;
                    self.containers.push(Container::Object(object));
                    if tracking {
                        if self.ranges_stack.len() >= MAXIMUM_RANGES_STACK_RECURSION {
                            return Err(JsonError::StackOverflow);
                        }
                        let start = self.lexer.token_start as u32;
                        self.ranges_stack.push(JsonRangeEntry {
                            value: object,
                            begin: start,
                            end: start,
                            properties: JsonRangeProperties::Object(HashMap::new()),
                        });
                    }

                    let token_type = self.lexer.next();
                    if token_type == TokString || (self.mode != ParserMode::StrictJSON && token_type == TokIdentifier) {
                        loop {
                            let identifier = self.lexer.token_text().to_vec();

                            if self.lexer.next() != TokColon {
                                self.set_error_message_for_token(TokColon);
                                return Ok(None);
                            }

                            let next_type = self.lexer.next();
                            if next_type == TokLBrace || next_type == TokLBracket {
                                self.identifier_stack.push(identifier);
                                self.state_stack.push(ParserState::DoParseObjectEndExpression);
                                state = if next_type == TokLBrace {
                                    ParserState::StartParseObject
                                } else {
                                    ParserState::StartParseArray
                                };
                                continue 'machine;
                            }

                            // Caminho rápido do objeto de folhas.
                            let property_begin = self.lexer.token_start as u32;
                            let property_end = self.lexer.token_end as u32;
                            let Some(primitive) = self.parse_primitive_value()? else {
                                return Ok(None);
                            };

                            if !self.store_property(object, &identifier, primitive)? {
                                return Ok(None);
                            }
                            if tracking {
                                self.record_object_child(
                                    &identifier,
                                    JsonRangeEntry {
                                        value: primitive,
                                        begin: property_begin,
                                        end: property_end,
                                        properties: JsonRangeProperties::None,
                                    },
                                );
                            }

                            if self.lexer.token.kind != TokComma {
                                break;
                            }

                            let next_type = self.lexer.next();
                            if next_type != TokString
                                && (self.mode == ParserMode::StrictJSON || next_type != TokIdentifier)
                            {
                                self.parse_error = ascii_units("Property name must be a string literal");
                                return Ok(None);
                            }
                        }

                        if self.lexer.token.kind != TokRBrace {
                            self.set_error_message_for_token(TokRBrace);
                            return Ok(None);
                        }

                        if tracking {
                            last_value_range = self.close_range();
                        }
                        self.lexer.next();
                        self.containers.pop();
                        last_value = object;
                    } else {
                        if token_type != TokRBrace {
                            self.set_error_message_for_token(TokRBrace);
                            return Ok(None);
                        }

                        if tracking {
                            last_value_range = self.close_range();
                        }
                        self.lexer.next();
                        self.containers.pop();
                        last_value = object;
                    }
                }
                ParserState::DoParseObjectStartExpression => {
                    let token_type = self.lexer.next();
                    if token_type != TokString && (self.mode == ParserMode::StrictJSON || token_type != TokIdentifier) {
                        self.parse_error = ascii_units("Property name must be a string literal");
                        return Ok(None);
                    }
                    let identifier = self.lexer.token_text().to_vec();
                    self.identifier_stack.push(identifier);

                    // Confere os dois pontos.
                    if self.lexer.next() != TokColon {
                        self.set_error_message_for_token(TokColon);
                        return Ok(None);
                    }

                    self.lexer.next();
                    self.state_stack.push(ParserState::DoParseObjectEndExpression);
                    state = ParserState::StartParseExpression;
                    continue 'machine;
                }
                ParserState::DoParseObjectEndExpression => {
                    let object = match self.containers.last() {
                        Some(Container::Object(object)) => *object,
                        _ => unreachable!("o topo de m_objectStack não é um objeto em DoParseObjectEndExpression"),
                    };
                    let identifier = self.identifier_stack.pop().expect("m_identifierStack vazia");
                    if !self.store_property(object, &identifier, last_value)? {
                        return Ok(None);
                    }
                    if tracking {
                        let entry = std::mem::take(&mut last_value_range);
                        self.record_object_child(&identifier, entry);
                    }
                    if self.lexer.token.kind == TokComma {
                        state = ParserState::DoParseObjectStartExpression;
                        continue 'machine;
                    }
                    if self.lexer.token.kind != TokRBrace {
                        self.set_error_message_for_token(TokRBrace);
                        return Ok(None);
                    }

                    if tracking {
                        last_value_range = self.close_range();
                    }
                    self.lexer.next();
                    self.containers.pop();
                    last_value = object;
                }
                ParserState::StartParseExpression => {
                    let token_type = self.lexer.token.kind;
                    if token_type == TokLBracket {
                        state = ParserState::StartParseArray;
                        continue 'machine;
                    }
                    if token_type == TokLBrace {
                        state = ParserState::StartParseObject;
                        continue 'machine;
                    }

                    if tracking {
                        last_value_range = JsonRangeEntry {
                            value: JSValue::Empty,
                            begin: self.lexer.token_start as u32,
                            end: self.lexer.token_end as u32,
                            properties: JsonRangeProperties::None,
                        };
                    }
                    let Some(value) = self.parse_primitive_value()? else {
                        return Ok(None);
                    };
                    last_value = value;
                    if tracking {
                        last_value_range.value = last_value;
                    }
                }
            }

            if self.state_stack.is_empty() {
                if let Some(ranges) = ranges {
                    ranges.set_root(last_value_range);
                }
                return Ok(Some(last_value));
            }
            state = self.state_stack.pop().expect("m_stateStack vazia");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Entradas por tipo de parse
// ---------------------------------------------------------------------------------------------

/// Qual das tentativas do `LiteralParser` rodar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteralParseKind {
    /// `tryLiteralParse()` (modo `StrictJSON`): o `JSON.parse`.
    Strict,
    /// `tryLiteralParsePrimitiveValue()`: o `JSON.rawJSON`.
    StrictPrimitive,
    /// `tryEval()` (modo `SloppyJSON`): o literal do `eval`.
    Sloppy,
}

/// O desfecho de `literal_parse`.
#[derive(Clone, Debug)]
pub enum ParseOutcome {
    Value(JSValue),
    /// O `JSValue()` vazio, com o texto de `getErrorMessage()` (`"JSON Parse error: ..."`).
    Failed(WtfString),
}

/// Instancia o `LiteralParser` e roda a tentativa pedida. Com `ranges`, só o `Strict` registra os
/// intervalos de fonte (o `reviverMode == Enabled` do C++).
pub fn literal_parse<H: JsonHost + ?Sized>(
    host: &H,
    text: &WtfString,
    kind: LiteralParseKind,
    ranges: Option<&mut JsonRanges>,
) -> Result<ParseOutcome, JsonError> {
    let units = wtf_string_to_units(text);
    let mode = if kind == LiteralParseKind::Sloppy { ParserMode::SloppyJSON } else { ParserMode::StrictJSON };
    let mut parser = LiteralParser::new(host, &units, mode);
    let result = match kind {
        LiteralParseKind::Strict => parser.try_literal_parse(ranges)?,
        LiteralParseKind::StrictPrimitive => parser.try_literal_parse_primitive_value()?,
        LiteralParseKind::Sloppy => parser.try_eval()?,
    };
    Ok(match result {
        Some(value) => ParseOutcome::Value(value),
        None => ParseOutcome::Failed(parser.get_error_message()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_all(source: &str, mode: ParserMode) -> (Vec<TokenType>, String) {
        let units = ascii_units(source);
        let mut lexer = Lexer::new(&units, mode);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next();
            tokens.push(token);
            if token == TokEnd || token == TokError {
                break;
            }
        }
        let message = String::from_utf16_lossy(&lexer.error);
        (tokens, message)
    }

    #[test]
    fn lexes_structure_and_keywords() {
        let (tokens, message) = lex_all("{\"a\": [true, false, null, 1, -2.5]}", ParserMode::StrictJSON);
        assert_eq!(
            tokens,
            vec![
                TokLBrace, TokString, TokColon, TokLBracket, TokTrue, TokComma, TokFalse, TokComma, TokNull, TokComma,
                TokNumberInt32, TokComma, TokNumber, TokRBracket, TokRBrace, TokEnd
            ]
        );
        assert_eq!(message, "");
    }

    #[test]
    fn lexer_error_messages_match_jsc() {
        assert_eq!(lex_all("'a'", ParserMode::StrictJSON).1, "Single quotes (') are not allowed in JSON");
        assert_eq!(lex_all("\"abc", ParserMode::StrictJSON).1, "Unterminated string");
        assert_eq!(lex_all("\"\\x\"", ParserMode::StrictJSON).1, "Invalid escape character x");
        assert_eq!(lex_all("\"\\u12\"", ParserMode::StrictJSON).1, "\\u must be followed by 4 hex digits");
        assert_eq!(lex_all("\"\\u12G4\"", ParserMode::StrictJSON).1, "\"\\u12G4\" is not a valid unicode escape");
        assert_eq!(lex_all("#", ParserMode::StrictJSON).1, "Unrecognized token '#'");
        assert_eq!(lex_all("-", ParserMode::StrictJSON).1, "Invalid number");
        assert_eq!(lex_all("1.", ParserMode::StrictJSON).1, "Invalid digits after decimal point");
        // `parseJSONDouble` usa `chars_format::json` (fixed | scientific): um expoente sem dígitos é
        // ignorado e o `e` vira um identificador, então a mensagem de expoente do `lexNumberError` não
        // é alcançada por esse caminho (o parser rejeita o identificador depois).
        assert_eq!(lex_all("1e", ParserMode::StrictJSON), (vec![TokNumber, TokIdentifier, TokEnd], String::new()));
    }

    #[test]
    fn sloppy_mode_accepts_single_quotes_and_rejects_backslash() {
        let (tokens, message) = lex_all("'a b'", ParserMode::SloppyJSON);
        assert_eq!(tokens, vec![TokString, TokEnd]);
        assert_eq!(message, "");
        assert_eq!(lex_all("'a\\nb'", ParserMode::SloppyJSON).1, "Unterminated string");
    }

    #[test]
    fn escapes_build_the_token_text() {
        let units = ascii_units("\"a\\n\\u0041\\\"z\"");
        let mut lexer = Lexer::new(&units, ParserMode::StrictJSON);
        assert_eq!(lexer.next(), TokString);
        assert_eq!(lexer.token_text(), &[0x61, 0x0A, 0x41, 0x22, 0x7A][..]);
    }

    #[test]
    fn number_edge_cases() {
        let units = ascii_units("-0 ");
        let mut lexer = Lexer::new(&units, ParserMode::StrictJSON);
        assert_eq!(lexer.next(), TokNumber);
        assert!(lexer.token.number == 0.0 && lexer.token.number.is_sign_negative());

        let units = ascii_units("123456789 ");
        let mut lexer = Lexer::new(&units, ParserMode::StrictJSON);
        assert_eq!(lexer.next(), TokNumberInt32);
        assert_eq!(lexer.token.int32, 123456789);

        // Dez dígitos já passam pelo parseJSONDouble.
        let units = ascii_units("1234567890 ");
        let mut lexer = Lexer::new(&units, ParserMode::StrictJSON);
        assert_eq!(lexer.next(), TokNumber);
        assert_eq!(lexer.token.number, 1234567890.0);
    }

    /// Roda `tryJSONPParse` num global novo; `None` é a recusa.
    fn jsonp(source: &str, needs_full_source_info: bool) -> Option<Vec<JsonpData>> {
        let global = crate::runtime::js_global_object::JSGlobalObject::init(&std::rc::Rc::new(VM::new()));
        let units = ascii_units(source);
        let mut parser = LiteralParser::new(&*global, &units, ParserMode::JSONP);
        let mut results = Vec::new();
        match parser.try_jsonp_parse(&mut results, needs_full_source_info) {
            Ok(true) => Some(results),
            _ => None,
        }
    }

    #[test]
    fn jsonp_accepts_the_supported_forms() {
        for source in ["var x = 2", "var x = 2;", "a.b = {}", "f([1])", "a[1] = 'x'", "a.b = 1; var c = [true, null]"] {
            assert!(jsonp(source, false).is_some(), "{source}");
        }
        let declared = jsonp("var x = 2;", false).unwrap();
        assert_eq!(declared.len(), 1);
        assert_eq!(declared[0].path.len(), 1);
        assert_eq!(declared[0].path[0].entry_type, JsonpPathEntryType::DeclareVar);
        assert!(matches!(declared[0].value, JSValue::Int32(2)));

        let call = jsonp("f([1])", false).unwrap();
        assert_eq!(call[0].path.len(), 1);
        assert_eq!(call[0].path[0].entry_type, JsonpPathEntryType::Call);

        let lookup = jsonp("a.b[3] = 1", false).unwrap();
        let types: Vec<_> = lookup[0].path.iter().map(|entry| entry.entry_type).collect();
        assert_eq!(types, vec![JsonpPathEntryType::Dot, JsonpPathEntryType::Dot, JsonpPathEntryType::Lookup]);
        assert_eq!(lookup[0].path[2].path_index, 3);

        assert_eq!(jsonp("var a = 1; var b = 2", false).unwrap().len(), 2);
    }

    #[test]
    fn jsonp_rejects_everything_else() {
        for source in [
            "var x;",
            "var x = 2; 1",
            "1; var x = 2",
            "var x = f()",
            "var x = 2 + 1",
            "var x = 2, x = 3",
            "/*x*/ var b2 = 2",
            "",
            "a[-1] = 1",
            "a[1.5] = 1",
            "var if = 1",
        ] {
            assert!(jsonp(source, false).is_none(), "{source}");
        }
        // A chamada só vale sem `needsFullSourceInfo`.
        assert!(jsonp("f(1)", true).is_none());
    }

    #[test]
    fn json_key_splits_index_and_name() {
        assert!(matches!(JsonKey::from_units(&ascii_units("12")), JsonKey::Index(12)));
        assert!(matches!(JsonKey::from_units(&ascii_units("012")), JsonKey::Name(_)));
        assert!(matches!(JsonKey::from_units(&ascii_units("a")), JsonKey::Name(_)));
    }
}
