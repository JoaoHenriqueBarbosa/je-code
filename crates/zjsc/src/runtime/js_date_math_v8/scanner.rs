//! Porte da parte de baixo do `DateParser` de `runtime/JSDateMath-v8.cpp` (v8::DateParser):
//! `KeywordTable`, `DateToken`, `InputReader` e `DateStringTokenizer`.
//!
//! O C++ lê `const unsigned char*`; aqui o leitor é genérico no tipo de caractere (`u8` ou `u16`)
//! pelo trait `AsciiChar`, como o `template<typename Char>` dos comentários do original.
//! Os compositores e o `Parse` ficam em `parser.rs`.

use crate::wtf::ascii_ctype::{is_ascii_whitespace, AsciiChar};

/// `kNone` (JSDateMath-v8.cpp:119), valor ausente. É `kMaxInt` (cpp:31).
pub(crate) const K_NONE: i32 = 0x7FFF_FFFF;

/// `kMaxSignificantDigits` (cpp:123): máximo de dígitos usados para montar o valor de um numeral.
pub(crate) const K_MAX_SIGNIFICANT_DIGITS: i32 = 9;

/// `AsciiAlphaToLower` (cpp:56).
const fn ascii_alpha_to_lower(c: u32) -> u32 {
    c | 0x20
}

/// `IsDecimalDigit` (cpp:59), via `IsInRange(c, '0', '9')` (cpp:46).
const fn is_decimal_digit(c: u32) -> bool {
    c.wrapping_sub('0' as u32) <= ('9' as u32 - '0' as u32)
}

/// `IsLineTerminator` (cpp:65).
pub(crate) const fn is_line_terminator(c: u32) -> bool {
    c == 0x000A || c == 0x000D || c == 0x2028 || c == 0x2029
}

/// `enum KeywordType` (cpp:213). Os valores são os mesmos do `enum` do C++ e servem de tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum KeywordType {
    Invalid = 0,
    MonthName = 1,
    TimeZoneName = 2,
    TimeSeparator = 3,
    AmPm = 4,
}

impl KeywordType {
    /// `static_cast<KeywordType>(tag)`.
    const fn from_i32(value: i32) -> KeywordType {
        match value {
            1 => KeywordType::MonthName,
            2 => KeywordType::TimeZoneName,
            3 => KeywordType::TimeSeparator,
            4 => KeywordType::AmPm,
            _ => KeywordType::Invalid,
        }
    }
}

/// `DateParser::KeywordTable` (cpp:354). Mapeia nomes de meses, fusos e am/pm para números.
pub(crate) struct KeywordTable;

impl KeywordTable {
    /// `kPrefixLength` (cpp:368).
    pub(crate) const PREFIX_LENGTH: usize = 3;
    /// `kTypeOffset` (cpp:369).
    const TYPE_OFFSET: usize = Self::PREFIX_LENGTH;
    /// `kValueOffset` (cpp:370).
    const VALUE_OFFSET: usize = Self::TYPE_OFFSET + 1;
    /// `kEntrySize` (cpp:371).
    const ENTRY_SIZE: usize = Self::VALUE_OFFSET + 1;

