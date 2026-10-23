// Fatia 1 de `yarr/YarrInterpreter.cpp` (linhas 1 a 600): `ByteTermDumper`, o `Interpreter<CharType>`
// (contextos de disjunção, `InputStream`, `testCharacterClass`, `checkCharacter`). Incluída por
// `include!` no fim de `yarr_interpreter.rs`, de modo que os `use` do topo daquele arquivo valem aqui.
//
// # CONTRATO UNIFICADO (fatias 1 a 6 seguem este modelo)
//
// - `DisjunctionContextRef` (`Copy`): `Disjunction(usize)` (índice em `disjunction_contexts`, o
//   `DisjunctionContext*` da raiz) ou `Parentheses(usize)` (o `getDisjunctionContext()` de
//   `parentheses_contexts[i]`). Todo `DisjunctionContext* context` do C++ vira `DisjunctionContextRef`.
// - Acesso: `self.context(ref) -> &DisjunctionContext`, `self.context_mut(ref) -> &mut DisjunctionContext`
//   (campos `term`, `match_begin`, `match_end`, `frame`), `self.frame(ref, slot) -> usize`,
//   `self.set_frame(ref, slot, valor)`.
// - `ParenthesesDisjunctionContext*` é `usize` (índice em `parentheses_contexts`), sem newtype.
// - `alloc_disjunction_context(&mut self, &ByteDisjunction) -> usize` (devolve índice; o chamador monta
//   `DisjunctionContextRef::Disjunction(i)`); `free_disjunction_context(&mut self, usize)`.
// - `alloc_parentheses_disjunction_context(&mut self, &ByteDisjunction, &ByteTerm) -> usize`;
//   `free_parentheses_disjunction_context(&mut self, usize)`.
// - `BackTrackInfoParentheses` é cópia local (`Copy`) com `begin`, `match_amount`, `last_context:
//   Option<usize>`, lida do quadro com `load(&frame, term.frame_location)` e gravada com `store`.
//   `Self::append_parentheses_disjunction_context(&mut bt, idx, &mut ctx)` e
//   `Self::pop_parentheses_disjunction_context(&mut bt, &self.parentheses_contexts)` são associadas.
// - `match_disjunction`/`match_non_zero_disjunction(&mut self, &ByteDisjunction, DisjunctionContextRef,
//   btrack: bool) -> JSRegExpResult`; `record_parentheses_match`/`reset_matches(&mut self, &ByteTerm,
//   usize)`; `parentheses_do_backtrack(&mut self, &ByteTerm, &mut BackTrackInfoParentheses)`.
//
// # Modelo de posse dos contextos
//
// O C++ aloca `DisjunctionContext` e `ParenthesesDisjunctionContext` em um `BumpPointerPool`
// (liberação em pilha: `dealloc(p)` descarta `p` e tudo que foi alocado depois). Aqui cada espécie de
// contexto vive em um `Vec` do `Interpreter`, endereçada por índice; `dealloc` vira `drain(índice..)` para a lista livre,
// com a mesma semântica de pilha. O `uintptr_t frame[1]` com `numberOfFrames` posições é um
// `Vec<usize>`. O `ParenthesesDisjunctionContext*` guardado como `uintptr_t` no frame
// (`BackTrackInfoParentheses::lastContext`) é `Option<usize>`, codificado no frame como índice + 1
// (0 é o ponteiro nulo). O `BitVector` de grupos nomeados duplicados é um `Vec<u32>` ordenado e sem
// repetição (a iteração em ordem crescente é a mesma; vazio não aloca). Os contextos liberados vão
// para listas livres (`free_*_contexts`) e voltam no próximo `alloc`, com `frame` e backup
// reaproveitados: a pilha não aloca nem solta memória por iteração de parênteses.
// `allocationSize` (cálculo de layout de memória), os números mágicos
// de `ASSERT_ENABLED` e `dump(PrintStream&)` não têm contrapartida observável e não são portados.

use crate::wtf::text::string_impl::CharType;
use crate::wtf::unicode::utf8_conversion::{u16_get_supplementary, u16_is_lead, u16_is_trail};
use crate::yarr::yarr::OFFSET_NO_MATCH;
use crate::yarr::yarr_pattern_cpp1::is_latin1;

