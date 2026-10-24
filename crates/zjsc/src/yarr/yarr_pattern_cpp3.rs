// Fatia 3 do porte de `yarr/YarrPattern.cpp` (linhas 1351 a 2062): do `tryConvertingForwardReferencesToBackreferences`
// ao `copyTerm` do `YarrPatternConstructor`. Incluída por `include!` em `yarr_pattern.rs`, sem `use`:
// o que não é de `yarr_pattern` vem por caminho completo ou por `use` dentro da função.

/// `current_character_class_constructor` do C++: `None` é o `m_baseCharacterClassConstructor`,
/// `Some(i)` é `m_characterClassStack[i]`. É macro (e não método) para que a borrow fique só nos dois
/// campos e o chamador possa ler `self.pattern` na mesma expressão.
macro_rules! current_ccc {
    ($s:expr) => {
        match $s.current_character_class_constructor {
            Some(i) => &mut $s.character_class_stack[i],
            None => &mut $s.base_character_class_constructor,
        }
    };
}

/// `YarrPatternConstructor::IgnoreCasePropertyClasses`. O C++ guarda os conjuntos num mapa estático
/// protegido por `Lock`; aqui é um mapa por thread (cada thread os calcula uma vez, com o mesmo
/// resultado), e as classes ficam em `Rc` para a consulta não copiar conjuntos do tamanho de `\P{L}`.
#[derive(Default)]
struct IgnoreCasePropertyClasses {
    closure: Option<std::rc::Rc<CharacterClass>>, // None: não enumerável
    inverted_core: Option<std::rc::Rc<CharacterClass>>, // None: não calculado ou não enumerável
    closure_computed: bool,
    inverted_core_computed: bool,
    inverted_core_is_base: bool,
}

thread_local! {
    static IGNORE_CASE_PROPERTY_CLASSES: std::cell::RefCell<HashMap<u32, IgnoreCasePropertyClasses>> =
        std::cell::RefCell::new(HashMap::new());
}

impl<'a> YarrPatternConstructor<'a> {
    pub fn try_converting_forward_references_to_backreferences(&mut self) {
        //  There are forward references that could actually be lookbehind back references.
        for i in 0..self.forward_references_in_lookbehind.len() {
            let unresolved_forward_reference = self.forward_references_in_lookbehind[i].clone();
            let alternative = unresolved_forward_reference.alternative();
            let term_index = unresolved_forward_reference.term_index() as usize;
            if unresolved_forward_reference.has_named_group() {
                let named_group_subpattern_id = match self
                    .pattern
                    .named_group_to_paren_indices
                    .get(unresolved_forward_reference.named_group())
                    .and_then(|indices| indices.last())
                {
                    Some(&id) => id,
                    None => continue,
                };

                let term = &mut self.pattern.alternative_mut(alternative).terms[term_index];
                if named_group_subpattern_id == term.back_reference_subpattern_id() {
                    term.set_back_reference_subpattern_id(0);
                    continue;
                }
                term.set_back_reference_subpattern_id(named_group_subpattern_id);
                term.convert_to_named_backreference();
                self.pattern.contains_backreferences = true;
            } else {
                let num_subpatterns = self.pattern.num_subpatterns;
                let term = &mut self.pattern.alternative_mut(alternative).terms[term_index];
                let back_reference_subpattern_id = term.back_reference_subpattern_id();
                if back_reference_subpattern_id != 0 && back_reference_subpattern_id <= num_subpatterns {
                    term.convert_to_numbered_backreference();
                    self.pattern.contains_backreferences = true;
                }
            }
        }

        self.forward_references_in_lookbehind.clear();
    }

    /// `m_alternative->m_terms.append(PatternTerm(characterClass, invert, m_flags, parenthesisMatchDirection()))`,
    /// que o C++ repete em cada ramo de `atomBuiltInCharacterClass`.
    fn append_character_class_term(&mut self, character_class: CharacterClassId, invert: bool) {
        let term = PatternTerm::new_character_class(character_class, invert, self.flags, self.parenthesis_match_direction());
        self.pattern.alternative_mut(self.alternative).terms.push(term);
    }





    // Case-insensitive Unicode property escapes.
    //
    // Character classes are matched by plain code point membership, and case-insensitivity is
    // handled by building classes that already hold every case variant (putChar / putRange add a
    // character's whole canonical-equivalence group). Property escapes used to be appended
    // verbatim, so /\p{Lu}/iu did not match 'a'. Under /iu the CharacterSetMatcher accepts ch when
    // some member canonicalizes like ch, i.e. the class must be the closure of the property set:
    //   \p{X}   -> closure(X)                       [^\p{X}] -> its complement (the term's invert)
    //   \P{X}   -> closure(complement(X)) = complement(X - closure(closure(X) - X))
    // Under /iv sets are folded first and ch is folded before the membership test, which in
    // membership terms is closure(X) for \p{X} and complement(closure(X)) for \P{X}.
    fn put_code_points(constructor: &mut CharacterClassConstructor, character_class: &CharacterClass) {
        for &ch in &character_class.matches8 {
            constructor.put_char(ch);
        }
        for range in &character_class.ranges8 {
            constructor.put_range(range.begin, range.end);
        }
        for &ch in &character_class.matches32 {
            constructor.put_char(ch);
        }
        for range in &character_class.ranges32 {
            constructor.put_range(range.begin, range.end);
        }
    }

