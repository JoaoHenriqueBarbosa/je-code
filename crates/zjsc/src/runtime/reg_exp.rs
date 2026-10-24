//! Tradução de `runtime/RegExp.{h,cpp}` e `RegExpCache.{h,cpp}`, a parte de criação e de consulta
//! do estado de construção: `RegExp::create` (com o `RegExpCache::lookupOrCreate`),
//! `createWithoutCaching`, `finishCreation`, `isValid`, `errorMessage`, `reset`, os acessores de
//! flags, padrão, átomo, sub-padrões e grupos nomeados.
//!
//! `match` entra como `match_ovector` (interpretador do Yarr, bytecode compilado sob demanda).
//!
//! Fora desta fatia, e por quê: `matchInline` com `MatchFrom`, `compile*`, o JIT do Yarr
//! (`YarrCodeBlock`) e o `m_firstCharacterBitmap` (dependem do `YarrInterpreter` ligado à
//! execução, de `JSGlobalObject` e do `StringView`), `errorToThrow`, `ensureGroupsStructure`,
//! `createFromCache`/`finishCreationFromCache` (cache de bytecode no disco), `dumpToStream`,
//! `estimatedSize`, `visitChildren`/`destroy` e o `m_strongCache`/`m_emptyRegExp` do `RegExpCache`
//! (só seguram a célula viva, e aqui nada é coletado).
//!
//! DIVERGÊNCIA (heap ausente, camada 3): `RegExp` é um `JSCell` no C++. Aqui é um valor compartilhado
//! por `Rc`, e o `JSValue::Cell(usize)` guarda o `cell_id` atribuído pelo registro central
//! (`cell_registry`), que mantém a célula viva. O `m_weakCache` do `RegExpCache` é um cache à parte:
//! a chave é (padrão, flags), como em `RegExpKey`, e o mesmo par devolve sempre a mesma célula (sem
//! coleta, a entrada nunca morre).
//! `vm.regExpStructure` não existe: a célula não carrega `Structure`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::vm::VM;
use crate::wtf::fixed_vector::FixedVector;
use crate::wtf::text::atom_string::{null_atom, AtomString};
use crate::wtf::text::wtf_string::String as WtfString;
use crate::yarr::yarr::{ExecutionMode, SpecificPattern};
use crate::yarr::yarr_error_code::{error_message, has_error, ErrorCode};
use crate::wtf::text::string_view::StringView;
use crate::yarr::yarr::OFFSET_NO_MATCH;
use crate::yarr::yarr_flags::{FlagSet, Flags};
use crate::yarr::yarr_interpreter::{byte_compile, interpret, BytecodePattern};
use crate::yarr::yarr_pattern::YarrPattern;

/// `RegExp::RegExpState`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegExpState {
    ParseError,
    JITCode,
    ByteCode,
    NotCompiled,
}

/// `RegExp::RareData` (sem o `m_cachedGroupsStructureID`, que é do `ensureGroupsStructure`).
struct RareData {
    num_duplicate_named_capture_groups: u32,
    capture_group_names: FixedVector<AtomString>,
    /// O primeiro elemento do vetor é o `subpatternId` no caso sem duplicata. No caso duplicado, o
    /// primeiro é o `namedCaptureGroupId` e os demais são os `subpatternId` de cada grupo duplicado.
    named_group_to_paren_indices: HashMap<WtfString, Vec<u32>>,
}

/// `class RegExp`.
pub struct RegExp {
    cell_id: usize,
    pattern_string: WtfString,
    atom: WtfString,
    state: RegExpState,
    specific_pattern: SpecificPattern,
    flags: FlagSet,
    construction_error_code: ErrorCode,
    num_subpatterns: u32,
    rare_data: Option<RareData>,
    ovector: FixedVector<i32>,
    /// `m_regExpBytecode`: o bytecode do Yarr, compilado na primeira execução (`byteCodeCompileIfNecessary`).
    bytecode: RefCell<Option<Rc<BytecodePattern>>>,
}

