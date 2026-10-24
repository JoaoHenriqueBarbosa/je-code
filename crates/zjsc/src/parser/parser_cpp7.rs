// Sétima fatia de `parser/Parser.cpp` (linhas 4521 a 5216), incluída por `include!` em `parser.rs`.
// Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Vai de `parseAwaitExpression` até `tryParseArgumentsDotLengthForFastPath` (fim do trecho):
// `parseAwaitExpression`, `parseConditionalExpression`, `isUnaryOpExcludingUpdateOp`,
// `isBinaryOperator`, `parseBinaryExpression`, `parseProperty`, `parsePropertyMethod`,
// `parseGetterSetter`, `recordPauseLocation`, `recordFunctionEntryLocation`,
// `recordFunctionLeaveLocation`, `parseObjectLiteral`, `parseArrayLiteral`, `parseClassExpression`,
// `parseFunctionExpression`, `parseAsyncFunctionExpression`, `parseTemplateString`,
// `parseTemplateLiteral`, `createResolveAndUseVariable` e `tryParseArgumentsDotLengthForFastPath`.
//
// Convenções desta fatia (as de `parser_cpp1.rs` e `parser_cpp3.rs` valem):
//
// - `SetForScope` (`m_parseMode`, `m_parserState.nonLHSCount`) vira "salvar o valor antigo e
//   restaurá-lo em cada saída", com um `macro_rules!` local passado em `@hook { ... }`.
// - `typename TreeBuilder::BinaryExprContext` é a guarda do `TreeBuilder`: construída com
//   `begin_binary_expr_context` e destruída com `end_binary_expr_context` em cada saída.
// - `goto parseProperty` e `goto namedProperty` de `parseProperty`: o primeiro é `continue` do laço
//   rotulado; o segundo é cair no bloco `namedProperty`, que vem depois da cadeia de `case`.
// - `const Identifier*` de ponta nula vira `Option<&Identifier>` nos parâmetros e `Identifier` quando
//   o token garante o valor (o `ASSERT(ident)` do C++).
// - `recordPauseLocation` e irmãs recebem o `JSTextPosition` por valor (é `Copy`), como as chamadas
//   de `parser_cpp3.rs` e `parser_cpp4.rs` já fazem.
// - `TreeExpression createResolveAndUseVariable` devolve `Option<B::Expression>` (nunca `None`).

/// `ALWAYS_INLINE static bool isUnaryOpExcludingUpdateOp(JSTokenType)`.
#[inline(always)]
fn is_unary_op_excluding_update_op(token: JSTokenType) -> bool {
    if is_update_op(token) {
        return false;
    }
    is_unary_op(token)
}

