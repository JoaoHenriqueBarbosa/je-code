// Porte de `yarr/YarrPattern.cpp`, segunda fatia (linhas 769 a 1350): o fim do
// `CharacterClassConstructor` (`latin1Op` até `isUnionSetOp`) e o começo do `YarrPatternConstructor`
// (`UnresolvedForwardReference`, construtor, `resetForReparsing`, `addCaptureGroupForName`,
// `registerCopiedForwardReferences`, `resolveForwardReferencesInLookbehindTo`).
//
// Esta fatia é incluída por `include!` no fim de `yarr_pattern_cpp1.rs` e, por isso, não tem `use`:
// os itens do módulo da fatia 1 (`is_latin1`, `u_is_bmp`, `merge_ranges_from`, `CharacterClass`,
// `CharacterClassWidths`, `CharacterRange`, `CompileMode`, `CharacterClassSetOp`, `UCHAR_MAX_VALUE`)
// são usados pelo nome, e o resto vem por caminho completo.

/// `WTF::BitSet<N>`: conjunto de bits de tamanho fixo (`size` é múltiplo de 64), iterado em ordem
/// crescente. Só as operações que o `YarrPattern.cpp` usa.
pub(crate) struct BitSet {
    words: Vec<u64>,
    size: usize,
}

impl BitSet {
    fn new(size: usize) -> Self {
        BitSet { words: vec![0; size / 64], size }
    }

    fn set(&mut self, index: usize) {
        self.words[index / 64] |= 1u64 << (index % 64);
    }

    /// `merge`: união.
    fn merge(&mut self, other: &BitSet) {
        for (word, other_word) in self.words.iter_mut().zip(other.words.iter()) {
            *word |= *other_word;
        }
    }

    /// `filter`: interseção.
    fn filter(&mut self, other: &BitSet) {
        for (word, other_word) in self.words.iter_mut().zip(other.words.iter()) {
            *word &= *other_word;
        }
    }

    /// `exclude`: subtração.
    fn exclude(&mut self, other: &BitSet) {
        for (word, other_word) in self.words.iter_mut().zip(other.words.iter()) {
            *word &= !*other_word;
        }
    }

    fn invert(&mut self) {
        for word in self.words.iter_mut() {
            *word = !*word;
        }
    }

    fn clear_all(&mut self) {
        for word in self.words.iter_mut() {
            *word = 0;
        }
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.size).filter(move |&index| self.words[index / 64] & (1u64 << (index % 64)) != 0)
    }
}

/// Marca no `BitSet` cada `match` e cada caractere de cada `range` (o trecho repetido de `latin1Op` e
/// `latin1Invert`).
fn set_matches_and_ranges(bit_set: &mut BitSet, matches: &[u32], ranges: &[CharacterRange]) {
    for &match_ch in matches {
        bit_set.set(match_ch as usize);
    }

    for range in ranges {
        for ch in range.begin..=range.end {
            bit_set.set(ch as usize);
        }
    }
}

/// Percorre os bits ligados de `bit_set` (somando `base` a cada índice) e chama `emit(lo, hi)` para
/// cada corrida de caracteres consecutivos: o laço comum de `latin1Op`, `latin1Invert` e
/// `nonLatin1OpSorted`, onde só o `addCharToResults` muda.
fn for_each_run(bit_set: &BitSet, base: u32, mut emit: impl FnMut(u32, u32)) {
    let mut first_char_unset = true;
    let mut lo: u32 = 0;
    let mut hi: u32 = 0;

    for set_val in bit_set.iter() {
        let ch = set_val as u32 + base;
        if first_char_unset {
            lo = ch;
            hi = ch;
            first_char_unset = false;
        } else if ch == hi + 1 {
            hi = ch;
        } else {
            emit(lo, hi);
            lo = ch;
            hi = ch;
        }
    }

    if !first_char_unset {
        emit(lo, hi);
    }
}

/// O `addCharToResults` de `latin1Op` e `latin1Invert` (sem coalescência).
fn push_latin1_run(result_matches: &mut Vec<u32>, result_ranges: &mut Vec<CharacterRange>, lo: u32, hi: u32) {
    if lo == hi {
        result_matches.push(lo);
    } else {
        result_ranges.push(CharacterRange::new(lo, hi));
    }
}