/// Referência compartilhada, o `RegExp*` do C++.
pub type RegExpRef = Rc<RegExp>;

thread_local! {
    /// `RegExpCache::m_weakCache`: do padrão para as células com ele (uma por conjunto de flags).
    static CACHE: RefCell<HashMap<WtfString, Vec<RegExpRef>>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): o `RegExpCache` vive no `VM` no C++, e cada
/// `RegExp` do cache é uma célula do programa.
pub(crate) fn reset_for_program() {
    let taken = CACHE.try_with(|cache| std::mem::take(&mut *cache.borrow_mut()));
    drop(taken);
}

impl RegExp {
    /// `RegExp::create(VM&, const String&, OptionSet<Yarr::Flags>)`: `vm.regExpCache()->lookupOrCreate`.
    pub fn create(vm: &VM, pattern_string: &WtfString, flags: FlagSet) -> RegExpRef {
        let cached = CACHE.with(|cache| {
            cache
                .borrow()
                .get(pattern_string)
                .and_then(|entries| entries.iter().find(|entry| entry.flags == flags).cloned())
        });
        if let Some(reg_exp) = cached {
            return reg_exp;
        }

        let reg_exp = RegExp::create_without_caching(vm, pattern_string, flags);
        CACHE.with(|cache| cache.borrow_mut().entry(pattern_string.clone()).or_default().push(Rc::clone(&reg_exp)));
        reg_exp
    }

