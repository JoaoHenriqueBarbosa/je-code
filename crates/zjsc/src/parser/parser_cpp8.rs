// Oitava fatia de `parser/Parser.cpp` (linhas 5217 a 5978, o fim do arquivo), incluída por `include!`
// em `parser.rs`. Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Vai de `parsePrimaryExpression` até `printUnexpectedTokenText`, mais as duas instanciações
// explícitas de template do fim (`template class Parser<Lexer<Latin1Character>>` e
// `template class Parser<Lexer<char16_t>>`), que em Rust não precisam existir: cada uso de
// `Parser<T>` com `T: CharType` já as instancia.
//
// Convenções desta fatia (as de `parser_cpp1.rs`, `parser_cpp5.rs` e `parser_cpp7.rs` valem):
//
// - `SetForScope nonLHSCountScope(m_parserState.nonLHSCount)` vira salvar o valor antigo e restaurá-lo
//   em cada saída (as macros de erro recebem o restauro em `@hook`). `AllowInOverride` e
//   `UnaryExprContext` de `parseUnaryExpression` são destruídos na ordem inversa de construção, também
//   por `@hook` (`AllowInOverride::restore` e `end_unary_expr_context`).
// - `goto identifierExpression` de `parsePrimaryExpression` vira cair para o trecho depois do `match`:
//   todo ramo que não é o `goto` devolve valor antes de chegar lá.
// - `goto endOfChain` de `parseMemberExpression` é o `break` do laço rotulado.
// - `std::optional<CallOrApplyDepthScope>` vira um booleano (empilhou ou não) e a pilha
//   `call_or_apply_depth_scopes` do parser; o destrutor é `pop_call_or_apply_depth_scope`.
// - `IgnoredPositions` (só para o `SyntaxChecker`) não existe: um `Vec<JSTextPosition>` serve aos dois
//   construtores, e o `SyntaxChecker` ignora as posições como no C++.
// - `ASSERT_UNUSED(oldTokenStackDepth...)` e o `ASSERT` de `oldTokenStackDepth + tokenStackDepth` de
//   `parseUnaryExpression` (só de depuração) somem.
// - A conversão implícita de `TemplateLiteral` e de `ClassExpression` para `Expression` vira
//   `template_literal_as_expression` e `class_expression_as_expression` no `TreeBuilder` (mesmo
//   padrão de `comma_as_expression`).
// - O `static_cast<DotAccessorNode*>` de `recordCallOrApplyDepth` vira `dot_accessor_identifier` no
//   `TreeBuilder`: `Some(identifier)` quando a expressão é um `DotAccessorNode`.
// - `RELEASE_ASSERT_NOT_REACHED()` e `CRASH()` viram `unreachable!`.

/// `handleProductionOrFail(token, tokenString, operation, production)` com o gancho dos destrutores
/// vivos.
macro_rules! handle_production_or_fail_hooked {
    ($p:expr, @hook $h:block, $token:expr, $token_string:expr, $operation:expr, $production:expr) => {
        consume_or_fail!($p, @hook $h, $token, "Expected '", $token_string, "' to ", $operation, " a ", $production);
    };
}

/// `handleProductionOrFail2(token, tokenString, operation, production)` com o gancho dos destrutores
/// vivos.
macro_rules! handle_production_or_fail2_hooked {
    ($p:expr, @hook $h:block, $token:expr, $token_string:expr, $operation:expr, $production:expr) => {
        consume_or_fail!($p, @hook $h, $token, "Expected '", $token_string, "' to ", $operation, " an ", $production);
    };
}

/// `static const char* operatorString(bool prefix, unsigned tok)`.
fn operator_string(prefix: bool, tok: u32) -> &'static str {
    match tok {
        MINUSMINUS | AUTOMINUSMINUS => {
            if prefix {
                "prefix-decrement"
            } else {
                "decrement"
            }
        }
        PLUSPLUS | AUTOPLUSPLUS => {
            if prefix {
                "prefix-increment"
            } else {
                "increment"
            }
        }
        EXCLAMATION => "logical-not",
        TILDE => "bitwise-not",
        TYPEOF => "typeof",
        VOIDTOKEN => "void",
        DELETETOKEN => "delete",
        _ => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
    }
}