/// Passa para o `BitSet` de um pedaço de `nonLatin1OpSorted` os `matches` e `ranges` que caem em
/// `[chunk_lo, chunk_hi]`, avançando os índices (os dois laços repetidos para o lado esquerdo e o
/// direito).
fn fill_chunk_bit_set(
    bit_set: &mut BitSet,
    matches: &[u32],
    ranges: &[CharacterRange],
    match_index: &mut usize,
    range_index: &mut usize,
    chunk_lo: u32,
    chunk_hi: u32,
) {
    while *match_index < matches.len() {
        let ch = matches[*match_index];
        if ch > chunk_hi {
            break;
        }

        bit_set.set((ch - chunk_lo) as usize);
        *match_index += 1;
    }

    while *range_index < ranges.len() {
        let range = ranges[*range_index];
        if range.begin > chunk_hi {
            break;
        }

        let begin = chunk_lo.max(range.begin);
        let end = range.end.min(chunk_hi);

        for ch in begin..=end {
            bit_set.set((ch - chunk_lo) as usize);
        }

        if range.end > chunk_hi {
            break;
        }
        *range_index += 1;
    }
}

/// `coalesceMatchesAndRanges`, o lambda de `coalesceTables`.
fn coalesce_matches_and_ranges(matches: &mut Vec<u32>, ranges: &mut Vec<CharacterRange>) {
    let mut matches_index: usize = 0;
    let mut ranges_index: usize = 0;

    while matches_index < matches.len() && ranges_index < ranges.len() {
        if ranges[ranges_index].begin != 0 {
            while matches_index < matches.len() && matches[matches_index] < ranges[ranges_index].begin - 1 {
                matches_index += 1;
            }

            if matches_index < matches.len() && matches[matches_index] == ranges[ranges_index].begin - 1 {
                ranges[ranges_index].begin = matches[matches_index];
                matches.remove(matches_index);
            }
        }

        // Matches inside the range are redundant; drop them (one removal per run, so this
        // stays linear) so every consumer, notably appendInverted's complement walk, sees
        // disjoint matches and ranges.
        let mut first_inside = matches_index;
        while first_inside < matches.len() && matches[first_inside] < ranges[ranges_index].begin {
            first_inside += 1;
        }
        let mut past_inside = first_inside;
        while past_inside < matches.len() && matches[past_inside] <= ranges[ranges_index].end {
            past_inside += 1;
        }
        if past_inside > first_inside {
            matches.drain(first_inside..past_inside);
        }
        matches_index = first_inside;

        if matches_index < matches.len() {
            if matches[matches_index] > ranges[ranges_index].end + 1 {
                ranges_index += 1;
                continue;
            }

            if matches[matches_index] == ranges[ranges_index].end + 1 {
                ranges[ranges_index].end = matches[matches_index];
                matches.remove(matches_index);

                merge_ranges_from(ranges, ranges_index);
            } else {
                matches_index += 1;
            }
        }
    }

    if ranges.len() > 1 {
        for ranges_index in (1..ranges.len()).rev() {
            if ranges[ranges_index].begin == ranges[ranges_index - 1].end + 1 {
                ranges[ranges_index - 1].end = ranges[ranges_index].end;
                ranges.remove(ranges_index);
            }
        }
    }
}