    /// `DateParser::KeywordTable::array` (cpp:607-638). O tipo ocupa o quarto byte da linha.
    const ARRAY: [[i8; Self::ENTRY_SIZE]; 28] = {
        const MN: i8 = KeywordType::MonthName as i8;
        const AP: i8 = KeywordType::AmPm as i8;
        const TZ: i8 = KeywordType::TimeZoneName as i8;
        const TS: i8 = KeywordType::TimeSeparator as i8;
        const IN: i8 = KeywordType::Invalid as i8;
        [
            [b'j' as i8, b'a' as i8, b'n' as i8, MN, 1],
            [b'f' as i8, b'e' as i8, b'b' as i8, MN, 2],
            [b'm' as i8, b'a' as i8, b'r' as i8, MN, 3],
            [b'a' as i8, b'p' as i8, b'r' as i8, MN, 4],
            [b'm' as i8, b'a' as i8, b'y' as i8, MN, 5],
            [b'j' as i8, b'u' as i8, b'n' as i8, MN, 6],
            [b'j' as i8, b'u' as i8, b'l' as i8, MN, 7],
            [b'a' as i8, b'u' as i8, b'g' as i8, MN, 8],
            [b's' as i8, b'e' as i8, b'p' as i8, MN, 9],
            [b'o' as i8, b'c' as i8, b't' as i8, MN, 10],
            [b'n' as i8, b'o' as i8, b'v' as i8, MN, 11],
            [b'd' as i8, b'e' as i8, b'c' as i8, MN, 12],
            [b'a' as i8, b'm' as i8, 0, AP, 0],
            [b'p' as i8, b'm' as i8, 0, AP, 12],
            [b'u' as i8, b't' as i8, 0, TZ, 0],
            [b'u' as i8, b't' as i8, b'c' as i8, TZ, 0],
            [b'z' as i8, 0, 0, TZ, 0],
            [b'g' as i8, b'm' as i8, b't' as i8, TZ, 0],
            [b'c' as i8, b'd' as i8, b't' as i8, TZ, -5],
            [b'c' as i8, b's' as i8, b't' as i8, TZ, -6],
            [b'e' as i8, b'd' as i8, b't' as i8, TZ, -4],
            [b'e' as i8, b's' as i8, b't' as i8, TZ, -5],
            [b'm' as i8, b'd' as i8, b't' as i8, TZ, -6],
            [b'm' as i8, b's' as i8, b't' as i8, TZ, -7],
            [b'p' as i8, b'd' as i8, b't' as i8, TZ, -7],
            [b'p' as i8, b's' as i8, b't' as i8, TZ, -8],
            [b't' as i8, 0, 0, TS, 0],
            [0, 0, 0, IN, 0],
        ]
    };

    /// `KeywordTable::Lookup` (cpp:641). `pre` é o prefixo da palavra em minúsculas, preenchido
    /// com zeros até `PREFIX_LENGTH`, e `len` o comprimento da palavra. Devolve o índice da
    /// entrada (o da sentinela `Invalid` se não achar).
    pub(crate) fn lookup(pre: &[u32; 3], len: i32) -> usize {
        let mut i = 0;
        while Self::ARRAY[i][Self::TYPE_OFFSET] != KeywordType::Invalid as i8 {
            let mut j = 0;
            while j < Self::PREFIX_LENGTH && pre[j] == Self::ARRAY[i][j] as u32 {
                j += 1;
            }
            // Confere se casou e se o comprimento é legal. Palavra maior que a palavra-chave só
            // é permitida para nomes de mês.
            if j == Self::PREFIX_LENGTH
                && (len <= Self::PREFIX_LENGTH as i32
                    || Self::ARRAY[i][Self::TYPE_OFFSET] == KeywordType::MonthName as i8)
            {
                return i;
            }
            i += 1;
        }
        i
    }

    /// `KeywordTable::GetType` (cpp:361).
    pub(crate) fn get_type(i: usize) -> KeywordType {
        KeywordType::from_i32(Self::ARRAY[i][Self::TYPE_OFFSET] as i32)
    }

    /// `KeywordTable::GetValue` (cpp:366).
    pub(crate) fn get_value(i: usize) -> i32 {
        Self::ARRAY[i][Self::VALUE_OFFSET] as i32
    }
}

/// `enum TagType` de `DateToken` (cpp:298). Tags de palavra-chave são os valores de `KeywordType`.
const INVALID_TOKEN_TAG: i32 = -6;
const UNKNOWN_TOKEN_TAG: i32 = -5;
const WHITE_SPACE_TAG: i32 = -4;
const NUMBER_TAG: i32 = -3;
const SYMBOL_TAG: i32 = -2;
const END_OF_INPUT_TAG: i32 = -1;
const KEYWORD_TAG_START: i32 = 0;

/// `DateParser::DateToken` (cpp:221).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DateToken {
    tag: i32,
    /// Número de caracteres (cpp:315).
    length: i32,
    value: i32,
}

impl DateToken {
    /// Construtor privado `DateToken(int tag, int length, int value)` (cpp:307).
    const fn new(tag: i32, length: i32, value: i32) -> DateToken {
        DateToken { tag, length, value }
    }

