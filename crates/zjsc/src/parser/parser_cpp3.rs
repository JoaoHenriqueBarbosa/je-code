// Terceira fatia de `parser/Parser.cpp` (linhas 1476 a 2128), incluída por `include!` em `parser.rs`.
// Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Vai de `parseDefaultValueForDestructuringPattern` até o fim de `parseBlockStatement`:
// `parseForStatement`, `parseBreakStatement`, `parseContinueStatement`, `parseReturnStatement`,
// `parseThrowStatement`, `parseWithStatement`, `parseSwitchStatement`, `parseSwitchClauses`,
// `parseSwitchDefaultClause`, `parseTryStatement`, `parseDebuggerStatement` e `parseBlockStatement`.
//
// Convenções desta fatia (as de `parser_cpp1.rs` valem):
//
// - `DepthManager` (linhas 194 a 200 do `.cpp`) e `SetForScope` viram "salvar o valor antigo e
//   restaurá-lo em cada saída". Como as macros de erro só aceitam um `@hook` por chamada, cada
//   função que tem destrutor vivo declara um `macro_rules!` local (`exit_cleanup`, `catch_cleanup`,
//   `restore_switch_case_body`) com o que os destrutores fariam, na ordem inversa de construção,
//   e o passa em `@hook { ... }`. `AutoCleanupLexicalScope::new()` é declarado cedo: enquanto
//   inválido o `cleanup` não faz nada, igual ao objeto ainda não usado do C++.
// - `AutoPopScope`/`AutoCleanupLexicalScope` não têm `Drop`: `cleanup(self)` é chamado onde o C++
//   destruiria o objeto.
// - O que o `.cpp` escreve com as macros `semanticFailIfTrue`, `semanticFailIfFalse`,
//   `failIfTrueIfStrict`, `matchOrFail` e `handleProductionOrFail` quando há destrutor vivo (essas
//   macros de `parser_cpp1.rs` não têm a forma com `@hook`) é expandido à mão com
//   `internal_fail_with_message!`, `handle_error_token!` e `consume_or_fail!`, com o mesmo texto.
// - `goto standardForLoop` e `goto enumerationLoop` de `parseForStatement` viram dois booleanos que
//   desviam para o mesmo trecho, na mesma ordem de avaliação do C++.
// - `const Identifier* unused = nullptr` (diretiva de `parseStatement`) vira `Option<Identifier>`.
// - `DeclarationStacks::FunctionStack` é `FunctionStack`.
// - `parseBlockStatement(context)` com o argumento padrão `BlockType::Normal` passa o tipo explícito.

impl<T: CharType> Parser<T> {
    /// `template <class TreeBuilder> TreeExpression parseDefaultValueForDestructuringPattern(TreeBuilder&)`.
    pub(crate) fn parse_default_value_for_destructuring_pattern<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        if !self.match_(EQUAL) {
            return None;
        }

