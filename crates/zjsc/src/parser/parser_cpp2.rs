// Segunda fatia de `parser/Parser.cpp` (linhas 719 a 1475), incluída por `include!` em `parser.rs`.
// Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Convenções adicionais desta fatia, além das de `parser_cpp1.rs`:
//
// - `const Identifier**` (saída `duplicateIdentifier`) vira `Option<&mut Option<Identifier>>` e
//   `bool*` (`hasDestructuringPattern`) vira `Option<&mut bool>`; a reborrow nas chamadas
//   recursivas usa `as_deref_mut()`.
// - `const Identifier*& directive` e `unsigned*` seguem a forma de `parse_source_elements`:
//   `&mut Option<Identifier>` e `Option<&mut u32>`.
// - `DepthManager` (`SetForScope<int>` sobre `m_statementDepth`) e o `SetForScope` de
//   `m_parserState.nonLHSCount` viram salvar o valor antigo e restaurá-lo depois de um fechamento
//   que contém o corpo inteiro: todo `return` das macros de erro sai do fechamento e o valor é
//   restaurado em seguida, como o destrutor faria. A classe `DepthManager` em si não existe como
//   tipo (é só esse salvar e restaurar).
// - `Scope::MaybeParseAsGeneratorFunctionForScope` (linhas 175 a 190) não é usada em nenhum ponto
//   de `Parser.cpp`, `Parser.h` ou de qualquer outro arquivo do JavaScriptCore (a declaração
//   antecipada em `Parser.h` 918 é a única referência), por isso não se porta.
// - Os `TreeX x = 0` viram `Option<B::X>` (None é o zero). O que o C++ passa adiante já testado
//   como não nulo vira `.unwrap_or_default()` (o valor nulo do `TreeNode`), nunca `unwrap`.
// - Os parâmetros com valor padrão do `Parser.h` são todos passados por extenso.
// - `Parser::createAssignmentElement(context, ...)` (NEVER_INLINE, só repassa a
//   `context.createAssignmentElement(...)`) não existe: é função de repasse, os chamadores chamam
//   `context.create_assignment_element` direto.
// - A conversão implícita `ArrayPattern`/`ObjectPattern` para `DestructuringPattern` e `Comma` para
//   `Expression` do C++ vira `array_pattern_as_destructuring_pattern`,
//   `object_pattern_as_destructuring_pattern` e `comma_as_expression` no `TreeBuilder`.

/// `static const char* destructuringKindToVariableKindName(DestructuringKind)`.
fn destructuring_kind_to_variable_kind_name(kind: DestructuringKind) -> &'static str {
    match kind {
        DestructuringKind::DestructureToLet | DestructuringKind::DestructureToConst => "lexical variable name",
        DestructuringKind::DestructureToVariables => "variable name",
        DestructuringKind::DestructureToParameters => "parameter name",
        DestructuringKind::DestructureToCatchParameters => "catch parameter name",
        DestructuringKind::DestructureToExpressions => "expression name",
    }
}

impl<T: CharType> Parser<T> {
    /// `template <class TreeBuilder> TreeSourceElements parseSingleFunction(TreeBuilder&, std::optional<int>)`.
    pub(crate) fn parse_single_function<B: TreeBuilder>(&mut self, context: &mut B, function_constructor_parameters_end_position: Option<i32>) -> Option<B::SourceElements> {
        let source_elements = context.create_source_elements();
        let mut statement: Option<B::Statement> = None;
        // O `case IDENT` cai no `default` (`[[fallthrough]]`) quando não é `async` sem escape.
        let mut take_default = true;
        match self.token.type_ {
            FUNCTION => {
                statement = self.parse_function_declaration(context, FunctionDeclarationType::Declaration, ExportType::NotExported, DeclarationDefaultContext::Standard, function_constructor_parameters_end_position);
                take_default = false;
            }
            IDENT => {
                if self.token.data.ident.as_ref() == Some(&self.vm.property_names.r#async) && !self.token.data.escaped {
                    let function_start = self.token_start();
                    self.next(LexerFlagSet::empty());
                    fail_if_false!(self, self.match_(FUNCTION) && !self.lexer.has_line_terminator_before_token(), "Cannot parse the async function");
                    statement = self.parse_async_function_declaration(context, function_start, ExportType::NotExported, DeclarationDefaultContext::Standard, function_constructor_parameters_end_position);
                    take_default = false;
                }
            }
            _ => {}
        }
        if take_default {
            fail_due_to_unexpected_token!(self);
        }

        if let Some(statement) = statement {
            context.set_end_offset(&statement, self.last_token_location.end_offset as i32);
            context.append_statement(&source_elements, statement);
        }

        propagate_error!(self);
        Some(source_elements)
    }