    /// `IsInvalid` (cpp:223).
    pub(crate) fn is_invalid(&self) -> bool {
        self.tag == INVALID_TOKEN_TAG
    }
    /// `IsUnknown()` (cpp:224).
    pub(crate) fn is_unknown(&self) -> bool {
        self.tag == UNKNOWN_TOKEN_TAG
    }
    /// `IsNumber` (cpp:225).
    pub(crate) fn is_number(&self) -> bool {
        self.tag == NUMBER_TAG
    }
    /// `IsSymbol()` (cpp:226).
    pub(crate) fn is_symbol(&self) -> bool {
        self.tag == SYMBOL_TAG
    }
    /// `IsWhiteSpace` (cpp:227).
    pub(crate) fn is_white_space(&self) -> bool {
        self.tag == WHITE_SPACE_TAG
    }
    /// `IsEndOfInput` (cpp:228).
    pub(crate) fn is_end_of_input(&self) -> bool {
        self.tag == END_OF_INPUT_TAG
    }
    /// `IsKeyword` (cpp:229).
    pub(crate) fn is_keyword(&self) -> bool {
        self.tag >= KEYWORD_TAG_START
    }

    /// `length()` (cpp:231).
    pub(crate) fn length(&self) -> i32 {
        self.length
    }

    /// `number()` (cpp:233).
    pub(crate) fn number(&self) -> i32 {
        debug_assert!(self.is_number());
        self.value
    }
    /// `keyword_type()` (cpp:238).
    pub(crate) fn keyword_type(&self) -> KeywordType {
        debug_assert!(self.is_keyword());
        KeywordType::from_i32(self.tag)
    }
    /// `keyword_value()` (cpp:243).
    pub(crate) fn keyword_value(&self) -> i32 {
        debug_assert!(self.is_keyword());
        self.value
    }
    /// `symbol()` (cpp:248), o `char` do símbolo como byte ASCII.
    pub(crate) fn symbol(&self) -> u8 {
        debug_assert!(self.is_symbol());
        self.value as u8
    }
    /// `IsSymbol(char symbol)` (cpp:253).
    pub(crate) fn is_symbol_char(&self, symbol: u8) -> bool {
        self.is_symbol() && self.symbol() == symbol
    }
    /// `IsKeywordType` (cpp:257).
    pub(crate) fn is_keyword_type(&self, tag: KeywordType) -> bool {
        self.tag == tag as i32
    }
    /// `IsFixedLengthNumber` (cpp:258).
    pub(crate) fn is_fixed_length_number(&self, length: i32) -> bool {
        self.is_number() && self.length == length
    }
    /// `IsAsciiSign` (cpp:262).
    pub(crate) fn is_ascii_sign(&self) -> bool {
        self.tag == SYMBOL_TAG && (self.value == '-' as i32 || self.value == '+' as i32)
    }
    /// `ascii_sign` (cpp:266): 1 para '+' e -1 para '-'.
    pub(crate) fn ascii_sign(&self) -> i32 {
        debug_assert!(self.is_ascii_sign());
        44 - self.value
    }
    /// `IsKeywordZ` (cpp:271).
    pub(crate) fn is_keyword_z(&self) -> bool {
        self.is_keyword_type(KeywordType::TimeZoneName) && self.length == 1 && self.value == 0
    }
    /// `IsUnknown(int character)` (cpp:275).
    pub(crate) fn is_unknown_char(&self, character: i32) -> bool {
        self.is_unknown() && self.value == character
    }

