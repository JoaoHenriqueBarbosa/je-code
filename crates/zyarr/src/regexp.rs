//! API segura sobre o YARR: o `RegExp` do JavaScript sobre texto UTF-16.
//!
//! Cobre o que o `RegExp` do JSC faz entre o construtor (`RegExp::create`, `finishCreation`) e o
//! `matchInline`: compilar padrão e flags, devolver o vetor de capturas e resolver os grupos
//! nomeados (inclusive os nomes duplicados do ES2025). A lógica de `lastIndex` com `g` e `y`
//! (`RegExpBuiltinExec`) fica no chamador: ele lê [`Flags`], passa o `lastIndex` como `start` e
//! grava o `end` do resultado de volta.

use std::fmt;
use std::ops::Range;

use crate::wtf::text::string_view::StringView;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::yarr::yarr::{ExecutionMode, OFFSET_NO_MATCH};
use crate::yarr::yarr_error_code::{ErrorCode, ErrorType, error_message, error_to_throw_type, has_error};
use crate::yarr::yarr_flags::{FlagSet, Flags as YarrFlag, parse_flags};
use crate::yarr::yarr_interpreter::{BytecodePattern, byte_compile, interpret};
use crate::yarr::yarr_pattern::YarrPattern;

/// A mensagem do JSC para flags inválidas (`RegExpConstructor.cpp`, como o zjsc a emite).
const INVALID_FLAGS_MESSAGE: &str = "Invalid flags supplied to RegExp constructor.";

/// O tipo do erro que o JSC lançaria.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// `SyntaxError`: padrão ou flags inválidos.
    SyntaxError,
    /// `RangeError`/OOME no JSC: o padrão excede o que o compilador aguenta (`TooManyDisjunctions`).
    OutOfMemory,
}

/// Erro de compilação, com a mensagem do JSC (por exemplo `Invalid regular expression: missing )`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegExpError {
    kind: ErrorKind,
    message: &'static str,
}

impl RegExpError {
    fn from_code(code: ErrorCode) -> RegExpError {
        let kind = match error_to_throw_type(code) {
            Some(ErrorType::OutOfMemoryError) => ErrorKind::OutOfMemory,
            _ => ErrorKind::SyntaxError,
        };
        RegExpError { kind, message: error_message(code) }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// A mensagem do JSC, sem prefixo de tipo (`SyntaxError: ` não faz parte dela).
    pub fn message(&self) -> &'static str {
        self.message
    }
}

impl fmt::Display for RegExpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl std::error::Error for RegExpError {}

/// As flags `d g i m s u v y` de um `RegExp`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// `d`: o resultado carrega os índices (`hasIndices`).
    pub has_indices: bool,
    /// `g`
    pub global: bool,
    /// `i`
    pub ignore_case: bool,
    /// `m`
    pub multiline: bool,
    /// `s`
    pub dot_all: bool,
    /// `u`
    pub unicode: bool,
    /// `v`
    pub unicode_sets: bool,
    /// `y`: [`RegExp::exec`] só casa exatamente em `start`.
    pub sticky: bool,
}

impl Flags {
    /// `parseFlags` do YARR: rejeita letra desconhecida, repetida e `u` junto com `v`.
    pub fn parse(flags: &[u16]) -> Option<Flags> {
        parse_flags(flags).map(Flags::from_set)
    }

    fn from_set(set: FlagSet) -> Flags {
        Flags {
            has_indices: set.contains(YarrFlag::HasIndices),
            global: set.contains(YarrFlag::Global),
            ignore_case: set.contains(YarrFlag::IgnoreCase),
            multiline: set.contains(YarrFlag::Multiline),
            dot_all: set.contains(YarrFlag::DotAll),
            unicode: set.contains(YarrFlag::Unicode),
            unicode_sets: set.contains(YarrFlag::UnicodeSets),
            sticky: set.contains(YarrFlag::Sticky),
        }
    }

    /// `u` ou `v`: o casamento é por ponto de código.
    pub fn either_unicode(&self) -> bool {
        self.unicode || self.unicode_sets
    }

    /// A propriedade `flags` do `RegExp`: as letras em ordem alfabética (`dgimsuvy`).
    pub fn to_flags_string(&self) -> String {
        let letters = [
            (self.has_indices, 'd'),
            (self.global, 'g'),
            (self.ignore_case, 'i'),
            (self.multiline, 'm'),
            (self.dot_all, 's'),
            (self.unicode, 'u'),
            (self.unicode_sets, 'v'),
            (self.sticky, 'y'),
        ];
        letters.iter().filter(|(on, _)| *on).map(|(_, letter)| *letter).collect()
    }
}