        self.next(B::DONT_BUILD_STRINGS); // consume '='
        self.parse_assignment_expression(context)
    }

    /// `template <class TreeBuilder> TreeStatement parseForStatement(TreeBuilder&)`.
    pub(crate) fn parse_for_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(FOR));
        let location = self.token_location();
        let start_line = self.token_line();
        let mut is_await_for = false;
        self.next(LexerFlagSet::empty());

        // DepthManager statementDepth(&m_statementDepth); m_statementDepth++;
        let old_statement_depth = self.statement_depth;
        self.statement_depth += 1;

        // AutoCleanupLexicalScope lexicalScope: declarado aqui, inválido até o `set_is_valid` (o
        // destrutor do C++ não faz nada enquanto o objeto é inválido).
        let mut lexical_scope = AutoCleanupLexicalScope::new();
        // Destrutores vivos: primeiro `lexicalScope`, depois `statementDepth`.
        macro_rules! exit_cleanup {
            () => {{
                lexical_scope.cleanup(self);
                self.statement_depth = old_statement_depth;
            }};
        }
        // `return result` com os destrutores já rodados (o `lexicalScope` já foi desempilhado).
        macro_rules! finish {
            ($result:expr) => {{
                let result = $result;
                self.statement_depth = old_statement_depth;
                return Some(result);
            }};
        }

        if self.match_(AWAIT) {
            let current = self.current_scope();
            if !(self.scope_stack[current].is_async_function() || is_module_parse_mode(self.source_parse_mode())) {
                internal_fail_with_message!(self, @hook { exit_cleanup!(); }, false, "for-await-of can only be used in an async function or async generator");
            }
            is_await_for = true;
            let function_scope = self.current_function_scope();
            self.scope_stack[function_scope].set_uses_await();
            self.next(LexerFlagSet::empty());
        }

        // handleProductionOrFail(OPENPAREN, "(", "start", "for-loop header");
        consume_or_fail!(self, @hook { exit_cleanup!(); }, OPENPAREN, "Expected '", "(", "' to ", "start", " a ", "for-loop header");
        let non_lhs_count = self.parser_state.non_lhs_count;
        let mut declarations: i32 = 0;
        let decl_location = self.token_location();
        let mut decls_start = *self.token_start_position();
        let mut decls: Option<B::Expression> = None;
        let mut pattern: Option<B::DestructuringPattern> = None;
        let is_var_declaration = self.match_(VAR);
        let is_let_declaration = self.match_(LET);
        let is_const_declaration = self.match_(CONSTTOKEN);
        let mut is_using_declaration = false;
        let mut is_await_using_declaration = false;
        if Options::use_explicit_resource_management()
            && self.match_(IDENT)
            && self.token.data.ident.as_ref() == Some(&self.vm.property_names.using_identifier)
            && !self.token.data.escaped
        {
            let save_point = self.create_save_point(context);
            self.next(LexerFlagSet::empty());
            if !self.lexer.has_line_terminator_before_token() && self.match_spec_identifier() {
                if self.match_contextual_keyword(&self.vm.property_names.of) {
                    // "for (using of ..." - the spec has [lookahead != `using` `of`] on ForDeclaration,
                    // so this is only a using declaration if 'of' is a binding name with an initializer.
                    // "for (using of = init; ...)" -> using declaration, 'of' is binding name
                    // "for (using of expr)" -> for-of loop, 'using' is identifier, 'of' is keyword
                    // "for (using of of expr)" -> for-of loop, 'using' is identifier, first 'of' is keyword
                    self.next(LexerFlagSet::empty()); // consume 'of'
                    if self.match_(EQUAL) {
                        is_using_declaration = true;
                    }
                } else {
                    is_using_declaration = true;
                }
            }
            self.restore_save_point(context, &save_point);
        } else if Options::use_explicit_resource_management()
            && self.match_(AWAIT)
            && !self.parser_state.class_field_init_masks_async
            && (self.scope_stack[self.current_function_scope()].is_async_function_boundary() || is_module_parse_mode(self.source_parse_mode()))
        {
            let save_point = self.create_save_point(context);
            self.next(LexerFlagSet::empty());
            if !self.lexer.has_line_terminator_before_token()
                && self.match_(IDENT)
                && self.token.data.ident.as_ref() == Some(&self.vm.property_names.using_identifier)
                && !self.token.data.escaped
            {
                self.next(LexerFlagSet::empty());
                // Note: unlike sync `using`, there is no [lookahead != `of`] constraint for `await using`,
                // so `for (await using of of expr)` binds `of` as an identifier.
                if !self.lexer.has_line_terminator_before_token() && self.match_spec_identifier() {
                    is_await_using_declaration = true;
                }
            }
            self.restore_save_point(context, &save_point);
            if is_await_using_declaration {
                let function_scope = self.current_function_scope();
                self.scope_stack[function_scope].set_uses_await();
            }
        }
        let is_any_using_declaration = is_using_declaration || is_await_using_declaration;
        let is_lexical_declaration = is_let_declaration || is_const_declaration || is_any_using_declaration;
        let mut for_loop_const_does_not_have_initializer = false;
        let mut for_loop_initializer_contains_closure = false;

        // `auto popLexicalScopeIfNecessary = [&]() -> VariableEnvironment`.
        macro_rules! pop_lexical_scope_if_necessary {
            () => {
                if is_lexical_declaration {
                    // `auto [lexicalVariables, functionDeclarations] = popScope(...)`; devolve o primeiro.
                    self.pop_scope_cleanup(&mut lexical_scope, B::NEEDS_FREE_VARIABLE_INFO).0
                } else {
                    VariableEnvironment::default()
                }
            };
        }

        // Os dois `goto` do C++: `standardForLoop` (dentro do `if (match(SEMICOLON))`) e
        // `enumerationLoop` (depois dele).
        let mut goto_standard_for_loop = false;
        let mut goto_enumeration_loop = false;

        if is_var_declaration || is_lexical_declaration {
            /*
             for (var/let/const/using IDENT in/of expression) statement
             for (var/let/const varDeclarationList; expressionOpt; expressionOpt)
             */
            if is_lexical_declaration {
                let new_scope = self.push_scope();
                self.scope_stack[new_scope].set_is_lexical_scope();
                self.scope_stack[new_scope].prevent_var_declarations();
                lexical_scope.set_is_valid(new_scope, self);
            }

            let mut for_in_target: Option<B::DestructuringPattern> = None;
            let mut for_in_initializer: Option<B::Expression> = None;
            self.allows_in = false;
            let mut init_start = JSTextPosition::default();
            let mut init_end = JSTextPosition::default();
            let declaration_type = if is_var_declaration {
                DeclarationType::VarDeclaration
            } else if is_let_declaration {
                DeclarationType::LetDeclaration
            } else if is_const_declaration {
                DeclarationType::ConstDeclaration
            } else if is_using_declaration {
                DeclarationType::UsingDeclaration
            } else if is_await_using_declaration {
                DeclarationType::AwaitUsingDeclaration
            } else {
                // Invariante: o chamador só entra aqui com um dos cinco tokens de declaração (var, let, const, using, await using).
                unreachable!("RELEASE_ASSERT_NOT_REACHED")
            };
            let current = self.current_scope();
            let candidate_count_before_initializer = self.scope_stack[current].closed_variable_candidates().len();
            decls = self.parse_variable_declaration_list(
                context,
                &mut declarations,
                &mut for_in_target,
                &mut for_in_initializer,
                &mut decls_start,
                &mut init_start,
                &mut init_end,
                VarDeclarationListContext::ForLoopContext,
                declaration_type,
                ExportType::NotExported,
                &mut for_loop_const_does_not_have_initializer,
            );
            let current = self.current_scope();
            for_loop_initializer_contains_closure = self.scope_stack[current].closed_variable_candidates().len() > candidate_count_before_initializer;
            self.allows_in = true;
            propagate_error!(self, @hook { exit_cleanup!(); });

            // Remainder of a standard for loop is handled identically
            if self.match_(SEMICOLON) {
                goto_standard_for_loop = true;
            } else {
                fail_if_false!(self, @hook { exit_cleanup!(); }, declarations == 1, "can only declare a single variable in an enumeration");

                // Handle for-in with var declaration
                let in_location = *self.token_start_position();
                let mut is_of_enumeration = false;
                if !self.match_(INTOKEN) {
                    fail_if_false!(self, @hook { exit_cleanup!(); }, self.match_contextual_keyword(&self.vm.property_names.of), "Expected either 'in' or 'of' in enumeration syntax");
                    is_of_enumeration = true;
                    self.next(LexerFlagSet::empty());
                } else {
                    fail_if_true!(self, @hook { exit_cleanup!(); }, is_any_using_declaration, "Cannot use 'using' declaration in for-in loop");
                    fail_if_false!(self, @hook { exit_cleanup!(); }, !is_await_for, "Expected 'of' in for-await syntax");
                    self.next(LexerFlagSet::empty());
                }

                let has_any_assignments = for_in_initializer.is_some();
                if has_any_assignments {
                    if is_of_enumeration {
                        internal_fail_with_message!(self, @hook { exit_cleanup!(); }, false, "Cannot assign to the loop variable inside a for-of loop header");
                    }
                    let target_is_binding_node = for_in_target.as_ref().is_some_and(|target| context.is_binding_node(target));
                    if self.strict_mode() || (is_let_declaration || is_const_declaration) || !target_is_binding_node {
                        internal_fail_with_message!(self, @hook { exit_cleanup!(); }, false, "Cannot assign to the loop variable inside a for-in loop header");
                    }
                }

                // While for-in uses Expression, for-of / for-await-of use AssignmentExpression.
                // https://tc39.es/ecma262/#sec-for-in-and-for-of-statements
                let expr = if is_of_enumeration {
                    self.parse_assignment_expression(context)
                } else {
                    self.parse_expression(context)
                };
                fail_if_false!(self, @hook { exit_cleanup!(); }, expr.is_some(), "Expected expression to enumerate");
                let expr = expr.unwrap_or_default();
                self.record_pause_location(context.breakpoint_location(&expr));
                let expr_end = self.last_token_end_position();

                let end_line = self.token_line();

                // handleProductionOrFail(CLOSEPAREN, ")", "end", (isOfEnumeration ? "for-of header" : "for-in header"));
                consume_or_fail!(self, @hook { exit_cleanup!(); }, CLOSEPAREN, "Expected '", ")", "' to ", "end", " a ", if is_of_enumeration { "for-of header" } else { "for-in header" });

                let mut unused: Option<Identifier> = None;
                self.start_loop();
                let statement = self.parse_statement(context, &mut unused, None);
                self.end_loop();
                fail_if_false!(self, @hook { exit_cleanup!(); }, statement.is_some(), "Expected statement as body of for-", if is_of_enumeration { "of" } else { "in" }, " statement");
                let statement = statement.unwrap_or_default();
                let lexical_variables = pop_lexical_scope_if_necessary!();
                if is_of_enumeration {
                    finish!(context.create_for_of_loop_pattern(is_await_for, &location, for_in_target.unwrap_or_default(), expr, statement, &decl_location, decls_start, in_location, expr_end, start_line, end_line, lexical_variables));
                }
                debug_assert!(!is_await_for);
                if is_var_declaration && for_in_initializer.is_some() {
                    finish!(context.create_for_in_loop(&location, decls.unwrap_or_default(), expr, statement, &decl_location, decls_start, in_location, expr_end, start_line, end_line, lexical_variables));
                }
                finish!(context.create_for_in_loop_pattern(&location, for_in_target.unwrap_or_default(), expr, statement, &decl_location, decls_start, in_location, expr_end, start_line, end_line, lexical_variables));
            }
        } else if !self.match_(SEMICOLON) {
            if self.match_(OPENBRACE) || self.match_(OPENBRACKET) {
                let save_point = self.create_save_point(context);
                pattern = self.try_parse_destructuring_pattern_expression(context, AssignmentContext::AssignmentExpression);
                if pattern.is_some() && (self.match_(INTOKEN) || self.match_contextual_keyword(&self.vm.property_names.of)) {
                    // goto enumerationLoop
                    goto_enumeration_loop = true;
                } else {
                    pattern = None;
                    self.restore_save_point(context, &save_point);
                }
            }
            if !goto_enumeration_loop {
                self.allows_in = false;
                decls = self.parse_expression(context);
                self.allows_in = true;
                fail_if_false!(self, @hook { exit_cleanup!(); }, decls.is_some(), "Cannot parse for loop declarations");
                if let Some(decls_node) = &decls {
                    self.record_pause_location(context.breakpoint_location(decls_node));
                }
            }
        }

        if goto_standard_for_loop || (!goto_enumeration_loop && self.match_(SEMICOLON)) {
            // standardForLoop:
            fail_if_false!(self, @hook { exit_cleanup!(); }, !is_await_for, "Unexpected a ';' in for-await-of header");
            // Standard for loop
            if let Some(decls_node) = &decls {
                self.record_pause_location(context.breakpoint_location(decls_node));
            }
            self.next(LexerFlagSet::empty());
            let mut condition: Option<B::Expression> = None;
            fail_if_true!(self, @hook { exit_cleanup!(); }, for_loop_const_does_not_have_initializer && is_const_declaration, "const variables in for loops must have initializers");
            fail_if_true!(self, @hook { exit_cleanup!(); }, for_loop_const_does_not_have_initializer && is_any_using_declaration, "'using' declaration requires an initializer");

            if !self.match_(SEMICOLON) {
                condition = self.parse_expression(context);
                fail_if_false!(self, @hook { exit_cleanup!(); }, condition.is_some(), "Cannot parse for loop condition expression");
                if let Some(condition_node) = &condition {
                    self.record_pause_location(context.breakpoint_location(condition_node));
                }
            }
            consume_or_fail!(self, @hook { exit_cleanup!(); }, SEMICOLON, "Expected a ';' after the for loop condition expression");

            let mut increment: Option<B::Expression> = None;
            if !self.match_(CLOSEPAREN) {
                increment = self.parse_expression(context);
                fail_if_false!(self, @hook { exit_cleanup!(); }, increment.is_some(), "Cannot parse for loop iteration expression");
                if let Some(increment_node) = &increment {
                    self.record_pause_location(context.breakpoint_location(increment_node));
                }
            }
            let end_line = self.token_line();
            // handleProductionOrFail(CLOSEPAREN, ")", "end", "for-loop header");
            consume_or_fail!(self, @hook { exit_cleanup!(); }, CLOSEPAREN, "Expected '", ")", "' to ", "end", " a ", "for-loop header");
            let mut unused: Option<Identifier> = None;
            self.start_loop();
            let statement = self.parse_statement(context, &mut unused, None);
            self.end_loop();
            fail_if_false!(self, @hook { exit_cleanup!(); }, statement.is_some(), "Expected a statement as the body of a for loop");
            let lexical_variables = pop_lexical_scope_if_necessary!();
            finish!(context.create_for_loop(&location, decls.unwrap_or_default(), condition.unwrap_or_default(), increment.unwrap_or_default(), statement.unwrap_or_default(), start_line, end_line, lexical_variables, for_loop_initializer_contains_closure));
        }

        // For-in and For-of loop
        // enumerationLoop:
        fail_if_false!(self, @hook { exit_cleanup!(); }, non_lhs_count == self.parser_state.non_lhs_count, "Expected a reference on the left hand side of an enumeration statement");
        let mut is_of_enumeration = false;
        let in_location = *self.token_start_position();
        if !self.match_(INTOKEN) {
            fail_if_false!(self, @hook { exit_cleanup!(); }, self.match_contextual_keyword(&self.vm.property_names.of), "Expected either 'in' or 'of' in enumeration syntax");
            is_of_enumeration = true;
            self.next(LexerFlagSet::empty());
        } else {
            fail_if_false!(self, @hook { exit_cleanup!(); }, !is_await_for, "Expected 'of' in for-await syntax");
            self.next(LexerFlagSet::empty());
        }

        // While for-in uses Expression, for-of / for-await-of use AssignmentExpression.
        // https://tc39.es/ecma262/#sec-for-in-and-for-of-statements
        let expr = if is_of_enumeration {
            self.parse_assignment_expression(context)
        } else {
            self.parse_expression(context)
        };
        fail_if_false!(self, @hook { exit_cleanup!(); }, expr.is_some(), "Cannot parse subject for-", if is_of_enumeration { "of" } else { "in" }, " statement");
        let expr = expr.unwrap_or_default();
        self.record_pause_location(context.breakpoint_location(&expr));
        let expr_end = self.last_token_end_position();
        let end_line = self.token_line();

        // handleProductionOrFail(CLOSEPAREN, ")", "end", (isOfEnumeration ? "for-of header" : "for-in header"));
        consume_or_fail!(self, @hook { exit_cleanup!(); }, CLOSEPAREN, "Expected '", ")", "' to ", "end", " a ", if is_of_enumeration { "for-of header" } else { "for-in header" });
        let mut unused: Option<Identifier> = None;
        self.start_loop();
        let statement = self.parse_statement(context, &mut unused, None);
        self.end_loop();
        fail_if_false!(self, @hook { exit_cleanup!(); }, statement.is_some(), "Expected a statement as the body of a for-", if is_of_enumeration { "of" } else { "in" }, " loop");
        let statement = statement.unwrap_or_default();
        if pattern.is_some() {
            debug_assert!(decls.is_none());
            let lexical_variables = pop_lexical_scope_if_necessary!();
            if is_of_enumeration {
                finish!(context.create_for_of_loop_pattern(is_await_for, &location, pattern.unwrap_or_default(), expr, statement, &decl_location, decls_start, in_location, expr_end, start_line, end_line, lexical_variables));
            }
            debug_assert!(!is_await_for);
            finish!(context.create_for_in_loop_pattern(&location, pattern.unwrap_or_default(), expr, statement, &decl_location, decls_start, in_location, expr_end, start_line, end_line, lexical_variables));
        }

        let decls = decls.unwrap_or_default();
        if !self.is_simple_assignment_target(context, &decls, false) {
            internal_fail_with_message!(self, @hook { exit_cleanup!(); }, false, "Left side of assignment is not a reference");
        }

        let lexical_variables = pop_lexical_scope_if_necessary!();
        if is_of_enumeration {
            finish!(context.create_for_of_loop(is_await_for, &location, decls, expr, statement, &decl_location, decls_start, in_location, expr_end, start_line, end_line, lexical_variables));
        }
        debug_assert!(!is_await_for);
        finish!(context.create_for_in_loop(&location, decls, expr, statement, &decl_location, decls_start, in_location, expr_end, start_line, end_line, lexical_variables))
    }

    /// `template <class TreeBuilder> TreeStatement parseBreakStatement(TreeBuilder&)`.
    pub(crate) fn parse_break_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(BREAK));
        let location = self.token_location();
        let start = *self.token_start_position();
        let mut end = *self.token_end_position();
        self.next(LexerFlagSet::empty());

        let mut is_break_valid: Option<bool> = None;
        let current = self.current_scope();
        if self.scope_stack[current].is_static_block() {
            let valid = self.break_is_valid();
            is_break_valid = Some(valid);
            semantic_fail_if_true!(self, !self.scope_stack[current].break_is_valid() && !valid, "'break' cannot cross static block boundary");
        }

        if self.auto_semi_colon() {
            semantic_fail_if_false!(self, is_break_valid.unwrap_or(self.break_is_valid()), "'break' is only valid inside a switch or loop statement");
            return Some(context.create_break_statement_label(&location, &self.vm.property_names.null_identifier, start, end));
        }
        fail_if_false!(self, self.match_spec_identifier(), "Expected an identifier as the target for a break statement");
        let ident = self.token.data.ident.clone().unwrap_or_default();
        semantic_fail_if_false!(self, self.get_label(&ident).is_some(), "Cannot use the undeclared label '", ident, "'");
        end = *self.token_end_position();
        self.next(LexerFlagSet::empty());
        fail_if_false!(self, self.auto_semi_colon(), "Expected a ';' following a targeted break statement");
        Some(context.create_break_statement_label(&location, &ident, start, end))
    }

    /// `template <class TreeBuilder> TreeStatement parseContinueStatement(TreeBuilder&)`.
    pub(crate) fn parse_continue_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(CONTINUE));
        let location = self.token_location();
        let start = *self.token_start_position();
        let mut end = *self.token_end_position();
        self.next(LexerFlagSet::empty());

        let mut is_continue_valid: Option<bool> = None;
        let current = self.current_scope();
        if self.scope_stack[current].is_static_block() {
            let valid = self.continue_is_valid();
            is_continue_valid = Some(valid);
            semantic_fail_if_true!(self, !self.scope_stack[current].continue_is_valid() && !valid, "'continue' cannot cross static block boundary");
        }

        if self.auto_semi_colon() {
            semantic_fail_if_false!(self, is_continue_valid.unwrap_or(self.continue_is_valid()), "'continue' is only valid inside a loop statement");
            return Some(context.create_continue_statement_label(&location, &self.vm.property_names.null_identifier, start, end));
        }
        fail_if_false!(self, self.match_spec_identifier(), "Expected an identifier as the target for a continue statement");
        let ident = self.token.data.ident.clone().unwrap_or_default();
        let label = self.get_label(&ident).cloned();
        semantic_fail_if_false!(self, label.is_some(), "Cannot use the undeclared label '", ident, "'");
        semantic_fail_if_false!(self, label.as_ref().is_some_and(|label| label.is_loop), "Cannot continue to the label '", ident, "' as it is not targeting a loop");
        end = *self.token_end_position();
        self.next(LexerFlagSet::empty());
        fail_if_false!(self, self.auto_semi_colon(), "Expected a ';' following a targeted continue statement");
        Some(context.create_continue_statement_label(&location, &ident, start, end))
    }

    /// `template <class TreeBuilder> TreeStatement parseReturnStatement(TreeBuilder&)`.
    pub(crate) fn parse_return_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(RETURN));
        self.parser_state.return_statement_count += 1;
        let location = self.token_location();
        let current = self.current_scope();
        semantic_fail_if_false!(self, self.scope_stack[current].is_function() && !self.scope_stack[current].is_static_block(), "Return statements are only valid inside functions");
        let start = *self.token_start_position();
        let mut end = *self.token_end_position();
        self.next(LexerFlagSet::empty());
        // We do the auto semicolon check before attempting to parse expression
        // as we need to ensure the a line break after the return correctly terminates
        // the statement
        if self.match_(SEMICOLON) {
            end = *self.token_end_position();
        }

        if self.auto_semi_colon() {
            return Some(context.create_return_statement(&location, Default::default(), start, end));
        }
        let expr = self.parse_expression(context);
        fail_if_false!(self, expr.is_some(), "Cannot parse the return expression");
        end = self.last_token_end_position();
        if self.match_(SEMICOLON) {
            end = *self.token_end_position();
        }
        fail_if_false!(self, self.auto_semi_colon(), "Expected a ';' following a return statement");
        Some(context.create_return_statement(&location, expr.unwrap_or_default(), start, end))
    }

    /// `template <class TreeBuilder> TreeStatement parseThrowStatement(TreeBuilder&)`.
    pub(crate) fn parse_throw_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(THROW));
        let location = self.token_location();
        let start = *self.token_start_position();
        self.next(LexerFlagSet::empty());
        fail_if_true!(self, self.match_(SEMICOLON), "Expected expression after 'throw'");
        semantic_fail_if_true!(self, self.auto_semi_colon(), "Cannot have a newline after 'throw'");

        let expr = self.parse_expression(context);
        fail_if_false!(self, expr.is_some(), "Cannot parse expression for throw statement");
        let end = self.last_token_end_position();
        fail_if_false!(self, self.auto_semi_colon(), "Expected a ';' after a throw statement");

        Some(context.create_throw_statement(&location, expr.unwrap_or_default(), start, end))
    }

    /// `template <class TreeBuilder> TreeStatement parseWithStatement(TreeBuilder&)`.
    pub(crate) fn parse_with_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(WITH));
        let location = self.token_location();
        semantic_fail_if_true!(self, self.strict_mode(), "'with' statements are not valid in strict mode");
        let current = self.current_scope();
        self.scope_stack[current].set_needs_full_activation();
        let start_line = self.token_line();
        self.next(LexerFlagSet::empty());

        handle_production_or_fail!(self, OPENPAREN, "(", "start", "subject of a 'with' statement");
        let start = self.token_start();
        let expr = self.parse_expression(context);
        fail_if_false!(self, expr.is_some(), "Cannot parse 'with' subject expression");
        let expr = expr.unwrap_or_default();
        self.record_pause_location(context.breakpoint_location(&expr));
        let end = self.last_token_end_position();
        let end_line = self.token_line();
        handle_production_or_fail!(self, CLOSEPAREN, ")", "start", "subject of a 'with' statement");

        let pushed = self.push_scope();
        let mut with_scope = AutoPopScope::new(pushed);
        self.scope_stack[with_scope.scope()].set_tainted_by_with_scope();
        self.scope_stack[with_scope.scope()].prevent_all_variable_declarations();

        let mut unused: Option<Identifier> = None;
        let statement = self.parse_statement(context, &mut unused, None);
        fail_if_false!(self, @hook { with_scope.cleanup(self); }, statement.is_some(), "A 'with' statement must have a body");

        let result = context.create_with_statement(&location, expr, statement.unwrap_or_default(), start, end, start_line as u32, end_line as u32);
        self.pop_scope_auto(&mut with_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);
        with_scope.cleanup(self);
        Some(result)
    }

    /// `template <class TreeBuilder> TreeStatement parseSwitchStatement(TreeBuilder&)`.
    pub(crate) fn parse_switch_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(SWITCH));
        let location = self.token_location();
        let start_line = self.token_line();
        self.next(LexerFlagSet::empty());
        handle_production_or_fail!(self, OPENPAREN, "(", "start", "subject of a 'switch'");
        let expr = self.parse_expression(context);
        fail_if_false!(self, expr.is_some(), "Cannot parse switch subject expression");
        let expr = expr.unwrap_or_default();
        self.record_pause_location(context.breakpoint_location(&expr));
        let end_line = self.token_line();

        handle_production_or_fail!(self, CLOSEPAREN, ")", "end", "subject of a 'switch'");
        handle_production_or_fail!(self, OPENBRACE, "{", "start", "body of a 'switch'");
        let pushed = self.push_scope();
        let mut lexical_scope = AutoPopScope::new(pushed);
        self.scope_stack[lexical_scope.scope()].set_is_lexical_scope();
        self.scope_stack[lexical_scope.scope()].prevent_var_declarations();
        self.start_switch();
        let first_clauses = self.parse_switch_clauses(context);
        propagate_error!(self, @hook { lexical_scope.cleanup(self); });

        let default_clause = self.parse_switch_default_clause(context);
        propagate_error!(self, @hook { lexical_scope.cleanup(self); });

        let second_clauses = self.parse_switch_clauses(context);
        propagate_error!(self, @hook { lexical_scope.cleanup(self); });
        self.end_switch();
        // handleProductionOrFail(CLOSEBRACE, "}", "end", "body of a 'switch'");
        consume_or_fail!(self, @hook { lexical_scope.cleanup(self); }, CLOSEBRACE, "Expected '", "}", "' to ", "end", " a ", "body of a 'switch'");

        let (lexical_environment, function_declarations) = self.pop_scope_auto(&mut lexical_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);
        let result = context.create_switch_statement(
            &location,
            expr,
            first_clauses.unwrap_or_default(),
            default_clause.unwrap_or_default(),
            second_clauses.unwrap_or_default(),
            start_line,
            end_line,
            lexical_environment,
            function_declarations,
        );
        lexical_scope.cleanup(self);
        Some(result)
    }

    /// `template <class TreeBuilder> TreeClauseList parseSwitchClauses(TreeBuilder&)`.
    pub(crate) fn parse_switch_clauses<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::ClauseList> {
        if !self.match_(CASE) {
            return None;
        }
        let mut start_offset = self.token_start();
        self.next(LexerFlagSet::empty());
        let condition = self.parse_expression(context);
        fail_if_false!(self, condition.is_some(), "Cannot parse switch clause");
        consume_or_fail!(self, COLON, "Expected a ':' after switch clause expression");
        // SetForScope switchCaseScope(m_insideSwitchCaseBody, true);
        let old_inside_switch_case_body = self.inside_switch_case_body;
        self.inside_switch_case_body = true;
        macro_rules! restore_switch_case_body {
            () => {
                self.inside_switch_case_body = old_inside_switch_case_body;
            };
        }
        let statements = self.parse_source_elements(context, SourceElementsMode::DontCheckForStrictMode);
        fail_if_false!(self, @hook { restore_switch_case_body!(); }, statements.is_some(), "Cannot parse the body of a switch clause");
        let clause = context.create_clause(condition.unwrap_or_default(), statements.unwrap_or_default());
        context.set_start_offset(&clause, start_offset as i32);
        let clause_list = context.create_clause_list(clause);
        let mut tail = clause_list.clone();

        while self.match_(CASE) {
            start_offset = self.token_start();
            self.next(LexerFlagSet::empty());
            let condition = self.parse_expression(context);
            fail_if_false!(self, @hook { restore_switch_case_body!(); }, condition.is_some(), "Cannot parse switch case expression");
            consume_or_fail!(self, @hook { restore_switch_case_body!(); }, COLON, "Expected a ':' after switch clause expression");
            let statements = self.parse_source_elements(context, SourceElementsMode::DontCheckForStrictMode);
            fail_if_false!(self, @hook { restore_switch_case_body!(); }, statements.is_some(), "Cannot parse the body of a switch clause");
            let clause = context.create_clause(condition.unwrap_or_default(), statements.unwrap_or_default());
            context.set_start_offset(&clause, start_offset as i32);
            tail = context.create_clause_list_append(tail, clause);
        }
        restore_switch_case_body!();
        Some(clause_list)
    }

    /// `template <class TreeBuilder> TreeClause parseSwitchDefaultClause(TreeBuilder&)`.
    pub(crate) fn parse_switch_default_clause<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Clause> {
        if !self.match_(DEFAULT) {
            return None;
        }
        let start_offset = self.token_start();
        self.next(LexerFlagSet::empty());
        consume_or_fail!(self, COLON, "Expected a ':' after switch default clause");
        // SetForScope switchCaseScope(m_insideSwitchCaseBody, true);
        let old_inside_switch_case_body = self.inside_switch_case_body;
        self.inside_switch_case_body = true;
        let statements = self.parse_source_elements(context, SourceElementsMode::DontCheckForStrictMode);
        fail_if_false!(self, @hook { self.inside_switch_case_body = old_inside_switch_case_body; }, statements.is_some(), "Cannot parse the body of a switch default clause");
        let result = context.create_clause(Default::default(), statements.unwrap_or_default());
        context.set_start_offset(&result, start_offset as i32);
        self.inside_switch_case_body = old_inside_switch_case_body;
        Some(result)
    }

    /// `template <class TreeBuilder> TreeStatement parseTryStatement(TreeBuilder&)`.
    pub(crate) fn parse_try_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(TRY));
        let location = self.token_location();
        let mut catch_pattern: Option<B::DestructuringPattern> = None;
        let mut catch_block: Option<B::Statement> = None;
        let mut finally_block: Option<B::Statement> = None;
        let first_line = self.token_line();
        self.next(LexerFlagSet::empty());
        match_or_fail!(self, OPENBRACE, "Expected a block statement as body of a try statement");

        let return_statement_count_before_try_block = self.parser_state.return_statement_count;
        let try_block = self.parse_block_statement(context, BlockType::Normal);
        fail_if_false!(self, try_block.is_some(), "Cannot parse the body of try block");
        let try_block_contains_return = self.parser_state.return_statement_count != return_statement_count_before_try_block;
        let last_line = self.last_token_location.line;
        let mut catch_environment = VariableEnvironment::default();
        let mut function_stack: FunctionStack = Vec::new();
        if self.consume(CATCH) {
            if self.match_(OPENBRACE) {
                catch_block = self.parse_block_statement(context, BlockType::Normal);
                fail_if_false!(self, catch_block.is_some(), "Unable to parse 'catch' block");
            } else {
                // handleProductionOrFail(OPENPAREN, "(", "start", "'catch' target");
                consume_or_fail!(self, OPENPAREN, "Expected '", "(", "' to ", "start", " a ", "'catch' target");
                // DepthManager statementDepth(&m_statementDepth);
                let old_statement_depth = self.statement_depth;
                let current = self.current_scope();
                if self.scope_stack[current].is_static_block() && self.match_(AWAIT) {
                    internal_fail_with_message!(self, @hook { self.statement_depth = old_statement_depth; }, false, "Cannot use 'await' as identifier within static block");
                }
                self.statement_depth += 1;
                let pushed = self.push_scope();
                let mut catch_scope = AutoPopScope::new(pushed);
                // Destrutores vivos: primeiro `catchScope`, depois `statementDepth`.
                macro_rules! catch_cleanup {
                    () => {{
                        catch_scope.cleanup(self);
                        self.statement_depth = old_statement_depth;
                    }};
                }
                self.scope_stack[catch_scope.scope()].set_is_lexical_scope();
                self.scope_stack[catch_scope.scope()].prevent_var_declarations();
                let mut ident: Option<Identifier> = None;
                if self.match_spec_identifier() {
                    self.scope_stack[catch_scope.scope()].set_is_simple_catch_parameter_scope();
                    let catch_ident = self.token.data.ident.clone().unwrap_or_default();
                    ident = Some(catch_ident.clone());
                    catch_pattern = Some(context.create_binding_location(&self.token.location(), &catch_ident, self.token.start_position, self.token.end_position, AssignmentContext::DeclarationStatement));
                    self.next(LexerFlagSet::empty());
                    // failIfTrueIfStrict(catchScope->declareLexicalVariable(ident, false) & DeclarationResult::InvalidStrictMode, ...)
                    let declaration_result = self.scope_stack[catch_scope.scope()].declare_lexical_variable(&catch_ident, false, DeclarationImportType::NotImported, false, false);
                    if (declaration_result & DeclarationResult::INVALID_STRICT_MODE) != 0 && self.strict_mode() {
                        internal_fail_with_message!(self, @hook { catch_cleanup!(); }, false, "Cannot declare a catch variable named '", catch_ident, "' in strict mode");
                    }
                } else {
                    catch_pattern = self.parse_destructuring_pattern(context, DestructuringKind::DestructureToCatchParameters, ExportType::NotExported, None, None, AssignmentContext::DeclarationStatement, 0);
                    fail_if_false!(self, @hook { catch_cleanup!(); }, catch_pattern.is_some(), "Cannot parse this destructuring pattern");
                }
                // handleProductionOrFail(CLOSEPAREN, ")", "end", "'catch' target");
                consume_or_fail!(self, @hook { catch_cleanup!(); }, CLOSEPAREN, "Expected '", ")", "' to ", "end", " a ", "'catch' target");
                if !self.match_(OPENBRACE) {
                    handle_error_token!(self, @hook { catch_cleanup!(); });
                    internal_fail_with_message!(self, @hook { catch_cleanup!(); }, true, "Expected exception handler to be a block statement");
                }
                catch_block = self.parse_block_statement(context, BlockType::CatchBlock);
                fail_if_false!(self, @hook { catch_cleanup!(); }, catch_block.is_some(), "Unable to parse 'catch' block");
                // Handle `try { } catch (/* never used */ error) { }`
                if let Some(key) = ident.as_ref().and_then(|catch_ident| catch_ident.impl_()) {
                    let scope = &self.scope_stack[catch_scope.scope()];
                    if !scope.used_variables_contains(&key) && !scope.uses_eval() && !scope.has_variable_being_hoisted(&key) {
                        catch_pattern = None;
                    }
                }
                (catch_environment, function_stack) = self.pop_scope_auto(&mut catch_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);
                debug_assert!(function_stack.is_empty());
                assert!(ident.as_ref().is_none_or(|catch_ident| {
                    catch_environment.size() == 1 && catch_ident.impl_().is_some_and(|key| catch_environment.contains(&key))
                }));
                catch_cleanup!();
            }
        }

        if self.consume(FINALLY) {
            match_or_fail!(self, OPENBRACE, "Expected block statement for finally body");
            finally_block = self.parse_block_statement(context, BlockType::Normal);
            fail_if_false!(self, finally_block.is_some(), "Cannot parse finally body");
        }
        fail_if_false!(self, catch_block.is_some() || finally_block.is_some(), "Try statements must have at least a catch or finally block");

        if try_block_contains_return && finally_block.is_none() && self.scope_stack[self.current_function_scope()].constructor_kind() == ConstructorKind::Extends {
            // Empty `finally` statement is necessary to prevent BytecodeGenerator::emitReturn() from being
            // called inside the `try` block, which would otherwise result in errors thrown at steps 10-12
            // of https://tc39.es/ecma262/#sec-ecmascript-function-objects-construct-argumentslist-newtarget
            // being caught by the `catch` block.
            finally_block = Some(context.create_empty_statement(&location));
        }

        Some(context.create_try_statement(
            &location,
            try_block.unwrap_or_default(),
            catch_pattern.unwrap_or_default(),
            catch_block.unwrap_or_default(),
            finally_block.unwrap_or_default(),
            first_line,
            last_line,
            catch_environment,
        ))
    }

    /// `template <class TreeBuilder> TreeStatement parseDebuggerStatement(TreeBuilder&)`.
    pub(crate) fn parse_debugger_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(DEBUGGER));
        let location = self.token_location();
        let mut start_line = self.token_line();
        let end_line = start_line;
        self.next(LexerFlagSet::empty());
        if self.match_(SEMICOLON) {
            start_line = self.token_line();
        }
        fail_if_false!(self, self.auto_semi_colon(), "Debugger keyword must be followed by a ';'");
        Some(context.create_debugger(&location, start_line, end_line))
    }

    /// `template <class TreeBuilder> TreeStatement parseBlockStatement(TreeBuilder&, BlockType)`.
    pub(crate) fn parse_block_statement<B: TreeBuilder>(&mut self, context: &mut B, type_: BlockType) -> Option<B::Statement> {
        debug_assert!(self.match_(OPENBRACE));

        // A block statement inside a switch case/default clause allows using declarations.
        // SetForScope switchCaseScope(m_insideSwitchCaseBody, false);
        let old_inside_switch_case_body = self.inside_switch_case_body;
        self.inside_switch_case_body = false;

        // We should treat the first block statement of the function (the body of the function) as the lexical
        // scope of the function itself, and not the lexical scope of a 'block' statement within the function.
        let mut lexical_scope = AutoCleanupLexicalScope::new();
        // Destrutores vivos: primeiro `lexicalScope`, depois `switchCaseScope`.
        macro_rules! exit_cleanup {
            () => {{
                lexical_scope.cleanup(self);
                self.inside_switch_case_body = old_inside_switch_case_body;
            }};
        }
        let should_push_lexical_scope = self.statement_depth > 0 || type_ == BlockType::StaticBlock;
        if should_push_lexical_scope {
            let new_scope = self.push_scope();
            self.scope_stack[new_scope].set_is_lexical_scope();
            match type_ {
                BlockType::CatchBlock => {
                    self.scope_stack[new_scope].set_is_catch_block_scope();
                    self.scope_stack[new_scope].prevent_var_declarations();
                }
                BlockType::StaticBlock => {
                    self.scope_stack[new_scope].set_source_parse_mode(SourceParseMode::ClassStaticBlockMode);
                    self.scope_stack[new_scope].set_expected_super_binding(SuperBinding::Needed);
                }
                BlockType::Normal => {
                    self.scope_stack[new_scope].prevent_var_declarations();
                }
            }
            lexical_scope.set_is_valid(new_scope, self);
        }
        let location = self.token_location();
        let start_offset = self.token.data.offset as i32;
        let start = self.token_line();
        let mut lexical_environment = VariableEnvironment::default();
        let mut function_stack: FunctionStack = Vec::new();
        self.next(LexerFlagSet::empty());
        if self.match_(CLOSEBRACE) {
            let end_offset = self.token.data.offset as i32;
            self.next(LexerFlagSet::empty());
            if should_push_lexical_scope {
                (lexical_environment, function_stack) = self.pop_scope_cleanup(&mut lexical_scope, B::NEEDS_FREE_VARIABLE_INFO);
            }
            let result = context.create_block_statement(&location, Default::default(), start, self.last_token_location.line, lexical_environment, function_stack);
            context.set_start_offset(&result, start_offset);
            context.set_end_offset(&result, end_offset);
            self.inside_switch_case_body = old_inside_switch_case_body;
            return Some(result);
        }
        let subtree = self.parse_source_elements(context, SourceElementsMode::DontCheckForStrictMode);
        fail_if_false!(self, @hook { exit_cleanup!(); }, subtree.is_some(), "Cannot parse the body of the block statement");
        if !self.match_(CLOSEBRACE) {
            handle_error_token!(self, @hook { exit_cleanup!(); });
            internal_fail_with_message!(self, @hook { exit_cleanup!(); }, true, "Expected a closing '}' at the end of a block statement");
        }
        let end_offset = self.token.data.offset as i32;
        self.next(LexerFlagSet::empty());
        if should_push_lexical_scope {
            (lexical_environment, function_stack) = self.pop_scope_cleanup(&mut lexical_scope, B::NEEDS_FREE_VARIABLE_INFO);
        }
        let result = context.create_block_statement(&location, subtree.unwrap_or_default(), start, self.last_token_location.line, lexical_environment, function_stack);
        context.set_start_offset(&result, start_offset);
        context.set_end_offset(&result, end_offset);
        self.inside_switch_case_body = old_inside_switch_case_body;
        Some(result)
    }
}