/// `class ByteTermDumper`. As funções `dumpTerm` e `dumpDisjunction` ficam com o resto do `.cpp`.
pub struct ByteTermDumper<'a> {
    pub pattern: Option<&'a YarrPattern>,
    pub nesting: u32,
    pub line_indent: u32,
    pub compile_mode: CompileMode,
    pub recursive_dump: bool,
}

impl<'a> ByteTermDumper<'a> {
    /// `ByteTermDumper(YarrPattern* pattern = nullptr)`.
    pub fn new(pattern: Option<&'a YarrPattern>) -> ByteTermDumper<'a> {
        let mut dumper = ByteTermDumper {
            pattern,
            nesting: 0,
            line_indent: 0,
            compile_mode: CompileMode::Legacy,
            recursive_dump: false,
        };
        if let Some(pattern) = pattern {
            dumper.compile_mode = pattern.compile_mode();
        }
        dumper
    }

    /// `ByteTermDumper(CompileMode compileMode)`.
    pub fn with_compile_mode(compile_mode: CompileMode) -> ByteTermDumper<'a> {
        ByteTermDumper {
            pattern: None,
            nesting: 0,
            line_indent: 0,
            compile_mode,
            recursive_dump: false,
        }
    }

    pub fn unicode(&self) -> bool {
        self.compile_mode == CompileMode::Unicode
    }

    pub fn unicode_sets(&self) -> bool {
        self.compile_mode == CompileMode::UnicodeSets
    }

    pub fn either_unicode(&self) -> bool {
        self.unicode() || self.unicode_sets()
    }
}

/// `Interpreter<CharType>::errorCodePoint`.
pub const INTERPRETER_ERROR_CODE_POINT: u32 = 0xFFFF_FFFF;

/// `Interpreter<CharType>::verbose`.
pub const INTERPRETER_VERBOSE: bool = false;

/// `Interpreter<CharType>::BackTrackInfoParentheses`. No frame ocupa três palavras (`begin`,
/// `matchAmount`, `lastContext`), nesta ordem; `load` e `store` fazem a ida e a volta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackTrackInfoParentheses {
    pub begin: usize,
    pub match_amount: usize,
    pub last_context: Option<usize>,
}

impl BackTrackInfoParentheses {
    /// Lê a estrutura a partir de `frame[offset..offset + 3]`.
    pub fn load(frame: &[usize], offset: usize) -> BackTrackInfoParentheses {
        let raw = frame[offset + 2];
        BackTrackInfoParentheses {
            begin: frame[offset],
            match_amount: frame[offset + 1],
            last_context: if raw == 0 { None } else { Some(raw - 1) },
        }
    }

    /// Escreve a estrutura em `frame[offset..offset + 3]`.
    pub fn store(&self, frame: &mut [usize], offset: usize) {
        frame[offset] = self.begin;
        frame[offset + 1] = self.match_amount;
        frame[offset + 2] = match self.last_context {
            None => 0,
            Some(index) => index + 1,
        };
    }
}

/// `DisjunctionContext*` do C++: o contexto da raiz (índice em `disjunction_contexts`) ou o que vive
/// dentro de um `ParenthesesDisjunctionContext` (índice em `parentheses_contexts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisjunctionContextRef {
    Disjunction(usize),
    Parentheses(usize),
}

/// `Interpreter<CharType>::DisjunctionContext`.
#[derive(Clone, Debug, Default)]
pub struct DisjunctionContext {
    pub term: i32,
    pub match_begin: u32,
    pub match_end: u32,
    pub frame: Vec<usize>,
}

impl DisjunctionContext {
    /// `DisjunctionContext()` com o `frame` dimensionado para `numberOfFrames`.
    pub fn new(number_of_frames: u32) -> DisjunctionContext {
        DisjunctionContext {
            term: 0,
            match_begin: 0,
            match_end: 0,
            frame: vec![0; number_of_frames as usize],
        }
    }