    /// `template <class TreeBuilder> TreeStatement parseStatementListItem(TreeBuilder&, const Identifier*&, unsigned*)`.
    pub(crate) fn parse_statement_list_item<B: TreeBuilder>(&mut self, context: &mut B, directive: &mut Option<Identifier>, directive_literal_length: Option<&mut u32>) -> Option<B::Statement> {
        // The grammar is documented here:
        // http://www.ecma-international.org/ecma-262/6.0/index.html#sec-statements
        // DepthManager statementDepth(&m_statementDepth): restaurado em cada saída.
        let old_statement_depth = self.statement_depth;
        let result = (|| -> Option<B::Statement> {
            self.statement_depth += 1;
            fail_if_stack_overflow!(self, crate::runtime::vm::stack_cost::STATEMENT_LEVEL);
            let mut result: Option<B::Statement> = None;
            let mut should_set_end_offset = true;
            let mut should_set_pause_location = false;

            // `TreeBuilder::shouldSkipPauseLocation(result)`: o `ASTBuilder` devolve `!statement || ...`.
            let should_skip_pause_location = |context: &B, statement: &Option<B::Statement>| -> bool {
                match statement {
                    Some(statement) => context.should_skip_pause_location(statement),
                    None => true,
                }
            };

            match self.token.type_ {
                CONSTTOKEN => {
                    result = self.parse_variable_declaration(context, DeclarationType::ConstDeclaration, ExportType::NotExported);
                    should_set_pause_location = true;
                }
                LET => {
                    let mut should_parse_variable_declaration = true;
                    if !self.strict_mode() {
                        let save_point = self.create_save_point(context);
                        self.next(LexerFlagSet::empty());
                        // Intentionally use `matchIdentifierOrPossiblyEscapedContextualKeyword()` and not `matchSpecIdentifier()`.
                        // We would like contextual keywords to fall under parseVariableDeclaration even when not used as identifiers.
                        // For example, under a generator context, matchSpecIdentifier() for "yield" returns `false`.
                        // But we would like to enter parseVariableDeclaration and raise an error under the context of parseVariableDeclaration
                        // to raise consistent errors between "var", "const" and "let".
                        if !self.match_identifier_or_possibly_escaped_contextual_keyword() && !self.match_(OPENBRACE) && !self.match_(OPENBRACKET) {
                            should_parse_variable_declaration = false;
                        }
                        self.restore_save_point(context, &save_point);
                    }
                    if should_parse_variable_declaration {
                        result = self.parse_variable_declaration(context, DeclarationType::LetDeclaration, ExportType::NotExported);
                    } else {
                        let allow_function_declaration_as_statement = true;
                        result = self.parse_expression_or_label_statement(context, allow_function_declaration_as_statement);
                    }
                    should_set_pause_location = !should_skip_pause_location(context, &result);
                }
                CLASSTOKEN => {
                    result = self.parse_class_declaration(context, ExportType::NotExported, DeclarationDefaultContext::Standard);
                }
                FUNCTION => {
                    result = self.parse_function_declaration(context, FunctionDeclarationType::Declaration, ExportType::NotExported, DeclarationDefaultContext::Standard, None);
                }
                // Os quatro `case` seguintes do C++ caem um no outro (`[[fallthrough]]`): a entrada no
                // `ESCAPED_KEYWORD` percorre os quatro trechos, no `IDENT` os três últimos, no `AWAIT` os
                // dois últimos e no `YIELD` só o último. O `break` do C++ vira `break 'statement`.
                ESCAPED_KEYWORD | IDENT | AWAIT | YIELD => {
                    let entry = self.token.type_;
                    'statement: {
                        if entry == ESCAPED_KEYWORD && !self.match_allowed_escaped_contextual_keyword() {
                            fail_due_to_unexpected_token!(self);
                        }
                        if entry == ESCAPED_KEYWORD || entry == IDENT {
                            if Options::use_explicit_resource_management()
                                && self.token.data.ident.as_ref() == Some(&self.vm.property_names.using_identifier)
                                && !self.token.data.escaped
                            {
                                let save_point = self.create_save_point(context);
                                self.next(LexerFlagSet::empty());
                                if !self.lexer.has_line_terminator_before_token() && self.match_spec_identifier() {
                                    self.restore_save_point(context, &save_point);
                                    let current = self.current_scope();
                                    semantic_fail_if_true!(self, self.scope_stack[current].is_global_code() && !self.scope_stack[current].is_module_code() && self.statement_depth == 1, "'using' declaration is not allowed at the top level of a script or eval");
                                    semantic_fail_if_true!(self, self.inside_switch_case_body, "'using' declaration is not allowed directly in a switch case or default clause");
                                    result = self.parse_variable_declaration(context, DeclarationType::UsingDeclaration, ExportType::NotExported);
                                    should_set_pause_location = true;
                                    break 'statement;
                                }
                                self.restore_save_point(context, &save_point);
                            }
                            if self.token.data.ident.as_ref() == Some(&self.vm.property_names.r#async) && !self.token.data.escaped {
                                // Eagerly parse as AsyncFunctionDeclaration. This is the uncommon case,
                                // but could be mistakenly parsed as an AsyncFunctionExpression.
                                let save_point = self.create_save_point(context);
                                let function_start = self.token_start();
                                self.next(LexerFlagSet::empty());
                                if self.match_(FUNCTION) && !self.lexer.has_line_terminator_before_token() {
                                    result = self.parse_async_function_declaration(context, function_start, ExportType::NotExported, DeclarationDefaultContext::Standard, None);
                                    break 'statement;
                                }
                                self.restore_save_point(context, &save_point);
                            }
                        }
                        if entry != YIELD {
                            let function_scope = self.current_function_scope();
                            if Options::use_explicit_resource_management()
                                && self.match_(AWAIT)
                                && !self.parser_state.class_field_init_masks_async
                                && (self.scope_stack[function_scope].is_async_function_boundary() || is_module_parse_mode(self.source_parse_mode()))
                            {
                                let save_point = self.create_save_point(context);
                                self.next(LexerFlagSet::empty());
                                if !self.lexer.has_line_terminator_before_token()
                                    && self.match_(IDENT)
                                    && self.token.data.ident.as_ref() == Some(&self.vm.property_names.using_identifier)
                                    && !self.token.data.escaped
                                {
                                    self.next(LexerFlagSet::empty());
                                    if !self.lexer.has_line_terminator_before_token() && self.match_spec_identifier() {
                                        self.restore_save_point(context, &save_point);
                                        let current = self.current_scope();
                                        semantic_fail_if_true!(self, self.scope_stack[current].is_global_code() && !self.scope_stack[current].is_module_code() && self.statement_depth == 1, "'await using' declaration is not allowed at the top level of a script or eval");
                                        semantic_fail_if_true!(self, self.inside_switch_case_body, "'await using' declaration is not allowed directly in a switch case or default clause");
                                        let function_scope = self.current_function_scope();
                                        self.scope_stack[function_scope].set_uses_await();
                                        result = self.parse_variable_declaration(context, DeclarationType::AwaitUsingDeclaration, ExportType::NotExported);
                                        should_set_pause_location = true;
                                        break 'statement;
                                    }
                                }
                                self.restore_save_point(context, &save_point);
                            }
                        }
                        // case YIELD:
                        let current = self.current_scope();
                        if self.scope_stack[current].is_static_block() {
                            fail_if_true!(self, self.match_(YIELD), "Cannot use 'yield' within static block");
                            fail_if_true!(self, self.match_(AWAIT), "Cannot use 'await' within static block");
                        }
                        // This is a convenient place to notice labeled statements
                        // (even though we also parse them as normal statements)
                        // because we allow the following type of code in sloppy mode:
                        // ``` function foo() { label: function bar() { } } ```
                        let allow_function_declaration_as_statement = true;
                        result = self.parse_expression_or_label_statement(context, allow_function_declaration_as_statement);
                        should_set_pause_location = !should_skip_pause_location(context, &result);
                    }
                }
                _ => {
                    self.statement_depth -= 1; // parseStatement() increments the depth.
                    result = self.parse_statement(context, directive, directive_literal_length);
                    should_set_end_offset = false;
                }
            }

            if let Some(statement) = &result {
                if should_set_end_offset {
                    context.set_end_offset(statement, self.last_token_location.end_offset as i32);
                }
                if should_set_pause_location {
                    self.record_pause_location(context.breakpoint_location(statement));
                }
            }

            result
        })();
        self.statement_depth = old_statement_depth;
        result
    }

    /// `template <class TreeBuilder> TreeStatement parseVariableDeclaration(TreeBuilder&, DeclarationType, ExportType)`.
    pub(crate) fn parse_variable_declaration<B: TreeBuilder>(&mut self, context: &mut B, declaration_type: DeclarationType, export_type: ExportType) -> Option<B::Statement> {
        debug_assert!(
            self.match_(VAR)
                || self.match_(LET)
                || self.match_(CONSTTOKEN)
                || (declaration_type == DeclarationType::UsingDeclaration && self.match_(IDENT))
                || (declaration_type == DeclarationType::AwaitUsingDeclaration && self.match_(AWAIT))
        );
        let location = self.token_location();
        let start = self.token_line();
        let end: i32 = 0;
        let mut scratch: i32 = 0;
        let mut scratch1: Option<B::DestructuringPattern> = None;
        let mut scratch2: Option<B::Expression> = None;
        // O C++ passa o mesmo `scratch3` nos três parâmetros de saída; só são escritos, então três
        // variáveis distintas se comportam igual.
        let mut scratch3_ident_start = JSTextPosition::default();
        let mut scratch3_init_start = JSTextPosition::default();
        let mut scratch3_init_end = JSTextPosition::default();
        let mut scratch_bool = false;
        let variable_decls = self.parse_variable_declaration_list(
            context,
            &mut scratch,
            &mut scratch1,
            &mut scratch2,
            &mut scratch3_ident_start,
            &mut scratch3_init_start,
            &mut scratch3_init_end,
            VarDeclarationListContext::VarDeclarationContext,
            declaration_type,
            export_type,
            &mut scratch_bool,
        );
        propagate_error!(self);
        fail_if_false!(self, self.auto_semi_colon(), "Expected ';' after variable declaration");

        Some(context.create_declaration_statement(&location, variable_decls.unwrap_or_default(), start, end))
    }

    /// `template <class TreeBuilder> TreeStatement parseDoWhileStatement(TreeBuilder&)`.
    pub(crate) fn parse_do_while_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(DO));
        let start_line = self.token_line();
        self.next(LexerFlagSet::empty());
        let mut unused: Option<Identifier> = None;
        self.start_loop();
        let statement = self.parse_statement(context, &mut unused, None);
        self.end_loop();
        fail_if_false!(self, statement.is_some(), "Expected a statement following 'do'");
        let end_line = self.token_line();
        let location = self.token_location();
        handle_production_or_fail!(self, WHILE, "while", "end", "do-while loop");
        handle_production_or_fail!(self, OPENPAREN, "(", "start", "do-while loop condition");
        semantic_fail_if_true!(self, self.match_(CLOSEPAREN), "Must provide an expression as a do-while loop condition");
        let expr = self.parse_expression(context);
        fail_if_false!(self, expr.is_some(), "Unable to parse do-while loop condition");
        let expr = expr.unwrap_or_default();
        self.record_pause_location(context.breakpoint_location(&expr));
        handle_production_or_fail!(self, CLOSEPAREN, ")", "end", "do-while loop condition");
        self.consume(SEMICOLON); // Always performs automatic semicolon insertion.
        Some(context.create_do_while_statement(&location, statement.unwrap_or_default(), expr, start_line, end_line))
    }

    /// `template <class TreeBuilder> TreeStatement parseWhileStatement(TreeBuilder&)`.
    pub(crate) fn parse_while_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(WHILE));
        let location = self.token_location();
        let start_line = self.token_line();
        self.next(LexerFlagSet::empty());

        handle_production_or_fail!(self, OPENPAREN, "(", "start", "while loop condition");
        semantic_fail_if_true!(self, self.match_(CLOSEPAREN), "Must provide an expression as a while loop condition");
        let expr = self.parse_expression(context);
        fail_if_false!(self, expr.is_some(), "Unable to parse while loop condition");
        let expr = expr.unwrap_or_default();
        self.record_pause_location(context.breakpoint_location(&expr));
        let end_line = self.token_line();
        handle_production_or_fail!(self, CLOSEPAREN, ")", "end", "while loop condition");

        let mut unused: Option<Identifier> = None;
        self.start_loop();
        let statement = self.parse_statement(context, &mut unused, None);
        self.end_loop();
        fail_if_false!(self, statement.is_some(), "Expected a statement as the body of a while loop");
        Some(context.create_while_statement(&location, expr, statement.unwrap_or_default(), start_line, end_line))
    }

    /// `template <class TreeBuilder> TreeExpression parseVariableDeclarationList(...)`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn parse_variable_declaration_list<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        declarations: &mut i32,
        last_pattern: &mut Option<B::DestructuringPattern>,
        last_initializer: &mut Option<B::Expression>,
        ident_start: &mut JSTextPosition,
        init_start: &mut JSTextPosition,
        init_end: &mut JSTextPosition,
        declaration_list_context: VarDeclarationListContext,
        declaration_type: DeclarationType,
        export_type: ExportType,
        for_loop_const_does_not_have_initializer: &mut bool,
    ) -> Option<B::Expression> {
        debug_assert!(
            declaration_type == DeclarationType::LetDeclaration
                || declaration_type == DeclarationType::VarDeclaration
                || declaration_type == DeclarationType::ConstDeclaration
                || declaration_type == DeclarationType::UsingDeclaration
                || declaration_type == DeclarationType::AwaitUsingDeclaration
        );
        let mut head: Option<B::Expression> = None;
        let mut head_location = JSTokenLocation::default();
        let mut tail: Option<B::Comma> = None;
        let mut last_ident: Option<Identifier>;
        let mut last_ident_token = JSToken::default();
        let assignment_context = self.assignment_context_from_declaration_type(declaration_type);
        let is_using_declaration = declaration_type == DeclarationType::UsingDeclaration || declaration_type == DeclarationType::AwaitUsingDeclaration;
        loop {
            *last_pattern = None;
            last_ident = None;
            let mut location = self.token_location();
            self.next(LexerFlagSet::empty());
            if head.is_none() && declaration_type == DeclarationType::AwaitUsingDeclaration {
                debug_assert!(self.match_(IDENT) && self.token.data.ident.as_ref() == Some(&self.vm.property_names.using_identifier));
                self.next(LexerFlagSet::empty());
            }
            if head.is_some() {
                // Move the location of subsequent declarations after the comma.
                location = self.token_location();
            }
            let mut node: Option<B::Expression> = None;
            *declarations += 1;
            let mut has_initializer = false;

            fail_if_true!(self, self.match_(PRIVATENAME), "Cannot use a private name to declare a variable");
            if is_using_declaration {
                // 'using' declarations cannot have a destructuring pattern.
                fail_if_true!(self, self.match_(OPENBRACE) || self.match_(OPENBRACKET), "'using' declarations cannot have a destructuring pattern");
                fail_if_false!(self, self.match_spec_identifier(), "Expected an identifier name in 'using' declaration");
            }
            if self.match_spec_identifier() {
                let current = self.current_scope();
                semantic_fail_if_true!(self, self.scope_stack[current].is_static_block() && self.is_arguments_identifier(), "Cannot use 'arguments' as an identifier in static block");
                fail_if_true!(
                    self,
                    self.is_possibly_escaped_let(&self.token) && (declaration_type == DeclarationType::LetDeclaration || declaration_type == DeclarationType::ConstDeclaration || is_using_declaration),
                    "Cannot use 'let' as an identifier name for a LexicalDeclaration"
                );
                semantic_fail_if_true!(self, self.is_disallowed_identifier_await(&self.token), "Cannot use 'await' as a ", self.declaration_type_to_variable_kind(declaration_type), " ", self.disallowed_identifier_await_reason());
                let var_start = *self.token_start_position();
                let var_start_location = self.token_location();
                *ident_start = var_start;
                let name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                last_ident = Some(name.clone());
                last_ident_token = self.token.clone();
                self.next(LexerFlagSet::empty());
                has_initializer = self.match_(EQUAL);
                let declaration_result = self.declare_variable(&name, declaration_type, DeclarationImportType::NotImported);
                if declaration_result != DeclarationResult::VALID {
                    fail_if_true_if_strict!(self, (declaration_result & DeclarationResult::INVALID_STRICT_MODE) != 0, "Cannot declare a variable named ", name, " in strict mode");
                    if (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0 {
                        semantic_fail_if_true!(self, declaration_type == DeclarationType::LetDeclaration, "Cannot declare a let variable twice: '", name, "'");
                        semantic_fail_if_true!(self, declaration_type == DeclarationType::ConstDeclaration, "Cannot declare a const variable twice: '", name, "'");
                        semantic_fail_if_true!(self, is_using_declaration, "Cannot declare a using variable twice: '", name, "'");
                        debug_assert!(declaration_type == DeclarationType::VarDeclaration);
                        semantic_fail!(self, "Cannot declare a var variable that shadows a let/const/class variable: '", name, "'");
                    }
                }
                if export_type == ExportType::Exported {
                    semantic_fail_if_false!(self, self.export_name(&name), "Cannot export a duplicate name '", name, "'");
                    if let Some(data) = &self.module_scope_data {
                        data.export_binding_same_name(&name);
                    }
                }

                if has_initializer {
                    let var_divot = *self.token_start_position() + 1i32;
                    *init_start = *self.token_start_position();
                    self.next(B::DONT_BUILD_STRINGS); // consume '='
                    propagate_error!(self);
                    let initializer = self.parse_assignment_expression(context);
                    *init_end = self.last_token_end_position();
                    *last_initializer = initializer.clone();
                    fail_if_false!(self, initializer.is_some(), "Expected expression as the intializer for the variable '", name, "'");

                    node = Some(context.create_assign_resolve(&location, &name, initializer.unwrap_or_default(), var_start, var_divot, self.last_token_end_position(), assignment_context));
                } else {
                    if is_using_declaration {
                        if declaration_list_context == VarDeclarationListContext::ForLoopContext {
                            *for_loop_const_does_not_have_initializer = true;
                        } else {
                            fail_if_false!(self, false, "'using' declaration requires an initializer");
                        }
                    }
                    if declaration_list_context == VarDeclarationListContext::ForLoopContext && declaration_type == DeclarationType::ConstDeclaration {
                        *for_loop_const_does_not_have_initializer = true;
                    }
                    fail_if_true!(self, declaration_list_context != VarDeclarationListContext::ForLoopContext && declaration_type == DeclarationType::ConstDeclaration, "const declared variable '", name, "'", " must have an initializer");
                    if declaration_type == DeclarationType::VarDeclaration {
                        node = Some(context.create_empty_var_expression(&var_start_location, &name));
                    } else {
                        node = Some(context.create_empty_let_expression(&var_start_location, &name));
                    }
                }
            } else {
                last_ident = None;
                debug_assert!(!is_using_declaration); // Already handled above with failIfFalse(matchSpecIdentifier()).
                let pattern: Option<B::DestructuringPattern>;
                {
                    let allows_in_operator = true;
                    // SetForScope allowsInScope(m_allowsIn, allowsInOperator)
                    let old_allows_in = self.allows_in;
                    self.allows_in = allows_in_operator;
                    let kind = self.destructuring_kind_from_declaration_type(declaration_type);
                    pattern = self.parse_destructuring_pattern(context, kind, export_type, None, None, assignment_context, 0);
                    self.allows_in = old_allows_in;
                }
                fail_if_false!(self, pattern.is_some(), "Cannot parse this destructuring pattern");
                has_initializer = self.match_(EQUAL);
                fail_if_true!(self, declaration_list_context == VarDeclarationListContext::VarDeclarationContext && !has_initializer, "Expected an initializer in destructuring variable declaration");
                *last_pattern = pattern.clone();
                if has_initializer {
                    self.next(B::DONT_BUILD_STRINGS); // consume '='
                    let rhs = self.parse_assignment_expression(context);
                    propagate_error!(self);
                    debug_assert!(rhs.is_some());
                    node = Some(context.create_destructuring_assignment(&location, pattern.unwrap_or_default(), rhs.clone().unwrap_or_default()));
                    *last_initializer = rhs;
                }
            }

            if let Some(node) = node {
                if head.is_none() {
                    head = Some(node);
                    head_location = location;
                } else {
                    if tail.is_none() {
                        let head_expression = head.clone().unwrap_or_default();
                        self.record_pause_location(context.breakpoint_location(&head_expression));
                        let comma = context.create_comma_expr(&head_location, head_expression);
                        head = Some(comma.clone().into());
                        tail = Some(comma);
                    }
                    self.record_pause_location(context.breakpoint_location(&node));
                    let current_tail = tail.take().unwrap_or_default();
                    tail = Some(context.append_to_comma_expr(&location, current_tail, node));
                }
            }
            if !self.match_(COMMA) {
                break;
            }
        }
        if let Some(last_ident) = last_ident {
            *last_pattern = Some(context.create_binding_location(&last_ident_token.location(), &last_ident, last_ident_token.start_position, last_ident_token.end_position, assignment_context));
        }

        head
    }

    /// `bool Parser<LexerType>::declareRestOrNormalParameter(const Identifier&, const Identifier**)`.
    pub(crate) fn declare_rest_or_normal_parameter(&mut self, name: &Identifier, duplicate_identifier: Option<&mut Option<Identifier>>) -> bool {
        let declaration_result = self.declare_parameter(name);
        if (declaration_result & DeclarationResult::INVALID_STRICT_MODE) != 0 && self.strict_mode() {
            semantic_fail_if_true!(self, self.is_eval_or_arguments(name), "Cannot destructure to a parameter name '", name, "' in strict mode");
            if self.parser_state.last_function_name.as_ref() == Some(name) {
                semantic_fail!(self, "Cannot declare a parameter named '", name, "' as it shadows the name of a strict mode function");
            }
            semantic_failure_due_to_keyword!(self, "parameter name");
            if !self.lexer.is_reparsing_function() && self.has_declared_parameter(name) {
                semantic_fail!(self, "Cannot declare a parameter named '", name, "' in strict mode as it has already been declared");
            }
            semantic_fail!(self, "Cannot declare a parameter named '", name, "' in strict mode");
        }
        if (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0 {
            // It's not always an error to define a duplicate parameter.
            // It's only an error when there are default parameter values or destructuring parameters.
            // We note this value now so we can check it later.
            if let Some(duplicate_identifier) = duplicate_identifier {
                *duplicate_identifier = Some(name.clone());
            }
        }

        true
    }

    /// `template <class TreeBuilder> NEVER_INLINE TreeDestructuringPattern createBindingPattern(...)`.
    pub(crate) fn create_binding_pattern<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        kind: DestructuringKind,
        export_type: ExportType,
        name: &Identifier,
        token: &JSToken,
        binding_context: AssignmentContext,
        duplicate_identifier: Option<&mut Option<Identifier>>,
    ) -> Option<B::DestructuringPattern> {
        match kind {
            DestructuringKind::DestructureToVariables => {
                let declaration_result = self.declare_variable(name, DeclarationType::VarDeclaration, DeclarationImportType::NotImported);
                fail_if_true_if_strict!(self, (declaration_result & DeclarationResult::INVALID_STRICT_MODE) != 0, "Cannot declare a variable named '", name, "' in strict mode");
                semantic_fail_if_true!(self, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Cannot declare a var variable that shadows a let/const/class variable: '", name, "'");
            }

            DestructuringKind::DestructureToLet | DestructuringKind::DestructureToConst | DestructuringKind::DestructureToCatchParameters => {
                let declaration_type = if kind == DestructuringKind::DestructureToConst { DeclarationType::ConstDeclaration } else { DeclarationType::LetDeclaration };
                let declaration_result = self.declare_variable(name, declaration_type, DeclarationImportType::NotImported);
                if declaration_result != DeclarationResult::VALID {
                    fail_if_true_if_strict!(self, (declaration_result & DeclarationResult::INVALID_STRICT_MODE) != 0, "Cannot destructure to a variable named '", name, "' in strict mode");
                    fail_if_true!(self, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Cannot declare a lexical variable twice: '", name, "'");
                }
            }

            DestructuringKind::DestructureToParameters => {
                self.declare_rest_or_normal_parameter(name, duplicate_identifier);
                propagate_error!(self);
            }

            DestructuringKind::DestructureToExpressions => {}
        }

        if export_type == ExportType::Exported {
            semantic_fail_if_false!(self, self.export_name(name), "Cannot export a duplicate name '", name, "'");
            if let Some(data) = &self.module_scope_data {
                data.export_binding_same_name(name);
            }
        }
        Some(context.create_binding_location(&token.location(), name, token.start_position, token.end_position, binding_context))
    }

    /// `template <class TreeBuilder> TreeSourceElements parseArrowFunctionSingleExpressionBodySourceElements(TreeBuilder&)`.
    pub(crate) fn parse_arrow_function_single_expression_body_source_elements<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::SourceElements> {
        debug_assert!(!self.match_(OPENBRACE));

        let location = self.token_location();
        let start = *self.token_start_position();

        fail_if_stack_overflow!(self);
        let expr = self.parse_assignment_expression(context);
        fail_if_false!(self, expr.is_some(), "Cannot parse the arrow function expression");
        let expr = expr.unwrap_or_default();

        context.set_end_offset(&expr, self.last_token_location.end_offset as i32);

        let end = *self.token_end_position();

        let source_elements = context.create_source_elements();
        let body = context.create_return_statement(&location, expr, start, end);
        context.set_end_offset(&body, self.last_token_location.end_offset as i32);
        self.record_pause_location(context.breakpoint_location(&body));
        context.append_statement(&source_elements, body);

        Some(source_elements)
    }

    /// `template <class TreeBuilder> TreeDestructuringPattern tryParseDestructuringPatternExpression(TreeBuilder&, AssignmentContext)`.
    pub(crate) fn try_parse_destructuring_pattern_expression<B: TreeBuilder>(&mut self, context: &mut B, binding_context: AssignmentContext) -> Option<B::DestructuringPattern> {
        self.parse_destructuring_pattern(context, DestructuringKind::DestructureToExpressions, ExportType::NotExported, None, None, binding_context, 0)
    }

    /// `template <class TreeBuilder> TreeDestructuringPattern parseBindingOrAssignmentElement(...)`.
    pub(crate) fn parse_binding_or_assignment_element<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        kind: DestructuringKind,
        export_type: ExportType,
        duplicate_identifier: Option<&mut Option<Identifier>>,
        has_destructuring_pattern: Option<&mut bool>,
        binding_context: AssignmentContext,
        depth: i32,
    ) -> Option<B::DestructuringPattern> {
        if kind == DestructuringKind::DestructureToExpressions {
            return self.parse_assignment_element(context, kind, export_type, duplicate_identifier, has_destructuring_pattern, binding_context, depth);
        }
        self.parse_destructuring_pattern(context, kind, export_type, duplicate_identifier, has_destructuring_pattern, binding_context, depth)
    }

    /// `template <class TreeBuilder> TreeDestructuringPattern parseObjectRestAssignmentElement(TreeBuilder&)`.
    pub(crate) fn parse_object_rest_assignment_element<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::DestructuringPattern> {
        let start_position = *self.token_start_position();
        let element = self.parse_member_expression(context);

        semantic_fail_if_true!(self, element.as_ref().is_none_or(|element| !context.is_assignment_location(element)), "Invalid destructuring assignment target");
        let element = element.unwrap_or_default();
        if self.strict_mode() {
            if let Some(last_identifier) = self.parser_state.last_identifier.clone() {
                if context.is_resolve(&element) {
                    let is_eval_or_arguments = self.vm.property_names.eval == last_identifier || self.vm.property_names.arguments == last_identifier;
                    fail_if_true!(self, is_eval_or_arguments, "Cannot modify '", last_identifier, "' in strict mode");
                }
            }
        }

        Some(context.create_assignment_element(&element, start_position, self.last_token_end_position()))
    }

    /// `template <class TreeBuilder> TreeDestructuringPattern parseAssignmentElement(...)`.
    pub(crate) fn parse_assignment_element<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        kind: DestructuringKind,
        export_type: ExportType,
        mut duplicate_identifier: Option<&mut Option<Identifier>>,
        mut has_destructuring_pattern: Option<&mut bool>,
        binding_context: AssignmentContext,
        depth: i32,
    ) -> Option<B::DestructuringPattern> {
        if self.match_(OPENBRACE) || self.match_(OPENBRACKET) {
            let save_point = self.create_save_point(context);
            let assignment_target = self.parse_destructuring_pattern(context, kind, export_type, duplicate_identifier.as_deref_mut(), has_destructuring_pattern.as_deref_mut(), binding_context, depth);
            if assignment_target.is_some() && !self.match_(DOT) && !self.match_(OPENBRACKET) && !self.match_(OPENPAREN) && !self.match_(BACKQUOTE) {
                return assignment_target;
            }
            self.restore_save_point(context, &save_point);
        }

        let start_position = *self.token_start_position();
        let element = self.parse_member_expression(context);

        semantic_fail_if_false!(self, element.as_ref().is_some_and(|element| context.is_assignment_location(element)), "Invalid destructuring assignment target");
        let element = element.unwrap_or_default();

        if self.strict_mode() {
            if let Some(last_identifier) = self.parser_state.last_identifier.clone() {
                if context.is_resolve(&element) {
                    let is_eval_or_arguments = self.vm.property_names.eval == last_identifier || self.vm.property_names.arguments == last_identifier;
                    fail_if_true_if_strict!(self, is_eval_or_arguments, "Cannot modify '", last_identifier, "' in strict mode");
                }
            }
        }

        Some(context.create_assignment_element(&element, start_position, self.last_token_end_position()))
    }

    /// `template <class TreeBuilder> NEVER_INLINE TreeDestructuringPattern parseObjectRestElement(...)`.
    pub(crate) fn parse_object_rest_element<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        kind: DestructuringKind,
        export_type: ExportType,
        duplicate_identifier: Option<&mut Option<Identifier>>,
        binding_context: AssignmentContext,
    ) -> Option<B::DestructuringPattern> {
        debug_assert!(kind != DestructuringKind::DestructureToExpressions);
        fail_if_stack_overflow!(self);

        if !self.match_spec_identifier() {
            semantic_failure_due_to_keyword!(self, destructuring_kind_to_variable_kind_name(kind));
            fail_with_message!(self, "Expected a binding element");
        }
        fail_if_true!(self, self.match_(LET) && (kind == DestructuringKind::DestructureToLet || kind == DestructuringKind::DestructureToConst), "Cannot use 'let' as an identifier name for a LexicalDeclaration");
        semantic_fail_if_true!(self, self.is_disallowed_identifier_await(&self.token), "Cannot use 'await' as a ", destructuring_kind_to_variable_kind_name(kind), " ", self.disallowed_identifier_await_reason());
        let name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
        let token = self.token.clone();
        let pattern = self.create_binding_pattern(context, kind, export_type, &name, &token, binding_context, duplicate_identifier);
        self.next(LexerFlagSet::empty());
        pattern
    }

    /// `template <class TreeBuilder> NEVER_INLINE TreeDestructuringPattern parseObjectRestBindingOrAssignmentElement(...)`.
    pub(crate) fn parse_object_rest_binding_or_assignment_element<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        kind: DestructuringKind,
        export_type: ExportType,
        duplicate_identifier: Option<&mut Option<Identifier>>,
        binding_context: AssignmentContext,
    ) -> Option<B::DestructuringPattern> {
        if kind == DestructuringKind::DestructureToExpressions {
            return self.parse_object_rest_assignment_element(context);
        }
        self.parse_object_rest_element(context, kind, export_type, duplicate_identifier, binding_context)
    }

    /// `template <class TreeBuilder> NEVER_INLINE TreeDestructuringPattern parseDestructuringPattern(...)`.
    pub(crate) fn parse_destructuring_pattern<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        kind: DestructuringKind,
        export_type: ExportType,
        mut duplicate_identifier: Option<&mut Option<Identifier>>,
        mut has_destructuring_pattern: Option<&mut bool>,
        binding_context: AssignmentContext,
        depth: i32,
    ) -> Option<B::DestructuringPattern> {
        fail_if_stack_overflow!(self);
        self.parser_state.assignment_count += 1;
        // SetForScope nonLHSCountScope(m_parserState.nonLHSCount): restaurado em cada saída.
        let old_non_lhs_count = self.parser_state.non_lhs_count;
        let result = (|| -> Option<B::DestructuringPattern> {
            let pattern: Option<B::DestructuringPattern>;
            match self.token.type_ {
                OPENBRACKET => {
                    let divot_start = *self.token_start_position();
                    let array_pattern = context.create_array_pattern(&self.token.location());
                    self.next(LexerFlagSet::empty());

                    if let Some(has_destructuring_pattern) = has_destructuring_pattern.as_deref_mut() {
                        *has_destructuring_pattern = true;
                    }

                    let mut rest_element_was_found = false;

                    loop {
                        while self.match_(COMMA) {
                            context.append_array_pattern_skip_entry(&array_pattern, &self.token.location());
                            self.next(LexerFlagSet::empty());
                        }
                        propagate_error!(self);

                        if self.match_(CLOSEBRACKET) {
                            break;
                        }

                        if self.match_(DOTDOTDOT) {
                            let location = self.token.location();
                            self.next(LexerFlagSet::empty());
                            let inner_pattern = self.parse_binding_or_assignment_element(context, kind, export_type, duplicate_identifier.as_deref_mut(), has_destructuring_pattern.as_deref_mut(), binding_context, depth + 1);
                            if kind == DestructuringKind::DestructureToExpressions && inner_pattern.is_none() {
                                return None;
                            }
                            fail_if_false!(self, inner_pattern.is_some(), "Cannot parse this destructuring pattern");
                            context.append_array_pattern_rest_entry(&array_pattern, &location, inner_pattern.unwrap_or_default());
                            rest_element_was_found = true;
                            break;
                        }

                        let location = self.token.location();
                        let inner_pattern = self.parse_binding_or_assignment_element(context, kind, export_type, duplicate_identifier.as_deref_mut(), has_destructuring_pattern.as_deref_mut(), binding_context, depth + 1);
                        if kind == DestructuringKind::DestructureToExpressions && inner_pattern.is_none() {
                            return None;
                        }
                        fail_if_false!(self, inner_pattern.is_some(), "Cannot parse this destructuring pattern");
                        let default_value = self.parse_default_value_for_destructuring_pattern(context);
                        propagate_error!(self);
                        context.append_array_pattern_entry(&array_pattern, &location, inner_pattern.unwrap_or_default(), default_value.unwrap_or_default());
                        if !self.consume(COMMA) {
                            break;
                        }
                    }

                    consume_or_fail!(self, CLOSEBRACKET, if rest_element_was_found { "Expected a closing ']' following a rest element destructuring pattern" } else { "Expected either a closing ']' or a ',' following an element destructuring pattern" });
                    context.finish_array_pattern(&array_pattern, divot_start, divot_start, self.last_token_end_position());
                    pattern = Some(array_pattern.into());
                }
                OPENBRACE => {
                    let divot_start = *self.token_start_position();
                    let object_pattern = context.create_object_pattern(&self.token.location());
                    self.next(LexerFlagSet::empty());

                    if let Some(has_destructuring_pattern) = has_destructuring_pattern.as_deref_mut() {
                        *has_destructuring_pattern = true;
                    }

                    let mut rest_element_was_found = false;

                    loop {
                        let mut was_string = false;

                        if self.match_(CLOSEBRACE) {
                            break;
                        }

                        if self.match_(DOTDOTDOT) {
                            let location = self.token.location();
                            self.next(LexerFlagSet::empty());
                            let inner_pattern = self.parse_object_rest_binding_or_assignment_element(context, kind, export_type, duplicate_identifier.as_deref_mut(), binding_context);
                            propagate_error!(self);
                            let Some(inner_pattern) = inner_pattern else {
                                return None;
                            };
                            context.append_object_pattern_rest_entry(&self.vm, &object_pattern, &location, inner_pattern);
                            rest_element_was_found = true;
                            context.set_contains_object_rest_element(&object_pattern, rest_element_was_found);
                            break;
                        }

                        let mut property_name: Option<Identifier> = None;
                        let mut property_expression: Option<B::Expression> = None;
                        let mut inner_pattern: Option<B::DestructuringPattern> = None;
                        let location = self.token.location();
                        let escaped_keyword = self.match_(ESCAPED_KEYWORD);
                        if escaped_keyword || self.match_spec_identifier() {
                            let let_matched = self.match_(LET);
                            let name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                            property_name = Some(name.clone());
                            let identifier_token = self.token.clone();
                            self.next(LexerFlagSet::empty());
                            if self.consume(COLON) {
                                inner_pattern = self.parse_binding_or_assignment_element(context, kind, export_type, duplicate_identifier.as_deref_mut(), has_destructuring_pattern.as_deref_mut(), binding_context, depth + 1);
                            } else {
                                semantic_fail_if_true!(self, let_matched && (kind == DestructuringKind::DestructureToLet || kind == DestructuringKind::DestructureToConst), "Cannot use the keyword 'let' as a lexical variable name");
                                semantic_fail_if_true!(self, escaped_keyword, "Cannot use abbreviated destructuring syntax for keyword '", name, "'");
                                semantic_fail_if_true!(self, self.is_disallowed_identifier_await(&identifier_token), "Cannot use 'await' as a ", destructuring_kind_to_variable_kind_name(kind), " ", self.disallowed_identifier_await_reason());
                                if kind == DestructuringKind::DestructureToExpressions {
                                    let is_eval_or_arguments = self.vm.property_names.eval == name || self.vm.property_names.arguments == name;
                                    fail_if_true_if_strict!(self, is_eval_or_arguments, "Cannot modify '", name, "' in strict mode");

                                    if self.match_(EQUAL) {
                                        let is_eval = self.vm.property_names.eval == name;
                                        let current = self.current_scope();
                                        self.scope_stack[current].use_variable_identifier(&name, is_eval);
                                    }
                                }
                                inner_pattern = self.create_binding_pattern(context, kind, export_type, &name, &identifier_token, binding_context, duplicate_identifier.as_deref_mut());
                            }
                        } else {
                            let token_type = self.token.type_;
                            match self.token.type_ {
                                DOUBLE | INTEGER => {
                                    let number = self.token.data.double_value;
                                    let arena = self.parser_arena.identifier_arena();
                                    property_name = Some(arena.borrow_mut().make_numeric_identifier(&self.vm, number));
                                }
                                STRING => {
                                    property_name = self.token.data.ident.clone();
                                    was_string = true;
                                }
                                BIGINT => {
                                    let big_int_string = self.token.data.big_int_string.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                                    let radix = self.token.data.radix;
                                    let arena = self.parser_arena.identifier_arena();
                                    property_name = arena.borrow_mut().make_big_int_decimal_identifier(&self.vm, &big_int_string, radix);
                                    fail_if_false!(self, property_name.is_some(), "Cannot parse big int property name");
                                }
                                OPENBRACKET => {
                                    self.next(LexerFlagSet::empty());
                                    property_expression = self.parse_assignment_expression(context);
                                    fail_if_false!(self, property_expression.is_some(), "Cannot parse computed property name");
                                    match_or_fail!(self, CLOSEBRACKET, "Expected ']' to end end a computed property name");
                                }
                                _ => {
                                    if self.token.type_ != RESERVED && self.token.type_ != RESERVED_IF_STRICT && (self.token.type_ & KEYWORD_TOKEN_FLAG) == 0 {
                                        if kind == DestructuringKind::DestructureToExpressions {
                                            return None;
                                        }
                                        fail_with_message!(self, "Expected a property name");
                                    }
                                    property_name = self.token.data.ident.clone();
                                }
                            }
                            self.next(LexerFlagSet::empty());
                            if !self.consume(COLON) {
                                if kind == DestructuringKind::DestructureToExpressions {
                                    return None;
                                }
                                let reported_name = property_name.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                                semantic_fail_if_true!(self, token_type == RESERVED, "Cannot use abbreviated destructuring syntax for reserved name '", reported_name, "'");
                                semantic_fail_if_true!(self, token_type == RESERVED_IF_STRICT, "Cannot use abbreviated destructuring syntax for reserved name '", reported_name, "' in strict mode");
                                semantic_fail_if_true!(self, (token_type & KEYWORD_TOKEN_FLAG) != 0, "Cannot use abbreviated destructuring syntax for keyword '", reported_name, "'");
                                fail_with_message!(self, "Expected a ':' prior to a named destructuring property");
                            }
                            inner_pattern = self.parse_binding_or_assignment_element(context, kind, export_type, duplicate_identifier.as_deref_mut(), has_destructuring_pattern.as_deref_mut(), binding_context, depth + 1);
                        }
                        if kind == DestructuringKind::DestructureToExpressions && inner_pattern.is_none() {
                            return None;
                        }
                        fail_if_false!(self, inner_pattern.is_some(), "Cannot parse this destructuring pattern");
                        let default_value = self.parse_default_value_for_destructuring_pattern(context);
                        propagate_error!(self);
                        if let Some(property_expression) = property_expression {
                            context.append_object_pattern_computed_entry(&self.vm, &object_pattern, &location, property_expression, inner_pattern.unwrap_or_default(), default_value.unwrap_or_default());
                            context.set_contains_computed_property(&object_pattern, true);
                        } else {
                            debug_assert!(property_name.is_some());
                            let entry_name = property_name.unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                            context.append_object_pattern_entry(&object_pattern, &location, was_string, &entry_name, inner_pattern.unwrap_or_default(), default_value.unwrap_or_default());
                        }
                        if !self.consume(COMMA) {
                            break;
                        }
                    }

                    if kind == DestructuringKind::DestructureToExpressions && !self.match_(CLOSEBRACE) {
                        return None;
                    }
                    consume_or_fail!(self, CLOSEBRACE, if rest_element_was_found { "Expected a closing '}' following a rest element destructuring pattern" } else { "Expected either a closing '}' or an ',' after a property destructuring pattern" });
                    context.finish_object_pattern(&object_pattern, divot_start, divot_start, self.last_token_end_position());
                    pattern = Some(object_pattern.into());
                }

                _ => {
                    if !self.match_spec_identifier() {
                        if kind == DestructuringKind::DestructureToExpressions {
                            return None;
                        }
                        semantic_failure_due_to_keyword!(self, destructuring_kind_to_variable_kind_name(kind));
                        fail_if_true!(self, kind != DestructuringKind::DestructureToParameters && self.match_(PRIVATENAME), "Cannot use a private name as a ", destructuring_kind_to_variable_kind_name(kind));
                        fail_with_message!(self, "Expected a parameter pattern or a ')' in parameter list");
                    }
                    fail_if_true!(self, self.match_(LET) && (kind == DestructuringKind::DestructureToLet || kind == DestructuringKind::DestructureToConst), "Cannot use 'let' as an identifier name for a LexicalDeclaration");
                    semantic_fail_if_true!(self, self.is_disallowed_identifier_await(&self.token), "Cannot use 'await' as a ", destructuring_kind_to_variable_kind_name(kind), " ", self.disallowed_identifier_await_reason());
                    let name = self.token.data.ident.clone().unwrap_or_else(|| self.vm.property_names.null_identifier.clone());
                    let token = self.token.clone();
                    pattern = self.create_binding_pattern(context, kind, export_type, &name, &token, binding_context, duplicate_identifier.as_deref_mut());
                    self.next(LexerFlagSet::empty());
                }
            }
            pattern
        })();
        self.parser_state.non_lhs_count = old_non_lhs_count;
        result
    }
}
