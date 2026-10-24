// Porte de `yarr/YarrInterpreter.cpp`, linhas 1201 a 1777: de `matchParenthesesOnceEnd` até
// `matchDotStarEnclosure` (os membros de `Interpreter<CharType>` ligados a parênteses, asserções
// parentéticas e ao enclosure `.*`). Incluído por `include!` no fim de `yarr_interpreter.rs`.
//
// Usa o contrato unificado descrito no topo de `yarr_interpreter_cpp1.rs` (`DisjunctionContextRef`,
// `context_mut`, contextos de parênteses como `usize`). `match_disjunction` é da fatia 4;
// `record_parentheses_match`, `reset_matches` e `parentheses_do_backtrack` são da fatia 2.
//
// O `BackTrackInfoParentheses` do C++ é um `reinterpret_cast` do quadro; aqui `match_parentheses` e
// `backtrack_parentheses` o carregam do quadro (`load`), operam sobre a cópia local e a gravam de volta
// (`store`) antes de devolver, o que é equivalente porque nenhum contexto aninhado lê esse trecho do
// quadro. Como a alocação do Rust não falha, os ramos `if (!context) return ErrorNoMemory` de
// `allocParenthesesDisjunctionContext` não existem (mesma decisão da fatia 1).

impl<'a, C: crate::wtf::text::string_impl::CharType> Interpreter<'a, C> {
    pub fn match_parentheses_once_end(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.atom.quantity_max_count == 1);
        let pattern = self.pattern;

        if term.capture() {
            let subpattern_id = term.subpattern_id() as usize;
            // Para casamentos Backward, os índices capturados são gravados fim e depois início.
            self.output[(subpattern_id << 1) + 1 - term.match_direction() as usize] =
                self.input.get_pos().wrapping_sub(term.input_position);

            if term.duplicate_named_group_id() != 0 {
                // Registra qual dos subpadrões nomeados duplicados casou.
                let offset = pattern.offset_for_duplicate_named_group_id(term.duplicate_named_group_id());
                self.output[offset as usize] = subpattern_id as u32;
            }
        }

        if term.atom.quantity_type == QuantifierType::FixedCount {
            return true;
        }