    /// Reaproveita um contexto liberado: o `frame` mantém a capacidade e volta a ter exatamente
    /// `numberOfFrames` posições zeradas, sem alocar quando a capacidade já basta.
    pub fn reset(&mut self, number_of_frames: u32) {
        self.term = 0;
        self.match_begin = 0;
        self.match_end = 0;
        self.frame.clear();
        self.frame.resize(number_of_frames as usize, 0);
    }
}

/// `Interpreter<CharType>::ParenthesesDisjunctionContext`. O `DisjunctionContext` que o C++ constrói
/// logo após o objeto (`getDisjunctionContext()`) é o campo `disjunction_context`.
#[derive(Clone, Debug, Default)]
pub struct ParenthesesDisjunctionContext {
    pub next: Option<usize>,
    pub num_nested_subpatterns: u32,
    pub num_backup_ids: usize,
    /// O `BitVector m_duplicateNamedGroups` do C++: ids em ordem crescente e sem repetição (a
    /// iteração do `BitVector` é crescente). Vazio, não aloca.
    pub duplicate_named_groups: Vec<u32>,
    pub subpattern_and_group_id_backup: Vec<u32>,
    pub disjunction_context: DisjunctionContext,
}

impl ParenthesesDisjunctionContext {
    /// `ParenthesesDisjunctionContext(pattern, output, term, numDuplicateNamedGroups, duplicateNamedGroups)`
    /// sobre um contexto reaproveitado do pool (ou `Default`). O chamador já preencheu
    /// `duplicate_named_groups`; `num_duplicate_named_groups` é o seu comprimento. `frame_size` é o
    /// `m_frameSize` da disjunção do termo (o C++ o reserva depois do objeto).
    pub fn init(
        &mut self,
        pattern: &BytecodePattern,
        output: &mut [u32],
        term: &ByteTerm,
        frame_size: u32,
    ) {
        let num_nested_subpatterns = match term.atom.parentheses_disjunction {
            Some(id) => pattern.parentheses_disjunction(id).num_subpatterns,
            None => 0,
        };
        let num_backup_ids = (num_nested_subpatterns as usize) * 2 + self.duplicate_named_groups.len();
        let first_subpattern_id = term.subpattern_id() as usize;

        self.next = None;
        self.num_nested_subpatterns = num_nested_subpatterns;
        self.num_backup_ids = num_backup_ids;
        self.subpattern_and_group_id_backup.clear();
        self.subpattern_and_group_id_backup.resize(num_backup_ids, 0);
        self.disjunction_context.reset(frame_size);

        for i in 0..((num_nested_subpatterns as usize) << 1) {
            self.subpattern_and_group_id_backup[i] = output[(first_subpattern_id << 1) + i];
            output[(first_subpattern_id << 1) + i] = OFFSET_NO_MATCH;
        }

        for name_group_idx in 0..self.duplicate_named_groups.len() {
            let offset =
                pattern.offset_for_duplicate_named_group_id(self.duplicate_named_groups[name_group_idx]) as usize;
            let backup_offset = self.backup_offset_for_duplicate_named_group(name_group_idx);
            self.subpattern_and_group_id_backup[backup_offset] = output[offset];
            output[offset] = 0;
        }
    }

    /// `restoreOutput(output, firstSubpatternId)`.
    pub fn restore_output(&self, pattern: &BytecodePattern, output: &mut [u32], first_subpattern_id: u32) {
        for i in 0..((self.num_nested_subpatterns as usize) << 1) {
            output[((first_subpattern_id as usize) << 1) + i] = self.subpattern_and_group_id_backup[i];
        }

        let mut name_group_idx = 0;
        for &duplicate_named_group_id in self.duplicate_named_groups.iter() {
            output[pattern.offset_for_duplicate_named_group_id(duplicate_named_group_id) as usize] =
                self.subpattern_and_group_id_backup[self.backup_offset_for_duplicate_named_group(name_group_idx)];
            name_group_idx += 1;
        }
    }

