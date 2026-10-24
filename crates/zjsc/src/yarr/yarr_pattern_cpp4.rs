// Porte de `yarr/YarrPattern.cpp`, quarta fatia (linhas 2060 a 2755): do `quantifyAtom` até as
// constantes `alternationFactoringMinRun` e `alternationWrapMinRunWhenFrameFree` da
// `YarrPatternConstructor`. Fatia incluída por `include!`, sem `use` no nível do módulo (os `use`
// locais de função não colidem com nada).
//
// Campos usados da `YarrPatternConstructor`:
//   `pattern` (`&mut YarrPattern`), `alternative` (`AlternativeId`, o `m_alternative`),
//   `forward_references_in_lookbehind` (`Vec<UnresolvedForwardReference>`), `error` (`ErrorCode`).
// Métodos usados e definidos em outras fatias: `parenthesis_match_direction`, `is_safe_to_recurse`,
// `copy_disjunction(DisjunctionId, bool) -> Option<DisjunctionId>`,
// `copy_term(PatternTerm, bool) -> Option<PatternTerm>`, `class_contains_code_point`
// (associada, devolve `TriState`) e `parentheses_must_match` (associada).
// `UnresolvedForwardReference`: `new(alternative, term_index)`,
// `with_named_group(alternative, term_index, named_group)`, `alternative()`, `term_index()`,
// `has_named_group()`, `named_group()`. O `term()` do C++ (ponteiro) se compara por
// (alternativa, índice).
//
// `quantify_atom`, `disjunction`, `aborted_due_to_error` e `abort_error_code` são os métodos do
// `Delegate`, aqui como inerentes. O `recomputeStartsWithBOL` sobrecarregado vira
// `recompute_starts_with_bol_disjunction(id)` e `recompute_starts_with_bol()`.

/// `CheckedUint32` (`Checked<uint32_t, RecordOverflow>`): o estouro fica registrado.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CheckedUint32 {
    value: u32,
    overflowed: bool,
}

impl CheckedUint32 {
    pub(crate) fn new(value: u32) -> Self {
        CheckedUint32 { value, overflowed: false }
    }

    pub(crate) fn value(&self) -> u32 {
        self.value
    }

    pub(crate) fn has_overflowed(&self) -> bool {
        self.overflowed
    }
}

impl std::ops::AddAssign<u32> for CheckedUint32 {
    fn add_assign(&mut self, rhs: u32) {
        match self.value.checked_add(rhs) {
            Some(value) => self.value = value,
            None => self.overflowed = true,
        }
    }
}

impl std::ops::AddAssign<CheckedUint32> for CheckedUint32 {
    fn add_assign(&mut self, rhs: CheckedUint32) {
        *self += rhs.value;
        self.overflowed |= rhs.overflowed;
    }
}

impl std::ops::MulAssign<u32> for CheckedUint32 {
    fn mul_assign(&mut self, rhs: u32) {
        match self.value.checked_mul(rhs) {
            Some(value) => self.value = value,
            None => self.overflowed = true,
        }
    }
}

impl std::ops::Sub<u32> for CheckedUint32 {
    type Output = CheckedUint32;

    fn sub(self, rhs: u32) -> CheckedUint32 {
        match self.value.checked_sub(rhs) {
            Some(value) => CheckedUint32 { value, overflowed: self.overflowed },
            None => CheckedUint32 { value: self.value.wrapping_sub(rhs), overflowed: true },
        }
    }
}