    // These sets depend only on the property, so they are built once per process (patterns are
    // rebuilt several times per RegExp, and \P{L}-sized closures take milliseconds) and shared;
    // the classes are immutable once published.
    // (No porte o conjunto é compartilhado por `Rc` na cache; cada uso o copia para o
    // `character_classes` do padrão, que é dono de todas as classes dos termos.)

    // closure(X) for a property class, or None if the class is not enumerable (table only / strings).
    fn ignore_case_unicode_property_closure(
        &mut self,
        class_id: crate::yarr::yarr::BuiltInCharacterClassID,
    ) -> Option<std::rc::Rc<CharacterClass>> {
        let closure_computed = IGNORE_CASE_PROPERTY_CLASSES
            .with(|map| map.borrow().get(&class_id.0).is_some_and(|entry| entry.closure_computed));
        if !closure_computed {
            let base_id = self.pattern.unicode_character_class_for(class_id);
            let base = self.pattern.character_class(base_id);
            let mut closure = None;
            if base.has_single_characters() && !base.has_strings() && !base.any_character {
                let mut closed = CharacterClassConstructor::new(true, CompileMode::UnicodeSets);
                Self::put_code_points(&mut closed, base);
                closure = Some(std::rc::Rc::new(closed.char_class()));
            }
            IGNORE_CASE_PROPERTY_CLASSES.with(|map| {
                let mut map = map.borrow_mut();
                let entry = map.entry(class_id.0).or_default();
                entry.closure = closure;
                entry.closure_computed = true;
            });
        }
        IGNORE_CASE_PROPERTY_CLASSES.with(|map| map.borrow().get(&class_id.0).and_then(|entry| entry.closure.clone()))
    }

    /// `ignoreCaseUnicodePropertyClass`: a classe fechada, já copiada para o padrão.
    fn ignore_case_unicode_property_class(&mut self, class_id: crate::yarr::yarr::BuiltInCharacterClassID) -> Option<CharacterClassId> {
        let closure = self.ignore_case_unicode_property_closure(class_id)?;
        Some(self.pattern.append_character_class((*closure).clone()))
    }

    // K = X - closure(closure(X) - X): \P{X} under /iu is exactly [^K]. None when not enumerable.
    fn ignore_case_inverted_unicode_property_core(
        &mut self,
        class_id: crate::yarr::yarr::BuiltInCharacterClassID,
    ) -> Option<CharacterClassId> {
        use crate::yarr::yarr_parser::CharacterClassSetOp;

        let closure = self.ignore_case_unicode_property_closure(class_id);
        let base_id = self.pattern.unicode_character_class_for(class_id);
        let cached = IGNORE_CASE_PROPERTY_CLASSES.with(|map| {
            map.borrow()
                .get(&class_id.0)
                .filter(|entry| entry.inverted_core_computed)
                .map(|entry| (entry.inverted_core_is_base, entry.inverted_core.clone()))
        });
        if let Some((inverted_core_is_base, inverted_core)) = cached {
            if inverted_core_is_base {
                return Some(base_id);
            }
            return inverted_core.map(|core| self.pattern.append_character_class((*core).clone()));
        }

        let mut inverted_core_is_base = false;
        let mut inverted_core: Option<std::rc::Rc<CharacterClass>> = None;
        if let Some(closure) = &closure {
            let base = self.pattern.character_class(base_id);
            let same_set = closure.matches8 == base.matches8
                && closure.matches32 == base.matches32
                && closure.ranges8 == base.ranges8
                && closure.ranges32 == base.ranges32;
            if same_set {
                inverted_core_is_base = true; // closed under case already (Any, ...): [^X] is exact
            } else {
                let mut boundary = CharacterClassConstructor::new(false, CompileMode::UnicodeSets); // closure(X) - X
                boundary.append(closure);
                boundary.combining_set_op(CharacterClassSetOp::Subtraction);
                boundary.append(base);
                let boundary_class = boundary.char_class();

                let mut mixed = CharacterClassConstructor::new(true, CompileMode::UnicodeSets); // groups only partly inside X
                Self::put_code_points(&mut mixed, &boundary_class);
                let mixed_class = mixed.char_class();

                let mut core = CharacterClassConstructor::new(false, CompileMode::UnicodeSets); // X minus those groups
                core.append(base);
                core.combining_set_op(CharacterClassSetOp::Subtraction);
                core.append(&mixed_class);
                inverted_core = Some(std::rc::Rc::new(core.char_class()));
            }
        }
        IGNORE_CASE_PROPERTY_CLASSES.with(|map| {
            let mut map = map.borrow_mut();
            let entry = map.entry(class_id.0).or_default();
            entry.inverted_core_is_base = inverted_core_is_base;
            entry.inverted_core = inverted_core.clone();
            entry.inverted_core_computed = true;
        });
        if inverted_core_is_base {
            return Some(base_id);
        }
        inverted_core.map(|core| self.pattern.append_character_class((*core).clone()))
    }