    /// `DateToken::Keyword` (cpp:277).
    pub(crate) fn new_keyword(tag: KeywordType, value: i32, length: i32) -> DateToken {
        DateToken::new(tag as i32, length, value)
    }
    /// `DateToken::Number` (cpp:281).
    pub(crate) fn new_number(value: i32, length: i32) -> DateToken {
        DateToken::new(NUMBER_TAG, length, value)
    }
    /// `DateToken::Symbol` (cpp:285).
    pub(crate) fn new_symbol(symbol: u8) -> DateToken {
        DateToken::new(SYMBOL_TAG, 1, symbol as i32)
    }
    /// `DateToken::EndOfInput` (cpp:289).
    pub(crate) fn end_of_input() -> DateToken {
        DateToken::new(END_OF_INPUT_TAG, 0, -1)
    }
    /// `DateToken::WhiteSpace` (cpp:290).
    pub(crate) fn white_space(length: i32) -> DateToken {
        DateToken::new(WHITE_SPACE_TAG, length, -1)
    }
    /// `DateToken::Unknown` (cpp:294).
    pub(crate) fn unknown() -> DateToken {
        DateToken::new(UNKNOWN_TOKEN_TAG, 1, -1)
    }
    /// `DateToken::Invalid` (cpp:295).
    pub(crate) fn invalid() -> DateToken {
        DateToken::new(INVALID_TOKEN_TAG, 0, -1)
    }
}

/// `DateParser::InputReader` (cpp:127). Leitura básica e classificação de caracteres.
pub(crate) struct InputReader<'a, C: AsciiChar> {
    index: i32,
    buffer: &'a [C],
    ch: u32,
}

impl<'a, C: AsciiChar> InputReader<'a, C> {
    /// Construtor (cpp:129): o tamanho é o do slice, e já avança para o primeiro caractere.
    pub(crate) fn new(buffer: &'a [C]) -> Self {
        let mut reader = InputReader { index: 0, buffer, ch: 0 };
        reader.next();
        reader
    }

    /// `position` (cpp:137).
    pub(crate) fn position(&self) -> i32 {
        self.index
    }

    /// `Next` (cpp:140): avança para o próximo caractere (0 no fim).
    pub(crate) fn next(&mut self) {
        self.ch = if (self.index as usize) < self.buffer.len() {
            self.buffer[self.index as usize].to_u32()
        } else {
            0
        };
        self.index += 1;
    }

    /// `ReadUnsignedNumeral` (cpp:149): lê dígitos como número sem sinal, limitando o valor a
    /// `K_MAX_SIGNIFICANT_DIGITS` dígitos e pulando os restantes.
    pub(crate) fn read_unsigned_numeral(&mut self) -> i32 {
        let mut n: i32 = 0;
        let mut i = 0;
        // Primeiro, pula os zeros à esquerda.
        while self.ch == '0' as u32 {
            self.next();
        }
        // Depois, faz a conversão.
        while self.is_ascii_digit() {
            if i < K_MAX_SIGNIFICANT_DIGITS {
                n = n * 10 + self.ch as i32 - '0' as i32;
            }
            i += 1;
            self.next();
        }
        n
    }

    /// `ReadWord` (cpp:169): lê uma palavra (sequência de caracteres >= 'A'), preenche `prefix`
    /// com o prefixo em minúsculas e completa o resto com zeros. Devolve o comprimento da palavra.
    pub(crate) fn read_word(&mut self, prefix: &mut [u32]) -> i32 {
        let prefix_size = prefix.len() as i32;
        let mut len: i32 = 0;
        while self.is_ascii_alpha_or_above() && !self.is_white_space_char() {
            if len < prefix_size {
                prefix[len as usize] = ascii_alpha_to_lower(self.ch);
            }
            self.next();
            len += 1;
        }
        let mut i = len;
        while i < prefix_size {
            prefix[i as usize] = 0;
            i += 1;
        }
        len
    }

    /// `Skip` (cpp:183): devolve se de fato pulou algo.
    pub(crate) fn skip(&mut self, c: u32) -> bool {
        if self.ch == c {
            self.next();
            return true;
        }
        false
    }

    /// `SkipWhiteSpace` (cpp:906).
    pub(crate) fn skip_white_space(&mut self) -> bool {
        if is_ascii_whitespace(self.ch) || is_line_terminator(self.ch) {
            self.next();
            return true;
        }
        false
    }

    /// `SkipParentheses` (cpp:916): pula um grupo de parênteses aninhados.
    pub(crate) fn skip_parentheses(&mut self) -> bool {
        if self.ch != '(' as u32 {
            return false;
        }
        let mut balance: i32 = 0;
        loop {
            if self.ch == ')' as u32 {
                balance -= 1;
            } else if self.ch == '(' as u32 {
                balance += 1;
            }
            self.next();
            if !(balance > 0 && self.ch != 0) {
                break;
            }
        }
        true
    }