    /// `backupOffsetForDuplicateNamedGroup(duplicateNamedGroup)`.
    pub fn backup_offset_for_duplicate_named_group(&self, duplicate_named_group: usize) -> usize {
        let offset = ((self.num_nested_subpatterns as usize) << 1) + duplicate_named_group;
        debug_assert!(offset < self.num_backup_ids);
        offset
    }
}

/// `Interpreter<CharType>::InputStream`. `input` é o `std::span` inteiro; `input.data()` e
/// `input.size()` viram a fatia e o comprimento.
pub struct InputStream<'a, C: CharType> {
    input: &'a [C],
    pos: u32,
    length: u32,
    decode_surrogate_pairs: bool,
}

impl<'a, C: CharType> InputStream<'a, C> {
    /// `InputStream(std::span<const CharType> input, unsigned start, bool decodeSurrogatePairs)`.
    pub fn new(input: &'a [C], start: u32, decode_surrogate_pairs: bool) -> InputStream<'a, C> {
        InputStream {
            input,
            pos: start,
            length: input.len() as u32,
            decode_surrogate_pairs,
        }
    }

    fn at(&self, index: u32) -> u32 {
        self.input[index as usize].into()
    }

    pub fn next(&mut self) {
        self.pos = self.pos.wrapping_add(1);
    }

    pub fn rewind(&mut self, amount: u32) {
        debug_assert!(self.pos >= amount);
        self.pos = self.pos.wrapping_sub(amount);
    }

    pub fn read(&self) -> u32 {
        debug_assert!(self.pos < self.length);
        if self.pos < self.length {
            return self.at(self.pos);
        }
        INTERPRETER_ERROR_CODE_POINT
    }

    pub fn read_checked(&mut self, negative_position_offset: u32) -> u32 {
        assert!(self.pos >= negative_position_offset);
        let p = self.pos - negative_position_offset;
        debug_assert!(p < self.length);
        let result = self.at(p);
        if u16_is_lead(result)
            && self.decode_surrogate_pairs
            && p + 1 < self.length
            && u16_is_trail(self.at(p + 1))
        {
            if self.at_end() {
                return INTERPRETER_ERROR_CODE_POINT;
            }
            self.next();
            return u16_get_supplementary(result, self.at(p + 1));
        } else if self.decode_surrogate_pairs && p > 0 && u16_is_trail(result) && u16_is_lead(self.at(p - 1)) {
            return INTERPRETER_ERROR_CODE_POINT;
        }
        result
    }

    pub fn read_checked_dont_advance(&self, negative_position_offset: u32) -> u32 {
        assert!(self.pos >= negative_position_offset);
        let p = self.pos - negative_position_offset;
        debug_assert!(p < self.length);
        let result = self.at(p);
        if u16_is_lead(result)
            && self.decode_surrogate_pairs
            && p + 1 < self.length
            && u16_is_trail(self.at(p + 1))
        {
            return u16_get_supplementary(result, self.at(p + 1));
        }
        if u16_is_trail(result) && self.decode_surrogate_pairs && p > 0 && u16_is_lead(self.at(p - 1)) {
            return INTERPRETER_ERROR_CODE_POINT;
        }
        result
    }

    /// `readForCharacterDump(negativePositionOffest)`: só para a macro `DUMP_CURR_CHAR`, sem o
    /// efeito colateral do `next()` de `readChecked`.
    pub fn read_for_character_dump(&self, negative_position_offset: u32) -> u32 {
        assert!(self.pos >= negative_position_offset);
        let p = self.pos - negative_position_offset;
        debug_assert!(p < self.length);
        let result = self.at(p);
        if u16_is_lead(result)
            && self.decode_surrogate_pairs
            && p + 1 < self.length
            && u16_is_trail(self.at(p + 1))
        {
            if self.at_end() {
                return INTERPRETER_ERROR_CODE_POINT;
            }
            return u16_get_supplementary(result, self.at(p + 1));
        }
        result
    }

    pub fn try_read_backward(&mut self, negative_position_offset: u32) -> u32 {
        if self.pos < negative_position_offset {
            return INTERPRETER_ERROR_CODE_POINT;
        }
        let p = self.pos - negative_position_offset;
        debug_assert!(p < self.length);
        let result = self.at(p);
        if u16_is_trail(result) && self.decode_surrogate_pairs && p > 0 && u16_is_lead(self.at(p - 1)) {
            self.rewind(1);
            return u16_get_supplementary(self.at(p - 1), result);
        }
        result
    }

    pub fn read_surrogate_pair_checked(&self, negative_position_offset: u32) -> u32 {
        assert!(self.pos >= negative_position_offset);
        let p = self.pos - negative_position_offset;
        debug_assert!(p < self.length);
        if p + 1 >= self.length {
            return INTERPRETER_ERROR_CODE_POINT;
        }
        let first = self.at(p);
        let second = self.at(p + 1);
        if u16_is_lead(first) && u16_is_trail(second) {
            return u16_get_supplementary(first, second);
        }
        INTERPRETER_ERROR_CODE_POINT
    }

    pub fn reread(&self, from: u32) -> u32 {
        debug_assert!(from < self.length);
        let result = self.at(from);
        if self.decode_surrogate_pairs {
            if u16_is_lead(result) && from + 1 < self.length && u16_is_trail(self.at(from + 1)) {
                return u16_get_supplementary(result, self.at(from + 1));
            }
            if u16_is_trail(result) && from > 0 && u16_is_lead(self.at(from - 1)) {
                return INTERPRETER_ERROR_CODE_POINT;
            }
        }
        result
    }

    pub fn prev(&self) -> u32 {
        debug_assert!(!(self.pos > self.length));
        if self.pos != 0 && self.length != 0 {
            return self.at(self.pos - 1);
        }
        INTERPRETER_ERROR_CODE_POINT
    }

    pub fn get_pos(&self) -> u32 {
        self.pos
    }

    pub fn set_pos(&mut self, p: u32) {
        self.pos = p;
    }

    /// `atStart()`.
    pub fn at_start(&self) -> bool {
        self.pos == 0
    }

    /// `atEnd()`.
    pub fn at_end(&self) -> bool {
        self.pos == self.length
    }

    pub fn end(&self) -> u32 {
        self.length
    }

    pub fn check_input(&mut self, count: u32) -> bool {
        // `(pos + count) <= length && (pos + count) >= pos` com a soma de `unsigned`.
        let sum = self.pos.wrapping_add(count);
        if sum <= self.length && sum >= self.pos {
            self.pos = sum;
            return true;
        }
        false
    }

    pub fn uncheck_input(&mut self, count: u32) {
        assert!(self.pos >= count);
        self.pos -= count;
    }

    pub fn try_uncheck_input(&mut self, count: u32) -> bool {
        if count > self.pos {
            return false;
        }
        self.pos -= count;
        true
    }

    /// `atStart(unsigned negativePositionOffset)`.
    pub fn at_start_with_offset(&self, negative_position_offset: u32) -> bool {
        self.pos == negative_position_offset
    }

    /// `atEnd(unsigned negativePositionOffest)`.
    pub fn at_end_with_offset(&self, negative_position_offset: u32) -> bool {
        assert!(self.pos >= negative_position_offset);
        (self.pos - negative_position_offset) == self.length
    }

    pub fn is_available_input(&self, offset: u32) -> bool {
        let sum = self.pos.wrapping_add(offset);
        sum <= self.length && sum >= self.pos
    }

    pub fn is_valid_negative_input_offset(&self, offset: u32) -> bool {
        self.pos >= offset && (self.pos - offset) < self.length
    }
}

/// `Options::maxRegExpStackSize()` (192 MB, `runtime/OptionsList.h`).
const MAX_REG_EXP_STACK_SIZE: usize = 192 * 1024 * 1024;

/// `std::midpoint(low, high)` para `size_t` (arredonda em direção a `low`).
fn size_midpoint(low: usize, high: usize) -> usize {
    low + (high - low) / 2
}

/// A lambda `linearSearchMatches` de `testCharacterClass`.
fn linear_search_matches(ch: u32, matches: &[u32]) -> bool {
    matches.iter().any(|&candidate| ch == candidate)
}

/// A lambda `binarySearchMatches` de `testCharacterClass`.
fn binary_search_matches(ch: u32, matches: &[u32]) -> bool {
    let mut low: usize = 0;
    let mut high: usize = matches.len().wrapping_sub(1);

    while low <= high {
        let mid = size_midpoint(low, high);
        let diff = ch.wrapping_sub(matches[mid]) as i32;
        if diff == 0 {
            return true;
        }

        if diff < 0 {
            if mid == low {
                return false;
            }
            high = mid - 1;
        } else {
            low = mid + 1;
        }
    }
    false
}

/// A lambda `linearSearchRanges` de `testCharacterClass`.
fn linear_search_ranges(ch: u32, ranges: &[crate::yarr::yarr_pattern::CharacterRange]) -> bool {
    ranges.iter().any(|range| ch >= range.begin && ch <= range.end)
}

/// A lambda `binarySearchRanges` de `testCharacterClass`.
fn binary_search_ranges(ch: u32, ranges: &[crate::yarr::yarr_pattern::CharacterRange]) -> bool {
    let mut low: usize = 0;
    let mut high: usize = ranges.len().wrapping_sub(1);

    while low <= high {
        let mid = size_midpoint(low, high);
        let range_begin_diff = ch.wrapping_sub(ranges[mid].begin) as i32;
        if range_begin_diff >= 0 && ch <= ranges[mid].end {
            return true;
        }

        if range_begin_diff < 0 {
            if mid == low {
                return false;
            }
            high = mid - 1;
        } else {
            low = mid + 1;
        }
    }
    false
}

/// `Interpreter<CharType>`. Os membros `pattern`, `compileMode`, `output`, `input`, `allocatorPool`,
/// `startOffset`, `noNewlineBefore` e `remainingMatchCount` do C++ (o `StackCheck` fica com a fatia
/// que define `isSafeToRecurse`). `allocatorPool` vira as duas pilhas de contextos.
pub struct Interpreter<'a, C: CharType> {
    pub pattern: &'a BytecodePattern,
    pub compile_mode: CompileMode,
    pub output: &'a mut [u32],
    pub input: InputStream<'a, C>,
    pub disjunction_contexts: Vec<DisjunctionContext>,
    pub parentheses_contexts: Vec<ParenthesesDisjunctionContext>,
    /// Contextos liberados à espera de reuso (o espaço que o `BumpPointerPool` devolve ao `dealloc`
    /// e entrega de novo ao próximo `alloc`): alocar de novo reaproveita `frame` e backup.
    pub free_disjunction_contexts: Vec<DisjunctionContext>,
    pub free_parentheses_contexts: Vec<ParenthesesDisjunctionContext>,
    pub start_offset: u32,
    pub no_newline_before: u32,
    pub remaining_match_count: u32,
    /// Bytes vivos nos contextos de retrocesso, contra `MAX_REG_EXP_STACK_SIZE` (o `BumpPointerPool`
    /// do C++ com `Options::maxRegExpStackSize()`).
    pub context_bytes: usize,
    /// `m_stackCheck`.
    pub stack_check: crate::yarr::yarr_pattern_cpp1::StackCheck,
}