/// Um casamento. Todos os índices são em unidades de código UTF-16 sobre a entrada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// `captures[0]` é o casamento inteiro (sempre `Some`); `captures[n]` é o grupo `n`, `None` se
    /// não participou.
    pub captures: Vec<Option<Range<usize>>>,
    /// Os grupos nomeados na ordem da primeira ocorrência de cada nome. Com nomes duplicados
    /// (`/(?<a>x)|(?<a>y)/`) vale o grupo que participou, como no `groups` do JS.
    pub groups: Vec<(String, Option<Range<usize>>)>,
}

impl Match {
    /// `index` do resultado do JS.
    pub fn start(&self) -> usize {
        self.captures[0].as_ref().map_or(0, |range| range.start)
    }

    /// O `lastIndex` que o `RegExpBuiltinExec` grava com `g` ou `y`.
    pub fn end(&self) -> usize {
        self.captures[0].as_ref().map_or(0, |range| range.end)
    }

    /// O grupo nomeado `name`, se existir e tiver participado.
    pub fn group(&self, name: &str) -> Option<Range<usize>> {
        self.groups.iter().find(|(group, _)| group == name).and_then(|(_, range)| range.clone())
    }
}

/// Um `RegExp` compilado. Imutável depois de criado; `exec` não guarda estado.
#[derive(Debug)]
pub struct RegExp {
    flags: Flags,
    bytecode: Box<BytecodePattern>,
    num_subpatterns: u32,
    /// O nome de cada grupo, indexado por `número - 1` (`None` para o grupo sem nome).
    capture_names: Vec<Option<String>>,
    /// Quantidade de grupos nomeados repetidos (`numDuplicateNamedCaptureGroups`).
    num_duplicate_named_capture_groups: u32,
    /// `named_group_to_paren_indices`, só preenchido quando há nome duplicado.
    duplicate_name_indices: Vec<(String, Vec<u32>)>,
    /// `offsetVectorBaseForNamedCaptures`.
    offset_vector_base: u32,
}

/// O texto de um `WTF::String` como `String` do Rust (Latin1 direto, UTF-16 com substituição).
fn wtf_to_string(text: &WtfString) -> String {
    if text.is_8bit() {
        text.span8().iter().map(|&byte| byte as char).collect()
    } else {
        String::from_utf16_lossy(text.span16())
    }
}

impl RegExp {
    /// `new RegExp(pattern, flags)` sobre texto UTF-16.
    pub fn new(pattern: &[u16], flags: &[u16]) -> Result<RegExp, RegExpError> {
        let flag_set = parse_flags(flags)
            .ok_or(RegExpError { kind: ErrorKind::SyntaxError, message: INVALID_FLAGS_MESSAGE })?;
        let pattern_string = WtfString::from_utf16(pattern);

        let mut code = ErrorCode::NoError;
        let mut yarr_pattern = YarrPattern::new(&pattern_string, flag_set, &mut code, ExecutionMode::IncludeSubpatterns);
        if has_error(code) {
            return Err(RegExpError::from_code(code));
        }

        let num_subpatterns = yarr_pattern.num_subpatterns;
        // O YARR só preenche `capture_group_names` (índice 0 reservado) quando há grupo nomeado.
        let mut capture_names = yarr_pattern
            .capture_group_names
            .iter()
            .skip(1)
            .map(|name| (!name.is_empty()).then(|| wtf_to_string(name)))
            .collect::<Vec<_>>();
        capture_names.resize(num_subpatterns as usize, None);
        let num_duplicate_named_capture_groups = yarr_pattern.num_duplicate_named_capture_groups;
        let duplicate_name_indices = if num_duplicate_named_capture_groups == 0 {
            Vec::new()
        } else {
            yarr_pattern
                .named_group_to_paren_indices
                .iter()
                .map(|(name, indices)| (wtf_to_string(name), indices.clone()))
                .collect()
        };

        let bytecode = byte_compile(&mut yarr_pattern, &mut code);
        if has_error(code) {
            return Err(RegExpError::from_code(code));
        }
        let bytecode = bytecode.ok_or(RegExpError::from_code(ErrorCode::TooManyDisjunctions))?;
        let offset_vector_base = (num_subpatterns + 1) * 2;

        Ok(RegExp {
            flags: Flags::from_set(flag_set),
            bytecode,
            num_subpatterns,
            capture_names,
            num_duplicate_named_capture_groups,
            duplicate_name_indices,
            offset_vector_base,
        })
    }