    fn is_ignore_case_unicode_property(&self, class_id: crate::yarr::yarr::BuiltInCharacterClassID) -> bool {
        class_id.0 >= crate::yarr::yarr::BuiltInCharacterClassID::BaseUnicodePropertyID.0
            && self.ignore_case()
            && self.pattern.either_unicode()
    }











    // A class that contains strings matches, at each position, its longest member that matches there
    // (ClassStringDisjunction semantics), then backtracks to shorter ones: one alternative per string,
    // longest first, then the single characters, then the empty member. Prefix factoring
    // (factorAlternatives) later shares common prefixes among those alternatives where that is safe,
    // and the JIT dispatches them on their first code point, so \p{RGI_Emoji}'s ~3,800 alternatives
    // are not tried one after the other. A list long enough to be dispatched sits in a group of
    // its own inside the group the atom's quantifier (if any) applies to, because the dispatcher
    // serves once-through groups only: \p{RGI_Emoji}+ then iterates at the speed \p{RGI_Emoji}
    // matches. (Shorter lists keep the single group, the shape checkForTerminalParentheses can
    // turn into a string list.)
    fn expand_class_with_strings(&mut self, character_class_id: CharacterClassId) {
        use crate::yarr::yarr::{ALTERNATION_DISPATCH_MIN_ALTERNATIVES, ALTERNATION_DISPATCH_MIN_TOTAL_SIZE};
        use crate::yarr::yarr_parser::CreateDisjunctionPurpose;

        let (strings, has_single_characters, any_character) = {
            let character_class = self.pattern.character_class(character_class_id);
            (
                character_class.strings.clone(),
                character_class.has_single_characters(),
                character_class.any_character,
            )
        };

        // (Only where the JIT could dispatch it -- forward, exact case, no empty member, and past
        // the dispatcher's size floor -- since the extra group otherwise just costs frame slots.)
        let mut string_count: usize = 0;
        let mut total_string_length: usize = 0;
        let mut has_empty_member = false;
        for string in &strings {
            has_empty_member |= string.is_empty();
            string_count += 1;
            total_string_length += string.len();
        }
        let dispatch_group = self.parenthesis_match_direction() == MatchDirection::Forward
            && !self.ignore_case()
            && !has_empty_member
            && string_count >= ALTERNATION_DISPATCH_MIN_ALTERNATIVES as usize
            && total_string_length >= ALTERNATION_DISPATCH_MIN_TOTAL_SIZE as usize;
        if dispatch_group {
            self.atom_parentheses_subpattern_begin(false, None);
        }
        self.atom_parentheses_subpattern_begin(false, None);
        let mut alternative_count: u32 = 0;
        let mut has_empty_string = false;
        for string in &strings {
            if string.is_empty() {
                has_empty_string = true;
                continue;
            }
            if alternative_count != 0 {
                self.disjunction(CreateDisjunctionPurpose::ForNextAlternative);
            }
            for &ch in string {
                self.atom_pattern_character(ch, /* hyphenIsRange */ false);
            }
            alternative_count += 1;
        }
        if has_single_characters || any_character {
            if alternative_count != 0 {
                self.disjunction(CreateDisjunctionPurpose::ForNextAlternative);
            }
            self.append_character_class_term(character_class_id, false);
            alternative_count += 1;
        }
        if has_empty_string && alternative_count != 0 {
            self.disjunction(CreateDisjunctionPurpose::ForNextAlternative);
        }
        self.atom_parentheses_end();
        if dispatch_group {
            self.atom_parentheses_end();
        }
    }