impl YarrPatternConstructor<'_> {




    /// `setupAlternativeOffsets`.
    pub(crate) fn setup_alternative_offsets(
        &mut self,
        alt: crate::yarr::yarr_pattern::AlternativeId,
        mut current_call_frame_size: CheckedUint32,
        initial_input_position: u32,
        new_call_frame_size: &mut CheckedUint32,
    ) -> crate::yarr::yarr_error_code::ErrorCode {
        use crate::yarr::yarr::{
            YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_BACK_REFERENCE,
            YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_CHARACTER_CLASS, YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES,
            YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_ONCE, YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_TERMINAL,
            YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHETICAL_ASSERTION, YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PATTERN_CHARACTER,
            YARR_STACK_SPACE_FOR_DOT_STAR_ENCLOSURE,
        };
        use crate::yarr::yarr_error_code::{has_error, ErrorCode};
        use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType, QuantifierType};

        if !self.is_safe_to_recurse() {
            return ErrorCode::TooManyDisjunctions;
        }

        let mut error = ErrorCode::NoError;
        self.pattern.alternative_mut(alt).has_fixed_size = true;
        let mut current_input_position = CheckedUint32::new(initial_input_position);

        let mut i = 0usize;
        while i < self.pattern.alternative(alt).terms.len() {
            let mut term = self.pattern.alternative(alt).terms[i];

            // Grava o termo de volta e devolve o código de erro.
            macro_rules! fail {
                ($code:expr) => {{
                    self.pattern.alternative_mut(alt).terms[i] = term;
                    return $code;
                }};
            }

            match term.type_ {
                PatternTermType::AssertionBOL | PatternTermType::AssertionEOL | PatternTermType::AssertionWordBoundary => {
                    term.input_position = current_input_position.value();
                }

                PatternTermType::NumberedBackReference | PatternTermType::NamedBackReference => {
                    term.input_position = current_input_position.value();
                    term.frame_location = current_call_frame_size.value();
                    current_call_frame_size += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_BACK_REFERENCE;
                    if current_call_frame_size.has_overflowed() {
                        fail!(ErrorCode::FrameTooLarge);
                    }
                    self.pattern.alternative_mut(alt).has_fixed_size = false;
                }

                PatternTermType::NumberedForwardReference | PatternTermType::NamedForwardReference => {}

                PatternTermType::PatternCharacter => {
                    term.input_position = current_input_position.value();
                    if term.quantity_type != QuantifierType::FixedCount {
                        term.frame_location = current_call_frame_size.value();
                        current_call_frame_size += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PATTERN_CHARACTER;
                        if current_call_frame_size.has_overflowed() {
                            fail!(ErrorCode::FrameTooLarge);
                        }
                        self.pattern.alternative_mut(alt).has_fixed_size = false;
                    } else if self.pattern.either_unicode() {
                        let mut temp_count = CheckedUint32::new(term.quantity_max_count);
                        // U16_LENGTH
                        temp_count *= if term.pattern_character() <= 0xffff { 1 } else { 2 };
                        if temp_count.has_overflowed() {
                            fail!(ErrorCode::OffsetTooLarge);
                        }
                        current_input_position += temp_count;
                    } else {
                        current_input_position += term.quantity_max_count;
                    }
                }

                PatternTermType::CharacterClass => {
                    term.input_position = current_input_position.value();
                    if term.quantity_type != QuantifierType::FixedCount {
                        term.frame_location = current_call_frame_size.value();
                        current_call_frame_size += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_CHARACTER_CLASS;
                        if current_call_frame_size.has_overflowed() {
                            fail!(ErrorCode::FrameTooLarge);
                        }
                        self.pattern.alternative_mut(alt).has_fixed_size = false;
                    } else if self.pattern.either_unicode() {
                        term.frame_location = current_call_frame_size.value();
                        current_call_frame_size += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_CHARACTER_CLASS;
                        if current_call_frame_size.has_overflowed() {
                            fail!(ErrorCode::FrameTooLarge);
                        }
                        let character_class = self.pattern.character_class(term.character_class());
                        if character_class.has_one_character_size() && !term.invert() {
                            let mut temp_count = CheckedUint32::new(term.quantity_max_count);
                            temp_count *= if character_class.has_non_bmp_characters() { 2 } else { 1 };
                            if temp_count.has_overflowed() {
                                fail!(ErrorCode::OffsetTooLarge);
                            }
                            current_input_position += temp_count;
                        } else {
                            current_input_position += term.quantity_max_count;
                            self.pattern.alternative_mut(alt).has_fixed_size = false;
                        }
                    } else {
                        current_input_position += term.quantity_max_count;
                    }
                }

                PatternTermType::ParenthesesSubpattern => {
                    // Nota: para parênteses fixos de uma vez, garantimos pelo menos o mínimo
                    // disponível; os demais se viram sozinhos.
                    term.frame_location = current_call_frame_size.value();
                    let disjunction = term.parentheses().disjunction;
                    if term.quantity_max_count == 1 && !term.parentheses().is_copy {
                        current_call_frame_size += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_ONCE;
                        if current_call_frame_size.has_overflowed() {
                            fail!(ErrorCode::FrameTooLarge);
                        }
                        let initial = current_call_frame_size;
                        error = self.setup_disjunction_offsets(disjunction, initial, current_input_position.value(), &mut current_call_frame_size);
                        if has_error(error) {
                            fail!(error);
                        }
                        // Se a quantidade é fixa, confere previamente o tamanho mínimo.
                        if term.quantity_type == QuantifierType::FixedCount {
                            current_input_position += self.pattern.disjunction(disjunction).minimum_size;
                        }
                        term.input_position = current_input_position.value();
                    } else if term.parentheses().is_terminal {
                        current_call_frame_size += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_TERMINAL;
                        if current_call_frame_size.has_overflowed() {
                            fail!(ErrorCode::FrameTooLarge);
                        }
                        let initial = current_call_frame_size;
                        error = self.setup_disjunction_offsets(disjunction, initial, current_input_position.value(), &mut current_call_frame_size);
                        if has_error(error) {
                            fail!(error);
                        }
                        term.input_position = current_input_position.value();
                    } else {
                        term.input_position = current_input_position.value();
                        current_call_frame_size += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES;
                        if current_call_frame_size.has_overflowed() {
                            fail!(ErrorCode::FrameTooLarge);
                        }
                        let initial = current_call_frame_size;
                        error = self.setup_disjunction_offsets(disjunction, initial, current_input_position.value(), &mut current_call_frame_size);
                        if has_error(error) {
                            fail!(error);
                        }
                        // O JIT salva os slots interiores do frame deste grupo [base+4,
                        // m_callFrameSize) em um ParenContext com índices relativos a base+4, então a
                        // área de frame salvo só precisa comportar o maior tamanho entre todos os
                        // grupos repetitivos.
                        let inner_frame_base = term.frame_location.wrapping_add(YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES);
                        let call_frame_size = self.pattern.disjunction(disjunction).call_frame_size;
                        debug_assert!(call_frame_size >= inner_frame_base);
                        self.pattern.max_paren_context_frame_size = self
                            .pattern
                            .max_paren_context_frame_size
                            .max(call_frame_size.wrapping_sub(inner_frame_base));
                    }
                    // Contagem fixa de 1 poderia ser aceita se tiverem tamanho fixo *E* todas as
                    // alternativas tiverem o mesmo comprimento.
                    self.pattern.alternative_mut(alt).has_fixed_size = false;
                }

                PatternTermType::ParentheticalAssertion => {
                    let disjunction_initial_input_position =
                        if term.match_direction() == MatchDirection::Forward { current_input_position.value() } else { 0 };
                    term.input_position = current_input_position.value();
                    term.frame_location = current_call_frame_size.value();
                    current_call_frame_size += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHETICAL_ASSERTION;
                    if current_call_frame_size.has_overflowed() {
                        fail!(ErrorCode::FrameTooLarge);
                    }
                    let disjunction = term.parentheses().disjunction;
                    let initial = current_call_frame_size;
                    error = self.setup_disjunction_offsets(disjunction, initial, disjunction_initial_input_position, &mut current_call_frame_size);
                    if has_error(error) {
                        fail!(error);
                    }
                }

                PatternTermType::DotStarEnclosure => {
                    debug_assert!(!self.pattern.save_initial_start_value);
                    self.pattern.alternative_mut(alt).has_fixed_size = false;
                    term.input_position = initial_input_position;
                    self.pattern.initial_start_value_frame_location = current_call_frame_size.value();
                    current_call_frame_size += YARR_STACK_SPACE_FOR_DOT_STAR_ENCLOSURE;
                    if current_call_frame_size.has_overflowed() {
                        fail!(ErrorCode::FrameTooLarge);
                    }
                    self.pattern.save_initial_start_value = true;
                }
            }
            if current_input_position.has_overflowed() {
                fail!(ErrorCode::OffsetTooLarge);
            }
            self.pattern.alternative_mut(alt).terms[i] = term;
            i += 1;
        }

        self.pattern.alternative_mut(alt).minimum_size = (current_input_position - initial_input_position).value();
        *new_call_frame_size = CheckedUint32::new(current_call_frame_size.value());
        error
    }

    /// `setupDisjunctionOffsets`.
    pub(crate) fn setup_disjunction_offsets(
        &mut self,
        disjunction: crate::yarr::yarr_pattern::DisjunctionId,
        mut initial_call_frame_size: CheckedUint32,
        initial_input_position: u32,
        call_frame_size: &mut CheckedUint32,
    ) -> crate::yarr::yarr_error_code::ErrorCode {
        use crate::yarr::yarr::YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_ALTERNATIVE;
        use crate::yarr::yarr_error_code::{has_error, ErrorCode};
        use crate::yarr::yarr_pattern::AlternativeId;

        if !self.is_safe_to_recurse() {
            return ErrorCode::TooManyDisjunctions;
        }

        let alternatives_len = self.pattern.disjunction(disjunction).alternatives.len();
        if disjunction != self.pattern.body && alternatives_len > 1 {
            initial_call_frame_size += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_ALTERNATIVE;
            if initial_call_frame_size.has_overflowed() {
                return ErrorCode::FrameTooLarge;
            }
        }

        let share_offsets = disjunction == self.pattern.body;

        let mut minimum_input_size = u32::MAX;
        let mut maximum_call_frame_size = 0u32;
        let mut has_fixed_size = true;
        let mut error = ErrorCode::NoError;

        let mut per_alternative_initial = initial_call_frame_size;
        for alt in 0..alternatives_len {
            let alternative = AlternativeId { disjunction, index: alt as u32 };
            let mut current_alternative_call_frame_size = CheckedUint32::new(0);
            error = self.setup_alternative_offsets(alternative, per_alternative_initial, initial_input_position, &mut current_alternative_call_frame_size);
            if has_error(error) {
                return error;
            }
            let alternative = self.pattern.alternative(alternative);
            minimum_input_size = minimum_input_size.min(alternative.minimum_size);
            maximum_call_frame_size = maximum_call_frame_size.max(current_alternative_call_frame_size.value());
            has_fixed_size &= alternative.has_fixed_size;
            if alternative.minimum_size > i32::MAX as u32 {
                self.pattern.contains_unsigned_length_pattern = true;
            }
            if !share_offsets {
                per_alternative_initial = current_alternative_call_frame_size;
            }
        }

        debug_assert!(maximum_call_frame_size >= initial_call_frame_size.value());

        let target = self.pattern.disjunction_mut(disjunction);
        target.has_fixed_size = has_fixed_size;
        target.minimum_size = minimum_input_size;
        target.call_frame_size = maximum_call_frame_size;
        *call_frame_size = CheckedUint32::new(maximum_call_frame_size);
        error
    }

    /// `setupOffsets`.
    pub fn setup_offsets(&mut self) -> crate::yarr::yarr_error_code::ErrorCode {
        // FIXME do C++: Yarr não deveria usar a pilha para tratar subpadrões (rdar://problem/26436314).
        let mut ignored_call_frame_size = CheckedUint32::new(0);
        let body = self.pattern.body;
        self.setup_disjunction_offsets(body, CheckedUint32::new(0), 0, &mut ignored_call_frame_size)
    }

    /// Os termos são só `PatternCharacter` de contagem fixa 1 (o `isPureCharacterSequence` do C++).
    fn is_pure_character_sequence(inner_terms: &[crate::yarr::yarr_pattern::PatternTerm]) -> bool {
        use crate::yarr::yarr_pattern::{PatternTermType, QuantifierType};

        for inner_term in inner_terms {
            if inner_term.type_ != PatternTermType::PatternCharacter
                || inner_term.quantity_type != QuantifierType::FixedCount
                || inner_term.quantity_max_count != 1
            {
                return false;
            }
        }
        true
    }

    /// O `unwrapSingleGroup` do C++: ao ignorar capturas, uma alternativa que é exatamente um grupo de
    /// uma vez envolvendo uma string fixa pura (por exemplo "(t0)") equivale a essa string. Devolve a
    /// disjunção envolvida para o chamador achatar a alternativa nos termos de caractere do grupo;
    /// `None` se a forma não casa.
    fn unwrap_single_group(
        pattern: &crate::yarr::yarr_pattern::YarrPattern,
        inner_terms: &[crate::yarr::yarr_pattern::PatternTerm],
    ) -> Option<crate::yarr::yarr_pattern::DisjunctionId> {
        use crate::yarr::yarr_pattern::{PatternTermType, QuantifierType};

        if inner_terms.len() != 1 {
            return None;
        }

        let only = &inner_terms[0];
        if only.type_ != PatternTermType::ParenthesesSubpattern
            || only.quantity_type != QuantifierType::FixedCount
            || only.quantity_min_count != 1
            || only.quantity_max_count != 1
            || only.parentheses().is_copy
        {
            return None;
        }

        let inner = only.parentheses().disjunction;
        let inner_disjunction = pattern.disjunction(inner);
        if inner_disjunction.alternatives.len() != 1 {
            return None;
        }

        // Deixa um grupo de captura vazio (por exemplo "()") para o caminho genérico: achatá-lo em
        // uma sequência vazia o esconderia da contabilidade de alternativa vazia abaixo, que varre
        // os termos antes do achatamento.
        if inner_disjunction.alternatives[0].terms.is_empty() {
            return None;
        }

        if !Self::is_pure_character_sequence(&inner_disjunction.alternatives[0].terms) {
            return None;
        }

        Some(inner)
    }

    // Esta otimização identifica conjuntos de parênteses em que nunca precisaremos retroceder.
    // Nesses casos não precisamos guardar o estado das iterações anteriores.
    // Hoje podemos evitar o retrocesso:
    //   * quando os parênteses estão no fim da expressão regular (último termo de qualquer das
    //     alternativas da disjunção do corpo principal);
    //   * quando os parênteses não capturam e são quantificados gulosos sem limite (*);
    //   * quando os parênteses não contêm subpadrões de captura;
    //   * quando os parênteses contêm um subpadrão sem captura ancorado em BOL com uma única
    //     alternativa de strings fixas, por exemplo /^(?:foo|bar|baz). Nesse caso simplificamos um
    //     pouco mais o casamento parando na primeira alternativa de string casada, sem saltar para o
    //     retrocesso por causa de ajuste de offsets. Em vez disso ajustamos os offsets, se preciso,
    //     no topo do código JIT de casamento da alternativa seguinte.
    /// `checkForTerminalParentheses`.
    pub fn check_for_terminal_parentheses(&mut self) {
        use crate::yarr::yarr::ExecutionMode;
        use crate::yarr::yarr_pattern::{AlternativeId, PatternTermType, QuantifierType};
        use crate::yarr::yarr::QUANTIFY_INFINITE;

        // Em padrões só de casamento os resultados de captura nunca são observados, então a
        // otimização de lista de strings pode rodar mesmo com subpadrões de captura declarados, e um
        // grupo de captura envolvendo uma string fixa (por exemplo "(t0)") pode ser tratado como essa
        // string. Referências para trás e grupos nomeados ainda podem observar capturas mesmo no modo
        // só de casamento, então mantém a saída conservadora para eles.
        let ignore_captures = self.pattern.execution_mode != ExecutionMode::IncludeSubpatterns
            && !self.pattern.contains_backreferences
            && !self.pattern.has_named_capture_groups
            && self.pattern.num_duplicate_named_capture_groups == 0
            && !self.pattern.contains_lookbehinds;

        let body = self.pattern.body;
        let alternatives_len = self.pattern.disjunction(body).alternatives.len();
        let has_observable_captures = self.pattern.num_subpatterns != 0 && !ignore_captures;
        if !has_observable_captures {
            self.pattern.disjunction_mut(body).alternatives[alternatives_len - 1].is_last_alternative = true;

            if alternatives_len == 1 && self.pattern.disjunction(body).alternatives[0].starts_with_bol {
                let first = AlternativeId { disjunction: body, index: 0 };
                let terms = self.pattern.alternative(first).terms.clone();

                let mut is_string_list = false;

                if terms.len() >= 2
                    && terms[0].type_ == PatternTermType::AssertionBOL
                    && terms[1].type_ == PatternTermType::ParenthesesSubpattern
                    && terms[1].quantity_type == QuantifierType::FixedCount
                    && terms[1].quantity_max_count == 1
                    && !terms[1].parentheses().is_copy
                    && (terms.len() == 2
                        || (terms.len() == 3 && terms[2].type_ == PatternTermType::AssertionEOL && !self.pattern.multiline()))
                {
                    // Começamos supondo que é uma lista de strings e provamos o contrário.
                    is_string_list = true;

                    let nested_disjunction = terms[1].parentheses().disjunction;
                    const EMPTY_ALTERNATIVE_NOT_FOUND: u32 = u32::MAX;
                    let mut first_empty_alternative = EMPTY_ALTERNATIVE_NOT_FOUND;
                    let nested_len = self.pattern.disjunction(nested_disjunction).alternatives.len();

                    // Passo 1: confirma que toda alternativa é uma string fixa (possivelmente depois de
                    // enxergar através de um único grupo envolvente). Ainda não altera a árvore, para
                    // que uma alternativa que não é string, vista tarde, nunca deixe a árvore
                    // reescrita pela metade.
                    for alt in 0..nested_len {
                        if !is_string_list {
                            break;
                        }
                        let inner_terms = &self.pattern.disjunction(nested_disjunction).alternatives[alt].terms;

                        if inner_terms.is_empty() && first_empty_alternative == EMPTY_ALTERNATIVE_NOT_FOUND {
                            first_empty_alternative = alt as u32;
                        }

                        if Self::is_pure_character_sequence(inner_terms) {
                            continue;
                        }
                        if ignore_captures && Self::unwrap_single_group(&self.pattern, inner_terms).is_some() {
                            continue;
                        }
                        is_string_list = false;
                    }

                    // Passo 2: agora que a lista inteira está confirmada, acha as alternativas de grupo
                    // envolvido trocando o termo de grupo por uma cópia dos termos de caractere dele.
                    if is_string_list && ignore_captures {
                        for alt in 0..nested_len {
                            let alternative_terms = &self.pattern.disjunction(nested_disjunction).alternatives[alt].terms;
                            if let Some(inner) = Self::unwrap_single_group(&self.pattern, alternative_terms) {
                                let flattened = self.pattern.disjunction(inner).alternatives[0].terms.clone();
                                self.pattern.disjunction_mut(nested_disjunction).alternatives[alt].terms = flattened;
                            }
                        }
                    }

                    let is_eol_string_list = terms.len() == 3 && terms[2].type_ == PatternTermType::AssertionEOL;
                    {
                        let parentheses = self.pattern.alternative_mut(first).terms[1].parentheses_mut();
                        parentheses.is_string_list = is_string_list;
                        parentheses.is_eol_string_list = is_eol_string_list;
                    }

                    // Numa lista de strings sem EOL, a primeira alternativa vazia sempre casa e
                    // encerra o casamento, então as seguintes são inalcançáveis. Descarta-as para que a
                    // alternativa vazia seja a última e o JIT possa cair direto no sucesso.
                    if is_string_list
                        && !is_eol_string_list
                        && first_empty_alternative != EMPTY_ALTERNATIVE_NOT_FOUND
                        && ((first_empty_alternative + 1) as usize) < nested_len
                    {
                        let nested = self.pattern.disjunction_mut(nested_disjunction);
                        nested.alternatives.truncate(first_empty_alternative as usize + 1);
                        if let Some(last) = nested.alternatives.last_mut() {
                            last.is_last_alternative = true;
                        }
                    }
                }

                if is_string_list {
                    return;
                }
            }
        }

        for alt in 0..alternatives_len {
            let alternative = AlternativeId { disjunction: body, index: alt as u32 };
            let terms = &mut self.pattern.alternative_mut(alternative).terms;
            if let Some(term) = terms.last_mut() {
                // Parênteses sem captura gulosos * ou + na posição final não precisam de estado de
                // retrocesso entre iterações.
                // 1. Caso * (term.quantityMinCount == 0)
                //
                // Um padrão como /(?:AA|A)*/ nunca pode falhar, porque * tem sucesso mesmo com zero
                // iterações. E como está na posição final, todo retrocesso vem da tentativa gulosa de
                // casar (?:AA|A). Isso significa que nunca restauraremos o estado da iteração anterior
                // para explorar outro casamento. Quando falhamos na iteração atual, podemos dizer "o
                // casamento terminou", porque estamos na posição final.
                //
                // 2. Caso + (term.quantityMinCount == 1)
                //
                // Um padrão como /(?:AA|A)+/ precisa casar ao menos uma vez. Se falhamos nesta 1ª
                // iteração, como é a inicial, não temos contexto a restaurar e simplesmente falhamos.
                // Se casamos uma vez e na segunda iteração falhamos, também não precisamos restaurar a
                // 1ª, pois ela já teve sucesso (e + basta); como é a posição final, podemos dizer "o
                // casamento terminou".
                //
                // Resultado: os casos term.quantityMinCount <= 1 nunca exigem um ParenContext por
                // iteração quando temos esses padrões gulosos na posição final. Por isso os marcamos
                // `isTerminal`, para pular a geração de ParenContext como otimização.
                //
                // Já term.quantityMinCount >= 2 não funciona assim. Seja uma RegExp
                // `/(?:AA|A){2,}/.exec("AA")`. A resposta certa é "AA", pois "A" e "A" casam com 2
                // iterações. Mas isso se obtém restaurando a 1ª iteração e tomando outra alternativa
                // "A" no lugar de "AA". Logo o estado de retrocesso entre iterações é necessário.
                if term.type_ == PatternTermType::ParenthesesSubpattern
                    && term.quantity_type == QuantifierType::Greedy
                    && term.quantity_min_count <= 1
                    && term.quantity_max_count == QUANTIFY_INFINITE
                    && !term.capture()
                    && !term.contains_any_captures()
                {
                    term.parentheses_mut().is_terminal = true;
                }
            }
        }
    }

    // Otimização de auto-possessificação
    //
    // "Quantificador possessivo" é mais um tipo de quantificador suportado em motores de RegExp que
    // não são de JS (por exemplo PCRE2): /a++/, /a*+/. Difere do guloso (/a+/, /a*/) porque nunca
    // retrocede. Depois de casar gulosamente, mesmo que o resto do padrão falhe, não retrocedemos.
    // Por exemplo /.*+b/ com "textb": em /.*b/ o .* retrocede para poupar o "b" ao resto do padrão,
    // mas o possessivo drena toda a entrada e nunca retrocede, então o casamento falha.
    //
    // O benefício é eliminar o custo do retrocesso, de modo que a falha fica rápida.
    //
    // A RegExp de JS não tem quantificadores possessivos, mas podemos suportá-los internamente e
    // converter padrões para possessivo se o retrocesso deste termo nunca produzir casamentos
    // potenciais para o resto do padrão.
    //
    // Um termo guloso `T` de um só caractere seguido imediatamente de um termo obrigatório `U` cujo
    // primeiro caractere nunca pode ser um caractere que `T` casa é efetivamente possessivo. Depois
    // que `T` casou gulosamente, devolver caracteres nunca deixa `U` casar (uma posição devolvida
    // ainda tem um caractere de `T`, que `U` rejeita), então todo o retrocesso em `T` é inútil.
    // Marcamos esse `T` para o JIT pular a geração (e a execução) desse retrocesso morto.

    /// O `termMatchesCharacter` do C++ (lambda).
    fn term_matches_character(
        pattern: &crate::yarr::yarr_pattern::YarrPattern,
        term: &crate::yarr::yarr_pattern::PatternTerm,
        ch: u32,
    ) -> crate::parser::source_tainted_origin::TriState {
        use crate::parser::source_tainted_origin::TriState;
        use crate::wtf::ascii_ctype::{is_ascii, to_ascii_upper};
        use crate::yarr::yarr_pattern::PatternTermType;

        if term.type_ == PatternTermType::PatternCharacter {
            let pc = term.pattern_character();
            if pc == ch {
                return TriState::True;
            }
            if term.ignore_case() {
                if !is_ascii(pc) || !is_ascii(ch) {
                    return TriState::Indeterminate;
                }
                if to_ascii_upper(pc) == to_ascii_upper(ch) {
                    return TriState::True;
                }
            }
            return TriState::False;
        }

        debug_assert!(term.type_ == PatternTermType::CharacterClass);
        let raw = Self::class_contains_code_point(pattern.character_class(term.character_class()), ch);
        if raw == TriState::Indeterminate {
            return TriState::Indeterminate;
        }
        if term.invert() {
            return if raw == TriState::True { TriState::False } else { TriState::True };
        }
        raw
    }

    /// O `followerForcesPossessive` do C++ (lambda): verdadeiro se, e somente se, `next` (o termo logo
    /// depois do termo guloso) é obrigatório e seu primeiro caractere é provadamente disjunto do
    /// conjunto de `greedy`.
    fn follower_forces_possessive(
        pattern: &crate::yarr::yarr_pattern::YarrPattern,
        greedy: &crate::yarr::yarr_pattern::PatternTerm,
        next: &crate::yarr::yarr_pattern::PatternTerm,
    ) -> bool {
        use crate::parser::source_tainted_origin::TriState;
        use crate::wtf::ascii_ctype::{is_ascii, to_ascii_lower, to_ascii_upper};
        use crate::yarr::yarr_pattern::{PatternTermType, QuantifierType};

        if next.type_ != PatternTermType::PatternCharacter {
            return false;
        }

        if next.quantity_type != QuantifierType::FixedCount || next.quantity_min_count < 1 {
            return false;
        }

        // Todo caractere que `next` aceitaria precisa ser rejeitado por `greedy`.
        let fc = next.pattern_character();
        if Self::term_matches_character(pattern, greedy, fc) != TriState::False {
            return false;
        }

        if next.ignore_case() {
            if !is_ascii(fc) {
                return false; // Não raciocina sobre dobras de caixa fora do ASCII.
            }

            if Self::term_matches_character(pattern, greedy, to_ascii_upper(fc)) != TriState::False {
                return false;
            }

            if Self::term_matches_character(pattern, greedy, to_ascii_lower(fc)) != TriState::False {
                return false;
            }
        }
        true
    }

    /// `optimizePossessiveQuantifiers`.
    pub fn optimize_possessive_quantifiers(&mut self) {
        use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType, QuantifierType};

        let is_possessifiable_greedy_term = |term: &crate::yarr::yarr_pattern::PatternTerm| -> bool {
            term.quantity_type == QuantifierType::Greedy
                && (term.type_ == PatternTermType::PatternCharacter || term.type_ == PatternTermType::CharacterClass)
        };

        for d in 0..self.pattern.disjunctions.len() {
            for a in 0..self.pattern.disjunctions[d].alternatives.len() {
                if self.pattern.disjunctions[d].alternatives[a].match_direction() != MatchDirection::Forward {
                    continue;
                }

                for i in 1..self.pattern.disjunctions[d].alternatives[a].terms.len() {
                    let terms = &self.pattern.disjunctions[d].alternatives[a].terms;
                    let current = &terms[i - 1];
                    let next = &terms[i];
                    if !is_possessifiable_greedy_term(current) {
                        continue;
                    }

                    if !Self::follower_forces_possessive(&self.pattern, current, next) {
                        continue;
                    }

                    self.pattern.disjunctions[d].alternatives[a].terms[i - 1].possessive = true;
                }
            }
        }
    }

    // m_startsWithBOL significa "todo casamento desta alternativa começa no início da entrada", que
    // é o que optimizeBOL() transforma em onceThrough. Esta passada é a fonte autoritativa do flag,
    // então roda antes de qualquer consumidor dele. Devolve true quando toda alternativa de
    // `disjunction` precisa começar no início da entrada.
    /// `recomputeStartsWithBOL(PatternDisjunction*)`.
    pub fn recompute_starts_with_bol_disjunction(&mut self, disjunction: crate::yarr::yarr_pattern::DisjunctionId) -> bool {
        use crate::yarr::yarr_error_code::ErrorCode;
        use crate::yarr::yarr_pattern::{AlternativeId, MatchDirection, PatternTermType};

        if !self.is_safe_to_recurse() {
            self.error = ErrorCode::PatternTooLarge;
            return false;
        }

        let mut all_alternatives_start_with_bol = true;
        for alt in 0..self.pattern.disjunction(disjunction).alternatives.len() {
            let alternative = AlternativeId { disjunction, index: alt as u32 };
            let mut starts_with_bol = false;
            for index in 0..self.pattern.alternative(alternative).terms.len() {
                let term = self.pattern.alternative(alternative).terms[index];
                let mut term_starts_with_bol = false;
                match term.type_ {
                    PatternTermType::AssertionBOL => {
                        term_starts_with_bol = term.match_direction() == MatchDirection::Forward;
                    }
                    PatternTermType::ParenthesesSubpattern | PatternTermType::ParentheticalAssertion => {
                        // Recursa mesmo num termo que não é o primeiro, cujo resultado não é usado:
                        // as disjunções aninhadas carregam o próprio flag e o copyTerms() filtra por
                        // ele em toda profundidade de aninhamento, então todas precisam ser
                        // recalculadas. Só propaga o flag para fora de um parêntese que o
                        // copyTerms() deixaria matar a alternativa.
                        term_starts_with_bol = self.recompute_starts_with_bol_disjunction(term.parentheses().disjunction)
                            && term.match_direction() == MatchDirection::Forward
                            && Self::parentheses_must_match(&term);
                    }
                    _ => {}
                }
                // Só o termo inicial pode ancorar a alternativa. Conservador para casos como /\b^a/,
                // como o parser já fazia.
                if index == 0 {
                    starts_with_bol = term_starts_with_bol;
                }
            }
            self.pattern.alternative_mut(alternative).starts_with_bol = starts_with_bol;
            if !starts_with_bol {
                all_alternatives_start_with_bol = false;
            }
        }
        all_alternatives_start_with_bol
    }

    /// `recomputeStartsWithBOL()`.
    pub fn recompute_starts_with_bol(&mut self) {
        // Sem `^` inicial em lugar nenhum, o parser nunca definiu o flag.
        if self.pattern.contains_bol {
            let body = self.pattern.body;
            self.recompute_starts_with_bol_disjunction(body);
        }
    }

    /// `optimizeBOL`.
    pub fn optimize_bol(&mut self) {
        // Procura expressões com âncora de início de linha (^) e as desenrola.
        // Por exemplo /^a|^b|c/ vira /^a|^b|c/, executado uma vez, seguido de /c/, que itera.
        // Este código depende de recomputeStartsWithBOL() ter marcado as alternativas com
        // m_startsWithBOL e de m_containsBOL vindo do código de parsing.
        // Neste ponto só vale para expressões sem multilinha.
        let disjunction = self.pattern.body;

        // Começamos com segurança, pois o modo `m` pode mudar com modificadores.
        if self.pattern.contains_modifiers || !self.pattern.contains_bol || self.pattern.multiline() {
            return;
        }

        let loop_disjunction = self.copy_disjunction(disjunction, true);

        // Marca as alternativas da disjunção como "onceThrough".
        for alt in 0..self.pattern.disjunction(disjunction).alternatives.len() {
            self.pattern.disjunction_mut(disjunction).alternatives[alt].set_once_through();
        }

        if let Some(loop_disjunction) = loop_disjunction {
            // Move as alternativas de loopDisjunction para disjunction (o `parent` de cada uma
            // continua apontando para loopDisjunction, como o ponteiro do C++, que só muda de dono).
            let moved = std::mem::take(&mut self.pattern.disjunction_mut(loop_disjunction).alternatives);
            self.pattern.disjunction_mut(disjunction).alternatives.extend(moved);
        }
    }

    // Abaixo destas contagens de alternativas, as formas de código mais simples que o JIT emite para
    // uma alternação curta (comparações literais fundidas, a varredura SIMD de duas alternativas,
    // grupos inlináveis sem frame) vencem as reescritas; as transformações só compensam em
    // alternações largas.
    /// `alternationFactoringMinRun`: fatoração de prefixo e dobra de nível superior.
    pub const ALTERNATION_FACTORING_MIN_RUN: usize = 8;
    /// `alternationWrapMinRunWhenFrameFree`.
    pub const ALTERNATION_WRAP_MIN_RUN_WHEN_FRAME_FREE: usize = 16;
}