impl CharacterClassConstructor {
    pub fn latin1_op(&mut self, rhs_matches: &[u32], rhs_ranges: &[CharacterRange]) {
        let mut result_matches: Vec<u32> = Vec::new();
        let mut result_ranges: Vec<CharacterRange> = Vec::new();
        let mut lhs_latin1_bit_set = BitSet::new(0x100);
        let mut rhs_latin1_bit_set = BitSet::new(0x100);

        set_matches_and_ranges(&mut lhs_latin1_bit_set, &self.matches8, &self.ranges8);
        set_matches_and_ranges(&mut rhs_latin1_bit_set, rhs_matches, rhs_ranges);

        match self.set_op {
            CharacterClassSetOp::Default | CharacterClassSetOp::Union => {
                lhs_latin1_bit_set.merge(&rhs_latin1_bit_set);
            }

            CharacterClassSetOp::Intersection => {
                lhs_latin1_bit_set.filter(&rhs_latin1_bit_set);
            }

            CharacterClassSetOp::Subtraction => {
                lhs_latin1_bit_set.exclude(&rhs_latin1_bit_set);
            }
        }

        for_each_run(&lhs_latin1_bit_set, 0, |lo, hi| {
            push_latin1_run(&mut result_matches, &mut result_ranges, lo, hi);
        });

        self.matches8 = result_matches;
        self.ranges8 = result_ranges;
    }

    pub fn latin1_invert(&mut self) {
        let mut result_matches: Vec<u32> = Vec::new();
        let mut result_ranges: Vec<CharacterRange> = Vec::new();
        let mut latin1_bit_set = BitSet::new(0x100);

        set_matches_and_ranges(&mut latin1_bit_set, &self.matches8, &self.ranges8);

        latin1_bit_set.invert();

        for_each_run(&latin1_bit_set, 0, |lo, hi| {
            push_latin1_run(&mut result_matches, &mut result_ranges, lo, hi);
        });

        self.matches8 = result_matches;
        self.ranges8 = result_ranges;
    }

    pub fn non_latin1_op_sorted(&mut self, rhs_matches32: &[u32], rhs_ranges32: &[CharacterRange]) {
        let mut result_matches: Vec<u32> = Vec::new();
        let mut result_ranges: Vec<CharacterRange> = Vec::new();

        const CHUNK_SIZE: usize = 2048;
        let mut lhs_chunk_bit_set = BitSet::new(CHUNK_SIZE);
        let mut rhs_chunk_bit_set = BitSet::new(CHUNK_SIZE);

        let mut chunk_lo: u32 = i32::MAX as u32;

        let mut lhs_match_index: usize = 0;
        let mut lhs_range_index: usize = 0;
        let mut rhs_match_index: usize = 0;
        let mut rhs_range_index: usize = 0;

        let lhs_matches_len = self.matches32.len();
        let lhs_ranges_len = self.ranges32.len();
        let set_op = self.set_op;

        let lhs_has_more = |match_index: usize, range_index: usize| match_index < lhs_matches_len || range_index < lhs_ranges_len;
        let rhs_has_more =
            |match_index: usize, range_index: usize| match_index < rhs_matches32.len() || range_index < rhs_ranges32.len();
        let can_produce_more = |lhs_match_index: usize, lhs_range_index: usize, rhs_match_index: usize, rhs_range_index: usize| match set_op {
            CharacterClassSetOp::Default | CharacterClassSetOp::Union => {
                lhs_has_more(lhs_match_index, lhs_range_index) || rhs_has_more(rhs_match_index, rhs_range_index)
            }
            CharacterClassSetOp::Intersection => {
                lhs_has_more(lhs_match_index, lhs_range_index) && rhs_has_more(rhs_match_index, rhs_range_index)
            }
            CharacterClassSetOp::Subtraction => lhs_has_more(lhs_match_index, lhs_range_index),
        };

        if !self.matches32.is_empty() {
            chunk_lo = chunk_lo.min(self.matches32[0]);
        }

        if !self.ranges32.is_empty() {
            chunk_lo = chunk_lo.min(self.ranges32[0].begin);
        }

        if !rhs_matches32.is_empty() {
            chunk_lo = chunk_lo.min(rhs_matches32[0]);
        }

        if !rhs_ranges32.is_empty() {
            chunk_lo = chunk_lo.min(rhs_ranges32[0].begin);
        }

        while can_produce_more(lhs_match_index, lhs_range_index, rhs_match_index, rhs_range_index) {
            let chunk_hi = chunk_lo + CHUNK_SIZE as u32 - 1;

            fill_chunk_bit_set(
                &mut lhs_chunk_bit_set,
                &self.matches32,
                &self.ranges32,
                &mut lhs_match_index,
                &mut lhs_range_index,
                chunk_lo,
                chunk_hi,
            );
            fill_chunk_bit_set(
                &mut rhs_chunk_bit_set,
                rhs_matches32,
                rhs_ranges32,
                &mut rhs_match_index,
                &mut rhs_range_index,
                chunk_lo,
                chunk_hi,
            );

            match set_op {
                CharacterClassSetOp::Default | CharacterClassSetOp::Union => {
                    lhs_chunk_bit_set.merge(&rhs_chunk_bit_set);
                }

                CharacterClassSetOp::Intersection => {
                    lhs_chunk_bit_set.filter(&rhs_chunk_bit_set);
                }

                CharacterClassSetOp::Subtraction => {
                    lhs_chunk_bit_set.exclude(&rhs_chunk_bit_set);
                }
            }

            for_each_run(&lhs_chunk_bit_set, chunk_lo, |lo, hi| {
                if lo == hi {
                    result_matches.push(lo);
                } else {
                    // Coalesce the prior range with the new (lo, hi) range if they are adjacent.
                    if let Some(last) = result_ranges.last_mut() {
                        if last.end + 1 == lo {
                            last.end = hi;
                            return;
                        }
                    }

                    result_ranges.push(CharacterRange::new(lo, hi));
                }
            });

            chunk_lo = chunk_hi + 1;
            lhs_chunk_bit_set.clear_all();
            rhs_chunk_bit_set.clear_all();
        }

        self.matches32 = result_matches;
        self.ranges32 = result_ranges;
    }