impl<'a, C: CharType> Interpreter<'a, C> {
    /// `Interpreter::bolUnsatisfiable`.
    pub const BOL_UNSATISFIABLE: u32 = u32::MAX;

    /// `DisjunctionContext*` desreferenciado (leitura).
    pub fn context(&self, context: DisjunctionContextRef) -> &DisjunctionContext {
        match context {
            DisjunctionContextRef::Disjunction(index) => &self.disjunction_contexts[index],
            DisjunctionContextRef::Parentheses(index) => &self.parentheses_contexts[index].disjunction_context,
        }
    }

    /// `DisjunctionContext*` desreferenciado (escrita).
    pub fn context_mut(&mut self, context: DisjunctionContextRef) -> &mut DisjunctionContext {
        match context {
            DisjunctionContextRef::Disjunction(index) => &mut self.disjunction_contexts[index],
            DisjunctionContextRef::Parentheses(index) => {
                &mut self.parentheses_contexts[index].disjunction_context
            }
        }
    }

    /// `context->frame[slot]`.
    pub fn frame(&self, context: DisjunctionContextRef, slot: usize) -> usize {
        self.context(context).frame[slot]
    }

    /// `context->frame[slot] = value`.
    pub fn set_frame(&mut self, context: DisjunctionContextRef, slot: usize, value: usize) {
        self.context_mut(context).frame[slot] = value;
    }