    /// `RegExp::createWithoutCaching`: o construtor e o `finishCreation`.
    pub fn create_without_caching(_vm: &VM, pattern_string: &WtfString, flags: FlagSet) -> RegExpRef {
        debug_assert!(!flags.contains(Flags::DeletedValue));
        let mut reg_exp = RegExp {
            cell_id: 0,
            pattern_string: pattern_string.clone(),
            atom: WtfString::default(),
            state: RegExpState::NotCompiled,
            specific_pattern: SpecificPattern::None,
            flags,
            construction_error_code: ErrorCode::NoError,
            num_subpatterns: 0,
            rare_data: None,
            ovector: FixedVector::default(),
            bytecode: RefCell::new(None),
        };
        reg_exp.finish_creation();

        reg_exp.cell_id = cell_registry::reserve();
        let cell_id = reg_exp.cell_id;
        let reg_exp = Rc::new(reg_exp);
        cell_registry::set(cell_id, CellEntry::RegExp(Rc::clone(&reg_exp)));
        reg_exp
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`; `None` se o id não é de um `RegExp`.
    pub fn from_cell_id(cell_id: usize) -> Option<RegExpRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::RegExp(reg_exp)) => Some(reg_exp),
            _ => None,
        }
    }

    /// `RegExp::finishCreation(VM&)`.
    fn finish_creation(&mut self) {
        let mut pattern =
            YarrPattern::new(&self.pattern_string, self.flags, &mut self.construction_error_code, ExecutionMode::IncludeSubpatterns);
        if !self.is_valid() {
            self.state = RegExpState::ParseError;
            return;
        }

        self.atom = std::mem::take(&mut pattern.atom);
        self.specific_pattern = pattern.specific_pattern;

        self.num_subpatterns = pattern.num_subpatterns;
        if !pattern.capture_group_names.is_empty() || !pattern.named_group_to_paren_indices.is_empty() {
            let names = std::mem::take(&mut pattern.capture_group_names);
            self.rare_data = Some(RareData {
                num_duplicate_named_capture_groups: pattern.num_duplicate_named_capture_groups,
                capture_group_names: FixedVector::from_vec(names.iter().map(AtomString::from_string).collect()),
                named_group_to_paren_indices: std::mem::take(&mut pattern.named_group_to_paren_indices),
            });
        }

        let mut offset_vector_size = self.offset_vector_base_for_named_captures();
        if self.has_named_captures() {
            offset_vector_size += self.rare_data.as_ref().map_or(0, |rare| rare.num_duplicate_named_capture_groups);
        }
        self.ovector = FixedVector::filled(offset_vector_size as usize, &0);
    }

    /// Identidade da célula (o valor que `JSValue::from_cell` guarda).
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// `flags()`.
    pub fn flags(&self) -> FlagSet {
        self.flags
    }

    /// `pattern()`.
    pub fn pattern(&self) -> &WtfString {
        &self.pattern_string
    }

    /// `isValid()`.
    pub fn is_valid(&self) -> bool {
        !has_error(self.construction_error_code)
    }

    /// `errorMessage()`: o `ASCIILiteral` de `Yarr::errorMessage`, como fatia de bytes ASCII.
    pub fn error_message(&self) -> &'static [u8] {
        error_message(self.construction_error_code).as_bytes()
    }

    /// `reset()`.
    pub fn reset(&mut self) {
        self.state = RegExpState::NotCompiled;
        self.construction_error_code = ErrorCode::NoError;
    }

    /// `numSubpatterns()`.
    pub fn num_subpatterns(&self) -> u32 {
        self.num_subpatterns
    }

    /// `offsetVectorBaseForNamedCaptures()`.
    pub fn offset_vector_base_for_named_captures(&self) -> u32 {
        (self.num_subpatterns() + 1) * 2
    }

    /// `offsetVectorSize()`.
    pub fn offset_vector_size(&self) -> i32 {
        self.ovector.size() as i32
    }

    /// `hasNamedCaptures()`.
    pub fn has_named_captures(&self) -> bool {
        self.rare_data.as_ref().is_some_and(|rare| !rare.capture_group_names.is_empty())
    }

    /// `hasDuplicateNamedCaptureGroups()`.
    pub fn has_duplicate_named_capture_groups(&self) -> bool {
        self.rare_data.as_ref().is_some_and(|rare| rare.num_duplicate_named_capture_groups != 0)
    }

    /// `getCaptureGroupNameForSubpatternId(unsigned)`: `nullAtom()` quando não há nome.
    pub fn get_capture_group_name_for_subpattern_id(&self, i: u32) -> AtomString {
        match &self.rare_data {
            Some(rare) if i != 0 && !rare.capture_group_names.is_empty() => {
                rare.capture_group_names[i as usize].clone()
            }
            _ => null_atom(),
        }
    }

    /// `subpatternIdForGroupName(StringView, ovector)`: `ovector` indexa por `offsetVectorBaseForNamedCaptures`.
    pub fn subpattern_id_for_group_name(&self, group_name: &WtfString, ovector: &[i32]) -> u32 {
        let Some(rare) = &self.rare_data else {
            return 0;
        };
        let Some(indices) = rare.named_group_to_paren_indices.get(group_name) else {
            return 0;
        };
        if indices.len() == 1 {
            return indices[0];
        }

        ovector[(self.offset_vector_base_for_named_captures() + indices[0] - 1) as usize] as u32
    }

    /// `hasValidAtom()`.
    pub fn has_valid_atom(&self) -> bool {
        !self.atom.is_null()
    }

    /// `atom()`.
    pub fn atom(&self) -> &WtfString {
        &self.atom
    }

    /// `specificPattern()`.
    pub fn specific_pattern(&self) -> SpecificPattern {
        self.specific_pattern
    }

    /// `m_state`.
    pub fn state(&self) -> RegExpState {
        self.state
    }

    /// `hasIndices()`.
    pub fn has_indices(&self) -> bool {
        self.flags.contains(Flags::HasIndices)
    }

    /// `global()`.
    pub fn global(&self) -> bool {
        self.flags.contains(Flags::Global)
    }

    /// `sticky()`.
    pub fn sticky(&self) -> bool {
        self.flags.contains(Flags::Sticky)
    }

    /// `globalOrSticky()`.
    pub fn global_or_sticky(&self) -> bool {
        self.global() || self.sticky()
    }

    /// `byteCodeCompileIfNecessary`: compila o bytecode na primeira chamada. `None` é o erro de
    /// compilação (o `m_state = ParseError` do C++, que aqui não muta a célula compartilhada).
    fn bytecode_if_necessary(&self) -> Option<Rc<BytecodePattern>> {
        if let Some(bytecode) = self.bytecode.borrow().as_ref() {
            return Some(Rc::clone(bytecode));
        }
        if !self.is_valid() {
            return None;
        }
        let mut error_code = ErrorCode::NoError;
        let mut pattern = YarrPattern::new(&self.pattern_string, self.flags, &mut error_code, ExecutionMode::IncludeSubpatterns);
        if has_error(error_code) {
            return None;
        }
        let bytecode: Rc<BytecodePattern> = Rc::from(byte_compile(&mut pattern, &mut error_code)?);
        *self.bytecode.borrow_mut() = Some(Rc::clone(&bytecode));
        Some(bytecode)
    }

    /// `match(globalObject, input, startOffset, ovector)`: o vetor de offsets inteiro (tamanho
    /// `offsetVectorSize()`, `-1` nos grupos que não participaram) quando casa, e `None` quando o
    /// C++ devolve `-1`. Em `ovector[0]` fica a posição do casamento e em `ovector[1]` o fim.
    ///
    /// DIVERGÊNCIA: o `ovector` do C++ é o `m_ovector` da célula, reaproveitado a cada chamada; aqui
    /// cada chamada devolve o seu, porque a célula é compartilhada por `Rc` e imutável. O JIT do Yarr
    /// e o `matchInline` com `MatchFrom` não existem: tudo passa pelo interpretador.
    pub fn match_ovector(&self, input: StringView, start_offset: u32) -> Option<Vec<i32>> {
        // `matchInline`: só um padrão unicode sobre uma entrada de 16 bits precisa do ajuste de fronteira.
        if !(self.flags.contains(Flags::Unicode) || self.flags.contains(Flags::UnicodeSets)) || input.is_8bit() {
            return self.match_ovector_once(input, start_offset);
        }

        // `matchInlineAtCodePointBoundaries`: um início no meio de um par substituto recua um, e um casamento
        // que começa no meio de um par é descartado e a busca recomeça logo depois dele.
        let units = input.span16();
        let splits_surrogate_pair = |offset: u32| {
            let offset = offset as usize;
            offset != 0
                && offset < units.len()
                && (0xDC00..=0xDFFF).contains(&units[offset])
                && (0xD800..=0xDBFF).contains(&units[offset - 1])
        };
        let mut start_offset = start_offset;
        if splits_surrogate_pair(start_offset) {
            start_offset -= 1;
        }
        let mut result = self.match_ovector_once(input, start_offset);
        while let Some(ovector) = &result {
            let position = ovector[0];
            if position > start_offset as i32 && splits_surrogate_pair(position as u32) {
                start_offset = position as u32 + 1;
                result = self.match_ovector_once(input, start_offset);
            } else {
                break;
            }
        }
        result
    }

    /// `matchInlineOnce`: uma única tentativa do interpretador a partir de `start_offset`.
    fn match_ovector_once(&self, input: StringView, start_offset: u32) -> Option<Vec<i32>> {
        if start_offset > input.length() {
            return None;
        }
        let bytecode = self.bytecode_if_necessary()?;
        let mut output = vec![OFFSET_NO_MATCH; self.ovector.size()];
        let position = interpret(&bytecode, input, start_offset, &mut output);
        if position == OFFSET_NO_MATCH {
            return None;
        }
        debug_assert_eq!(output[0], position);
        Some(output.into_iter().map(|offset| offset as i32).collect())
    }
}