    pub fn non_latin1_invert(&mut self) {
        let current_set_op = self.set_op;
        self.set_op = CharacterClassSetOp::Subtraction;

        // `std::swap` com um vetor vazio e um com o intervalo completo acima do Latin-1.
        let matches = std::mem::take(&mut self.matches32);
        let ranges = std::mem::replace(&mut self.ranges32, vec![CharacterRange::new(0x0100, UCHAR_MAX_VALUE)]);

        self.non_latin1_op_sorted(&matches, &ranges);

        self.set_op = current_set_op;
    }

    pub fn coalesce_tables(&mut self) {
        coalesce_matches_and_ranges(&mut self.matches8, &mut self.ranges8);
        coalesce_matches_and_ranges(&mut self.matches32, &mut self.ranges32);

        if self.matches8.is_empty()
            && self.matches32.is_empty()
            && self.ranges8.len() == 1
            && self.ranges32.len() == 1
            && self.ranges8[0].begin == 0
            && self.ranges8[0].end == 0xff
            && self.ranges32[0].begin == 0x100
            && self.ranges32[0].end == UCHAR_MAX_VALUE
        {
            self.any_character = true;
        }
    }

    pub fn has_non_bmp_characters(&self) -> bool {
        self.character_widths & CharacterClassWidths::HasNonBMPChars
    }

    pub fn character_widths(&self) -> CharacterClassWidths {
        self.character_widths
    }

    pub fn compute_character_widths(character_class: &CharacterClass) -> CharacterClassWidths {
        let mut widths = CharacterClassWidths::Unknown;
        if !character_class.matches8.is_empty() || !character_class.ranges8.is_empty() {
            widths |= CharacterClassWidths::HasBMPChars;
        }
        for &ch in &character_class.matches32 {
            widths |= if u_is_bmp(ch) {
                CharacterClassWidths::HasBMPChars
            } else {
                CharacterClassWidths::HasNonBMPChars
            };
        }
        for range in &character_class.ranges32 {
            if u_is_bmp(range.begin) {
                widths |= CharacterClassWidths::HasBMPChars;
            }
            if !u_is_bmp(range.end) {
                widths |= CharacterClassWidths::HasNonBMPChars;
            }
        }
        widths
    }

    pub fn any_character(&self) -> bool {
        self.any_character
    }

    pub fn is_union_set_op(&self) -> bool {
        self.set_op == CharacterClassSetOp::Default || self.set_op == CharacterClassSetOp::Union
    }
}