    /// `appendParenthesesDisjunctionContext(backTrack, context)`: `context` é o contexto de índice
    /// `context_index` no vetor de contextos de parênteses.
    pub fn append_parentheses_disjunction_context(
        back_track: &mut BackTrackInfoParentheses,
        context_index: usize,
        context: &mut ParenthesesDisjunctionContext,
    ) {
        context.next = back_track.last_context;
        back_track.last_context = Some(context_index);
        back_track.match_amount += 1;
    }

    /// `popParenthesesDisjunctionContext(backTrack)`.
    pub fn pop_parentheses_disjunction_context(
        back_track: &mut BackTrackInfoParentheses,
        contexts: &[ParenthesesDisjunctionContext],
    ) {
        assert!(back_track.match_amount != 0);
        let last_context = back_track.last_context;
        assert!(last_context.is_some());
        back_track.last_context = last_context.and_then(|index| contexts[index].next);
        back_track.match_amount -= 1;
    }

    /// `allocDisjunctionContext(disjunction)`: devolve o índice do novo contexto. A alocação do Rust
    /// não falha, então o ramo `nullptr` do `ensureCapacity` não existe.
    pub fn alloc_disjunction_context(&mut self, disjunction: &ByteDisjunction) -> usize {
        self.context_bytes += Self::disjunction_context_bytes(disjunction.frame_size as usize);
        let context = match self.free_disjunction_contexts.pop() {
            Some(mut recycled) => {
                recycled.reset(disjunction.frame_size);
                recycled
            }
            None => DisjunctionContext::new(disjunction.frame_size),
        };
        self.disjunction_contexts.push(context);
        self.disjunction_contexts.len() - 1
    }