    /// `Is` (cpp:196).
    pub(crate) fn is(&self, c: u32) -> bool {
        self.ch == c
    }
    /// `IsEnd` (cpp:197).
    pub(crate) fn is_end(&self) -> bool {
        self.ch == 0
    }
    /// `IsAsciiDigit` (cpp:198). Dígitos não ASCII não são suportados.
    pub(crate) fn is_ascii_digit(&self) -> bool {
        is_decimal_digit(self.ch)
    }
    /// `IsAsciiAlphaOrAbove` (cpp:199).
    pub(crate) fn is_ascii_alpha_or_above(&self) -> bool {
        self.ch >= 'A' as u32
    }
    /// `IsWhiteSpaceChar` (cpp:200).
    pub(crate) fn is_white_space_char(&self) -> bool {
        is_ascii_whitespace(self.ch)
    }
    /// `IsAsciiSign` (cpp:201).
    pub(crate) fn is_ascii_sign(&self) -> bool {
        self.ch == '+' as u32 || self.ch == '-' as u32
    }
    /// `GetAsciiSignValue` (cpp:204): 1 para '+' e -1 para '-'.
    pub(crate) fn get_ascii_sign_value(&self) -> i32 {
        44 - self.ch as i32
    }
}

/// `DateParser::DateStringTokenizer` (cpp:320).
pub(crate) struct DateStringTokenizer<'r, 'a, C: AsciiChar> {
    in_: &'r mut InputReader<'a, C>,
    next: DateToken,
}

impl<'r, 'a, C: AsciiChar> DateStringTokenizer<'r, 'a, C> {
    /// Construtor (cpp:322): já escaneia o primeiro token.
    pub(crate) fn new(in_: &'r mut InputReader<'a, C>) -> Self {
        let next = Self::scan(in_);
        DateStringTokenizer { in_, next }
    }

    /// `Next` (cpp:327).
    pub(crate) fn next(&mut self) -> DateToken {
        let result = self.next;
        self.next = Self::scan(self.in_);
        result
    }

    /// `Peek` (cpp:334).
    pub(crate) fn peek(&self) -> DateToken {
        self.next
    }

    /// `SkipSymbol` (cpp:335).
    pub(crate) fn skip_symbol(&mut self, symbol: u8) -> bool {
        if self.next.is_symbol_char(symbol) {
            self.next = Self::scan(self.in_);
            return true;
        }
        false
    }

    /// `Scan` (cpp:867).
    fn scan(in_: &mut InputReader<'a, C>) -> DateToken {
        let pre_pos = in_.position();
        if in_.is_end() {
            return DateToken::end_of_input();
        }
        if in_.is_ascii_digit() {
            let n = in_.read_unsigned_numeral();
            let length = in_.position() - pre_pos;
            return DateToken::new_number(n, length);
        }
        if in_.skip(':' as u32) {
            return DateToken::new_symbol(b':');
        }
        if in_.skip('-' as u32) {
            return DateToken::new_symbol(b'-');
        }
        if in_.skip('+' as u32) {
            return DateToken::new_symbol(b'+');
        }
        if in_.skip('.' as u32) {
            return DateToken::new_symbol(b'.');
        }
        if in_.skip(')' as u32) {
            return DateToken::new_symbol(b')');
        }
        if in_.is_ascii_alpha_or_above() && !in_.is_white_space_char() {
            debug_assert!(KeywordTable::PREFIX_LENGTH == 3);
            let mut buffer = [0u32; 3];
            let length = in_.read_word(&mut buffer);
            let index = KeywordTable::lookup(&buffer, length);
            return DateToken::new_keyword(
                KeywordTable::get_type(index),
                KeywordTable::get_value(index),
                length,
            );
        }
        if in_.skip_white_space() {
            return DateToken::white_space(in_.position() - pre_pos);
        }
        if in_.skip_parentheses() {
            return DateToken::unknown();
        }
        in_.next();
        DateToken::unknown()
    }
}