/// `YarrPatternConstructor::UnresolvedForwardReference`: o `PatternAlternative*` do C++ é o
/// `AlternativeId`, e `term()` recebe o `YarrPattern` que é dono da alternativa.
#[derive(Clone, Debug)]
pub struct UnresolvedForwardReference {
    alternative: crate::yarr::yarr_pattern::AlternativeId,
    term_index: u32,
    named_group: crate::wtf::text::wtf_string::String,
}

impl UnresolvedForwardReference {
    pub fn new(alternative: crate::yarr::yarr_pattern::AlternativeId, term_index: u32) -> Self {
        UnresolvedForwardReference {
            alternative,
            term_index,
            named_group: crate::wtf::text::wtf_string::String::default(),
        }
    }

    pub fn new_with_named_group(
        alternative: crate::yarr::yarr_pattern::AlternativeId,
        term_index: u32,
        named_group: crate::wtf::text::wtf_string::String,
    ) -> Self {
        UnresolvedForwardReference { alternative, term_index, named_group }
    }

    pub fn term<'p>(&self, pattern: &'p mut crate::yarr::yarr_pattern::YarrPattern) -> &'p mut crate::yarr::yarr_pattern::PatternTerm {
        &mut pattern.alternative_mut(self.alternative).terms[self.term_index as usize]
    }

    pub fn alternative(&self) -> crate::yarr::yarr_pattern::AlternativeId {
        self.alternative
    }

    pub fn term_index(&self) -> u32 {
        self.term_index
    }

    pub fn has_named_group(&self) -> bool {
        !self.named_group.is_null()
    }

    pub fn named_group(&self) -> &crate::wtf::text::wtf_string::String {
        &self.named_group
    }
}

/// Cria a disjunção do corpo com a primeira alternativa e a registra como `m_body` (o trecho que o
/// construtor e `resetForReparsing` do C++ repetem). Devolve a alternativa corrente.
fn add_new_body_disjunction(pattern: &mut crate::yarr::yarr_pattern::YarrPattern) -> crate::yarr::yarr_pattern::AlternativeId {
    let body = crate::yarr::yarr_pattern::DisjunctionId(pattern.disjunctions.len() as u32);
    pattern.disjunctions.push(crate::yarr::yarr_pattern::PatternDisjunction::new(None));
    pattern.body = body;
    pattern
        .disjunction_mut(body)
        .add_new_alternative(body, 1, crate::yarr::yarr_pattern::MatchDirection::Forward)
}

/// O lambda `namesThisGroup` de `resolveForwardReferencesInLookbehindTo`. A entrada do nome é
/// `[id, id]` para um nome único e `[duplicateNamedGroupId, id1, id2, ...]` para um duplicado
/// (`addCaptureGroupForName`), então a posição `[0]` nunca é comparada: é uma repetição do id ou não
/// é um id de subpadrão.
fn names_this_group(
    named_group_to_paren_indices: &std::collections::HashMap<crate::wtf::text::wtf_string::String, Vec<u32>>,
    name: &crate::wtf::text::wtf_string::String,
    subpattern_id: u32,
) -> bool {
    match named_group_to_paren_indices.get(name) {
        None => false,
        Some(ids) => ids.len() > 1 && ids[1..].contains(&subpattern_id),
    }
}

impl<'a> YarrPatternConstructor<'a> {
    pub fn new(pattern: &'a mut crate::yarr::yarr_pattern::YarrPattern, flags: crate::yarr::yarr_flags::FlagSet) -> Self {
        let base_character_class_constructor = CharacterClassConstructor::new(pattern.ignore_case(), pattern.compile_mode());
        let alternative = add_new_body_disjunction(pattern);

        let mut parenthesis_context = ParenthesisContext::new();
        parenthesis_context.set_flags(flags);

        YarrPatternConstructor {
            pattern,
            alternative,
            base_character_class_constructor,
            current_character_class_constructor: None,
            character_class_stack: Vec::new(),
            forward_references_in_lookbehind: Vec::new(),
            stack_check: Default::default(),
            factoring_budget: 0,
            error: crate::yarr::yarr_error_code::ErrorCode::NoError,
            invert_character_class: false,
            parenthesis_context,
            initial_flags: flags,
            flags,
        }
    }