    /// `DisjunctionContext::allocationSize(numberOfFrames)` (build de release, alinhado a 8 bytes).
    fn disjunction_context_bytes(number_of_frames: usize) -> usize {
        16 + number_of_frames * std::mem::size_of::<usize>()
    }

    /// `ParenthesesDisjunctionContext::allocationSize(numBackupIds)` (build de release, alinhado a 8
    /// bytes), sem o `DisjunctionContext` que vem depois.
    fn parentheses_context_bytes(num_backup_ids: usize) -> usize {
        (24 + num_backup_ids * 4).next_multiple_of(8)
    }

    /// `freeDisjunctionContext(context)`: `dealloc` descarta o contexto e tudo que veio depois.
    pub fn free_disjunction_context(&mut self, context: usize) {
        for freed in self.disjunction_contexts.drain(context..) {
            self.context_bytes -= Self::disjunction_context_bytes(freed.frame.len());
            self.free_disjunction_contexts.push(freed);
        }
    }

    /// `allocParenthesesDisjunctionContext(disjunction, output, term)`: devolve o índice do novo
    /// contexto (o `output` é o do próprio interpretador), ou `None` quando o orçamento de
    /// `Options::maxRegExpStackSize` (a pilha de contextos do C++, `ensureCapacity` nulo) estoura;
    /// o chamador devolve `ErrorNoMemory`.
    pub fn alloc_parentheses_disjunction_context(
        &mut self,
        disjunction: &ByteDisjunction,
        term: &ByteTerm,
    ) -> Option<usize> {
        let pattern = self.pattern;
        let mut context = self.free_parentheses_contexts.pop().unwrap_or_default();
        context.duplicate_named_groups.clear();
        let first_subpattern_id = term.subpattern_id();
        let num_nested_subpatterns = match term.atom.parentheses_disjunction {
            Some(id) => pattern.parentheses_disjunction(id).num_subpatterns,
            None => 0,
        };

        if pattern.has_duplicate_named_capture_groups() {
            for i in 0..num_nested_subpatterns {
                let subpattern_id = first_subpattern_id + i;
                let duplicate_named_group =
                    pattern.duplicate_named_group_for_subpattern_id[subpattern_id as usize];
                if duplicate_named_group != 0 {
                    context.duplicate_named_groups.push(duplicate_named_group);
                }
            }
            // Ordem crescente e sem repetição, como a iteração do `BitVector`.
            context.duplicate_named_groups.sort_unstable();
            context.duplicate_named_groups.dedup();
        }
        let num_duplicate_named_groups = context.duplicate_named_groups.len() as u32;

        let size = Self::parentheses_context_bytes(
            (num_nested_subpatterns as usize) * 2 + num_duplicate_named_groups as usize,
        ) + Self::disjunction_context_bytes(disjunction.frame_size as usize);
        // O limite é conferido antes de `init`, que já salva e zera o `output`; no C++ o
        // `ensureCapacity` falha antes de construir o contexto.
        if self.context_bytes.saturating_add(size) > MAX_REG_EXP_STACK_SIZE {
            self.free_parentheses_contexts.push(context);
            return None;
        }
        self.context_bytes += size;

        context.init(pattern, &mut *self.output, term, disjunction.frame_size);
        self.parentheses_contexts.push(context);
        Some(self.parentheses_contexts.len() - 1)
    }

