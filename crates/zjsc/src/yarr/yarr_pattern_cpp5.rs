// Fatia 5 do porte de `yarr/YarrPattern.cpp` (linhas 2751 a 3390): do `alternationFactoringMinRun` ao
// `computeEndAnchoredFixedSize` do `YarrPatternConstructor`. O `extractSpecificPattern` (linha 3391 em
// diante) abre numa função só e fica inteiro para a fatia seguinte.
//
// Incluída por `include!`, sem `use`: o que não é do módulo hospedeiro vem por caminho completo.
// `is_latin1` é o da fatia 1.
//
// Campos do `YarrPatternConstructor` usados aqui: `pattern`, `flags` e `factoring_budget` (o
// `m_factoringBudget`, `usize`, que a struct precisa declarar e o construtor iniciar em 0).
// Métodos do construtor definidos em outra fatia e chamados aqui: `is_safe_to_recurse(&mut self)`,
// `multiline(&self)` e `dot_all(&self)` (linhas 3830 em diante do C++). De `YarrPattern`:
// `any_character_class` e `newline_character_class`, ambos devolvendo `CharacterClassId`.
//
// Modelo de posse: no C++ as alternativas são `unique_ptr` e o `m_parent` das disjunções aninhadas
// aponta para a alternativa dona. Aqui a alternativa é um valor dentro de `Vec`, e o `AlternativeId`
// é sua posição, então, quando a fatoração reordena ou muda alternativas de disjunção, o
// `renumber_nested_parents` refaz o `parent` das disjunções aninhadas (termos que não são cópia,
// os únicos cujo `m_parent` o C++ mantém apontando para a própria alternativa). As disjunções novas
// entram em `pattern.disjunctions` ao serem criadas (precisam de id), e não ao fim da função como
// no C++; a ordem desse vetor não tem efeito observável.

/// `isSameLiteralTerm`: dois termos iniciais casam para fins de prefixo só se são o mesmo
/// caractere fixo e sensível a caixa.
fn is_same_literal_term(a: &crate::yarr::yarr_pattern::PatternTerm, b: &crate::yarr::yarr_pattern::PatternTerm) -> bool {
    use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType, QuantifierType};
    a.type_ == PatternTermType::PatternCharacter
        && b.type_ == PatternTermType::PatternCharacter
        && a.quantity_type == QuantifierType::FixedCount
        && b.quantity_type == QuantifierType::FixedCount
        && a.quantity_min_count == 1
        && a.quantity_max_count == 1
        && b.quantity_min_count == 1
        && b.quantity_max_count == 1
        && !a.ignore_case()
        && !b.ignore_case()
        && a.match_direction() == MatchDirection::Forward
        && b.match_direction() == MatchDirection::Forward
        && a.pattern_character() == b.pattern_character()
}

/// `TermParentheses` de um termo de parênteses ou de asserção parentética, se for um.
fn nested_disjunction_of(term: &crate::yarr::yarr_pattern::PatternTerm) -> Option<crate::yarr::yarr_pattern::TermParentheses> {
    use crate::yarr::yarr_pattern::{PatternTermType, TermPayload};
    if term.type_ != PatternTermType::ParenthesesSubpattern && term.type_ != PatternTermType::ParentheticalAssertion {
        return None;
    }
    match term.payload {
        TermPayload::Parentheses(parentheses) => Some(parentheses),
        _ => None,
    }
}

impl<'a> YarrPatternConstructor<'a> {
    // for a short alternation (fused literal compares, the two-alternative
    // SIMD scan, frame-free inlinable groups) beat the rewrites; the transforms
    // pay for themselves only on wide alternations.
    const FACTORING_BUDGET_BASE: usize = 1 << 16;
    const FACTORING_BUDGET_PER_TERM: usize = 16;

    /// `lineTerminators`: os code points `LineTerminator` do ECMAScript (o que `newlineCharacterClass()` guarda).
    const LINE_TERMINATORS: [u32; 4] = [0x0a, 0x0d, 0x2028, 0x2029];

    pub fn alternative_needs_frame(alternative: &crate::yarr::yarr_pattern::PatternAlternative) -> bool {
        use crate::yarr::yarr_pattern::{PatternTermType, QuantifierType};
        for term in &alternative.terms {
            match term.type_ {
                PatternTermType::AssertionBOL | PatternTermType::AssertionEOL | PatternTermType::AssertionWordBoundary => continue,
                PatternTermType::PatternCharacter | PatternTermType::CharacterClass => {
                    if term.quantity_type == QuantifierType::FixedCount {
                        continue;
                    }
                    return true;
                }
                _ => return true,
            }
        }
        false
    }