    pub fn add_capture_group_for_name(&mut self, group_name: crate::wtf::text::wtf_string::String, subpattern_id: u32) {
        self.pattern.has_named_capture_groups = true;

        let is_new_entry = !self.pattern.named_group_to_paren_indices.contains_key(&group_name);
        let this_group_name_subpattern_ids = self
            .pattern
            .named_group_to_paren_indices
            .entry(group_name.clone())
            .or_insert_with(Vec::new);
        if is_new_entry {
            while (self.pattern.capture_group_names.len() as u32) < subpattern_id {
                self.pattern.capture_group_names.push(crate::wtf::text::wtf_string::String::default());
            }
            self.pattern.capture_group_names.push(group_name);

            this_group_name_subpattern_ids.push(subpattern_id);
        } else if this_group_name_subpattern_ids.len() == 2 {
            // This named group is now a duplicate.
            self.pattern.num_duplicate_named_capture_groups += 1;
            this_group_name_subpattern_ids[0] = self.pattern.num_duplicate_named_capture_groups;
        }

        this_group_name_subpattern_ids.push(subpattern_id);
    }

    // A forward reference inside a lookbehind becomes a backreference as soon as the group it
    // names closes within that lookbehind. Doing this at the group's own close (rather than only
    // when the lookbehind closes) matters when an enclosing group is later quantified: quantifyAtom
    // deep-copies the group, and a copy of a still-unresolved reference would stay a forward
    // reference forever (/(?<=(?:\1(a))+)b/ used to match "xab").
    // A still-pending forward reference that quantifyAtom (or optimizeBOL) deep-copies must be
    // resolved in every copy, so each copy gets its own pending entry.
    pub fn register_copied_forward_references(
        &mut self,
        source: crate::yarr::yarr_pattern::AlternativeId,
        copy: crate::yarr::yarr_pattern::AlternativeId,
        source_term_indices: &[u32],
    ) {
        if self.forward_references_in_lookbehind.is_empty() {
            return;
        }
        let mut added: Vec<UnresolvedForwardReference> = Vec::new();
        for reference in &self.forward_references_in_lookbehind {
            if reference.alternative() != source {
                continue;
            }
            for copied_index in 0..source_term_indices.len() {
                if source_term_indices[copied_index] != reference.term_index() {
                    continue;
                }
                if reference.has_named_group() {
                    added.push(UnresolvedForwardReference::new_with_named_group(
                        copy,
                        copied_index as u32,
                        reference.named_group().clone(),
                    ));
                } else {
                    added.push(UnresolvedForwardReference::new(copy, copied_index as u32));
                }
            }
        }
        self.forward_references_in_lookbehind.extend(added);
    }

    pub fn resolve_forward_references_in_lookbehind_to(&mut self, subpattern_id: u32) {
        if self.forward_references_in_lookbehind.is_empty() {
            return;
        }
        let pattern = &mut *self.pattern;
        // `removeAllMatching`: o closure do `retain` devolve `true` para o que fica.
        self.forward_references_in_lookbehind.retain(|reference| {
            if reference.has_named_group() {
                if !names_this_group(&pattern.named_group_to_paren_indices, reference.named_group(), subpattern_id) {
                    return true;
                }
                let term = reference.term(pattern);
                if term.type_ != crate::yarr::yarr_pattern::PatternTermType::NamedForwardReference {
                    return true;
                }
                term.set_back_reference_subpattern_id(subpattern_id);
                term.convert_to_named_backreference();
            } else {
                let term = reference.term(pattern);
                let current_id = match term.payload {
                    crate::yarr::yarr_pattern::TermPayload::BackReferenceSubpatternId(id) => Some(id),
                    _ => None,
                };
                if current_id != Some(subpattern_id) || term.type_ != crate::yarr::yarr_pattern::PatternTermType::NumberedForwardReference {
                    return true;
                }
                term.convert_to_numbered_backreference();
            }
            pattern.contains_backreferences = true;
            false
        });
    }
}