    /// `freeParenthesesDisjunctionContext(context)`.
    pub fn free_parentheses_disjunction_context(&mut self, context: usize) {
        for freed in self.parentheses_contexts.drain(context..) {
            self.context_bytes -= Self::parentheses_context_bytes(freed.num_backup_ids)
                + Self::disjunction_context_bytes(freed.disjunction_context.frame.len());
            self.free_parentheses_contexts.push(freed);
        }
    }

    /// `testCharacterClass(characterClass, ch)`.
    pub fn test_character_class(character_class: &CharacterClass, ch: u32) -> bool {
        if character_class.any_character {
            return true;
        }

        if let Some(table) = character_class.table {
            if ch < CharacterClass::TABLE_SIZE {
                return table[ch as usize] != 0;
            }
        }

        if let Some(latin1_table) = &character_class.latin1_table {
            if is_latin1(ch) {
                return latin1_table.data[ch as usize] != 0;
            }
        }

        const THRESHOLD_FOR_BINARY_SEARCH: usize = 6;

        if !is_latin1(ch) {
            if !character_class.matches32.is_empty() {
                if character_class.matches32.len() > THRESHOLD_FOR_BINARY_SEARCH {
                    if binary_search_matches(ch, &character_class.matches32) {
                        return true;
                    }
                } else if linear_search_matches(ch, &character_class.matches32) {
                    return true;
                }
            }

            if !character_class.ranges32.is_empty() {
                if character_class.ranges32.len() > THRESHOLD_FOR_BINARY_SEARCH {
                    if binary_search_ranges(ch, &character_class.ranges32) {
                        return true;
                    }
                } else if linear_search_ranges(ch, &character_class.ranges32) {
                    return true;
                }
            }
        } else {
            if !character_class.matches8.is_empty() {
                if character_class.matches8.len() > THRESHOLD_FOR_BINARY_SEARCH {
                    if binary_search_matches(ch, &character_class.matches8) {
                        return true;
                    }
                } else if linear_search_matches(ch, &character_class.matches8) {
                    return true;
                }
            }

            if !character_class.ranges8.is_empty() {
                if character_class.ranges8.len() > THRESHOLD_FOR_BINARY_SEARCH {
                    if binary_search_ranges(ch, &character_class.ranges8) {
                        return true;
                    }
                } else if linear_search_ranges(ch, &character_class.ranges8) {
                    return true;
                }
            }
        }

        false
    }

    /// `checkCharacter(term, negativeInputOffset)`.
    pub fn check_character(&mut self, term: &ByteTerm, negative_input_offset: u32) -> bool {
        debug_assert!(term.is_character_type());
        if term.match_direction() == MatchDirection::Forward {
            return term.pattern_character() == self.input.read_checked(negative_input_offset);
        }

        term.pattern_character() == self.input.try_read_backward(negative_input_offset)
    }
}