    pub fn charge_factoring_budget(&mut self, cost: usize) -> bool {
        if cost > self.factoring_budget {
            self.factoring_budget = 0;
            return false;
        }
        self.factoring_budget -= cost;
        true
    }

    // Alternation prefix factoring.
    //
    // Alternatives are tried leftmost-first, so their order is observable -- but
    // only between alternatives that can match at the same starting position.
    // Two alternatives that must begin with different literal characters have
    // disjoint starting points, so a maximal run of consecutive alternatives
    // that each begin with a (non-optional, case-sensitive) literal character
    // may be stably sorted by that character and then merged on common
    // prefixes:  /aq|bx|ar|by/ becomes /a(?:q|r)|b(?:x|y)/. Stability keeps
    // same-first-character alternatives in source order, and any alternative
    // that does not start with such a character (a class, group, anchor,
    // optional atom, or the empty alternative) is a barrier that no reordering
    // crosses. The rewrite is applied recursively to the factored suffixes. It
    // is a pure pattern-level equivalence, so both the JIT and the interpreter
    // see the factored form.

    /// `firstLiteralCharacter`: o caractere inicial de uma alternativa, se o primeiro termo é um
    /// caractere fixo, sensível a caixa e que precisa consumir entrada.
    pub fn first_literal_character(alternative: &crate::yarr::yarr_pattern::PatternAlternative) -> Option<u32> {
        use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType, QuantifierType};
        if alternative.terms.is_empty() {
            return None;
        }
        let term = &alternative.terms[0];
        if term.type_ != PatternTermType::PatternCharacter {
            return None;
        }
        if term.quantity_type != QuantifierType::FixedCount || term.quantity_min_count != 1 || term.quantity_max_count != 1 {
            return None;
        }
        if term.ignore_case() || term.match_direction() != MatchDirection::Forward {
            return None;
        }
        Some(term.pattern_character())
    }

    /// Refaz o `parent` das disjunções aninhadas das alternativas de `disjunction` depois que elas
    /// mudaram de posição ou de disjunção (o C++ não precisa: o ponteiro da alternativa é estável).
    fn renumber_nested_parents(&mut self, disjunction: crate::yarr::yarr_pattern::DisjunctionId) {
        let mut updates: Vec<(crate::yarr::yarr_pattern::DisjunctionId, crate::yarr::yarr_pattern::AlternativeId)> = Vec::new();
        for (index, alternative) in self.pattern.disjunction(disjunction).alternatives.iter().enumerate() {
            for term in &alternative.terms {
                if let Some(parentheses) = nested_disjunction_of(term) {
                    if !parentheses.is_copy {
                        updates.push((parentheses.disjunction, crate::yarr::yarr_pattern::AlternativeId { disjunction, index: index as u32 }));
                    }
                }
            }
        }
        for (nested, owner) in updates {
            self.pattern.disjunction_mut(nested).parent = Some(owner);
        }
    }

    // Merge a group of alternatives (already known to share their leading
    // literal term) into a single alternative:  a X | a Y | a Z  ->  a (?: X | Y | Z),
    // with the longest common literal prefix hoisted and the suffixes factored
    // recursively. `members` are removed from their owner and re-parented.
    pub fn merge_shared_prefix(&mut self, mut members: Vec<crate::yarr::yarr_pattern::PatternAlternative>) -> crate::yarr::yarr_pattern::PatternAlternative {
        use crate::yarr::yarr_pattern::{
            AlternativeId, DisjunctionId, MatchDirection, PatternAlternative, PatternDisjunction, PatternTerm, PatternTermType,
        };
        debug_assert!(members.len() >= 2);

        // Longest common prefix of literal terms across all members. A member
        // that is a prefix of a longer sibling contributes an empty suffix
        // alternative, which keeps its own position in the (stable) order.
        let mut prefix_length: usize = 1;
        loop {
            let first = &members[0].terms;
            if prefix_length >= first.len() {
                break;
            }
            let mut all_share = true;
            for member in &members {
                if prefix_length >= member.terms.len() || !is_same_literal_term(&first[prefix_length], &member.terms[prefix_length]) {
                    all_share = false;
                    break;
                }
            }
            if !all_share {
                break;
            }
            prefix_length += 1;
        }

        let mut merged = PatternAlternative::new(members[0].parent, members[0].first_subpattern_id, MatchDirection::Forward);
        for i in 0..prefix_length {
            merged.terms.push(members[0].terms[i]);
        }

        // The suffix disjunction holds each member's remaining terms, in the
        // members' (stable, first-character-sorted) order. The group's capture
        // span is computed from the terms (see accumulateCaptureRange).
        let suffix_disjunction = DisjunctionId(self.pattern.disjunctions.len() as u32);
        self.pattern.disjunctions.push(PatternDisjunction::new(None));
        let mut first_capture_id = u32::MAX;
        let mut last_capture_id: u32 = 0;
        let mut contains_bol = false;
        for member in members.iter_mut() {
            let suffix_index = self
                .pattern
                .disjunction_mut(suffix_disjunction)
                .add_new_alternative(suffix_disjunction, member.first_subpattern_id, MatchDirection::Forward);
            let member_size = member.terms.len();
            let moved_terms: Vec<PatternTerm> = member.terms.drain(prefix_length..).collect();
            {
                let suffix = self.pattern.alternative_mut(suffix_index);
                suffix.last_subpattern_id = member.last_subpattern_id;
                suffix.contains_bol = member.contains_bol;
                suffix.terms = moved_terms;
            }
            self.charge_factoring_budget(member_size);
            member.terms.clear();
            self.reparent_nested_disjunctions(suffix_index);
            let suffix = self.pattern.alternative_mut(suffix_index);
            Self::clear_terminal_marks(suffix);
            Self::accumulate_capture_range(suffix, &mut first_capture_id, &mut last_capture_id);
            contains_bol |= member.contains_bol;
        }
        members.clear();
        if let Some(last) = self.pattern.disjunction_mut(suffix_disjunction).alternatives.last_mut() {
            last.is_last_alternative = true;
        }

        // Factor the suffixes themselves (a shared second character, and so on).
        self.factor_alternatives(suffix_disjunction);

        let has_captures = first_capture_id <= last_capture_id;
        merged.last_subpattern_id = if has_captures { last_capture_id } else { 0 };
        merged.contains_bol = contains_bol;
        let mut group = PatternTerm::new_parentheses(
            PatternTermType::ParenthesesSubpattern,
            if has_captures { first_capture_id } else { self.pattern.num_subpatterns + 1 },
            suffix_disjunction,
            self.flags,
            /* capture */ false,
            false,
            MatchDirection::Forward,
        );
        group.parentheses_mut().last_subpattern_id = if has_captures { last_capture_id } else { 0 };
        merged.terms.push(group);
        // O `suffixDisjunction->m_parent = merged.get()` é refeito por `renumber_nested_parents`
        // quando `merged` ganha posição em `factor_alternatives`.
        merged
    }

    // A nested group's disjunction points back at its owning alternative; keep
    // that consistent when terms move to a new alternative.
    pub fn reparent_nested_disjunctions(&mut self, alternative: crate::yarr::yarr_pattern::AlternativeId) {
        let mut nested: Vec<crate::yarr::yarr_pattern::DisjunctionId> = Vec::new();
        for term in &self.pattern.alternative(alternative).terms {
            if let Some(parentheses) = nested_disjunction_of(term) {
                nested.push(parentheses.disjunction);
            }
        }
        for disjunction in nested {
            self.pattern.disjunction_mut(disjunction).parent = Some(alternative);
        }
    }

    // The [first, last] capture-subpattern ids actually contained in an
    // alternative's terms . Parser bookkeeping on the alternative
    // (m_firstSubpatternId / m_lastSubpatternId) is not reliable enough here:
    // m_lastSubpatternId is only set once a following sibling is parsed, and
    // sorting reorders which alternative comes first.
    pub fn accumulate_capture_range(alternative: &crate::yarr::yarr_pattern::PatternAlternative, first: &mut u32, last: &mut u32) {
        // A parenthesis term brackets its own capture (if any) and every capture nested in it as
        // [subpatternId, lastSubpatternId] (a superset after optimizeBOL's copies, which keep their
        // ids; harmless here), so the top-level terms suffice -- no recursion into the nesting.
        for term in &alternative.terms {
            let Some(parentheses) = nested_disjunction_of(term) else {
                continue;
            };
            if !term.contains_any_captures() {
                continue;
            }
            *first = (*first).min(parentheses.subpattern_id);
            *last = (*last).max(parentheses.last_subpattern_id);
        }
    }

    // A "terminal" parenthesis is valid only as the last term of a body
    // alternative (nothing after the body can force a backtrack into it). Once
    // an alternative moves inside a nested group that guarantee is gone, so
    // drop the marks (the general once/greedy path is used instead).
    pub fn clear_terminal_marks(alternative: &mut crate::yarr::yarr_pattern::PatternAlternative) {
        for term in alternative.terms.iter_mut() {
            if term.type_ == crate::yarr::yarr_pattern::PatternTermType::ParenthesesSubpattern {
                term.parentheses_mut().is_terminal = false;
            }
        }
    }

    // Rewrite `disjunction`'s alternatives in place: sort each barrier-free run
    // by leading literal and merge shared prefixes into nested groups.
    pub fn factor_alternatives(&mut self, disjunction: crate::yarr::yarr_pattern::DisjunctionId) {
        use crate::yarr::yarr_pattern::{MatchDirection, PatternAlternative, PatternTermType};
        if !self.is_safe_to_recurse() {
            return;
        }

        let alternatives_count = self.pattern.disjunction(disjunction).alternatives.len();
        if !self.charge_factoring_budget(alternatives_count) {
            return;
        }
        let alternatives = std::mem::take(&mut self.pattern.disjunction_mut(disjunction).alternatives);
        // Uma alternativa é "de corrida" se tem caractere inicial fixo e não é once-through; as outras
        // são barreiras.
        let eligible: Vec<bool> = alternatives
            .iter()
            .map(|alternative| Self::first_literal_character(alternative).is_some() && !alternative.once_through())
            .collect();
        let mut pending = alternatives.into_iter();
        let mut result: Vec<PatternAlternative> = Vec::with_capacity(alternatives_count);

        let mut i: usize = 0;
        while i < alternatives_count {
            // A barrier (no fixed leading literal, or the empty alternative) is
            // copied through untouched and never reordered across.
            if !eligible[i] {
                if let Some(alternative) = pending.next() {
                    result.push(alternative);
                }
                i += 1;
                continue;
            }

            // Gather the maximal run of literal-leading alternatives.
            let run_start = i;
            while i < alternatives_count && eligible[i] {
                i += 1;
            }
            let run_end = i;
            let mut run: Vec<PatternAlternative> = pending.by_ref().take(run_end - run_start).collect();

            // Small alternations are left alone: the sequential JIT path (fused
            // compares, the two-alternative SIMD scan, frame-free inlinable code)
            // is already optimal there, and factoring would forfeit those. The
            // rewrite pays for itself only on large alternations.
            if run_end - run_start < Self::ALTERNATION_FACTORING_MIN_RUN {
                result.extend(run);
                continue;
            }

            // `std::stable_sort` por caractere inicial (o `sort_by_key` do Rust também é estável).
            run.sort_by_key(|alternative| Self::first_literal_character(alternative).unwrap_or(0));

            // Walk the sorted run, merging each maximal group that shares the
            // leading literal term.
            let mut run_iterator = run.into_iter().peekable();
            while let Some(first) = run_iterator.next() {
                let mut members = vec![first];
                while let Some(next) = run_iterator.next_if(|next| is_same_literal_term(&members[0].terms[0], &next.terms[0])) {
                    members.push(next);
                }
                if members.len() == 1 {
                    result.extend(members);
                } else {
                    let mut merged = self.merge_shared_prefix(members);
                    merged.parent = disjunction;
                    result.push(merged);
                }
            }
        }

        // Every alternative was moved into `result`; always reinstall the list
        // (a run that sorted or merged nothing is simply the original order).
        for alternative in result.iter_mut() {
            alternative.parent = disjunction;
            alternative.is_last_alternative = false;
        }
        if let Some(last) = result.last_mut() {
            last.is_last_alternative = true;
        }
        self.pattern.disjunction_mut(disjunction).alternatives = result;
        self.renumber_nested_parents(disjunction);

        // Factor inside pre-existing (source-written) groups too, so that
        // /\b(?:about|above|after)\b/ shares its "a" prefix like a top-level
        // alternation would. Groups synthesized by mergeSharedPrefix were
        // already factored when built; re-running is a harmless no-op. Groups
        // that checkForTerminalParentheses already committed to a specialized
        // code shape (string lists, terminal parentheses) are left alone: that
        // shape is fixed by their alternatives, which restructuring would break.
        //
        // Not beneath a group that can repeat, though: every group factoring creates takes frame
        // slots, sibling alternatives no longer share them (setupDisjunctionOffsets), and a repeating
        // group saves and restores its whole frame span per iteration (and the interpreter sizes
        // its per-iteration context by it) -- a few thousand shared-prefix alternatives under a +
        // turned a 6-slot frame into thousands and ran out of backtracking space after ~6,000
        // iterations. Such groups keep their flat alternatives, as on main.
        let mut nested: Vec<crate::yarr::yarr_pattern::DisjunctionId> = Vec::new();
        for alternative in &self.pattern.disjunction(disjunction).alternatives {
            for term in &alternative.terms {
                if term.type_ != PatternTermType::ParenthesesSubpattern {
                    continue;
                }
                let Some(parentheses) = nested_disjunction_of(term) else {
                    continue;
                };
                if !parentheses.is_copy
                    && !parentheses.is_string_list
                    && !parentheses.is_terminal
                    && term.quantity_max_count == 1
                    && !self.pattern.disjunction(parentheses.disjunction).alternatives.is_empty()
                    && term.match_direction() == MatchDirection::Forward
                {
                    nested.push(parentheses.disjunction); // (a single alternative has nothing to factor, but its groups may)
                }
            }
        }
        for nested_disjunction in nested {
            self.factor_alternatives(nested_disjunction);
        }
    }

    // A large top-level alternation costs one entry attempt per alternative at
    // every candidate position when the alternatives are tried in sequence. Fold
    // the repeated (non-once-through) alternatives into a single alternative
    // holding one non-capturing group -- /X|Y|Z/ is exactly /(?:X|Y|Z)/ -- so
    // that the group's alternatives can be dispatched on their first character.
    // The rewrite itself is engine-neutral; setupOffsets() (which runs after
    // this) lays out the group like any hand-written one.
    pub fn factor_and_wrap_alternatives(&mut self) {
        if !crate::runtime::options_list::Options::use_reg_exp_alternation_factoring() {
            return;
        }
        self.factoring_budget = Self::FACTORING_BUDGET_BASE;
        let body = self.pattern.body;
        for alternative in &self.pattern.disjunction(body).alternatives {
            self.factoring_budget += Self::FACTORING_BUDGET_PER_TERM * alternative.terms.len();
        }
        self.factor_alternatives(body);
        self.wrap_alternatives_for_dispatch();
    }

    pub fn wrap_alternatives_for_dispatch(&mut self) {
        use crate::yarr::yarr_pattern::{AlternativeId, DisjunctionId, MatchDirection, PatternDisjunction, PatternTerm, PatternTermType};
        let body = self.pattern.body;

        // The body is laid out as [onceThrough..., repeated...] (see optimizeBOL).
        let alternatives = &self.pattern.disjunction(body).alternatives;
        let mut first_repeated: usize = 0;
        while first_repeated < alternatives.len() && alternatives[first_repeated].once_through() {
            first_repeated += 1;
        }
        let repeated_count = alternatives.len() - first_repeated;
        if repeated_count < Self::ALTERNATION_FACTORING_MIN_RUN {
            return;
        }
        // Every alternative costs the dispatcher at least one entry stub, so a run wider than it
        // accepts (left that wide because factoring could not merge it, e.g. under /i) is never
        // dispatched, and the wrapping group would only add its per-iteration bookkeeping.
        if repeated_count > crate::yarr::yarr::ALTERNATION_DISPATCH_MAX_STUBS as usize {
            return;
        }
        if repeated_count < Self::ALTERNATION_WRAP_MIN_RUN_WHEN_FRAME_FREE {
            let mut needs_frame = false;
            let mut i = first_repeated;
            while i < alternatives.len() && !needs_frame {
                needs_frame = Self::alternative_needs_frame(&alternatives[i]);
                i += 1;
            }
            if !needs_frame {
                return;
            }
        }

        // A DotStarEnclosure records match bounds through the enclosing body
        // alternative; keep such bodies in their existing shape.
        for i in first_repeated..alternatives.len() {
            for term in &alternatives[i].terms {
                if term.type_ == PatternTermType::DotStarEnclosure {
                    return;
                }
            }
        }

        // The group's capture span brackets the captures its alternatives
        // contain, computed from the terms themselves (sorting reorders which
        // alternative is first, and the parser leaves the last body alternative's
        // m_lastSubpatternId unset, so per-alternative bookkeeping is not reliable).
        let mut first_capture_id = u32::MAX;
        let mut last_capture_id: u32 = 0;
        let mut contains_bol = false;
        let mut starts_with_bol_count: u32 = 0;

        let group_disjunction = DisjunctionId(self.pattern.disjunctions.len() as u32);
        self.pattern.disjunctions.push(PatternDisjunction::new(None));
        let moved: Vec<crate::yarr::yarr_pattern::PatternAlternative> =
            self.pattern.disjunction_mut(body).alternatives.drain(first_repeated..).collect();
        let mut group_alternatives = Vec::with_capacity(moved.len());
        for mut alternative in moved {
            alternative.parent = group_disjunction;
            alternative.is_last_alternative = false;
            Self::clear_terminal_marks(&mut alternative);
            Self::accumulate_capture_range(&alternative, &mut first_capture_id, &mut last_capture_id);
            contains_bol |= alternative.contains_bol;
            if alternative.starts_with_bol {
                starts_with_bol_count += 1;
            }
            group_alternatives.push(alternative);
        }
        if let Some(last) = group_alternatives.last_mut() {
            last.is_last_alternative = true;
        }
        let group_alternatives_count = group_alternatives.len();
        self.pattern.disjunction_mut(group_disjunction).alternatives = group_alternatives;

        // With no captures inside, subpatternId > lastSubpatternId makes
        // containsAnyCaptures() false (the convention for capture-free groups).
        let has_captures = first_capture_id <= last_capture_id;
        let group_subpattern_id = if has_captures { first_capture_id } else { self.pattern.num_subpatterns + 1 };
        let group_last_subpattern_id = if has_captures { last_capture_id } else { 0 };

        let first_subpattern_id = if has_captures { first_capture_id.wrapping_sub(1) } else { self.pattern.num_subpatterns };
        let wrapped: AlternativeId = self
            .pattern
            .disjunction_mut(body)
            .add_new_alternative(body, first_subpattern_id, MatchDirection::Forward);
        {
            let wrapped_alternative = self.pattern.alternative_mut(wrapped);
            wrapped_alternative.last_subpattern_id = group_last_subpattern_id;
            wrapped_alternative.contains_bol = contains_bol;
            wrapped_alternative.starts_with_bol = starts_with_bol_count as usize == group_alternatives_count;
        }
        self.pattern.disjunction_mut(group_disjunction).parent = Some(wrapped);

        let mut group = PatternTerm::new_parentheses(
            PatternTermType::ParenthesesSubpattern,
            group_subpattern_id,
            group_disjunction,
            self.flags,
            /* capture */ false,
            false,
            MatchDirection::Forward,
        );
        group.parentheses_mut().last_subpattern_id = group_last_subpattern_id;
        self.pattern.alternative_mut(wrapped).terms.push(group);

        // As alternativas mudaram de disjunção e de posição.
        self.renumber_nested_parents(group_disjunction);
    }

    pub fn contains_capturing_terms(alternative: &crate::yarr::yarr_pattern::PatternAlternative, first_term_index: usize, end_index: usize) -> bool {
        use crate::yarr::yarr_pattern::PatternTermType;
        let terms = &alternative.terms;

        debug_assert!(end_index <= terms.len());
        for term_index in first_term_index..end_index {
            let term = &terms[term_index];

            if term.capture {
                return true;
            }

            if (term.type_ == PatternTermType::ParenthesesSubpattern || term.type_ == PatternTermType::ParentheticalAssertion) && term.contains_any_captures() {
                return true;
            }
        }

        false
    }

    // Code point membership of a class (ignoring inversion at the term). Table-backed classes
    // without explicit matches/ranges are answered from the table where the interpreter would
    // (YarrInterpreter's testCharacterClass); an inverted-storage table is Indeterminate.
    pub fn class_contains_code_point(character_class: &crate::yarr::yarr_pattern::CharacterClass, ch: u32) -> crate::parser::source_tainted_origin::TriState {
        use crate::parser::source_tainted_origin::TriState;
        if character_class.any_character {
            return TriState::True;
        }
        if character_class.has_strings() {
            return TriState::Indeterminate;
        }
        if !character_class.has_single_characters() {
            let Some(table) = character_class.table else {
                return TriState::False;
            };
            if character_class.table_inverted || ch >= crate::yarr::yarr_pattern::CharacterClass::TABLE_SIZE {
                return TriState::Indeterminate;
            }
            return if table[ch as usize] != 0 { TriState::True } else { TriState::False };
        }
        let is_latin1_char = is_latin1(ch);
        let matches = if is_latin1_char { &character_class.matches8 } else { &character_class.matches32 };
        for &candidate in matches {
            if candidate == ch {
                return TriState::True;
            }
        }
        let ranges = if is_latin1_char { &character_class.ranges8 } else { &character_class.ranges32 };
        for range in ranges {
            if ch >= range.begin && ch <= range.end {
                return TriState::True;
            }
        }
        TriState::False
    }

    pub fn is_line_terminator(ch: u32) -> bool {
        for terminator in Self::LINE_TERMINATORS {
            if ch == terminator {
                return true;
            }
        }
        false
    }

    pub fn character_class_may_match_newline(character_class: &crate::yarr::yarr_pattern::CharacterClass, invert: bool) -> bool {
        use crate::parser::source_tainted_origin::TriState;
        for ch in Self::LINE_TERMINATORS {
            let contains = Self::class_contains_code_point(character_class, ch);
            if contains == TriState::Indeterminate || (contains == TriState::True) != invert {
                return true;
            }
        }
        false
    }

    // Whether any term in [firstTermIndex, endIndex) could consume a line terminator.
    // Conservative: anything not understood says yes.
    pub fn terms_may_match_newline(&mut self, alternative: crate::yarr::yarr_pattern::AlternativeId, first_term_index: usize, end_index: usize) -> bool {
        use crate::yarr::yarr_pattern::{AlternativeId, PatternTermType};
        if !self.is_safe_to_recurse() {
            return true;
        }

        for term_index in first_term_index..end_index {
            let term = self.pattern.alternative(alternative).terms[term_index];
            match term.type_ {
                PatternTermType::AssertionBOL | PatternTermType::AssertionEOL | PatternTermType::AssertionWordBoundary => continue,
                PatternTermType::PatternCharacter => {
                    if term.quantity_max_count == 0 {
                        continue;
                    }
                    if Self::is_line_terminator(term.pattern_character()) {
                        return true;
                    }
                    continue;
                }
                PatternTermType::CharacterClass => {
                    if term.quantity_max_count == 0 {
                        continue;
                    }
                    if Self::character_class_may_match_newline(self.pattern.character_class(term.character_class()), term.invert()) {
                        return true;
                    }
                    continue;
                }
                PatternTermType::ParentheticalAssertion => {
                    // Lookarounds consume nothing, so they cannot carry the expression's own
                    // span across a line terminator (whatever they inspect).
                    continue;
                }
                PatternTermType::ParenthesesSubpattern => {
                    let nested_disjunction = term.parentheses().disjunction;
                    let nested_count = self.pattern.disjunction(nested_disjunction).alternatives.len();
                    for nested_index in 0..nested_count {
                        let nested = AlternativeId { disjunction: nested_disjunction, index: nested_index as u32 };
                        let nested_size = self.pattern.alternative(nested).terms.len();
                        if self.terms_may_match_newline(nested, 0, nested_size) {
                            return true;
                        }
                    }
                    continue;
                }
                _ => return true,
            }
        }
        false
    }

    // This optimization identifies alternatives in the form of
    // [^].*[?]<expression>.*[$] for expressions that don't have any
    // capturing terms. The alternative is changed to <expression>
    // followed by processing of the dot stars to find and adjust the
    // beginning and the end of the match.
    pub fn optimize_dot_star_wrapped_expressions(&mut self) {
        use crate::yarr::yarr_pattern::{AlternativeId, PatternTerm, PatternTermType, QuantifierType};
        let body = self.pattern.body;
        if self.pattern.disjunction(body).alternatives.len() != 1 {
            return;
        }

        // A sticky pattern must begin its match exactly at lastIndex, but the enclosure reports the
        // position of the wrapped expression rather than of the leading `.*` it absorbs, so
        // /^.*a.*$/y would fail on "xa" instead of matching the whole string at 0.
        if self.pattern.sticky() {
            return;
        }

        let dot_character_class = if self.dot_all() { self.pattern.any_character_class() } else { self.pattern.newline_character_class() };
        let alternative_id = AlternativeId { disjunction: body, index: 0 };
        let terms = self.pattern.alternative(alternative_id).terms.clone();
        if terms.len() >= 3 {
            let mut starts_with_bol = false;
            let mut ends_with_eol = false;

            let mut term_index: usize = 0;
            if terms[term_index].type_ == PatternTermType::AssertionBOL {
                starts_with_bol = true;
                term_index += 1;
            }

            let first_non_anchor_term = &terms[term_index];
            if first_non_anchor_term.type_ != PatternTermType::CharacterClass
                || first_non_anchor_term.character_class() != dot_character_class
                || first_non_anchor_term.quantity_min_count != 0
                || first_non_anchor_term.quantity_max_count != crate::yarr::yarr::QUANTIFY_INFINITE
            {
                return;
            }

            let first_expression_term = term_index + 1;

            term_index = terms.len() - 1;
            if terms[term_index].type_ == PatternTermType::AssertionEOL {
                ends_with_eol = true;
                term_index -= 1;
            }

            let last_non_anchor_term = &terms[term_index];
            if last_non_anchor_term.type_ != PatternTermType::CharacterClass
                || last_non_anchor_term.character_class() != dot_character_class
                || last_non_anchor_term.quantity_type != QuantifierType::Greedy
                || last_non_anchor_term.quantity_min_count != 0
                || last_non_anchor_term.quantity_max_count != crate::yarr::yarr::QUANTIFY_INFINITE
            {
                return;
            }

            let end_index = term_index;
            if first_expression_term >= end_index {
                return;
            }

            // Without /s the enclosure takes the FIRST occurrence of the expression at or
            // after the start and widens it to the enclosing line. Greedy semantics take
            // the LAST split point on that line where the expression matches, which is
            // the same span only if the expression can never itself consume a line
            // terminator: /^.*[e\s].*/ on "eq\n" is "eq\n" (the class takes the \n), not "eq".
            // A trailing non-/m $ pins the end to the input's end regardless, and the start then
            // depends only on the earliest expression start with a newline-free tail, which the
            // enclosure's first-occurrence-then-advance search also finds; keep those (validators
            // like /.*foo\s+bar.*$/) on the fast path.
            if !self.dot_all() && !(ends_with_eol && !self.multiline()) && self.terms_may_match_newline(alternative_id, first_expression_term, end_index) {
                return;
            }

            if !Self::contains_capturing_terms(self.pattern.alternative(alternative_id), first_expression_term, end_index) {
                let flags = self.flags;
                let alternative = self.pattern.alternative_mut(alternative_id);
                for term_index in (end_index..alternative.terms.len()).rev() {
                    alternative.terms.remove(term_index);
                }

                for term_index in (1..=first_expression_term).rev() {
                    alternative.terms.remove(term_index - 1);
                }

                alternative.terms.push(PatternTerm::new_dot_star_enclosure(starts_with_bol, ends_with_eol, flags));

                // The enclosure now carries the anchoring, so the alternative no longer starts with ^.
                alternative.starts_with_bol = false;
                self.pattern.contains_bol = false;
            }
        }
    }

    pub fn setup_named_captures(&mut self) {
        if !self.pattern.has_named_capture_groups {
            return;
        }

        let pattern = &mut *self.pattern;

        // Finish padding out m_captureGroupNames vector.
        while pattern.capture_group_names.len() as u32 <= pattern.num_subpatterns {
            pattern.capture_group_names.push(crate::wtf::text::wtf_string::String::default());
        }

        for named_group_indices in pattern.named_group_to_paren_indices.values_mut() {
            if named_group_indices.len() == 2 {
                // Since this named group is only used in one place, i.e. not a duplicate name,
                // make that subpatternId as the only value in the vector.
                debug_assert!(named_group_indices[0] == named_group_indices[1]);
                named_group_indices.pop();
            }
        }

        if pattern.num_duplicate_named_capture_groups != 0 {
            // `Vector::fill(0, n)`: redimensiona para `n` e preenche tudo com 0.
            pattern.duplicate_named_group_for_subpattern_id.clear();
            pattern.duplicate_named_group_for_subpattern_id.resize(pattern.num_subpatterns as usize + 1, 0);
            for named_group_indices in pattern.named_group_to_paren_indices.values() {
                if named_group_indices.len() > 2 {
                    let duplicate_named_group_id = named_group_indices[0];
                    for i in 1..named_group_indices.len() {
                        let subpattern_id = named_group_indices[i] as usize;
                        debug_assert!(pattern.duplicate_named_group_for_subpattern_id[subpattern_id] == 0);
                        pattern.duplicate_named_group_for_subpattern_id[subpattern_id] = duplicate_named_group_id;
                    }
                }
            }
        }
    }

    pub fn compute_end_anchored_fixed_size(&mut self) {
        let pattern = &mut *self.pattern;
        if pattern.multiline()
            || pattern.sticky()
            || pattern.contains_modifiers
            || pattern.contains_bol
            || pattern.contains_unsigned_length_pattern
            || !pattern.disjunction(pattern.body).has_fixed_size
            || pattern.save_initial_start_value
        {
            return;
        }

        let mut maximum_size: u32 = 0;
        for alternative in &pattern.disjunction(pattern.body).alternatives {
            if !alternative.has_fixed_size
                || alternative.terms.is_empty()
                || alternative.terms[alternative.terms.len() - 1].type_ != crate::yarr::yarr_pattern::PatternTermType::AssertionEOL
            {
                return;
            }
            maximum_size = maximum_size.max(alternative.minimum_size);
        }
        pattern.end_anchored_fixed_size = maximum_size;
    }
}