impl<T: CharType> Parser<T> {
    /// `template <class TreeBuilder> TreeExpression parseAwaitExpression(TreeBuilder&)`.
    pub(crate) fn parse_await_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        debug_assert!(self.match_(AWAIT));
        debug_assert!(!self.parser_state.class_field_init_masks_async);
        let location = self.token_location();
        let divot_start = *self.token_start_position();
        self.next(LexerFlagSet::empty());
        let argument_start = *self.token_start_position();
        let argument = self.parse_unary_expression(context);
        fail_if_false!(self, argument.is_some(), "Failed to parse await expression");
        let function_scope = self.current_function_scope();
        self.scope_stack[function_scope].set_uses_await();
        Some(context.create_await(&location, argument.unwrap_or_default(), divot_start, argument_start, self.last_token_end_position()))
    }

    /// `template <class TreeBuilder> TreeExpression parseConditionalExpression(TreeBuilder&)`.
    pub(crate) fn parse_conditional_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        let location = self.token_location();
        let cond = self.parse_binary_expression(context);
        fail_if_false!(self, cond.is_some(), "Cannot parse expression");
        if !self.match_(QUESTION) {
            return cond;
        }
        self.parser_state.non_trivial_expression_count += 1;
        self.parser_state.non_lhs_count += 1;
        self.next(B::DONT_BUILD_STRINGS);
        let lhs;
        {
            // this block is necessary so that we don't leave `in` enabled for the rhs
            let allow_in_override = AllowInOverride::new(self);
            lhs = self.parse_assignment_expression(context);
            allow_in_override.restore(self);
        }
        fail_if_false!(self, lhs.is_some(), "Cannot parse left hand side of ternary operator");
        let lhs = lhs.unwrap_or_default();
        context.set_end_offset(&lhs, self.last_token_location.end_offset as i32);
        consume_or_fail_with_flags!(self, COLON, B::DONT_BUILD_STRINGS, "Expected ':' in ternary operator");

        let rhs = self.parse_assignment_expression(context);
        fail_if_false!(self, rhs.is_some(), "Cannot parse right hand side of ternary operator");
        let rhs = rhs.unwrap_or_default();
        context.set_end_offset(&rhs, self.last_token_location.end_offset as i32);
        Some(context.create_conditional_expr(&location, cond.unwrap_or_default(), lhs, rhs))
    }

    /// `template <typename LexerType> int Parser<LexerType>::isBinaryOperator(JSTokenType)`.
    pub(crate) fn is_binary_operator(&self, token: JSTokenType) -> i32 {
        if self.allows_in {
            return (token & (BINARY_OP_TOKEN_PRECEDENCE_MASK << BINARY_OP_TOKEN_ALLOWS_IN_PRECEDENCE_ADDITIONAL_SHIFT)) as i32;
        }
        (token & BINARY_OP_TOKEN_PRECEDENCE_MASK) as i32
    }

    /// `template <class TreeBuilder> TreeExpression parseBinaryExpression(TreeBuilder&)`.
    pub(crate) fn parse_binary_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        let mut operand_stack_depth: i32 = 0;
        let mut operator_stack_depth: i32 = 0;
        // `typename TreeBuilder::BinaryExprContext binaryExprContext(context)`: o destrutor roda em
        // cada saída.
        let binary_expr_context = context.begin_binary_expr_context();
        macro_rules! exit_cleanup {
            () => {
                context.end_binary_expr_context(binary_expr_context);
            };
        }
        let location = self.token_location();
        let mut has_logical_operator = false;
        let mut has_coalesce_operator = false;

        let mut previous_operator: i32 = 0;
        loop {
            let expr_start = *self.token_start_position();
            let initial_assignments = self.parser_state.assignment_count;
            let leading_token_type_for_unary_expression = self.token.type_;

            let current: Option<B::Expression>;
            if self.match_(PRIVATENAME) {
                let ident = self.token.data.ident.clone().unwrap_or_else(Identifier::null_identifier);
                let scope = self.current_scope();
                self.scope_stack[scope].use_private_name(&ident);
                self.seen_private_name_use_in_non_reparsing_function_mode = true;
                self.next(LexerFlagSet::empty());
                if self.token.type_ != INTOKEN || previous_operator >= INTOKEN as i32 {
                    internal_fail_with_message!(self, @hook { exit_cleanup!(); }, false, "Bare private name can only be used as the left-hand side of an `in` expression");
                }
                current = Some(context.create_private_identifier_node(&location, &ident));
            } else {
                current = self.parse_unary_expression(context);
            }
            fail_if_false!(self, @hook { exit_cleanup!(); }, current.is_some(), "Cannot parse expression");

            context.append_binary_expression_info(&mut operand_stack_depth, current.unwrap_or_default(), expr_start, self.last_token_end_position(), self.last_token_end_position(), initial_assignments != self.parser_state.assignment_count);
            let precedence = self.is_binary_operator(self.token.type_);
            if precedence == 0 {
                break;
            }

            // 12.6 https://tc39.github.io/ecma262/#sec-exp-operator
            // ExponentiationExpresion is described as follows.
            //
            //     ExponentiationExpression[Yield]:
            //         UnaryExpression[?Yield]
            //         UpdateExpression[?Yield] ** ExponentiationExpression[?Yield]
            //
            // As we can see, the left hand side of the ExponentiationExpression is UpdateExpression, not UnaryExpression.
            // So placing UnaryExpression not included in UpdateExpression here is a syntax error.
            // This is intentional. For example, if UnaryExpression is allowed, we can have the code like `-x**y`.
            // But this is confusing: `-(x**y)` OR `(-x)**y`, which interpretation is correct?
            // To avoid this problem, ECMA262 makes unparenthesized exponentiation expression as operand of unary operators an early error.
            // More rationale: https://mail.mozilla.org/pipermail/es-discuss/2015-September/044232.html
            //
            // Here, we guarantee that the left hand side of this expression is not unary expression by checking the leading operator of the parseUnaryExpression.
            // This check just works. Let's consider the example,
            //     y <> -x ** z
            //          ^
            //          Check this.
            // If the binary operator <> has higher precedence than one of "**", this check does not work.
            // But it's OK for ** because the operator "**" has the highest operator precedence in the binary operators.
            fail_if_true!(self, @hook { exit_cleanup!(); }, self.match_(POW) && is_unary_op_excluding_update_op(leading_token_type_for_unary_expression), "Ambiguous unary expression in the left hand side of the exponentiation expression; parentheses must be used to disambiguate the expression");

            // Mixing ?? with || or && is currently specified as an early error.
            // Since ?? is the lowest-precedence binary operator, it suffices to check whether these ever coexist in the operator stack.
            if self.match_(AND) || self.match_(OR) {
                has_logical_operator = true;
            } else if self.match_(COALESCE) {
                has_coalesce_operator = true;
            }
            fail_if_true!(self, @hook { exit_cleanup!(); }, has_logical_operator && has_coalesce_operator, "Coalescing and logical operators used together in the same expression; parentheses must be used to disambiguate");

            self.parser_state.non_trivial_expression_count += 1;
            self.parser_state.non_lhs_count += 1;
            let operator_token = self.token.type_ as i32;
            self.next(B::DONT_BUILD_STRINGS);

            while operator_stack_depth != 0 && context.operator_stack_should_reduce(precedence) {
                debug_assert!(operand_stack_depth > 1);

                let rhs = context.get_from_operand_stack(-1);
                let lhs = context.get_from_operand_stack(-2);
                context.shrink_operand_stack_by(&mut operand_stack_depth, 2);
                context.append_binary_operation(&location, &mut operand_stack_depth, &mut operator_stack_depth, lhs, rhs);
                context.operator_stack_pop(&mut operator_stack_depth);
            }
            context.operator_stack_append(&mut operator_stack_depth, operator_token, precedence);
            previous_operator = operator_token;
        }
        while operator_stack_depth != 0 {
            debug_assert!(operand_stack_depth > 1);

            let rhs = context.get_from_operand_stack(-1);
            let lhs = context.get_from_operand_stack(-2);
            context.shrink_operand_stack_by(&mut operand_stack_depth, 2);
            context.append_binary_operation(&location, &mut operand_stack_depth, &mut operator_stack_depth, lhs, rhs);
            context.operator_stack_pop(&mut operator_stack_depth);
        }
        let result = context.pop_operand_stack(&mut operand_stack_depth);
        exit_cleanup!();
        Some(result)
    }

    /// `template <class TreeBuilder> TreeProperty parseProperty(TreeBuilder&)`.
    pub(crate) fn parse_property<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Property> {
        let mut parse_mode = SourceParseMode::MethodMode;
        let mut was_ident = false;
        let mut times_position: Option<u32> = None;
        let mut async_position: Option<u32> = None;

        if self.match_(TIMES) {
            times_position = Some(self.token_start());
            self.next(LexerFlagSet::empty());
            parse_mode = SourceParseMode::GeneratorWrapperMethodMode;
        }

        // `SetForScope innerParseMode(m_parseMode, parseMode)` seguido de `parsePropertyMethod` e
        // `propagateError()`: devolve o método e o `m_parseMode` antigo, que o chamador restaura depois
        // de criar a propriedade (o destrutor do C++ roda no fim do bloco).
        macro_rules! parse_method_in_mode {
            ($name:expr, $function_start:expr) => {{
                let old_parse_mode = self.parse_mode;
                self.parse_mode = parse_mode;
                let method = self.parse_property_method(context, $name, $function_start);
                propagate_error!(self, @hook { self.parse_mode = old_parse_mode; });
                (method.unwrap_or_default(), old_parse_mode)
            }};
        }

        // parseProperty:
        'parse_property: loop {
            let token_type = self.token.type_;
            if token_type == ESCAPED_KEYWORD || token_type == IDENT {
                if self.token.data.ident.as_ref() == Some(&self.vm.property_names.r#async) && !self.token.data.escaped {
                    async_position = Some(self.token_start());
                    if parse_mode == SourceParseMode::MethodMode {
                        let save_point = self.create_save_point(context);
                        self.next(LexerFlagSet::empty());

                        if self.match_(COLON) || self.match_(OPENPAREN) || self.match_(COMMA) || self.match_(CLOSEBRACE) {
                            self.restore_save_point(context, &save_point);
                            was_ident = true;
                            // goto namedProperty (o bloco abaixo, depois da cadeia)
                        } else {
                            fail_if_true!(self, self.lexer.has_line_terminator_before_token(), "Expected a property name following keyword 'async'");
                            if self.consume(TIMES) {
                                parse_mode = SourceParseMode::AsyncGeneratorWrapperMethodMode;
                            } else {
                                parse_mode = SourceParseMode::AsyncMethodMode;
                            }
                            continue 'parse_property;
                        }
                    }
                }
                // [[fallthrough]] para YIELD/AWAIT e dali para STRING.
                was_ident = true;
            } else if token_type == YIELD || token_type == AWAIT {
                was_ident = true;
            } else if token_type == STRING {
                // cai no `namedProperty`
            } else if token_type == DOUBLE || token_type == INTEGER {
                let function_start = times_position.unwrap_or(async_position.unwrap_or(self.token_start()));
                let identifier_arena = self.parser_arena.identifier_arena();
                let ident = identifier_arena.borrow_mut().make_numeric_identifier(&self.vm, self.token.data.double_value);
                self.next(LexerFlagSet::empty());

                if self.match_(OPENPAREN) {
                    let (method, old_parse_mode) = parse_method_in_mode!(Some(&ident), function_start);
                    let property = context.create_property_named(Some(&ident), method, PropertyNode::CONSTANT, SuperBinding::Needed, InferName::Allowed, ClassElementTag::No);
                    self.parse_mode = old_parse_mode;
                    return Some(property);
                }
                fail_if_true!(self, parse_mode != SourceParseMode::MethodMode, "Expected a parenthesis for argument list");

                consume_or_fail!(self, COLON, "Expected ':' after property name");
                let node = self.parse_assignment_expression(context);
                fail_if_false!(self, node.is_some(), "Cannot parse expression for property declaration");
                let node = node.unwrap_or_default();
                context.set_end_offset(&node, self.lexer.current_offset());
                return Some(context.create_property_named(Some(&ident), node, PropertyNode::CONSTANT, SuperBinding::NotNeeded, InferName::Allowed, ClassElementTag::No));
            } else if token_type == BIGINT {
                let big_int_string = self.token.data.big_int_string.clone().unwrap_or_else(Identifier::null_identifier);
                let radix = self.token.data.radix;
                let identifier_arena = self.parser_arena.identifier_arena();
                let ident = identifier_arena.borrow_mut().make_big_int_decimal_identifier(&self.vm, &big_int_string, radix);
                fail_if_false!(self, ident.is_some(), "Cannot parse big int property name");
                let ident = ident.unwrap_or_else(Identifier::null_identifier);
                let function_start = times_position.unwrap_or(async_position.unwrap_or(self.token_start()));
                self.next(LexerFlagSet::empty());

                if self.match_(OPENPAREN) {
                    let (method, old_parse_mode) = parse_method_in_mode!(Some(&ident), function_start);
                    let property = context.create_property_named(Some(&ident), method, PropertyNode::CONSTANT, SuperBinding::Needed, InferName::Allowed, ClassElementTag::No);
                    self.parse_mode = old_parse_mode;
                    return Some(property);
                }
                fail_if_true!(self, parse_mode != SourceParseMode::MethodMode, "Expected a parenthesis for argument list");

                consume_or_fail!(self, COLON, "Expected ':' after property name");
                let node = self.parse_assignment_expression(context);
                fail_if_false!(self, node.is_some(), "Cannot parse expression for property declaration");
                let node = node.unwrap_or_default();
                context.set_end_offset(&node, self.lexer.current_offset());
                return Some(context.create_property_named(Some(&ident), node, PropertyNode::CONSTANT, SuperBinding::NotNeeded, InferName::Allowed, ClassElementTag::No));
            } else if token_type == OPENBRACKET {
                let function_start = times_position.unwrap_or(async_position.unwrap_or(self.token_start()));
                self.next(LexerFlagSet::empty());
                let property_name = self.parse_assignment_expression(context);
                fail_if_false!(self, property_name.is_some(), "Cannot parse computed property name");
                let property_name = property_name.unwrap_or_default();
                handle_production_or_fail!(self, CLOSEBRACKET, "]", "end", "computed property name");

                if self.match_(OPENPAREN) {
                    let null_identifier = self.vm.property_names.null_identifier.clone();
                    let (method, old_parse_mode) = parse_method_in_mode!(Some(&null_identifier), function_start);
                    let property = context.create_property_computed(property_name, method, PropertyNode::CONSTANT | PropertyNode::COMPUTED, SuperBinding::Needed, ClassElementTag::No);
                    self.parse_mode = old_parse_mode;
                    return Some(property);
                }
                fail_if_true!(self, parse_mode != SourceParseMode::MethodMode, "Expected a parenthesis for argument list");

                consume_or_fail!(self, COLON, "Expected ':' after property name");
                let node = self.parse_assignment_expression(context);
                fail_if_false!(self, node.is_some(), "Cannot parse expression for property declaration");
                let node = node.unwrap_or_default();
                context.set_end_offset(&node, self.lexer.current_offset());
                return Some(context.create_property_computed(property_name, node, PropertyNode::CONSTANT | PropertyNode::COMPUTED, SuperBinding::NotNeeded, ClassElementTag::No));
            } else if token_type == DOTDOTDOT {
                let spread_location = self.token.location();
                let start = self.token.start_position;
                let divot = self.token.end_position;
                self.next(LexerFlagSet::empty());
                let elem = self.parse_assignment_expression(context);
                fail_if_false!(self, elem.is_some(), "Cannot parse subject of a spread operation");
                let node = context.create_object_spread_expression(&spread_location, elem.unwrap_or_default(), start, divot, self.last_token_end_position());
                return Some(context.create_property_expression(node, PropertyNode::SPREAD, SuperBinding::NotNeeded, ClassElementTag::No));
            } else {
                fail_if_false!(self, (self.token.type_ & KEYWORD_TOKEN_FLAG) != 0, "Expected a property name");
                was_ident = true; // Treat keyword token as an identifier
                // goto namedProperty
            }

            // namedProperty: (cases IDENT, ESCAPED_KEYWORD, YIELD, AWAIT, STRING e `default`)
            let ident = self.token.data.ident.clone().unwrap_or_else(Identifier::null_identifier);
            let was_unescaped_ident = was_ident && !self.token.data.escaped;
            let getter_or_setter_start_offset = self.token_start();
            let function_start = times_position.unwrap_or(async_position.unwrap_or(self.token_start()));
            let ident_token = self.token.clone();

            if was_unescaped_ident && !is_generator_method_parse_mode(parse_mode) && (ident == self.vm.property_names.get || ident == self.vm.property_names.set) {
                self.next(LexerFlagSet::new(&[LexerFlags::IgnoreReservedWords]));
            } else {
                let mut flags = B::DONT_BUILD_KEYWORDS;
                flags.add(LexerFlags::IgnoreReservedWords);
                self.next(flags);
            }

            if !is_generator_method_parse_mode(parse_mode) && !is_async_method_parse_mode(parse_mode) && self.match_(COLON) {
                self.next(LexerFlagSet::empty());
                let node = self.parse_assignment_expression(context);
                fail_if_false!(self, node.is_some(), "Cannot parse expression for property declaration");
                let node = node.unwrap_or_default();
                context.set_end_offset(&node, self.lexer.current_offset());
                let infer_name = if ident == self.vm.property_names.underscore_proto { InferName::Disallowed } else { InferName::Allowed };
                return Some(context.create_property_named(Some(&ident), node, PropertyNode::CONSTANT, SuperBinding::NotNeeded, infer_name, ClassElementTag::No));
            }

            if self.match_(OPENPAREN) {
                let (method, old_parse_mode) = parse_method_in_mode!(Some(&ident), function_start);
                let property = context.create_property_named(Some(&ident), method, PropertyNode::CONSTANT, SuperBinding::Needed, InferName::Allowed, ClassElementTag::No);
                self.parse_mode = old_parse_mode;
                return Some(property);
            }
            fail_if_true!(self, parse_mode != SourceParseMode::MethodMode, "Expected a parenthesis for argument list");

            fail_if_false!(self, was_ident, "Expected an identifier as property name");

            if self.match_(COMMA) || self.match_(CLOSEBRACE) {
                semantic_failure_due_to_keyword_checking_token!(self, ident_token, "shorthand property name");
                let start = *self.token_start_position();
                let location = self.token_location();
                let is_eval = self.vm.property_names.eval == ident;
                let scope = self.current_scope();
                self.scope_stack[scope].use_variable_identifier(&ident, is_eval);
                let node = context.create_resolve(&location, &ident, start, self.last_token_end_position(), true);
                return Some(context.create_property_named(Some(&ident), node, PropertyNode::CONSTANT | PropertyNode::SHORTHAND, SuperBinding::NotNeeded, InferName::Allowed, ClassElementTag::No));
            }

            let mut type_: Option<PropertyNodeType> = None;
            if was_unescaped_ident {
                if ident == self.vm.property_names.get {
                    type_ = Some(PropertyNode::GETTER);
                } else if ident == self.vm.property_names.set {
                    type_ = Some(PropertyNode::SETTER);
                }
            }
            fail_if_false!(self, type_.is_some(), "Expected a ':' following the property name '", ident, "'");
            return self.parse_getter_setter(context, type_.unwrap_or_default(), getter_or_setter_start_offset, ConstructorKind::None, ClassElementTag::No);
        }
    }

    /// `template <class TreeBuilder> TreeExpression parsePropertyMethod(TreeBuilder&, const Identifier*, unsigned)`.
    pub(crate) fn parse_property_method<B: TreeBuilder>(&mut self, context: &mut B, method_name: Option<&Identifier>, function_start: u32) -> Option<B::Expression> {
        debug_assert!(is_method_parse_mode(self.source_parse_mode()));
        let method_location = self.token_location();
        let mut method_info = ParserFunctionInfo::<B>::default();
        method_info.name = method_name.cloned();
        fail_if_false!(self, self.parse_function_info(context, FunctionNameRequirements::Unnamed, false, ConstructorKind::None, SuperBinding::Needed, function_start, &mut method_info, FunctionDefinitionType::Method, None), "Cannot parse this method");
        Some(context.create_method_definition(&method_location, &method_info))
    }

    /// `template <class TreeBuilder> TreeProperty parseGetterSetter(TreeBuilder&, PropertyNode::Type, unsigned, ConstructorKind, ClassElementTag)`.
    pub(crate) fn parse_getter_setter<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        type_: PropertyNodeType,
        getter_or_setter_start_offset: u32,
        constructor_kind: ConstructorKind,
        tag: ClassElementTag,
    ) -> Option<B::Property> {
        let mut string_property_name: Option<Identifier> = None;
        let mut numeric_property_name: f64 = 0.0;
        let mut computed_property_name: Option<B::Expression> = None;

        let location = self.token_location();

        let matches_private_name = self.match_(PRIVATENAME);
        if self.match_spec_identifier() || self.match_(STRING) || matches_private_name || (self.token.type_ & KEYWORD_TOKEN_FLAG) != 0 {
            let name = self.token.data.ident.clone().unwrap_or_else(Identifier::null_identifier);
            semantic_fail_if_true!(self, tag == ClassElementTag::Static && name == self.vm.property_names.prototype,
                "Cannot declare a static method named 'prototype'");
            semantic_fail_if_true!(self, tag == ClassElementTag::Instance && name == self.vm.property_names.constructor,
                "Cannot declare a getter or setter named 'constructor'");
            semantic_fail_if_true!(self, name == self.vm.property_names.constructor_private_field, "Cannot declare a private accessor named '#constructor'");

            if self.match_(PRIVATENAME) {
                semantic_fail_if_true!(self, tag == ClassElementTag::No, "Cannot declare a private setter or getter outside a class");
            }
            self.next(LexerFlagSet::empty());
            string_property_name = Some(name);
        } else if self.match_(DOUBLE) || self.match_(INTEGER) {
            numeric_property_name = self.token.data.double_value;
            self.next(LexerFlagSet::empty());
        } else if self.match_(BIGINT) {
            let big_int_string = self.token.data.big_int_string.clone().unwrap_or_else(Identifier::null_identifier);
            let radix = self.token.data.radix;
            let identifier_arena = self.parser_arena.identifier_arena();
            string_property_name = identifier_arena.borrow_mut().make_big_int_decimal_identifier(&self.vm, &big_int_string, radix);
            fail_if_false!(self, string_property_name.is_some(), "Cannot parse big int property name");
            self.next(LexerFlagSet::empty());
        } else if self.consume(OPENBRACKET) {
            computed_property_name = self.parse_assignment_expression(context);
            fail_if_false!(self, computed_property_name.is_some(), "Cannot parse computed property name");
            handle_production_or_fail!(self, CLOSEBRACKET, "]", "end", "computed property name");
        } else {
            fail_due_to_unexpected_token!(self);
        }

        let mut info = ParserFunctionInfo::<B>::default();

        // `SetForScope innerParseMode(m_parseMode, mode)` + `failIfFalse(match(OPENPAREN), ...)` +
        // `failIfFalse(parseFunctionInfo(...), ...)`: o `m_parseMode` volta ao valor antigo no fim do
        // ramo e em cada falha.
        macro_rules! parse_accessor_definition {
            ($mode:expr, $missing_parameter_list:expr, $cannot_parse:expr) => {{
                fail_if_false!(self, self.match_(OPENPAREN), $missing_parameter_list);
                let old_parse_mode = self.parse_mode;
                self.parse_mode = $mode;
                fail_if_false!(self, @hook { self.parse_mode = old_parse_mode; }, self.parse_function_info(context, FunctionNameRequirements::Unnamed, false, constructor_kind, SuperBinding::Needed, getter_or_setter_start_offset, &mut info, FunctionDefinitionType::Method, None), $cannot_parse);
                self.parse_mode = old_parse_mode;
            }};
        }

        if (type_ & PropertyNode::GETTER) != 0 {
            parse_accessor_definition!(SourceParseMode::GetterMode, "Expected a parameter list for getter definition", "Cannot parse getter definition");
        } else if (type_ & PropertyNode::SETTER) != 0 {
            parse_accessor_definition!(SourceParseMode::SetterMode, "Expected a parameter list for setter definition", "Cannot parse setter definition");
        } else if (type_ & PropertyNode::PRIVATE_SETTER) != 0 {
            parse_accessor_definition!(SourceParseMode::SetterMode, "Expected a parameter list for private setter definition", "Cannot parse private setter definition");
        } else if (type_ & PropertyNode::PRIVATE_GETTER) != 0 {
            parse_accessor_definition!(SourceParseMode::GetterMode, "Expected a parameter list for private getter definition", "Cannot parse private getter definition");
        }

        if let Some(name) = &string_property_name {
            return Some(context.create_getter_or_setter_property(&location, type_, name, &info, tag));
        }

        if let Some(computed) = computed_property_name {
            return Some(context.create_getter_or_setter_property_computed(&location, type_ | PropertyNode::COMPUTED, computed, &info, tag));
        }

        Some(context.create_getter_or_setter_property_number(&self.vm, &mut self.parser_arena, &location, type_, numeric_property_name, &info, tag))
    }

    /// `template <typename LexerType> void Parser<LexerType>::recordPauseLocation(const JSTextPosition&)`.
    pub(crate) fn record_pause_location(&mut self, position: JSTextPosition) {
        let Some(debugger_parse_data) = &self.debugger_parse_data else {
            return;
        };

        if position.line < 0 {
            return;
        }

        debugger_parse_data.borrow_mut().pause_positions.append_pause(position);
    }

    /// `template <typename LexerType> void Parser<LexerType>::recordFunctionEntryLocation(const JSTextPosition&)`.
    pub(crate) fn record_function_entry_location(&mut self, position: JSTextPosition) {
        let Some(debugger_parse_data) = &self.debugger_parse_data else {
            return;
        };

        debugger_parse_data.borrow_mut().pause_positions.append_entry(position);
    }

    /// `template <typename LexerType> void Parser<LexerType>::recordFunctionLeaveLocation(const JSTextPosition&)`.
    pub(crate) fn record_function_leave_location(&mut self, position: JSTextPosition) {
        let Some(debugger_parse_data) = &self.debugger_parse_data else {
            return;
        };

        debugger_parse_data.borrow_mut().pause_positions.append_leave(position);
    }

    /// `template <class TreeBuilder> TreeExpression parseObjectLiteral(TreeBuilder&)`.
    pub(crate) fn parse_object_literal<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        let location = self.token_location();
        consume_or_fail!(self, OPENBRACE, "Expected opening '{' at the start of an object literal");

        // SetForScope nonLHSCountScope(m_parserState.nonLHSCount): restaurado em cada saída.
        let old_non_lhs_count = self.parser_state.non_lhs_count;
        macro_rules! restore_non_lhs_count {
            () => {
                self.parser_state.non_lhs_count = old_non_lhs_count;
            };
        }
        if self.consume(CLOSEBRACE) {
            let result = context.create_object_literal(&location);
            restore_non_lhs_count!();
            return Some(result);
        }

        let property = self.parse_property(context);
        fail_if_false!(self, @hook { restore_non_lhs_count!(); }, property.is_some(), "Cannot parse object literal property");
        let mut property = property.unwrap_or_default();

        let mut seen_proto_setter = context.is_underscore_proto_setter(&property);

        let property_list = context.create_property_list(&location, property.clone());
        let mut tail = property_list.clone();
        while self.consume(COMMA) {
            if self.match_(CLOSEBRACE) {
                break;
            }
            let property_location = self.token_location();
            let parsed = self.parse_property(context);
            fail_if_false!(self, @hook { restore_non_lhs_count!(); }, parsed.is_some(), "Cannot parse object literal property");
            property = parsed.unwrap_or_default();
            if context.is_underscore_proto_setter(&property) {
                // https://tc39.es/ecma262/#sec-__proto__-property-names-in-object-initializers
                if seen_proto_setter {
                    internal_fail_with_message!(self, @hook { restore_non_lhs_count!(); }, false, "Attempted to redefine __proto__ property");
                }
                seen_proto_setter = true;
            }
            tail = context.create_property_list_append(&property_location, property.clone(), tail);
        }

        // handleProductionOrFail2(CLOSEBRACE, "}", "end", "object literal");
        consume_or_fail!(self, @hook { restore_non_lhs_count!(); }, CLOSEBRACE, "Expected '", "}", "' to ", "end", " an ", "object literal");

        let result = context.create_object_literal_with_properties(&location, property_list);
        restore_non_lhs_count!();
        Some(result)
    }

    /// `template <class TreeBuilder> TreeExpression parseArrayLiteral(TreeBuilder&)`.
    pub(crate) fn parse_array_literal<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        let location = self.token_location();
        consume_or_fail_with_flags!(self, OPENBRACKET, B::DONT_BUILD_STRINGS, "Expected an opening '[' at the beginning of an array literal");

        // SetForScope nonLHSCountScope(m_parserState.nonLHSCount): restaurado em cada saída.
        let old_non_lhs_count = self.parser_state.non_lhs_count;
        macro_rules! restore_non_lhs_count {
            () => {
                self.parser_state.non_lhs_count = old_non_lhs_count;
            };
        }

        let mut elisions: i32 = 0;
        while self.match_(COMMA) {
            self.next(B::DONT_BUILD_STRINGS);
            elisions += 1;
        }
        if self.consume(CLOSEBRACKET) {
            let result = context.create_array_elisions(&location, elisions);
            restore_non_lhs_count!();
            return Some(result);
        }

        let elem: Option<B::Expression>;
        if self.match_(DOTDOTDOT) {
            let spread_location = self.token.location();
            let start = self.token.start_position;
            let divot = self.token.end_position;
            self.next(LexerFlagSet::empty());
            let spread_expr = self.parse_assignment_expression(context);
            fail_if_false!(self, @hook { restore_non_lhs_count!(); }, spread_expr.is_some(), "Cannot parse subject of a spread operation");
            elem = Some(context.create_spread_expression(&spread_location, spread_expr.unwrap_or_default(), start, divot, self.last_token_end_position()));
        } else {
            elem = self.parse_assignment_expression(context);
        }
        fail_if_false!(self, @hook { restore_non_lhs_count!(); }, elem.is_some(), "Cannot parse array literal element");
        let element_list = context.create_element_list(elisions, elem.unwrap_or_default());
        let mut tail = element_list.clone();
        elisions = 0;
        while self.match_(COMMA) {
            self.next(B::DONT_BUILD_STRINGS);
            elisions = 0;

            while self.consume(COMMA) {
                elisions += 1;
            }

            if self.consume(CLOSEBRACKET) {
                let result = context.create_array_elisions_elements(&location, elisions, element_list);
                restore_non_lhs_count!();
                return Some(result);
            }

            if self.match_(DOTDOTDOT) {
                let spread_location = self.token.location();
                let start = self.token.start_position;
                let divot = self.token.end_position;
                self.next(LexerFlagSet::empty());
                let elem = self.parse_assignment_expression(context);
                fail_if_false!(self, @hook { restore_non_lhs_count!(); }, elem.is_some(), "Cannot parse subject of a spread operation");
                let spread = context.create_spread_expression(&spread_location, elem.unwrap_or_default(), start, divot, self.last_token_end_position());
                tail = context.create_element_list_append(tail, elisions, spread);
                continue;
            }
            let elem = self.parse_assignment_expression(context);
            fail_if_false!(self, @hook { restore_non_lhs_count!(); }, elem.is_some(), "Cannot parse array literal element");
            tail = context.create_element_list_append(tail, elisions, elem.unwrap_or_default());
        }

        if !self.consume(CLOSEBRACKET) {
            fail_if_false!(self, @hook { restore_non_lhs_count!(); }, self.match_(DOTDOTDOT), "Expected either a closing ']' or a ',' following an array element");
            internal_fail_with_message!(self, @hook { restore_non_lhs_count!(); }, false, "The '...' operator should come before a target expression");
        }

        let result = context.create_array_elements(&location, element_list);
        restore_non_lhs_count!();
        Some(result)
    }

    /// `template <class TreeBuilder> TreeClassExpression parseClassExpression(TreeBuilder&)`.
    pub(crate) fn parse_class_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::ClassExpression> {
        debug_assert!(self.match_(CLASSTOKEN));
        // SetForScope nonLHSCountScope(m_parserState.nonLHSCount)
        let old_non_lhs_count = self.parser_state.non_lhs_count;
        let mut info = ParserClassInfo::<B>::default();
        info.class_name = Some(self.vm.property_names.null_identifier.clone());
        let result = self.parse_class(context, FunctionNameRequirements::None, &mut info);
        self.parser_state.non_lhs_count = old_non_lhs_count;
        result
    }

    /// `template <class TreeBuilder> TreeExpression parseFunctionExpression(TreeBuilder&)`.
    pub(crate) fn parse_function_expression<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        debug_assert!(self.match_(FUNCTION));
        // SetForScope nonLHSCountScope(m_parserState.nonLHSCount)
        let old_non_lhs_count = self.parser_state.non_lhs_count;
        let location = self.token_location();
        let function_start = self.token_start();
        self.next(LexerFlagSet::empty());
        let mut function_info = ParserFunctionInfo::<B>::default();
        function_info.name = Some(self.vm.property_names.null_identifier.clone());
        let mut parse_mode = SourceParseMode::NormalFunctionMode;
        if self.consume(TIMES) {
            parse_mode = SourceParseMode::GeneratorWrapperFunctionMode;
        }
        // SetForScope setInnerParseMode(m_parseMode, parseMode): destruído antes do `nonLHSCountScope`.
        let old_parse_mode = self.parse_mode;
        self.parse_mode = parse_mode;
        macro_rules! exit_cleanup {
            () => {
                self.parse_mode = old_parse_mode;
                self.parser_state.non_lhs_count = old_non_lhs_count;
            };
        }

        let scope = self.current_scope();
        let constructor_kind = if self.scope_stack[scope].is_global_code() { self.constructor_kind_for_top_level_function_expressions } else { ConstructorKind::None };
        let expected_super_binding = if constructor_kind == ConstructorKind::Extends { SuperBinding::Needed } else { SuperBinding::NotNeeded };

        fail_if_false!(self, @hook { exit_cleanup!(); }, self.parse_function_info(context, FunctionNameRequirements::None, false, constructor_kind, expected_super_binding, function_start, &mut function_info, FunctionDefinitionType::Expression, None), "Cannot parse function expression");
        let result = context.create_function_expr(&location, &function_info);
        exit_cleanup!();
        Some(result)
    }

    /// `template <class TreeBuilder> TreeExpression parseAsyncFunctionExpression(TreeBuilder&, const JSTokenLocation&)`.
    pub(crate) fn parse_async_function_expression<B: TreeBuilder>(&mut self, context: &mut B, location: &JSTokenLocation) -> Option<B::Expression> {
        debug_assert!(self.match_(FUNCTION));
        self.next(LexerFlagSet::empty());
        let mut parse_mode = SourceParseMode::AsyncFunctionMode;

        if self.consume(TIMES) {
            parse_mode = SourceParseMode::AsyncGeneratorWrapperFunctionMode;
        }
        // SetForScope setInnerParseMode(m_parseMode, parseMode): restaurado em cada saída.
        let old_parse_mode = self.parse_mode;
        self.parse_mode = parse_mode;

        let mut function_info = ParserFunctionInfo::<B>::default();
        function_info.name = Some(self.vm.property_names.null_identifier.clone());
        fail_if_false!(self, @hook { self.parse_mode = old_parse_mode; }, self.parse_function_info(context, FunctionNameRequirements::None, false, ConstructorKind::None, SuperBinding::NotNeeded, location.start_offset, &mut function_info, FunctionDefinitionType::Expression, None), if parse_mode == SourceParseMode::AsyncFunctionMode { "Cannot parse async function expression" } else { "Cannot parse async generator function expression" });
        let result = context.create_function_expr(location, &function_info);
        self.parse_mode = old_parse_mode;
        Some(result)
    }

    /// `template <class TreeBuilder> typename TreeBuilder::TemplateString parseTemplateString(TreeBuilder&, bool, RawStringsBuildMode, bool&)`.
    pub(crate) fn parse_template_string<B: TreeBuilder>(&mut self, context: &mut B, is_template_head: bool, raw_strings_build_mode: RawStringsBuildMode, element_is_tail: &mut bool) -> Option<B::TemplateString> {
        if is_template_head {
            debug_assert!(self.match_(BACKQUOTE));
        } else {
            match_or_fail!(self, CLOSEBRACE, "Expected a closing '}' following an expression in template literal");
        }

        // Re-scan the token to recognize it as Template Element.
        let scanned_type = self.lexer.scan_template_string(&mut self.token, raw_strings_build_mode);
        self.token.type_ = scanned_type;
        match_or_fail!(self, TEMPLATE, "Expected an template element");
        let cooked = self.token.data.cooked.clone();
        let raw = self.token.data.raw.clone();
        *element_is_tail = self.token.data.is_tail;
        let location = self.token_location();
        self.next(LexerFlagSet::empty());
        Some(context.create_template_string(&location, cooked.as_ref(), raw.as_ref()))
    }

    /// `template <class TreeBuilder> typename TreeBuilder::TemplateLiteral parseTemplateLiteral(TreeBuilder&, RawStringsBuildMode)`.
    pub(crate) fn parse_template_literal<B: TreeBuilder>(&mut self, context: &mut B, raw_strings_build_mode: RawStringsBuildMode) -> Option<B::TemplateLiteral> {
        debug_assert!(self.match_(BACKQUOTE));
        // SetForScope nonLHSCountScope(m_parserState.nonLHSCount): restaurado em cada saída.
        let old_non_lhs_count = self.parser_state.non_lhs_count;
        macro_rules! restore_non_lhs_count {
            () => {
                self.parser_state.non_lhs_count = old_non_lhs_count;
            };
        }
        let location = self.token_location();
        let mut element_is_tail = false;

        let head_template_string = self.parse_template_string(context, true, raw_strings_build_mode, &mut element_is_tail);
        fail_if_false!(self, @hook { restore_non_lhs_count!(); }, head_template_string.is_some(), "Cannot parse head template element");

        let template_string_list = context.create_template_string_list(head_template_string.unwrap_or_default());
        let mut template_string_tail = template_string_list.clone();

        if element_is_tail {
            let result = context.create_template_literal(&location, template_string_list);
            restore_non_lhs_count!();
            return Some(result);
        }

        fail_if_true!(self, @hook { restore_non_lhs_count!(); }, self.match_(CLOSEBRACE), "Template literal expression cannot be empty");
        let expression = self.parse_expression(context);
        fail_if_false!(self, @hook { restore_non_lhs_count!(); }, expression.is_some(), "Cannot parse expression in template literal");

        let template_expression_list = context.create_template_expression_list(expression.unwrap_or_default());
        let mut template_expression_tail = template_expression_list.clone();

        let template_string = self.parse_template_string(context, false, raw_strings_build_mode, &mut element_is_tail);
        fail_if_false!(self, @hook { restore_non_lhs_count!(); }, template_string.is_some(), "Cannot parse template element");
        template_string_tail = context.create_template_string_list_append(template_string_tail, template_string.unwrap_or_default());

        while !element_is_tail {
            fail_if_true!(self, @hook { restore_non_lhs_count!(); }, self.match_(CLOSEBRACE), "Template literal expression cannot be empty");
            let expression = self.parse_expression(context);
            fail_if_false!(self, @hook { restore_non_lhs_count!(); }, expression.is_some(), "Cannot parse expression in template literal");

            template_expression_tail = context.create_template_expression_list_append(template_expression_tail, expression.unwrap_or_default());

            let template_string = self.parse_template_string(context, false, raw_strings_build_mode, &mut element_is_tail);
            fail_if_false!(self, @hook { restore_non_lhs_count!(); }, template_string.is_some(), "Cannot parse template element");
            template_string_tail = context.create_template_string_list_append(template_string_tail, template_string.unwrap_or_default());
        }

        let result = context.create_template_literal_with_expressions(&location, template_string_list, template_expression_list);
        restore_non_lhs_count!();
        Some(result)
    }

    /// `template <class LexerType> template <class TreeBuilder> TreeExpression createResolveAndUseVariable(TreeBuilder&, const Identifier*, bool, const JSTextPosition&, const JSTokenLocation&)`.
    pub(crate) fn create_resolve_and_use_variable<B: TreeBuilder>(&mut self, context: &mut B, ident: &Identifier, is_eval: bool, start: &JSTextPosition, location: &JSTokenLocation) -> Option<B::Expression> {
        let scope = self.current_scope();
        self.scope_stack[scope].use_variable_identifier(ident, is_eval);
        self.parser_state.last_identifier = Some(ident.clone());
        Some(context.create_resolve(location, ident, *start, self.last_token_end_position(), true))
    }

    /// `template <class TreeBuilder> TreeExpression tryParseArgumentsDotLengthForFastPath(TreeBuilder&)`.
    pub(crate) fn try_parse_arguments_dot_length_for_fast_path<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Expression> {
        // There is a fast path for getting `arguments.length` by reading `argumentCountIncludingThis`
        // directly from CallFrame. In that case, no need to materialize `arguments` object. The fast
        // path for `arguments.length` is applied by excluding 'arguments.length` pattern for
        // ArgumentsFeature except for two cases:
        // 1. 'arguments.length` modifications.
        // 2. Function level global variable declaration with identifier `arguments`.
        if context.has_arguments_feature() || !self.match_(IDENT) || !self.is_arguments_identifier() {
            return None;
        }

        // If semantic checks fail here, then let `parsePrimaryExpression` handle the error thrown.
        // Note that these checks must align to the checks in `parsePrimaryExpression` under
        // the clause with token type IDENT.
        let current = self.current_scope();
        let arguments_owner = self.closest_scope_owning_arguments();
        if self.scope_stack[current].is_static_block()
            || self.parser_state.is_parsing_class_field_initializer
            || self.scope_stack[arguments_owner].eval_context_type() == EvalContextType::InstanceFieldEvalContext
        {
            return None;
        }

        let arguments_save_point = self.create_save_point(context);
        let primary_start = *self.token_start_position();
        let primary_location = self.token_location();
        self.next(LexerFlagSet::empty());
        if self.match_(DOT) {
            let arguments_dot_save_point = self.create_save_point(context);
            self.next(LexerFlagSet::empty());
            if self.match_(IDENT) && self.token.data.ident.as_ref() == Some(&self.vm.property_names.length) {
                self.seen_arguments_dot_length = true;

                let is_eval = false;
                let arguments_identifier = self.vm.property_names.arguments.clone();
                let scope = self.current_scope();
                self.scope_stack[scope].use_variable_identifier(&arguments_identifier, is_eval);
                self.parser_state.last_identifier = Some(arguments_identifier.clone());

                let need_to_check_uses_arguments = false;
                let arguments_dot_expression = context.create_resolve(&primary_location, &arguments_identifier, primary_start, self.last_token_end_position(), need_to_check_uses_arguments);
                self.restore_save_point(context, &arguments_dot_save_point);
                return Some(arguments_dot_expression);
            }
        }
        self.restore_save_point(context, &arguments_save_point);
        None
    }
}