impl<T: CharType> Parser<T> {
    /// `template <class TreeBuilder> TreeExpression parsePrimaryExpression(TreeBuilder&)`.
    pub(crate) fn parse_primary_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        fail_if_stack_overflow!(self);
        match self.token.type_ {
            FUNCTION => return self.parse_function_expression(context),
            CLASSTOKEN => {
                let class_expression = self.parse_class_expression(context);
                return class_expression.map(|class_expression| class_expression.into());
            }
            OPENBRACE => return self.parse_object_literal(context),
            OPENBRACKET => return self.parse_array_literal(context),
            OPENPAREN => {
                self.next(LexerFlagSet::empty());
                // SetForScope nonLHSCountScope: restaurado em cada saída.
                let old_non_lhs_count = self.parser_state.non_lhs_count;
                let result = self.parse_expression(context);
                handle_production_or_fail_hooked!(self, @hook { self.parser_state.non_lhs_count = old_non_lhs_count; }, CLOSEPAREN, ")", "end", "compound expression");
                self.parser_state.non_lhs_count = old_non_lhs_count;
                return result;
            }
            THISTOKEN => {
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                let current = self.current_scope();
                if self.scope_stack[current].is_arrow_function() {
                    self.scope_stack[current].set_inner_arrow_function_uses_this();
                }
                return Some(context.create_this_expr(&location));
            }
            AWAIT => {
                let current = self.current_scope();
                semantic_fail_if_true!(self, self.scope_stack[current].is_static_block(), "The 'await' keyword is disallowed in the IdentifierReference position within static block");
                if self.parser_state.function_parse_phase == FunctionParsePhase::Parameters {
                    semantic_fail_if_false!(self, self.parser_state.allow_await, "Cannot use 'await' within a parameter default expression");
                } else if !self.parser_state.class_field_init_masks_async && (self.scope_stack[self.current_function_scope()].is_async_function_boundary() || is_module_parse_mode(self.source_parse_mode())) {
                    return self.parse_await_expression(context);
                }

                // goto identifierExpression
            }
            IDENT => {
                let current = self.current_scope();
                semantic_fail_if_true!(self, self.scope_stack[current].is_static_block() && self.is_arguments_identifier(), "Cannot use 'arguments' as an identifier in static block");
                if self.token.data.ident.as_ref() == Some(&self.vm.property_names.r#async) && !self.token.data.escaped {
                    let function_start = *self.token_start_position();
                    let ident = self.token.data.ident.clone().unwrap_or_default();
                    let location = self.token_location();
                    self.next(LexerFlagSet::empty());
                    if self.match_(FUNCTION) && !self.lexer.has_line_terminator_before_token() {
                        return self.parse_async_function_expression(context, &location);
                    }

                    // Avoid using variable if it is an arrow function parameter
                    if self.match_(ARROWFUNCTION) {
                        return None;
                    }

                    let is_eval = false;
                    return self.create_resolve_and_use_variable(context, &ident, is_eval, &function_start, &location);
                }
                if self.parser_state.is_parsing_class_field_initializer {
                    fail_if_true!(self, self.is_arguments_identifier(), "Cannot reference 'arguments' in class field initializer");
                }
                // identifierExpression:
            }
            BIGINT => {
                let ident = self.token.data.big_int_string.clone().unwrap_or_default();
                let radix = self.token.data.radix;
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                return Some(context.create_big_int(&location, &ident, radix));
            }
            STRING => {
                let ident = self.token.data.ident.clone().unwrap_or_default();
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                return Some(context.create_string(&location, &ident));
            }
            DOUBLE => {
                let d = self.token.data.double_value;
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                return Some(context.create_double_expr(&location, d));
            }
            INTEGER => {
                let d = self.token.data.double_value;
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                return Some(context.create_integer_expr(&location, d));
            }
            NULLTOKEN => {
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                return Some(context.create_null(&location));
            }
            TRUETOKEN => {
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                return Some(context.create_boolean(&location, true));
            }
            FALSETOKEN => {
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                return Some(context.create_boolean(&location, false));
            }
            DIVEQUAL | DIVIDE => {
                /* regexp */
                if self.match_(DIVEQUAL) {
                    self.token.type_ = self.lexer.scan_reg_exp(&mut self.token, u16::from(b'='));
                } else {
                    self.token.type_ = self.lexer.scan_reg_exp(&mut self.token, 0);
                }
                match_or_fail!(self, REGEXP, "Invalid regular expression");

                let pattern = self.token.data.pattern.clone().unwrap_or_default();
                let flags = self.token.data.flags.clone().unwrap_or_default();
                let start = *self.token_start_position();
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                let re = context.create_reg_exp(&location, &pattern, &flags, start, self.lexer.is_reparsing_function());
                if re == B::Expression::default() {
                    let error_code = crate::yarr::yarr_syntax_checker::check_syntax(pattern.string(), flags.string());
                    regex_fail!(self, &WtfString::from_latin1(crate::yarr::yarr_error_code::error_message(error_code).as_bytes()));
                }
                return Some(re);
            }
            BACKQUOTE => {
                let template_literal = self.parse_template_literal(context, RawStringsBuildMode::DontBuildRawStrings);
                return template_literal.map(|template_literal| template_literal.into());
            }
            YIELD => {
                if self.can_use_identifier_yield() {
                    // goto identifierExpression
                } else {
                    fail_due_to_unexpected_token!(self);
                }
            }
            LET => {
                if !self.strict_mode() {
                    // goto identifierExpression
                } else {
                    fail_due_to_unexpected_token!(self);
                }
            }
            ESCAPED_KEYWORD => {
                if self.match_allowed_escaped_contextual_keyword() {
                    // goto identifierExpression
                } else {
                    // [[fallthrough]] para o `default`
                    fail_due_to_unexpected_token!(self);
                }
            }
            _ => {
                fail_due_to_unexpected_token!(self);
            }
        }

        // identifierExpression:
        let start = *self.token_start_position();
        let ident = self.token.data.ident.clone().unwrap_or_default();
        if ident == self.vm.property_names.arguments {
            fail_if_true!(self, self.scope_stack[self.closest_scope_owning_arguments()].eval_context_type() == EvalContextType::InstanceFieldEvalContext, "arguments is not valid in this context");
        }
        let location = self.token_location();
        self.next(LexerFlagSet::empty());

        // Avoid using variable if it is an arrow function parameter
        if self.match_(ARROWFUNCTION) {
            return None;
        }