        let begin_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParenthesesOnce::begin_index()) as usize;
        self.context_mut(context).frame[begin_slot] != self.input.get_pos() as usize
    }

    pub fn backtrack_parentheses_once_begin(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.atom.quantity_max_count == 1);
        let pattern = self.pattern;
        let begin_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParenthesesOnce::begin_index()) as usize;

        if term.capture() {
            let subpattern_id = term.subpattern_id() as usize;
            self.output[subpattern_id << 1] = crate::yarr::yarr::OFFSET_NO_MATCH;
            self.output[(subpattern_id << 1) + 1] = crate::yarr::yarr::OFFSET_NO_MATCH;

            if term.duplicate_named_group_id() != 0 {
                // Limpa o subpatternId que casou.
                let offset = pattern.offset_for_duplicate_named_group_id(term.duplicate_named_group_id());
                self.output[offset as usize] = 0;
            }
        }

        match term.atom.quantity_type {
            QuantifierType::Greedy => {
                // Se voltamos até aqui, há outra chance: tentar casar nada.
                // `notFound` do WTF é `usize::MAX`.
                let frame_context = self.context_mut(context);
                debug_assert!(frame_context.frame[begin_slot] != usize::MAX);
                frame_context.frame[begin_slot] = usize::MAX;
                frame_context.term += term.atom.parentheses_width as i32;
                true
            }
            QuantifierType::NonGreedy | QuantifierType::FixedCount => false,
        }
    }

    pub fn backtrack_parentheses_once_end(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.atom.quantity_max_count == 1);
        let begin_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParenthesesOnce::begin_index()) as usize;

        match term.atom.quantity_type {
            QuantifierType::Greedy | QuantifierType::NonGreedy => {
                let begin = self.context_mut(context).frame[begin_slot];
                if term.atom.quantity_type == QuantifierType::Greedy && begin == usize::MAX {
                    self.context_mut(context).term -= term.atom.parentheses_width as i32;
                    return false;
                }
                if begin == usize::MAX {
                    let pos = self.input.get_pos();
                    self.context_mut(context).frame[begin_slot] = pos as usize;
                    if term.capture() {
                        // Tecnicamente este acesso a inputPosition deveria usar o do termo Begin,
                        // mas para repetições que não sejam de contagem fixa os valores são iguais
                        // (não há pré-checagem para casamentos gulosos ou não gulosos).
                        let subpattern_id = term.subpattern_id() as usize;
                        // Para casamentos Backward, os índices capturados são gravados fim e depois início.
                        self.output[(subpattern_id << 1) + term.match_direction() as usize] =
                            pos.wrapping_sub(term.input_position);
                    }
                    self.context_mut(context).term -= term.atom.parentheses_width as i32;
                    return true;
                }
                false
            }
            QuantifierType::FixedCount => false,
        }
    }

    pub fn match_parentheses_terminal_begin(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.atom.quantity_type == QuantifierType::Greedy);
        debug_assert!(term.atom.quantity_max_count == crate::yarr::yarr::QUANTIFY_INFINITE);
        debug_assert!(!term.capture());

        let pos = self.input.get_pos() as usize;
        let begin_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParenthesesTerminal::begin_index()) as usize;
        let entry_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParenthesesTerminal::entry_position_index()) as usize;
        let frame = &mut self.context_mut(context).frame;
        frame[begin_slot] = pos;
        frame[entry_slot] = pos;
        true
    }

    pub fn match_parentheses_terminal_end(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        let begin_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParenthesesTerminal::begin_index()) as usize;
        let entry_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParenthesesTerminal::entry_position_index()) as usize;
        let pos = self.input.get_pos() as usize;

        let frame_context = self.context_mut(context);
        if frame_context.frame[begin_slot] == pos {
            // Uma iteração vazia não pode ser repetida, então só é aceitável como a única
            // iteração que um mínimo de um exige, e só antes de qualquer consumo. Limpar
            // entryPosition registra que o mínimo foi atendido e rejeita qualquer nova
            // iteração vazia.
            if term.atom.quantity_min_count == 0 || frame_context.frame[entry_slot] != pos {
                return false;
            }
            frame_context.frame[entry_slot] = usize::MAX;
        }

        frame_context.frame[begin_slot] = pos;

        // Casamento bem-sucedido! O que vem agora? Voltar ao laço e tentar casar mais!
        // Volta ao primeiro termo do corpo, e não ao ParenthesesSubpatternTerminalBegin, cujas
        // gravações inicializam o grupo inteiro e não devem rodar de novo a cada iteração.
        frame_context.term -= term.atom.parentheses_width as i32;
        true
    }

    pub fn backtrack_parentheses_terminal_begin(
        &mut self,
        term: &ByteTerm,
        context: DisjunctionContextRef,
    ) -> bool {
        debug_assert!(term.atom.quantity_type == QuantifierType::Greedy);
        debug_assert!(term.atom.quantity_max_count == crate::yarr::yarr::QUANTIFY_INFINITE);
        debug_assert!(!term.capture());

        // Se voltamos até aqui, esta iteração dos parênteses falhou. Nada segue um grupo
        // terminal, então um mínimo já satisfeito torna essa falha um fim aceitável do casamento;
        // um mínimo não satisfeito falha o casamento.
        let pos = self.input.get_pos() as usize;
        let frame_context = self.context_mut(context);
        if term.atom.quantity_min_count != 0 {
            let entry_slot = (term.frame_location
                + crate::yarr::yarr_pattern::BackTrackInfoParenthesesTerminal::entry_position_index())
                as usize;
            if frame_context.frame[entry_slot] == pos {
                return false;
            }
        }

        frame_context.term += term.atom.parentheses_width as i32;
        true
    }

    pub fn backtrack_parentheses_terminal_end(&mut self, _term: &ByteTerm, _context: DisjunctionContextRef) -> bool {
        // Parênteses 'terminais' ficam no fim da regex e, portanto, um casamento além do fim
        // sempre deve ser devolvido como sucesso: nunca devemos voltar até aqui.
        unreachable!("RELEASE_ASSERT_NOT_REACHED")
    }

    pub fn match_parenthetical_assertion_begin(
        &mut self,
        term: &ByteTerm,
        context: DisjunctionContextRef,
    ) -> bool {
        debug_assert!(term.atom.quantity_max_count == 1);

        let pos = self.input.get_pos() as usize;
        let begin_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParentheticalAssertion::begin_index()) as usize;
        self.context_mut(context).frame[begin_slot] = pos;

        true
    }

    pub fn match_parenthetical_assertion_end(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.atom.quantity_max_count == 1);

        let begin_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParentheticalAssertion::begin_index()) as usize;
        let begin = self.context_mut(context).frame[begin_slot];

        self.input.set_pos(begin as u32);

        // Chegamos ao fim dos parênteses; se estão invertidos, isto é falha.
        if term.invert() {
            if term.contains_any_captures() {
                for subpattern in term.subpattern_id()..=term.last_subpattern_id() {
                    self.output[(subpattern << 1) as usize] = crate::yarr::yarr::OFFSET_NO_MATCH;
                    self.output[((subpattern << 1) + 1) as usize] = crate::yarr::yarr::OFFSET_NO_MATCH;
                }
            }
            self.context_mut(context).term -= term.atom.parentheses_width as i32;
            return false;
        }

        true
    }

    pub fn backtrack_parenthetical_assertion_begin(
        &mut self,
        term: &ByteTerm,
        context: DisjunctionContextRef,
    ) -> bool {
        debug_assert!(term.atom.quantity_max_count == 1);

        if term.match_direction() == MatchDirection::Backward {
            let begin_slot = (term.frame_location
                + crate::yarr::yarr_pattern::BackTrackInfoParentheticalAssertion::begin_index())
                as usize;
            let begin = self.context_mut(context).frame[begin_slot];
            self.input.set_pos(begin as u32);
        }

        // Falhamos em casar os parênteses; se estão invertidos, isto é vitória!
        if term.invert() {
            self.context_mut(context).term += term.atom.parentheses_width as i32;
            return true;
        }

        false
    }

    pub fn backtrack_parenthetical_assertion_end(
        &mut self,
        term: &ByteTerm,
        context: DisjunctionContextRef,
    ) -> bool {
        debug_assert!(term.atom.quantity_max_count == 1);

        let begin_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParentheticalAssertion::begin_index()) as usize;
        let begin = self.context_mut(context).frame[begin_slot];

        self.input.set_pos(begin as u32);

        if term.contains_any_captures() {
            for subpattern in term.subpattern_id()..=term.last_subpattern_id() {
                self.output[(subpattern << 1) as usize] = crate::yarr::yarr::OFFSET_NO_MATCH;
                self.output[((subpattern << 1) + 1) as usize] = crate::yarr::yarr::OFFSET_NO_MATCH;
            }
        }

        self.context_mut(context).term -= term.atom.parentheses_width as i32;
        false
    }

    /// `matchParentheses`: o `BackTrackInfoParentheses` é carregado do quadro e gravado de volta ao
    /// fim (ver o cabeçalho do arquivo).
    pub fn match_parentheses(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> crate::yarr::yarr::JSRegExpResult {
        let slot = term.frame_location as usize;
        let mut back_track = BackTrackInfoParentheses::load(&self.context_mut(context).frame, slot);
        let result = self.match_parentheses_with_back_track(term, &mut back_track);
        back_track.store(&mut self.context_mut(context).frame, slot);
        result
    }

    fn match_parentheses_with_back_track(
        &mut self,
        term: &ByteTerm,
        back_track: &mut BackTrackInfoParentheses,
    ) -> crate::yarr::yarr::JSRegExpResult {
        use crate::yarr::yarr::JSRegExpResult;
        let pattern = self.pattern;
        let Some(body_id) = term.atom.parentheses_disjunction else {
            return JSRegExpResult::ErrorInternal;
        };
        let disjunction_body = pattern.parentheses_disjunction(body_id);

        back_track.begin = self.input.get_pos() as usize;
        back_track.match_amount = 0;
        back_track.last_context = None;

        debug_assert!(
            term.atom.quantity_type != QuantifierType::FixedCount
                || term.atom.quantity_min_count == term.atom.quantity_max_count
        );

        let minimum_match_count = term.atom.quantity_min_count;

        // Trata os casamentos fixos e a parte mínima de um casamento de tamanho variável.
        if minimum_match_count != 0 {
            // Casa as iterações obrigatórias, voltando atrás nas anteriores quando preciso.
            let result = self.refill_parentheses_contexts_to_min_count(term, back_track, disjunction_body);
            if result != JSRegExpResult::Match {
                return result;
            }

            if let Some(last) = back_track.last_context {
                self.record_parentheses_match(term, last);
            }
        }

        match term.atom.quantity_type {
            QuantifierType::FixedCount => {
                debug_assert!(back_track.match_amount == term.atom.quantity_max_count as usize);
                JSRegExpResult::Match
            }

            QuantifierType::Greedy => {
                let result = self.extend_parentheses_contexts_to_max_count(term, back_track, disjunction_body);
                if result != JSRegExpResult::Match {
                    return result;
                }

                if back_track.match_amount != 0 {
                    if let Some(last) = back_track.last_context {
                        self.record_parentheses_match(term, last);
                    }
                }
                JSRegExpResult::Match
            }

            QuantifierType::NonGreedy => JSRegExpResult::Match,
        }
    }

    /// `backtrackParentheses`: carrega o `BackTrackInfoParentheses` do quadro e o grava de volta ao
    /// fim (ver o cabeçalho do arquivo).
    pub fn backtrack_parentheses(
        &mut self,
        term: &ByteTerm,
        context: DisjunctionContextRef,
    ) -> crate::yarr::yarr::JSRegExpResult {
        let slot = term.frame_location as usize;
        let mut back_track = BackTrackInfoParentheses::load(&self.context_mut(context).frame, slot);
        let result = self.backtrack_parentheses_with_back_track(term, &mut back_track);
        back_track.store(&mut self.context_mut(context).frame, slot);
        result
    }

    // As regras de backtracking diferem conforme a repetição seja gulosa ou não gulosa.
    //
    // Casamentos gulosos nunca devem tentar apenas acrescentar mais: os casos de 'mais' já
    // foram feitos. Sempre volte atrás, ao menos um pouquinho. Porém os casos em que se
    // remove um item da lista precisam de checagem, pois nunca casamos o caso de 'um a menos'.
    // Avançando, ainda se acrescenta o máximo possível.
    //
    // Não gulosos: o caso de 'um a menos' já foi feito, então não casar ao remover.
    // O caso de 'um a mais' não foi feito, então sempre tentar acrescentá-lo.
    fn backtrack_parentheses_with_back_track(
        &mut self,
        term: &ByteTerm,
        back_track: &mut BackTrackInfoParentheses,
    ) -> crate::yarr::yarr::JSRegExpResult {
        use crate::yarr::yarr::JSRegExpResult;
        let pattern = self.pattern;
        let Some(body_id) = term.atom.parentheses_disjunction else {
            return JSRegExpResult::ErrorInternal;
        };
        let disjunction_body = pattern.parentheses_disjunction(body_id);

        match term.atom.quantity_type {
            QuantifierType::FixedCount => {
                debug_assert!(back_track.match_amount == term.atom.quantity_max_count as usize);
                debug_assert!(term.atom.quantity_min_count == term.atom.quantity_max_count);

                let mut result = self.parentheses_do_backtrack(term, back_track);
                if result != JSRegExpResult::Match {
                    return result;
                }

                // Para FixedCount min == max, então reabastecer até o mínimo reabastece até o
                // máximo; o auxiliar trata também o laço interno de backtrack do conteúdo.
                result = self.refill_parentheses_contexts_to_min_count(term, back_track, disjunction_body);
                if result != JSRegExpResult::Match {
                    return result;
                }

                debug_assert!(back_track.match_amount == term.atom.quantity_max_count as usize);
                if let Some(last) = back_track.last_context {
                    self.record_parentheses_match(term, last);
                }
                JSRegExpResult::Match
            }

            QuantifierType::Greedy => {
                if back_track.match_amount == 0 {
                    return JSRegExpResult::NoMatch;
                }

                let Some(context) = back_track.last_context else {
                    return JSRegExpResult::ErrorInternal;
                };
                // Pelo RepeatMatcher, só as iterações além do mínimo obrigatório precisam ser
                // não vazias; uma iteração obrigatória (contagem <= min) pode casar vazio,
                // então o conteúdo é refeito permitindo casamento vazio nesse caso.
                let mut result = if back_track.match_amount <= term.atom.quantity_min_count as usize {
                    self.match_disjunction(disjunction_body, DisjunctionContextRef::Parentheses(context), true)
                } else {
                    self.match_non_zero_disjunction(
                        disjunction_body,
                        DisjunctionContextRef::Parentheses(context),
                        true,
                    )
                };
                if result == JSRegExpResult::Match {
                    result = self.extend_parentheses_contexts_to_max_count(term, back_track, disjunction_body);
                    if result != JSRegExpResult::Match {
                        return result;
                    }
                } else {
                    self.reset_matches(term, context);
                    Self::pop_parentheses_disjunction_context(back_track, &self.parentheses_contexts);
                    self.free_parentheses_disjunction_context(context);

                    if result != JSRegExpResult::NoMatch {
                        return result;
                    }

                    // Quando matchAmount cai abaixo do mínimo, não desistir de imediato:
                    // tentar parenthesesDoBacktrack nos contextos restantes para achar outra
                    // distribuição de casamentos que permita chegar ao mínimo, e então
                    // reabastecer até o mínimo.
                    //
                    // Por exemplo, /((a+){2,3}){2,3}$/ casada com "aaaaaa": `a+` esgota a
                    // entrada e isso faz a contagem `2` de {2,3} falhar. Em vez de declarar
                    // falha de imediato, devemos voltar atrás em `a+`, reduzindo a quantidade
                    // de `a` esgotada, e então os parênteses têm sucesso.
                    if back_track.match_amount < term.atom.quantity_min_count as usize {
                        result = self.parentheses_do_backtrack(term, back_track);
                        if result != JSRegExpResult::Match {
                            return result;
                        }

                        // O backtrack do conteúdo funcionou. Reabastecer até a contagem mínima.
                        result = self.refill_parentheses_contexts_to_min_count(term, back_track, disjunction_body);
                        if result != JSRegExpResult::Match {
                            return result;
                        }

                        // E agora expandir o casamento até a contagem máxima; só então o
                        // guloso está totalmente revisitado.
                        result = self.extend_parentheses_contexts_to_max_count(term, back_track, disjunction_body);
                        if result != JSRegExpResult::Match {
                            return result;
                        }
                    }
                }

                if back_track.match_amount != 0 {
                    if let Some(last) = back_track.last_context {
                        self.record_parentheses_match(term, last);
                    }
                }
                JSRegExpResult::Match
            }

            QuantifierType::NonGreedy => {
                // Se não chegamos ao limite, tentar acrescentar mais um casamento.
                if back_track.match_amount < term.atom.quantity_max_count as usize {
                    let Some(context) = self.alloc_parentheses_disjunction_context(disjunction_body, term) else {
                        return JSRegExpResult::ErrorNoMemory;
                    };
                    let result = self.match_non_zero_disjunction(
                        disjunction_body,
                        DisjunctionContextRef::Parentheses(context),
                        false,
                    );
                    if result == JSRegExpResult::Match {
                        Self::append_parentheses_disjunction_context(
                            back_track,
                            context,
                            &mut self.parentheses_contexts[context],
                        );
                        self.record_parentheses_match(term, context);
                        return JSRegExpResult::Match;
                    }

                    self.reset_matches(term, context);
                    self.free_parentheses_disjunction_context(context);

                    if result != JSRegExpResult::NoMatch {
                        return result;
                    }
                }

                // Não deu; voltar atrás procurando uma alternativa.
                while back_track.match_amount != 0 {
                    let Some(context) = back_track.last_context else {
                        return JSRegExpResult::ErrorInternal;
                    };
                    // Iterações obrigatórias (contagem <= min) podem casar vazio; só as extras
                    // precisam ser não vazias (RepeatMatcher).
                    let result = if back_track.match_amount <= term.atom.quantity_min_count as usize {
                        self.match_disjunction(disjunction_body, DisjunctionContextRef::Parentheses(context), true)
                    } else {
                        self.match_non_zero_disjunction(
                            disjunction_body,
                            DisjunctionContextRef::Parentheses(context),
                            true,
                        )
                    };
                    if result == JSRegExpResult::Match {
                        // Um backtrack bem-sucedido do conteúdo pode ter nos deixado abaixo do
                        // mínimo obrigatório (iterações foram removidas acima na busca por uma
                        // alternativa). O não guloso ainda exige ao menos min iterações, então
                        // reabastecer até o mínimo antes de aceitar, espelhando o caso guloso.
                        // Sem isso o interpretador devolveria um casamento com menos de
                        // quantityMinCount iterações (por exemplo /(?:xy|x){2,3}?yxw/ casando
                        // "xyxw" por engano).
                        let refill =
                            self.refill_parentheses_contexts_to_min_count(term, back_track, disjunction_body);
                        if refill != JSRegExpResult::Match {
                            return refill;
                        }

                        // Backtrack bem-sucedido! Estamos de volta ao jogo!
                        if back_track.match_amount != 0 {
                            if let Some(last) = back_track.last_context {
                                self.record_parentheses_match(term, last);
                            }
                        }
                        return JSRegExpResult::Match;
                    }

                    // Remove um casamento da pilha.
                    self.reset_matches(term, context);
                    Self::pop_parentheses_disjunction_context(back_track, &self.parentheses_contexts);
                    self.free_parentheses_disjunction_context(context);

                    if result != JSRegExpResult::NoMatch {
                        return result;
                    }
                }

                JSRegExpResult::NoMatch
            }
        }
    }

    pub fn refill_parentheses_contexts_to_min_count(
        &mut self,
        term: &ByteTerm,
        back_track: &mut BackTrackInfoParentheses,
        disjunction_body: &ByteDisjunction,
    ) -> crate::yarr::yarr::JSRegExpResult {
        use crate::yarr::yarr::JSRegExpResult;
        while back_track.match_amount < term.atom.quantity_min_count as usize {
            let Some(context) = self.alloc_parentheses_disjunction_context(disjunction_body, term) else {
                return JSRegExpResult::ErrorNoMemory;
            };
            let mut result = self.match_disjunction(
                disjunction_body,
                DisjunctionContextRef::Parentheses(context),
                false,
            );
            if result == JSRegExpResult::Match {
                Self::append_parentheses_disjunction_context(
                    back_track,
                    context,
                    &mut self.parentheses_contexts[context],
                );
                continue;
            }
            self.reset_matches(term, context);
            self.free_parentheses_disjunction_context(context);
            if result != JSRegExpResult::NoMatch {
                return result;
            }
            result = self.parentheses_do_backtrack(term, back_track);
            if result != JSRegExpResult::Match {
                return result;
            }
        }
        JSRegExpResult::Match
    }

    // A extensão gulosa acrescenta o máximo possível de iterações adicionais até
    // quantityMaxCount, cada uma exigida a casar ao menos um caractere.
    pub fn extend_parentheses_contexts_to_max_count(
        &mut self,
        term: &ByteTerm,
        back_track: &mut BackTrackInfoParentheses,
        disjunction_body: &ByteDisjunction,
    ) -> crate::yarr::yarr::JSRegExpResult {
        use crate::yarr::yarr::JSRegExpResult;
        while back_track.match_amount < term.atom.quantity_max_count as usize {
            let Some(context) = self.alloc_parentheses_disjunction_context(disjunction_body, term) else {
                return JSRegExpResult::ErrorNoMemory;
            };
            let result = self.match_non_zero_disjunction(
                disjunction_body,
                DisjunctionContextRef::Parentheses(context),
                false,
            );
            if result == JSRegExpResult::Match {
                Self::append_parentheses_disjunction_context(
                    back_track,
                    context,
                    &mut self.parentheses_contexts[context],
                );
                continue;
            }
            self.reset_matches(term, context);
            self.free_parentheses_disjunction_context(context);
            if result != JSRegExpResult::NoMatch {
                return result;
            }
            break;
        }
        JSRegExpResult::Match
    }

    pub fn match_dot_star_enclosure(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        let multiline = term.multiline();
        let is_newline = |this: &Interpreter<'a, C>, position: u32| -> bool {
            let pattern = this.pattern;
            Self::test_character_class(
                pattern.character_class(pattern.newline_character_class),
                this.input.reread(position),
            )
        };
        // Um ^ inicial vale na posição 0 ou, sob /m, logo depois de um terminador de linha.
        let is_line_start = |this: &Interpreter<'a, C>, position: u32| -> bool {
            position == 0 || (multiline && is_newline(this, position - 1))
        };
        // [startOffset, noNewlineBefore) sabidamente não tem terminador de linha. O intervalo só
        // cresce durante um casamento, então um enclosure rejeitado por falta de início de linha
        // (um RegExp /g retomado no meio da linha) não reescaneia esse trecho a cada ocorrência
        // posterior da expressão envolvida. Sem /m, um ^ rejeitado uma vez continua rejeitado
        // (ocorrências posteriores começam não antes), registrado como bolUnsatisfiable para
        // que falhem de imediato em vez de percorrer a linha de novo.
        debug_assert!(
            self.no_newline_before >= self.start_offset || self.no_newline_before == Self::BOL_UNSATISFIABLE
        );
        if self.no_newline_before == Self::BOL_UNSATISFIABLE {
            return false;
        }
        let reject_for_bol = |this: &mut Interpreter<'a, C>| -> bool {
            if !multiline {
                this.no_newline_before = Self::BOL_UNSATISFIABLE;
            }
            false
        };
        let expression_begin = self.context_mut(context).match_begin;

        if term.dot_all() {
            // Sob /s o .* inicial alcança de volta o ponto onde o casamento começou e o .* final
            // vai até o fim. Um ^ inicial ainda precisa valer no início do casamento: o próprio
            // offset inicial ou, sob /m, o primeiro início de linha depois dele que não passa
            // pela expressão envolvida.
            let mut match_begin = self.start_offset;
            if term.anchors_bol && !is_line_start(self, match_begin) {
                if !multiline {
                    return reject_for_bol(self);
                }
                match_begin = match_begin.max(self.no_newline_before);
                while match_begin < expression_begin && !is_newline(self, match_begin) {
                    match_begin += 1;
                }
                if match_begin >= expression_begin {
                    self.no_newline_before = self.no_newline_before.max(expression_begin);
                    return false; // nenhum início de linha em [startOffset, expressão]; uma ocorrência posterior pode ter
                }
                match_begin += 1; // logo depois da quebra de linha
            }
            let end = self.input.end();
            let frame_context = self.context_mut(context);
            frame_context.match_begin = match_begin;
            frame_context.match_end = end;
            return true;
        }

        // Voltar da expressão até o início da sua linha, mas sem entrar no trecho já sabido
        // sem quebra de linha.
        let mut match_begin = expression_begin;
        let bound = self.start_offset.max(self.no_newline_before);
        if match_begin > bound {
            match_begin -= 1;
            loop {
                if is_newline(self, match_begin) {
                    match_begin += 1;
                    break;
                }

                if match_begin == bound {
                    self.no_newline_before = expression_begin;
                    match_begin = self.start_offset;
                    break;
                }
                match_begin -= 1;
            }
        } else {
            match_begin = self.start_offset;
        }

        // A volta parou logo depois de uma quebra de linha ou no offset inicial; este último só
        // é início de linha em 0 ou (sob /m) quando o caractere anterior é quebra de linha, então
        // um RegExp /g retomado no meio da linha ("bxxaxx" a partir de lastIndex 3) não satisfaz
        // o ^ ali.
        if term.anchors_bol && !is_line_start(self, match_begin) {
            return reject_for_bol(self);
        }

        let mut match_end = self.input.get_pos();

        while match_end != self.input.end() && !is_newline(self, match_end) {
            match_end += 1;
        }

        if term.anchors_eol && match_end != self.input.end() && !multiline {
            return false;
        }

        let frame_context = self.context_mut(context);
        frame_context.match_begin = match_begin;
        frame_context.match_end = match_end;
        true
    }
}