    // deep copy the argument disjunction.  If filterStartsWithBOL is true,
    // skip alternatives with m_startsWithBOL set true, and those left impossible by that filtering.
    //
    // O C++ monta a disjunção nova solta e só a anexa a `m_disjunctions` no fim (depois das cópias
    // aninhadas, que se anexam antes). Para manter essa ordem de índices, a nova nasce com um id
    // provisório (`u32::MAX`), que as alternativas copiadas e as referências pendentes registradas
    // carregam até o id verdadeiro existir.
    pub fn copy_disjunction(&mut self, disjunction: DisjunctionId, filter_starts_with_bol: bool) -> Option<DisjunctionId> {
        use crate::yarr::yarr_error_code::{has_error, ErrorCode};

        const PROVISIONAL: DisjunctionId = DisjunctionId(u32::MAX);

        if !self.is_safe_to_recurse() {
            self.error = ErrorCode::PatternTooLarge;
            return None;
        }

        let mut new_disjunction: Option<PatternDisjunction> = None;
        let alternative_count = self.pattern.disjunction(disjunction).alternatives.len();
        for alt in 0..alternative_count {
            let source = AlternativeId { disjunction, index: alt as u32 };
            let (starts_with_bol, match_direction, first_subpattern_id, last_subpattern_id) = {
                let alternative = self.pattern.alternative(source);
                (
                    alternative.starts_with_bol,
                    alternative.match_direction(),
                    alternative.first_subpattern_id,
                    alternative.last_subpattern_id,
                )
            };
            if filter_starts_with_bol && starts_with_bol && match_direction != MatchDirection::Backward {
                continue;
            }

            let mut source_term_indices: Vec<u32> = Vec::new();
            let Some(copied_terms) = self.copy_terms(source, filter_starts_with_bol, &mut source_term_indices) else {
                continue;
            };

            if new_disjunction.is_none() {
                new_disjunction = Some(PatternDisjunction::new(self.pattern.disjunction(disjunction).parent));
            }
            let Some(new_disjunction) = new_disjunction.as_mut() else {
                continue;
            };
            let new_alternative = new_disjunction.add_new_alternative(PROVISIONAL, first_subpattern_id, match_direction);
            let alternative = &mut new_disjunction.alternatives[new_alternative.index as usize];
            alternative.last_subpattern_id = last_subpattern_id;
            alternative.terms = copied_terms;
            self.register_copied_forward_references(source, new_alternative, &source_term_indices);
        }

        if has_error(self.error) {
            return None;
        }

        let mut new_disjunction = new_disjunction?;

        let copied_disjunction = DisjunctionId(self.pattern.disjunctions.len() as u32);
        for alternative in &mut new_disjunction.alternatives {
            alternative.parent = copied_disjunction;
        }
        for reference in &mut self.forward_references_in_lookbehind {
            if reference.alternative.disjunction == PROVISIONAL {
                reference.alternative.disjunction = copied_disjunction;
            }
        }
        self.pattern.disjunctions.push(new_disjunction);
        Some(copied_disjunction)
    }

    // True when this parenthesis has to participate in every match of its alternative. An optional
    // one can be skipped, and a negative assertion succeeds when its content cannot match.
    fn parentheses_must_match(term: &PatternTerm) -> bool {
        term.quantity_min_count != 0 && !term.invert()
    }

    // Copy the terms of `alternative`, dropping the parentheses copyTerm() filtered out. Returns
    // None when one of those has to be matched, i.e. this alternative cannot match at all.
    fn copy_terms(
        &mut self,
        alternative: AlternativeId,
        filter_starts_with_bol: bool,
        source_term_indices: &mut Vec<u32>,
    ) -> Option<Vec<PatternTerm>> {
        let term_count = self.pattern.alternative(alternative).terms.len();
        let mut copied_terms: Vec<PatternTerm> = Vec::with_capacity(term_count);
        for term_index in 0..term_count {
            let term = self.pattern.alternative(alternative).terms[term_index];
            if let Some(copied) = self.copy_term(term, filter_starts_with_bol) {
                copied_terms.push(copied);
                source_term_indices.push(term_index as u32);
                continue;
            }
            // Every alternative inside this parenthesis was filtered out, so it can only match at
            // the start of the input.
            if Self::parentheses_must_match(&term) {
                return None;
            }
        }
        Some(copied_terms)
    }

    fn copy_term(&mut self, term: PatternTerm, filter_starts_with_bol: bool) -> Option<PatternTerm> {
        if !self.is_safe_to_recurse() {
            self.error = crate::yarr::yarr_error_code::ErrorCode::PatternTooLarge;
            return Some(term);
        }

        if term.type_ != PatternTermType::ParenthesesSubpattern && term.type_ != PatternTermType::ParentheticalAssertion {
            return Some(term);
        }

        if let Some(new_disjunction) =
            self.copy_disjunction(term.parentheses().disjunction, filter_starts_with_bol && !term.invert())
        {
            let mut term_copy = term;
            term_copy.parentheses_mut().disjunction = new_disjunction;
            self.pattern.has_copied_paren_subexpressions = true;
            return Some(term_copy);
        }
        None
    }
}