        let is_eval = ident == self.vm.property_names.eval;
        self.create_resolve_and_use_variable(context, &ident, is_eval, &start, &location)
    }

    /// `template <class TreeBuilder> TreeArguments parseArguments(TreeBuilder&)`.
    pub(crate) fn parse_arguments<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Arguments> {
        consume_or_fail_with_flags!(self, OPENPAREN, B::DONT_BUILD_STRINGS, "Expected opening '(' at start of argument list");
        let location = self.token_location();
        if self.match_(CLOSEPAREN) {
            self.next(LexerFlagSet::empty());
            return Some(context.create_arguments());
        }
        let arguments_start = self.token.start_position;
        let arguments_divot = self.token.end_position;

        let initial_assignments = self.parser_state.assignment_count;
        let mut arg_type = ArgumentType::Normal;
        let first_arg = self.parse_argument(context, &mut arg_type);
        fail_if_false!(self, first_arg.is_some(), "Cannot parse function argument");
        let first_arg = first_arg.unwrap_or_default();
        semantic_fail_if_true!(self, self.match_(DOTDOTDOT), "The '...' operator should come before the target expression");

        let mut has_spread = false;
        if arg_type == ArgumentType::Spread {
            has_spread = true;
        }
        let arg_list = context.create_arguments_list(&location, first_arg);
        let mut tail = arg_list.clone();

        while self.match_(COMMA) {
            let argument_location = self.token_location();
            self.next(B::DONT_BUILD_STRINGS);

            if self.match_(CLOSEPAREN) {
                break;
            }

            let arg = self.parse_argument(context, &mut arg_type);
            propagate_error!(self);
            semantic_fail_if_true!(self, self.match_(DOTDOTDOT), "The '...' operator should come before the target expression");

            if arg_type == ArgumentType::Spread {
                has_spread = true;
            }

            tail = context.create_arguments_list_append(&argument_location, tail, arg.unwrap_or_default());
        }

        handle_production_or_fail2!(self, CLOSEPAREN, ")", "end", "argument list");
        if has_spread {
            let element_list = context.create_element_list_from_arguments(arg_list);
            let array = context.create_array_elements(&location, element_list);
            let last_token_end = self.last_token_end_position();
            let spread_array = context.create_spread_expression(&location, array, arguments_start, arguments_divot, last_token_end);
            let spread_list = context.create_arguments_list(&location, spread_array);
            return Some(context.create_arguments_with_list(spread_list, initial_assignments != self.parser_state.assignment_count));
        }

        Some(context.create_arguments_with_list(arg_list, initial_assignments != self.parser_state.assignment_count))
    }

    /// `template <class TreeBuilder> TreeExpression parseArgument(TreeBuilder&, ArgumentType&)`.
    pub(crate) fn parse_argument<B: TreeBuilder>(&mut self, context: &mut B, type_: &mut ArgumentType) -> Option<B::Expression> {
        if self.match_(DOTDOTDOT) {
            let spread_location = self.token_location();
            let start = self.token.start_position;
            let divot = self.token.end_position;
            self.next(LexerFlagSet::empty());
            let spread_expr = self.parse_assignment_expression(context);
            propagate_error!(self);
            let end = self.last_token_end_position();
            *type_ = ArgumentType::Spread;
            return Some(context.create_spread_expression(&spread_location, spread_expr.unwrap_or_default(), start, divot, end));
        }

        *type_ = ArgumentType::Normal;
        self.parse_assignment_expression(context)
    }

    /// `template <typename TreeBuilder, typename ParserType> static inline void recordCallOrApplyDepth(ParserType*, VM&, std::optional<CallOrApplyDepthScope>&, Expression)`.
    /// Devolve se o `CallOrApplyDepthScope` foi criado (o `std::optional` com valor); só o `ASTBuilder`
    /// o cria.
    fn record_call_or_apply_depth<B: TreeBuilder>(&mut self, context: &B, expression: &B::Expression) -> bool {
        if B::CREATES_AST {
            if let Some(identifier) = context.dot_accessor_identifier(expression) {
                let is_call_or_apply = {
                    let builtin_names = self.vm.property_names.builtin_names();
                    identifier == *builtin_names.call_public_name() || identifier == *builtin_names.apply_public_name()
                };
                if is_call_or_apply {
                    self.push_call_or_apply_depth_scope();
                    return true;
                }
            }
        }
        false
    }

    /// `template <class TreeBuilder> TreeExpression parseMemberExpression(TreeBuilder&)`.
    pub(crate) fn parse_member_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        let mut base: B::Expression = Default::default();
        let expression_start = *self.token_start_position();
        let location = self.token_location();

        // No need to accumulate newTokenStartPositions if the builder is SyntaxChecker: o
        // `SyntaxChecker` ignora as posições, então o mesmo `Vec` serve aos dois construtores.
        let mut new_token_start_positions: Vec<JSTextPosition> = Vec::new();
        while self.match_(NEW) {
            new_token_start_positions.push(*self.token_start_position());
            self.next(LexerFlagSet::empty());
        }
        let mut new_count = new_token_start_positions.len();

        let mut base_is_super = self.match_(SUPER);
        let mut previous_base_was_super = false;
        let base_is_import = self.match_(IMPORT);
        let mut base_is_async_keyword = false;

        if new_count != 0 && self.consume(DOT) {
            if self.match_contextual_keyword(&self.vm.property_names.target) {
                let closest_ordinary_function_scope = self.closest_parent_ordinary_function_non_lexical_scope();
                let is_class_field_initializer = self.parser_state.is_parsing_class_field_initializer;
                let is_function_eval_context_type = self.is_inside_ordinary_function
                    && (self.scope_stack[closest_ordinary_function_scope].eval_context_type() == EvalContextType::FunctionEvalContext
                        || self.scope_stack[closest_ordinary_function_scope].eval_context_type() == EvalContextType::InstanceFieldEvalContext);
                let current = self.current_scope();
                semantic_fail_if_false!(self, self.scope_stack[current].is_function() || self.scope_stack[current].is_static_block() || is_function_eval_context_type || is_class_field_initializer, "new.target is only valid inside functions or static blocks");
                if self.scope_stack[current].is_arrow_function() {
                    semantic_fail_if_false!(self, !self.scope_stack[closest_ordinary_function_scope].is_global_code() || is_function_eval_context_type || is_class_field_initializer, "new.target is not valid inside arrow functions in global code");
                    self.scope_stack[current].set_inner_arrow_function_uses_new_target();
                }
                base = context.create_new_target_expr(&location);
                new_count -= 1;
                self.next(LexerFlagSet::empty());
            } else {
                fail_if_true!(self, self.match_(IDENT), "\"new.\" can only be followed with target");
                fail_due_to_unexpected_token!(self);
            }
        } else if base_is_super {
            let closest_ordinary_function_scope = self.closest_parent_ordinary_function_non_lexical_scope();
            let class_scope = self.closest_class_scope_or_top_level_scope();
            // Check if classScope is deeper than closestOrdinaryFunctionScope (i.e., we're in a class field initializer).
            let mut is_class_field_initializer = false;
            let mut scope = self.scope_stack[class_scope].containing_scope();
            while let Some(containing) = scope {
                if containing == closest_ordinary_function_scope {
                    is_class_field_initializer = true;
                    break;
                }
                scope = self.scope_stack[containing].containing_scope();
            }
            let current = self.current_scope();
            semantic_fail_if_false!(self, self.scope_stack[current].is_function() || is_class_field_initializer || (self.scope_stack[closest_ordinary_function_scope].is_eval_context() && self.scope_stack[closest_ordinary_function_scope].expected_super_binding() == SuperBinding::Needed), "super is not valid in this context");
            base = context.create_super_expr(&location);
            self.next(LexerFlagSet::empty());
            fail_if_true!(self, self.match_(OPENPAREN) && self.scope_stack[current].eval_context_type() == EvalContextType::InstanceFieldEvalContext, "super call is not valid in this context");
            let function_scope = self.current_function_scope();
            self.scope_stack[function_scope].set_needs_super_binding();
            // It unnecessary to check of using super during reparsing one more time. Also it can lead to syntax error
            // in case of arrow function because during reparsing we don't know whether we currently parse the arrow function
            // inside of the constructor or method.
            if !self.lexer.is_reparsing_function() {
                let function_super_binding = if !self.scope_stack[function_scope].is_arrow_function() && !self.scope_stack[closest_ordinary_function_scope].is_eval_context() {
                    self.scope_stack[function_scope].expected_super_binding()
                } else {
                    self.scope_stack[closest_ordinary_function_scope].expected_super_binding()
                };
                semantic_fail_if_true!(self, function_super_binding == SuperBinding::NotNeeded && !is_class_field_initializer, "super is not valid in this context");
            }
        } else if base_is_import {
            self.next(LexerFlagSet::empty());
            let mut expression_end = self.last_token_end_position();
            let mut is_import_meta = false;
            let mut deferred = false;
            if self.consume(DOT) {
                if self.match_contextual_keyword(self.vm.property_names.builtin_names().meta_public_name()) {
                    semantic_fail_if_false!(self, self.script_mode == JSParserScriptMode::Module, "import.meta is only valid inside modules");
                    let meta_private_name = self.vm.property_names.meta_private_name.clone();
                    let resolve = self.create_resolve_and_use_variable(context, &meta_private_name, false, &expression_start, &location);
                    base = context.create_import_meta_expr(&location, resolve.unwrap_or_default());
                    let current = self.current_scope();
                    self.scope_stack[current].set_uses_import_meta();
                    is_import_meta = true;
                    self.next(LexerFlagSet::empty());
                } else if Options::use_import_defer() && self.match_contextual_keyword(&self.vm.property_names.defer_keyword) {
                    // ImportCall : import . defer ImportCallArguments
                    // https://tc39.es/proposal-defer-import-eval/#sec-import-call-runtime-semantics-evaluation
                    deferred = true;
                    self.next(LexerFlagSet::empty());
                    expression_end = self.last_token_end_position();
                } else {
                    fail_if_true!(self, self.match_(IDENT), (if Options::use_import_defer() { "\"import.\" can only be followed with meta or defer" } else { "\"import.\" can only be followed with meta" }));
                    fail_due_to_unexpected_token!(self);
                }
            }
            if !is_import_meta {
                semantic_fail_if_true!(self, new_count != 0, "Cannot use new with import");
                consume_or_fail!(self, OPENPAREN, "import call expects one or two arguments");
                // SetForScope nonLHSCountScope: restaurado em cada saída.
                let old_non_lhs_count = self.parser_state.non_lhs_count;
                let expr = self.parse_assignment_expression(context);
                fail_if_false!(self, @hook { self.parser_state.non_lhs_count = old_non_lhs_count; }, expr.is_some(), "Cannot parse expression");
                let expr = expr.unwrap_or_default();
                let mut option_expression: B::Expression = Default::default();
                if self.consume(COMMA) {
                    if !self.match_(CLOSEPAREN) {
                        let parsed_option = self.parse_assignment_expression(context);
                        fail_if_false!(self, @hook { self.parser_state.non_lhs_count = old_non_lhs_count; }, parsed_option.is_some(), "Cannot parse expression");
                        option_expression = parsed_option.unwrap_or_default();
                        self.consume(COMMA);
                    }
                }
                consume_or_fail!(self, @hook { self.parser_state.non_lhs_count = old_non_lhs_count; }, CLOSEPAREN, "import call expects one or two arguments");
                let last_token_end = self.last_token_end_position();
                base = context.create_import_expr(&location, expr, option_expression, deferred, expression_start, expression_end, last_token_end);
                self.parser_state.non_lhs_count = old_non_lhs_count;
            }
        } else {
            let is_async = self.match_contextual_keyword(&self.vm.property_names.r#async);

            let arguments_dot_length_expression = self.try_parse_arguments_dot_length_for_fast_path(context).filter(|expression| *expression != B::Expression::default());
            let parsed_base = match arguments_dot_length_expression {
                Some(arguments_dot_length_expression) => Some(arguments_dot_length_expression),
                None => self.parse_primary_expression(context),
            };
            fail_if_false!(self, parsed_base.is_some(), "Cannot parse base expression");
            base = parsed_base.unwrap_or_default();
            if is_async && context.is_resolve(&base) && !self.lexer.has_line_terminator_before_token() {
                base_is_async_keyword = true;
                if self.match_spec_identifier() {
                    fail_due_to_unexpected_token!(self);
                }
            }
        }

        fail_if_false!(self, base != B::Expression::default(), "Cannot parse base expression");

        loop {
            let mut optional_chain_base: B::Expression = Default::default();
            let mut optional_chain_location = JSTokenLocation::default();
            let mut is_optional_call = false;
            let mut type_ = self.token.type_;

            if self.match_(QUESTIONDOT) {
                semantic_fail_if_true!(self, new_count != 0, "Cannot call constructor in an optional chain");
                semantic_fail_if_true!(self, base_is_super, "Cannot use super as the base of an optional chain");
                optional_chain_base = base.clone();
                optional_chain_location = self.token_location();

                let save_point = self.create_save_point(context);
                self.next(LexerFlagSet::empty());
                if self.match_(OPENBRACKET) || self.match_(OPENPAREN) || self.match_(BACKQUOTE) {
                    type_ = self.token.type_;
                } else {
                    type_ = DOT;
                    self.restore_save_point(context, &save_point);
                }
            }

            'end_of_chain: loop {
                match type_ {
                    OPENBRACKET => {
                        self.parser_state.non_trivial_expression_count += 1;
                        let expression_divot = *self.token_start_position();
                        self.next(LexerFlagSet::empty());
                        // SetForScope nonLHSCountScope: restaurado em cada saída.
                        let old_non_lhs_count = self.parser_state.non_lhs_count;
                        let initial_assignments = self.parser_state.assignment_count;
                        let property = self.parse_expression(context);
                        fail_if_false!(self, @hook { self.parser_state.non_lhs_count = old_non_lhs_count; }, property.is_some(), "Cannot parse subscript expression");
                        let token_end = *self.token_end_position();
                        base = context.create_bracket_access(&location, base, property.unwrap_or_default(), initial_assignments != self.parser_state.assignment_count, expression_start, expression_divot, token_end);

                        if base_is_super && self.scope_stack[self.current_scope()].is_arrow_function() {
                            let function_scope = self.current_function_scope();
                            self.scope_stack[function_scope].set_inner_arrow_function_uses_super_property();
                        }

                        handle_production_or_fail_hooked!(self, @hook { self.parser_state.non_lhs_count = old_non_lhs_count; }, CLOSEBRACKET, "]", "end", "subscript expression");
                        self.parser_state.non_lhs_count = old_non_lhs_count;
                    }
                    OPENPAREN => {
                        if base_is_super {
                            fail_if_true!(self, self.parser_state.is_parsing_class_field_initializer, "super call is not valid in class field initializer context");
                        }
                        self.parser_state.non_trivial_expression_count += 1;
                        // SetForScope nonLHSCountScope: restaurado em cada saída.
                        let old_non_lhs_count = self.parser_state.non_lhs_count;
                        if new_count != 0 {
                            new_count -= 1;
                            semantic_fail_if_true_hooked!(self, @hook { self.parser_state.non_lhs_count = old_non_lhs_count; }, base_is_super, "Cannot use new with super call");
                            let expression_end = self.last_token_end_position();
                            let arguments = self.parse_arguments(context);
                            fail_if_false!(self, @hook { self.parser_state.non_lhs_count = old_non_lhs_count; }, arguments.is_some(), "Cannot parse call arguments");
                            let last_token_end = self.last_token_end_position();
                            base = context.create_new_expr(&location, base, arguments.unwrap_or_default(), expression_start, expression_end, last_token_end);
                        } else {
                            let current = self.current_scope();
                            let used_variables_size = self.scope_stack[current].current_used_variables_size();
                            let expression_end = self.last_token_end_position();
                            // std::optional<CallOrApplyDepthScope> callOrApplyDepthScope: destruído em cada saída.
                            let call_or_apply_depth_scope_pushed = self.record_call_or_apply_depth(context, &base);

                            let arguments = self.parse_arguments(context);

                            if base_is_async_keyword && (arguments.is_none() || self.match_(ARROWFUNCTION)) {
                                self.scope_stack[current].revert_to_previous_used_variables(used_variables_size);
                                fail_due_to_unexpected_token!(self, @hook {
                                    if call_or_apply_depth_scope_pushed {
                                        self.pop_call_or_apply_depth_scope();
                                    }
                                    self.parser_state.non_lhs_count = old_non_lhs_count;
                                });
                            }

                            fail_if_false!(self, @hook {
                                if call_or_apply_depth_scope_pushed {
                                    self.pop_call_or_apply_depth_scope();
                                }
                                self.parser_state.non_lhs_count = old_non_lhs_count;
                            }, arguments.is_some(), "Cannot parse call arguments");
                            if base_is_super {
                                let function_scope = self.current_function_scope();
                                self.scope_stack[function_scope].set_has_direct_super();
                                // It unnecessary to check of using super during reparsing one more time. Also it can lead to syntax error
                                // in case of arrow function because during reparsing we don't know whether we currently parse the arrow function
                                // inside of the constructor or method.
                                if !self.lexer.is_reparsing_function() {
                                    let closest_ordinary_function_scope = self.closest_parent_ordinary_function_non_lexical_scope();
                                    semantic_fail_if_false_hooked!(self, @hook {
                                        if call_or_apply_depth_scope_pushed {
                                            self.pop_call_or_apply_depth_scope();
                                        }
                                        self.parser_state.non_lhs_count = old_non_lhs_count;
                                    }, self.scope_stack[closest_ordinary_function_scope].constructor_kind() == ConstructorKind::Extends || (self.scope_stack[closest_ordinary_function_scope].is_eval_context() && self.scope_stack[closest_ordinary_function_scope].derived_context_type() == DerivedContextType::DerivedConstructorContext), "super is not valid in this context");
                                }
                                if self.scope_stack[current].is_arrow_function() {
                                    self.scope_stack[function_scope].set_inner_arrow_function_uses_super_call();
                                }
                            }

                            is_optional_call = optional_chain_location.end_offset == expression_end.offset as u32;
                            let call_or_apply_child_depth = if call_or_apply_depth_scope_pushed {
                                self.current_call_or_apply_depth_scope().map_or(0, |scope| scope.distance_to_innermost_child())
                            } else {
                                0
                            };
                            let last_token_end = self.last_token_end_position();
                            base = context.make_function_call_node(&location, base, previous_base_was_super, arguments.unwrap_or_default(), expression_start, expression_end, last_token_end, call_or_apply_child_depth, is_optional_call);
                            if call_or_apply_depth_scope_pushed {
                                self.pop_call_or_apply_depth_scope();
                            }
                        }
                        self.parser_state.non_lhs_count = old_non_lhs_count;
                    }
                    DOT => {
                        self.parser_state.non_trivial_expression_count += 1;
                        let expression_divot = *self.token_start_position();
                        let mut flags = B::DONT_BUILD_KEYWORDS;
                        flags.add(LexerFlags::IgnoreReservedWords);
                        self.next(flags);
                        let ident = self.token.data.ident.clone().unwrap_or_default();
                        let mut dot_type = DotType::Name;
                        if self.match_(PRIVATENAME) {
                            debug_assert!(self.token.data.ident.is_some());
                            fail_if_true!(self, base_is_super, "Cannot access private names from super");
                            let current = self.current_scope();
                            if self.scope_stack[current].eval_context_type() == EvalContextType::InstanceFieldEvalContext {
                                semantic_fail_if_false!(self, self.scope_stack[current].has_private_name(&ident), "Cannot reference undeclared private field '", ident, "'");
                            }
                            self.scope_stack[current].use_private_name(&ident);
                            self.seen_private_name_use_in_non_reparsing_function_mode = true;
                            self.parser_state.last_private_name = Some(ident.clone());
                            dot_type = DotType::PrivateMember;
                            self.token.type_ = IDENT;
                        }
                        match_or_fail!(self, IDENT, "Expected a property name after ", (if optional_chain_base != B::Expression::default() { "'?.'" } else { "'.'" }));
                        let token_end = *self.token_end_position();
                        base = context.create_dot_access(&location, base, &ident, dot_type, expression_start, expression_divot, token_end);
                        if base_is_super && self.scope_stack[self.current_scope()].is_arrow_function() {
                            let function_scope = self.current_function_scope();
                            self.scope_stack[function_scope].set_inner_arrow_function_uses_super_property();
                        }
                        self.next(LexerFlagSet::empty());
                    }
                    BACKQUOTE => {
                        semantic_fail_if_true!(self, optional_chain_base != B::Expression::default(), "Cannot use tagged templates in an optional chain");
                        semantic_fail_if_true!(self, base_is_super, "Cannot use super as tag for tagged templates");
                        let expression_divot = *self.token_start_position();
                        // SetForScope nonLHSCountScope: restaurado em cada saída.
                        let old_non_lhs_count = self.parser_state.non_lhs_count;
                        let template_literal = self.parse_template_literal(context, RawStringsBuildMode::BuildRawStrings);
                        fail_if_false!(self, @hook { self.parser_state.non_lhs_count = old_non_lhs_count; }, template_literal.is_some(), "Cannot parse template literal");
                        let last_token_end = self.last_token_end_position();
                        base = context.create_tagged_template(&location, base, template_literal.unwrap_or_default(), expression_start, expression_divot, last_token_end);
                        self.seen_tagged_template_in_non_reparsing_function_mode = true;
                        self.parser_state.non_lhs_count = old_non_lhs_count;
                    }
                    _ => break 'end_of_chain,
                }
                previous_base_was_super = base_is_super;
                base_is_super = false;
                type_ = self.token.type_;
            }
            // endOfChain:
            if optional_chain_base != B::Expression::default() {
                let is_outermost = !self.match_(QUESTIONDOT);
                base = context.create_optional_chain(&location, if is_optional_call { Default::default() } else { optional_chain_base }, base, is_outermost);
            }
            if !self.match_(QUESTIONDOT) {
                break;
            }
        }

        semantic_fail_if_true!(self, base_is_super, (if new_count != 0 { "Cannot use new with super call" } else { "super is not valid in this context" }));
        while new_count != 0 {
            new_count -= 1;
            let last_token_end = self.last_token_end_position();
            base = context.create_new_expr_no_arguments(&location, base, expression_start, new_token_start_positions[new_count], last_token_end);
        }
        Some(base)
    }

    /// `template <class TreeBuilder> TreeExpression parseArrowFunctionExpression(TreeBuilder&, bool isAsync, const JSTokenLocation&)`.
    pub(crate) fn parse_arrow_function_expression<B: TreeBuilder>(&mut self, context: &mut B, is_async: bool, location: &JSTokenLocation) -> Option<B::Expression> {
        let mut info = ParserFunctionInfo::<B>::default();
        info.name = Some(self.vm.property_names.null_identifier.clone());

        // SetForScope innerParseMode: restaurado em cada saída.
        let old_parse_mode = self.parse_mode;
        self.parse_mode = if is_async { SourceParseMode::AsyncArrowFunctionMode } else { SourceParseMode::ArrowFunctionMode };
        fail_if_false!(self, @hook { self.parse_mode = old_parse_mode; }, self.parse_function_info(context, FunctionNameRequirements::Unnamed, true, ConstructorKind::None, SuperBinding::NotNeeded, location.start_offset, &mut info, FunctionDefinitionType::Expression, None), "Cannot parse arrow function expression");
        self.parse_mode = old_parse_mode;

        Some(context.create_arrow_function_expr(location, &info))
    }

    /// `template <class TreeBuilder> TreeExpression parseUnaryExpression(TreeBuilder&)`.
    pub(crate) fn parse_unary_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        let unary_expr_context = context.begin_unary_expr_context();
        let allow_in_override = AllowInOverride::new(self);
        // Destrutores, na ordem inversa de construção: `AllowInOverride`, depois `UnaryExprContext`.
        macro_rules! unary_exit {
            ($p:expr) => {{
                allow_in_override.restore($p);
                context.end_unary_expr_context(unary_expr_context);
            }};
        }
        let mut token_stack_depth: i32 = 0;
        let mut has_prefix_update_op = false;
        let mut last_operator: u32 = 0;

        if self.match_(AWAIT) && !self.parser_state.class_field_init_masks_async && (self.scope_stack[self.current_function_scope()].is_async_function_boundary() || is_module_parse_mode(self.source_parse_mode())) {
            semantic_fail_if_true_hooked!(self, @hook { unary_exit!(self); }, self.scope_stack[self.current_scope()].is_static_block(), "Cannot use 'await' within static block");
            let result = self.parse_await_expression(context);
            unary_exit!(self);
            return result;
        }

        let location = self.token_location();

        while is_unary_op(self.token.type_) {
            semantic_fail_if_true_hooked!(self, @hook { unary_exit!(self); }, has_prefix_update_op, "The ", operator_string(true, last_operator), " operator requires a reference expression");
            if is_update_op(self.token.type_) {
                has_prefix_update_op = true;
            }
            last_operator = self.token.type_;
            self.parser_state.non_lhs_count += 1;
            let token_start = *self.token_start_position();
            context.append_unary_token(&mut token_stack_depth, self.token.type_ as i32, token_start);
            self.next(LexerFlagSet::empty());
            self.parser_state.non_trivial_expression_count += 1;
        }
        let mut sub_expr_start = *self.token_start_position();
        debug_assert!(sub_expr_start.offset >= sub_expr_start.line_start_offset);
        let parsed_expr = self.parse_member_expression(context);
        if parsed_expr.is_none() {
            fail_if_true!(self, @hook { unary_exit!(self); }, last_operator != 0, "Cannot parse subexpression of ", operator_string(true, last_operator), "operator");
            fail_with_message!(self, @hook { unary_exit!(self); }, "Cannot parse member expression");
        }
        let mut expr = parsed_expr.unwrap_or_default();
        // `m_parserState.lastIdentifier` não muda até o fim da função (só `parseMemberExpression` o escreve).
        let last_identifier = self.parser_state.last_identifier.clone().unwrap_or_default();
        if is_update_op(last_operator) {
            semantic_fail_if_true_hooked!(self, @hook { unary_exit!(self); }, context.is_meta_property(&expr), self.meta_property_name(context, &expr), " can't come after a prefix operator");
            semantic_fail_if_false_hooked!(self, @hook { unary_exit!(self); }, self.is_simple_assignment_target(context, &expr, false), "Prefix ", (if last_operator == PLUSPLUS || last_operator == AUTOPLUSPLUS { "++" } else { "--" }), " operator applied to value that is not a reference");
        }
        let mut is_eval_or_arguments = false;
        if self.strict_mode() {
            if context.is_resolve(&expr) {
                is_eval_or_arguments = self.vm.property_names.eval == last_identifier || self.vm.property_names.arguments == last_identifier;
            }
        }
        fail_if_true_if_strict_hooked!(self, @hook { unary_exit!(self); }, is_eval_or_arguments && has_prefix_update_op, "Cannot modify '", last_identifier, "' in strict mode");
        match self.token.type_ {
            PLUSPLUS => {
                semantic_fail_if_true_hooked!(self, @hook { unary_exit!(self); }, context.is_meta_property(&expr), self.meta_property_name(context, &expr), " can't come before a postfix operator");
                semantic_fail_if_false_hooked!(self, @hook { unary_exit!(self); }, self.is_simple_assignment_target(context, &expr, false), "Postfix ++ operator applied to value that is not a reference");
                self.parser_state.non_trivial_expression_count += 1;
                self.parser_state.non_lhs_count += 1;
                let last_token_end = self.last_token_end_position();
                let token_end = *self.token_end_position();
                expr = context.make_postfix_node(&location, expr, Operator::PlusPlus, sub_expr_start, last_token_end, token_end);
                self.parser_state.assignment_count += 1;
                fail_if_true_if_strict_hooked!(self, @hook { unary_exit!(self); }, is_eval_or_arguments, "Cannot modify '", last_identifier, "' in strict mode");
                semantic_fail_if_true_hooked!(self, @hook { unary_exit!(self); }, has_prefix_update_op, "The ", operator_string(false, last_operator), " operator requires a reference expression");
                self.next(LexerFlagSet::empty());
            }
            MINUSMINUS => {
                semantic_fail_if_true_hooked!(self, @hook { unary_exit!(self); }, context.is_meta_property(&expr), self.meta_property_name(context, &expr), " can't come before a postfix operator");
                semantic_fail_if_false_hooked!(self, @hook { unary_exit!(self); }, self.is_simple_assignment_target(context, &expr, false), "Postfix -- operator applied to value that is not a reference");
                self.parser_state.non_trivial_expression_count += 1;
                self.parser_state.non_lhs_count += 1;
                let last_token_end = self.last_token_end_position();
                let token_end = *self.token_end_position();
                expr = context.make_postfix_node(&location, expr, Operator::MinusMinus, sub_expr_start, last_token_end, token_end);
                self.parser_state.assignment_count += 1;
                fail_if_true_if_strict_hooked!(self, @hook { unary_exit!(self); }, is_eval_or_arguments, "'", last_identifier, "' cannot be modified in strict mode");
                semantic_fail_if_true_hooked!(self, @hook { unary_exit!(self); }, has_prefix_update_op, "The ", operator_string(false, last_operator), " operator requires a reference expression");
                self.next(LexerFlagSet::empty());
            }
            _ => {}
        }

        let end = self.last_token_end_position();
        while token_stack_depth != 0 {
            sub_expr_start = context.unary_token_stack_last_start(&mut token_stack_depth);
            let token_type = context.unary_token_stack_last_type(&mut token_stack_depth) as JSTokenType;
            match token_type {
                EXCLAMATION => {
                    expr = context.create_logical_not(&location, expr);
                }
                TILDE => {
                    expr = context.make_bitwise_not_node(&location, expr);
                }
                MINUS => {
                    expr = context.make_negate_node(&location, expr);
                }
                PLUS => {
                    expr = context.create_unary_plus(&location, expr);
                }
                PLUSPLUS | AUTOPLUSPLUS => {
                    debug_assert!(self.is_simple_assignment_target(context, &expr, false));
                    expr = context.make_prefix_node(&location, expr, Operator::PlusPlus, sub_expr_start, sub_expr_start + 2, end);
                    self.parser_state.assignment_count += 1;
                }
                MINUSMINUS | AUTOMINUSMINUS => {
                    debug_assert!(self.is_simple_assignment_target(context, &expr, false));
                    expr = context.make_prefix_node(&location, expr, Operator::MinusMinus, sub_expr_start, sub_expr_start + 2, end);
                    self.parser_state.assignment_count += 1;
                }
                TYPEOF => {
                    expr = context.make_type_of_node(&location, expr, sub_expr_start, sub_expr_start, end);
                }
                VOIDTOKEN => {
                    expr = context.create_void(&location, expr);
                }
                DELETETOKEN => {
                    fail_if_true_if_strict_hooked!(self, @hook { unary_exit!(self); }, context.is_resolve(&expr), "Cannot delete unqualified property '", last_identifier, "' in strict mode");
                    semantic_fail_if_true_hooked!(self, @hook { unary_exit!(self); }, context.is_private_location(&expr), "Cannot delete private field ", self.parser_state.last_private_name.clone().unwrap_or_default());
                    let delete_start = context.unary_token_stack_last_start(&mut token_stack_depth);
                    expr = context.make_delete_node(&location, expr, delete_start, end, end);
                }
                _ => {
                    // If we get here something has gone horribly horribly wrong
                    // Invariante: o `match` externo cobre todos os operadores unários que o lexer produz (mesmo CRASH do C++).
                    unreachable!("CRASH");
                }
            }
            context.unary_token_stack_remove_last(&mut token_stack_depth);
        }
        unary_exit!(self);
        Some(expr)
    }

    /// `template <typename LexerType> void Parser<LexerType>::printUnexpectedTokenText(WTF::PrintStream&)`.
    pub(crate) fn print_unexpected_token_text(&self, out: &mut StringBuilder) {
        // `out.print(args...)`: cada argumento imprime como em `ParserPrintArg`.
        macro_rules! print_all {
            ($($arg:expr),+ $(,)?) => {
                $( ParserPrintArg::print_arg(&$arg, out); )+
            };
        }
        match self.token.type_ {
            EOFTOK => {
                print_all!("Unexpected end of script");
                return;
            }
            UNTERMINATED_IDENTIFIER_ESCAPE_ERRORTOK | UNTERMINATED_IDENTIFIER_UNICODE_ESCAPE_ERRORTOK => {
                print_all!("Incomplete unicode escape in identifier: '", self.get_token(), "'");
                return;
            }
            UNTERMINATED_MULTILINE_COMMENT_ERRORTOK => {
                print_all!("Unterminated multiline comment");
                return;
            }
            UNTERMINATED_NUMERIC_LITERAL_ERRORTOK => {
                print_all!("Unterminated numeric literal '", self.get_token(), "'");
                return;
            }
            UNTERMINATED_STRING_LITERAL_ERRORTOK => {
                print_all!("Unterminated string literal '", self.get_token(), "'");
                return;
            }
            INVALID_IDENTIFIER_ESCAPE_ERRORTOK => {
                print_all!("Invalid escape in identifier: '", self.get_token(), "'");
                return;
            }
            ESCAPED_KEYWORD => {
                print_all!("Unexpected escaped characters in keyword token: '", self.get_token(), "'");
                return;
            }
            INVALID_IDENTIFIER_UNICODE_ESCAPE_ERRORTOK => {
                print_all!("Invalid unicode escape in identifier: '", self.get_token(), "'");
                return;
            }
            INVALID_NUMERIC_LITERAL_ERRORTOK => {
                print_all!("Invalid numeric literal: '", self.get_token(), "'");
                return;
            }
            UNTERMINATED_OCTAL_NUMBER_ERRORTOK => {
                print_all!("Invalid use of octal: '", self.get_token(), "'");
                return;
            }
            INVALID_STRING_LITERAL_ERRORTOK => {
                print_all!("Invalid string literal: '", self.get_token(), "'");
                return;
            }
            INVALID_UNICODE_ENCODING_ERRORTOK => {
                print_all!("Invalid unicode encoding: '", self.get_token(), "'");
                return;
            }
            INVALID_IDENTIFIER_UNICODE_ERRORTOK => {
                print_all!("Invalid unicode code point in identifier: '", self.get_token(), "'");
                return;
            }
            ERRORTOK => {
                print_all!("Unrecognized token '", self.get_token(), "'");
                return;
            }
            STRING => {
                print_all!("Unexpected string literal ", self.get_token());
                return;
            }
            INTEGER | DOUBLE => {
                print_all!("Unexpected number '", self.get_token(), "'");
                return;
            }

            RESERVED_IF_STRICT => {
                print_all!("Unexpected use of reserved word '", self.get_token(), "' in strict mode");
                return;
            }

            RESERVED => {
                print_all!("Unexpected use of reserved word '", self.get_token(), "'");
                return;
            }

            INVALID_PRIVATE_NAME_ERRORTOK => {
                print_all!("Invalid private name '", self.get_token(), "'");
                return;
            }

            PRIVATENAME => {
                print_all!("Unexpected private name ", self.get_token());
                return;
            }

            AWAIT | IDENT => {
                print_all!("Unexpected identifier '", self.get_token(), "'");
                return;
            }

            _ => {}
        }

        if (self.token.type_ & KEYWORD_TOKEN_FLAG) != 0 {
            print_all!("Unexpected keyword '", self.get_token(), "'");
            return;
        }

        print_all!("Unexpected token '", self.get_token(), "'");
    }
}

// Instantiate the two flavors of Parser we need instead of putting most of this file in Parser.h
// template class Parser<Lexer<Latin1Character>>;
// template class Parser<Lexer<char16_t>>;
// (em Rust, `Parser<T>` com `T: CharType` instancia as duas, `LChar` e `UChar`, no uso)