    /// Atalho para texto UTF-8 (converte para UTF-16).
    pub fn from_str(pattern: &str, flags: &str) -> Result<RegExp, RegExpError> {
        let pattern: Vec<u16> = pattern.encode_utf16().collect();
        let flags: Vec<u16> = flags.encode_utf16().collect();
        RegExp::new(&pattern, &flags)
    }

    pub fn flags(&self) -> Flags {
        self.flags
    }

    /// Quantos grupos de captura o padrão tem (sem contar o casamento inteiro).
    pub fn capture_count(&self) -> usize {
        self.num_subpatterns as usize
    }

    /// O nome de cada grupo de captura (`número - 1`), `None` para os sem nome.
    pub fn capture_names(&self) -> &[Option<String>] {
        &self.capture_names
    }

    pub fn has_named_captures(&self) -> bool {
        self.capture_names.iter().any(Option::is_some)
    }

    /// Procura o primeiro casamento que começa em `start` ou depois (`matchInline` do JSC). Com a
    /// flag `y` só vale o casamento que começa exatamente em `start`, e `g` não muda nada aqui: o
    /// chamador decide de onde partir e o que fazer com [`Match::end`].
    ///
    /// Em `u` e `v` um `start` no meio de um par substituto recua uma unidade, e um casamento que
    /// começaria no meio de um par é descartado, como no `matchInlineAtCodePointBoundaries`.
    ///
    /// `None` quando não casa, quando `start > input.len()` ou quando a entrada não cabe em `u32`.
    /// O limite de passos do interpretador (`MATCH_LIMIT`) também dá `None`, como no JSC.
    pub fn exec(&self, input: &[u16], start: usize) -> Option<Match> {
        if start > input.len() || u32::try_from(input.len()).is_err() {
            return None;
        }
        let mut start = start as u32;
        if !self.flags.either_unicode() {
            return self.exec_once(input, start);
        }

        let splits_surrogate_pair = |offset: u32| {
            let offset = offset as usize;
            offset != 0
                && offset < input.len()
                && (0xDC00..=0xDFFF).contains(&input[offset])
                && (0xD800..=0xDBFF).contains(&input[offset - 1])
        };
        if splits_surrogate_pair(start) {
            start -= 1;
        }
        let mut result = self.exec_once(input, start);
        while let Some(found) = &result {
            let position = found.start() as u32;
            if position > start && splits_surrogate_pair(position) {
                start = position + 1;
                result = self.exec_once(input, start);
            } else {
                break;
            }
        }
        result
    }

    /// `matchInlineOnce`: uma tentativa do interpretador.
    fn exec_once(&self, input: &[u16], start: u32) -> Option<Match> {
        if start as usize > input.len() {
            return None;
        }
        let size = (self.offset_vector_base + self.num_duplicate_named_capture_groups) as usize;
        let mut output = vec![OFFSET_NO_MATCH; size];
        let position = interpret(&self.bytecode, StringView::from(input), start, &mut output);
        if position == OFFSET_NO_MATCH {
            return None;
        }

        let range_of = |index: usize| {
            let (begin, end) = (output[2 * index], output[2 * index + 1]);
            (begin != OFFSET_NO_MATCH && end != OFFSET_NO_MATCH && end >= begin).then_some(begin as usize..end as usize)
        };
        let captures: Vec<Option<Range<usize>>> = (0..=self.capture_count()).map(range_of).collect();

        let mut groups: Vec<(String, Option<Range<usize>>)> = Vec::new();
        for (index, name) in self.capture_names.iter().enumerate() {
            let Some(name) = name else { continue };
            let mut capture_index = index as u32 + 1;
            if self.num_duplicate_named_capture_groups != 0 {
                capture_index = self.subpattern_id_for_group_name(name, &output);
            }
            let value = if capture_index > 0 { captures[capture_index as usize].clone() } else { None };
            match groups.iter_mut().find(|(existing, _)| existing == name) {
                Some(slot) => slot.1 = value,
                None => groups.push((name.clone(), value)),
            }
        }
        Some(Match { captures, groups })
    }

    /// `RegExp::subpatternIdForGroupName`.
    fn subpattern_id_for_group_name(&self, name: &str, output: &[u32]) -> u32 {
        let Some((_, indices)) = self.duplicate_name_indices.iter().find(|(group, _)| group == name) else {
            return 0;
        };
        if indices.len() == 1 {
            return indices[0];
        }
        output[(self.offset_vector_base + indices[0] - 1) as usize]
    }
}
