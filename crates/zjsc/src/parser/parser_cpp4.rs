// Quarta fatia de `parser/Parser.cpp` (linhas 2129 a 2945), incluída por `include!` em `parser.rs`.
// Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Convenções desta fatia (as de `parser_cpp1.rs` valem):
//
// - Funções com destrutor RAII vivo em muitos pontos de saída (`parseFunctionInfo`,
//   `parseFunctionParameters`) rodam o corpo num closure que recebe o parser como `this`; as macros de
//   erro recebem `this` e o `return` delas sai do closure. Depois da chamada vêm os destrutores, na
//   ordem inversa de construção, como no C++. Quando há poucas saídas (`parseStatement`,
//   `parseFunctionBody`, `parseFunctionDeclarationStatement`) vale o `@hook` das macros.
// - `TreeFunctionBody` e `TreeFormalParameterList` (que `ParserFunctionInfo::body` guarda sem
//   `Option`) são devolvidos como `B::FunctionBody` e `B::FormalParameterList`; o valor nulo do C++ é
//   o `Default` do tipo associado, e o teste `if (x)` vira `x != Default::default()`.
// - `unsigned*`/`const Identifier**` de saída viram `Option<&mut u32>`/`Option<&mut Option<Identifier>>`;
//   `const Identifier*&` vira `&mut Option<Identifier>`. Argumentos padrão do `.h` entram explícitos.
// - `Scope::MaybeParseAsGeneratorFunctionForScope` e `DepthManager` (linhas 175 a 200) viram salvar o
//   valor antigo e restaurá-lo em cada saída, no ponto de uso.
// - `FunctionInfoType` (template de `parseFunctionParameters`) é o trait `HasParameterCount`, o único
//   campo que a função toca.
// - As duas sobrecargas de `adjustSuperBindingForBaseConstructor` e de `getMetadata` viram nomes
//   distintos / um trait por tipo de construtor.

/// O único campo de `FunctionInfoType` que `parseFunctionParameters` lê e escreve.
pub trait HasParameterCount {
    fn parameter_count_mut(&mut self) -> &mut u32;
}

impl<T: TreeBuilder> HasParameterCount for ParserFunctionInfo<T> {
    fn parameter_count_mut(&mut self) -> &mut u32 {
        &mut self.parameter_count
    }
}

// `getMetadata(ParserFunctionInfo<...>&)` (sobrecarga por tipo de construtor) é `TreeBuilder::get_metadata`.

/// `static ALWAYS_INLINE SuperBinding adjustSuperBindingForBaseConstructor(...)`, a sobrecarga de seis
/// argumentos.
#[inline(always)]
fn adjust_super_binding_for_base_constructor(constructor_kind: ConstructorKind, expected_super_binding: SuperBinding, parse_mode: SourceParseMode, scope_needs_super_binding: bool, current_scope_uses_eval: bool, inner_arrow_function_features: InnerArrowFunctionCodeFeatures) -> SuperBinding {
    if expected_super_binding == SuperBinding::NotNeeded {
        return SuperBinding::NotNeeded;
    }

    if constructor_kind == ConstructorKind::None
        && SourceParseModeSet::new(&[
            SourceParseMode::AsyncGeneratorWrapperMethodMode,
            SourceParseMode::GeneratorWrapperMethodMode,
            SourceParseMode::AsyncMethodMode,
        ])
        .contains(parse_mode)
    {
        return SuperBinding::Needed;
    }

    if constructor_kind == ConstructorKind::None || constructor_kind == ConstructorKind::Base {
        let is_super_used_in_inner_arrow_function = (inner_arrow_function_features & SUPER_PROPERTY_INNER_ARROW_FUNCTION_FEATURE) != 0;
        return if scope_needs_super_binding || is_super_used_in_inner_arrow_function || current_scope_uses_eval { SuperBinding::Needed } else { SuperBinding::NotNeeded };
    }

    SuperBinding::Needed
}

/// `static ALWAYS_INLINE SuperBinding adjustSuperBindingForBaseConstructor(..., Scope* functionScope)`.
#[inline(always)]
fn adjust_super_binding_for_base_constructor_scope(constructor_kind: ConstructorKind, expected_super_binding: SuperBinding, parse_mode: SourceParseMode, function_scope: &Scope) -> SuperBinding {
    adjust_super_binding_for_base_constructor(constructor_kind, expected_super_binding, parse_mode, function_scope.needs_super_binding(), function_scope.uses_eval(), function_scope.inner_arrow_function_features())
}

/// `static const char* stringArticleForFunctionMode(SourceParseMode)`.
fn string_article_for_function_mode(mode: SourceParseMode) -> &'static str {
    match mode {
        SourceParseMode::GetterMode
        | SourceParseMode::SetterMode
        | SourceParseMode::NormalFunctionMode
        | SourceParseMode::MethodMode
        | SourceParseMode::GeneratorBodyMode
        | SourceParseMode::GeneratorWrapperFunctionMode
        | SourceParseMode::GeneratorWrapperMethodMode => "a ",
        SourceParseMode::ArrowFunctionMode
        | SourceParseMode::AsyncFunctionMode
        | SourceParseMode::AsyncFunctionBodyMode
        | SourceParseMode::AsyncMethodMode
        | SourceParseMode::AsyncArrowFunctionBodyMode
        | SourceParseMode::AsyncArrowFunctionMode
        | SourceParseMode::AsyncGeneratorWrapperFunctionMode
        | SourceParseMode::AsyncGeneratorBodyMode
        | SourceParseMode::AsyncGeneratorWrapperMethodMode => "an ",
        SourceParseMode::ProgramMode
        | SourceParseMode::ModuleAnalyzeMode
        | SourceParseMode::ModuleEvaluateMode
        | SourceParseMode::ClassFieldInitializerMode
        | SourceParseMode::ClassStaticBlockMode => panic!("RELEASE_ASSERT_NOT_REACHED"),
    }
}

/// `static const char* stringForFunctionMode(SourceParseMode)`.
fn string_for_function_mode(mode: SourceParseMode) -> &'static str {
    match mode {
        SourceParseMode::GetterMode => "getter",
        SourceParseMode::SetterMode => "setter",
        SourceParseMode::NormalFunctionMode => "function",
        SourceParseMode::MethodMode => "method",
        SourceParseMode::GeneratorWrapperFunctionMode | SourceParseMode::GeneratorBodyMode => "generator function",
        SourceParseMode::GeneratorWrapperMethodMode => "generator method",
        SourceParseMode::ArrowFunctionMode => "arrow function",
        SourceParseMode::AsyncFunctionMode | SourceParseMode::AsyncFunctionBodyMode => "async function",
        SourceParseMode::AsyncMethodMode => "async method",
        SourceParseMode::AsyncArrowFunctionBodyMode | SourceParseMode::AsyncArrowFunctionMode => "async arrow function",
        SourceParseMode::AsyncGeneratorWrapperFunctionMode | SourceParseMode::AsyncGeneratorBodyMode => "async generator function",
        SourceParseMode::AsyncGeneratorWrapperMethodMode => "async generator method",
        SourceParseMode::ProgramMode
        | SourceParseMode::ModuleAnalyzeMode
        | SourceParseMode::ModuleEvaluateMode
        | SourceParseMode::ClassFieldInitializerMode
        | SourceParseMode::ClassStaticBlockMode => panic!("RELEASE_ASSERT_NOT_REACHED"),
    }
}

impl<T: CharType> Parser<T> {
    /// `template <class TreeBuilder> TreeStatement parseStatement(TreeBuilder&, const Identifier*& directive, unsigned* directiveLiteralLength)`.
    pub(crate) fn parse_statement<B: TreeBuilder>(&mut self, context: &mut B, directive: &mut Option<Identifier>, directive_literal_length: Option<&mut u32>) -> Option<B::Statement> {
        // DepthManager statementDepth(&m_statementDepth): restaurado em cada saída.
        let old_statement_depth = self.statement_depth;
        self.statement_depth += 1;
        let mut non_trivial_expression_count: i32 = 0;
        // failIfStackOverflow()
        let Some(_logical_stack_frame) = self.vm.enter_logical_frame(crate::runtime::vm::stack_cost::STATEMENT_LEVEL) else {
            update_error_message!(self, @hook { self.statement_depth = old_statement_depth; }, false, "Stack exhausted");
            self.has_stack_overflow = true;
            self.statement_depth = old_statement_depth;
            return None;
        };
        let mut result: Option<B::Statement> = None;
        let mut should_set_end_offset = true;
        let mut should_set_pause_location = false;
        let parent_allows_function_declaration_as_statement = self.immediate_parent_allows_function_declaration_in_statement;
        self.immediate_parent_allows_function_declaration_in_statement = false;

        match self.token.type_ {
            OPENBRACE => {
                result = self.parse_block_statement(context, BlockType::Normal);
                should_set_end_offset = false;
            }
            VAR => {
                result = self.parse_variable_declaration(context, DeclarationType::VarDeclaration, ExportType::NotExported);
                should_set_pause_location = true;
            }
            FUNCTION => {
                result = self.parse_function_declaration_statement(context, parent_allows_function_declaration_as_statement);
            }
            SEMICOLON => {
                let location = self.token_location();
                self.next(LexerFlagSet::empty());
                result = Some(context.create_empty_statement(&location));
            }
            IF => {
                result = self.parse_if_statement(context);
            }
            DO => {
                result = self.parse_do_while_statement(context);
            }
            WHILE => {
                result = self.parse_while_statement(context);
            }
            FOR => {
                result = self.parse_for_statement(context);
            }
            CONTINUE => {
                result = self.parse_continue_statement(context);
                should_set_pause_location = true;
            }
            BREAK => {
                result = self.parse_break_statement(context);
                should_set_pause_location = true;
            }
            RETURN => {
                result = self.parse_return_statement(context);
                should_set_pause_location = true;
            }
            WITH => {
                result = self.parse_with_statement(context);
            }
            SWITCH => {
                result = self.parse_switch_statement(context);
            }
            THROW => {
                result = self.parse_throw_statement(context);
                should_set_pause_location = true;
            }
            TRY => {
                result = self.parse_try_statement(context);
            }
            DEBUGGER => {
                result = self.parse_debugger_statement(context);
                should_set_pause_location = true;
            }
            EOFTOK | CASE | CLOSEBRACE | DEFAULT => {
                // These tokens imply the end of a set of source elements
                self.statement_depth = old_statement_depth;
                return None;
            }
            // `case ESCAPED_KEYWORD:` cai (`[[fallthrough]]`) no bloco de `LET`/`IDENT`/`AWAIT`/`YIELD`.
            ESCAPED_KEYWORD | LET | IDENT | AWAIT | YIELD => {
                if self.token.type_ == ESCAPED_KEYWORD && !self.match_allowed_escaped_contextual_keyword() {
                    fail_due_to_unexpected_token!(self, @hook { self.statement_depth = old_statement_depth; });
                }
                let allow_function_declaration_as_statement = false;
                result = self.parse_expression_or_label_statement(context, allow_function_declaration_as_statement);
                should_set_pause_location = match &result {
                    Some(statement) => !context.should_skip_pause_location(statement),
                    None => false,
                };
            }
            // `case STRING:` cai (`[[fallthrough]]`) no `default`.
            _ => {
                if self.token.type_ == STRING {
                    *directive = self.token.data.ident.clone();
                    if let Some(length) = directive_literal_length {
                        let location = self.token.location();
                        *length = location.end_offset - location.start_offset;
                    }
                    non_trivial_expression_count = self.parser_state.non_trivial_expression_count;
                }
                let expr_statement = self.parse_expression_statement(context);
                if directive.is_some() && non_trivial_expression_count != self.parser_state.non_trivial_expression_count {
                    *directive = None;
                }
                result = expr_statement;
                should_set_pause_location = true;
            }
        }

        if let Some(statement) = &result {
            if should_set_end_offset {
                context.set_end_offset(statement, self.last_token_location.end_offset as i32);
            }
            if should_set_pause_location {
                let breakpoint_location = context.breakpoint_location(statement);
                self.record_pause_location(breakpoint_location);
            }
        }

        self.statement_depth = old_statement_depth;
        result
    }

    /// `template <class TreeBuilder> TreeStatement parseFunctionDeclarationStatement(TreeBuilder&, bool)`.
    pub(crate) fn parse_function_declaration_statement<B: TreeBuilder>(&mut self, context: &mut B, parent_allows_function_declaration_as_statement: bool) -> Option<B::Statement> {
        semantic_fail_if_true!(self, self.strict_mode(), "Function declarations are only allowed inside blocks or switch statements in strict mode");
        fail_if_false!(self, parent_allows_function_declaration_as_statement, "Function declarations are only allowed inside block statements or at the top level of a program");

        // Any function declaration that isn't in a block is a syntax error unless it's
        // in an if/else statement. If it's in an if/else statement, we will magically
        // treat it as if the if/else statement is inside a block statement.
        // to the very top like a "var". For example:
        // function a() {
        //     if (cond) function foo() { }
        // }
        // will be rewritten as:
        // function a() {
        //     if (cond) { function foo() { } }
        // }
        let pushed = self.push_scope();
        let mut block_scope = AutoPopScope::new(pushed);
        let block_scope_ref = block_scope.scope();
        self.scope_stack[block_scope_ref].set_is_lexical_scope();
        self.scope_stack[block_scope_ref].prevent_var_declarations();
        let location = self.token_location();
        let start = self.token_line();

        let function = self.parse_function_declaration(context, FunctionDeclarationType::Statement, ExportType::NotExported, DeclarationDefaultContext::Standard, None);
        propagate_error!(self, @hook { block_scope.cleanup(self); });
        fail_if_false!(self, @hook { block_scope.cleanup(self); }, function.is_some(), "Expected valid function statement after 'function' keyword");
        let source_elements = context.create_source_elements();
        context.append_statement(&source_elements, function.unwrap_or_default());
        let (lexical_environment, function_declarations) = self.pop_scope_auto(&mut block_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);
        block_scope.cleanup(self);
        Some(context.create_block_statement(&location, source_elements, start, self.last_token_location.line, lexical_environment, function_declarations))
    }

    /// `template <class TreeBuilder> bool parseFormalParameters(TreeBuilder&, TreeFormalParameterList, bool, bool, unsigned&)`.
    pub(crate) fn parse_formal_parameters<B: TreeBuilder>(&mut self, context: &mut B, list: &B::FormalParameterList, is_arrow_function: bool, is_method: bool, parameter_count: &mut u32) -> bool {
        let mut has_default_parameter_values = false;
        let mut has_destructuring_pattern = false;
        let mut is_rest_parameter = false;
        let mut duplicate_parameter: Option<Identifier> = None;
        let mut rest_parameter_start: usize = 0;

        // `#define failIfDuplicateIfViolation()`: o `isRestParameter`, `hasDefaultParameterValues` e
        // `hasDestructuringPattern` são lidos no ponto de uso, como a macro do C++.
        macro_rules! fail_if_duplicate_if_violation {
            () => {
                if let Some(duplicate) = duplicate_parameter.clone() {
                    semantic_fail_if_true!(self, has_default_parameter_values, "Duplicate parameter '", duplicate, "' not allowed in function with default parameter values");
                    semantic_fail_if_true!(self, has_destructuring_pattern, "Duplicate parameter '", duplicate, "' not allowed in function with destructuring parameters");
                    semantic_fail_if_true!(self, is_rest_parameter, "Duplicate parameter '", duplicate, "' not allowed in function with a rest parameter");
                    semantic_fail_if_true!(self, is_arrow_function, "Duplicate parameter '", duplicate, "' not allowed in an arrow function");
                    semantic_fail_if_true!(self, is_method, "Duplicate parameter '", duplicate, "' not allowed in a method");
                }
            };
        }

        loop {
            let mut parameter: B::DestructuringPattern = Default::default();
            let mut default_value: Option<B::Expression> = None;

            if self.match_(CLOSEPAREN) {
                break;
            }

            if self.consume(DOTDOTDOT) {
                semantic_fail_if_true!(self, self.is_disallowed_identifier_await(&self.token), "Cannot use 'await' as a parameter name in an async function");
                let destructuring_pattern = self.parse_destructuring_pattern(context, DestructuringKind::DestructureToParameters, ExportType::NotExported, Some(&mut duplicate_parameter), Some(&mut has_destructuring_pattern), AssignmentContext::DeclarationStatement, 0);
                propagate_error!(self);
                let rest_parameter = context.create_rest_parameter(destructuring_pattern.unwrap_or_default(), rest_parameter_start);
                parameter = rest_parameter.into();
                fail_if_true!(self, self.match_(COMMA), "Rest parameter should be the last parameter in a function declaration"); // Let's have a good error message for this common case.
                is_rest_parameter = true;
            } else {
                parameter = self
                    .parse_destructuring_pattern(context, DestructuringKind::DestructureToParameters, ExportType::NotExported, Some(&mut duplicate_parameter), Some(&mut has_destructuring_pattern), AssignmentContext::DeclarationStatement, 0)
                    .unwrap_or_default();
            }
            fail_if_false!(self, parameter != B::DestructuringPattern::default(), "Cannot parse parameter pattern");
            if !is_rest_parameter {
                default_value = self.parse_default_value_for_destructuring_pattern(context);
                if default_value.is_some() {
                    has_default_parameter_values = true;
                }
            }
            propagate_error!(self);
            fail_if_duplicate_if_violation!();
            if is_rest_parameter || default_value.is_some() || has_destructuring_pattern {
                let current = self.current_scope();
                self.scope_stack[current].set_has_non_simple_parameter_list();
            }
            context.append_parameter(list, parameter, default_value.unwrap_or_default());
            if !is_rest_parameter {
                rest_parameter_start += 1;
                if !has_default_parameter_values {
                    *parameter_count += 1;
                }
            }
            if !(!is_rest_parameter && self.consume(COMMA)) {
                break;
            }
        }

        true
    }

    /// `template <class TreeBuilder> TreeFunctionBody parseFunctionBody(TreeBuilder&, SyntaxChecker&, ...)`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn parse_function_body<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        syntax_checker: &mut SyntaxChecker<'_>,
        start_location: &JSTokenLocation,
        start_column: i32,
        function_start: u32,
        function_name_start: i32,
        parameters_start: i32,
        constructor_kind: ConstructorKind,
        super_binding: SuperBinding,
        body_type: FunctionBodyType,
        parameter_count: u32,
    ) -> B::FunctionBody {
        // SetForScope overrideParsingClassFieldInitializer / maybeUnmaskAsync: restaurados em cada saída.
        let old_is_parsing_class_field_initializer = self.parser_state.is_parsing_class_field_initializer;
        self.parser_state.is_parsing_class_field_initializer = body_type != FunctionBodyType::StandardFunctionBodyBlock && old_is_parsing_class_field_initializer;
        let old_class_field_init_masks_async = self.parser_state.class_field_init_masks_async;
        self.parser_state.class_field_init_masks_async = !is_async_function_parse_mode(self.parse_mode) && old_class_field_init_masks_async;
        macro_rules! restore_class_field_state {
            () => {{
                self.parser_state.class_field_init_masks_async = old_class_field_init_masks_async;
                self.parser_state.is_parsing_class_field_initializer = old_is_parsing_class_field_initializer;
            }};
        }

        let is_arrow_function_body_expression = body_type == FunctionBodyType::ArrowFunctionBodyExpression;
        if !is_arrow_function_body_expression {
            self.next(LexerFlagSet::empty());
            if self.match_(CLOSEBRACE) {
                let end_column = self.token_column() as u32;
                let current = self.current_scope();
                let function_super_binding = adjust_super_binding_for_base_constructor_scope(constructor_kind, super_binding, self.source_parse_mode(), &self.scope_stack[current]);
                let end_location = self.token_location();
                let metadata = context.create_function_metadata(start_location, &end_location, start_column as u32, end_column, function_start, function_name_start, parameters_start, self.implementation_visibility(), self.lexically_scoped_features(), constructor_kind, function_super_binding, parameter_count, self.source_parse_mode(), is_arrow_function_body_expression);
                restore_class_field_state!();
                return metadata;
            }
        }

        // DepthManager statementDepth(&m_statementDepth)
        let old_statement_depth = self.statement_depth;
        self.statement_depth = 0;
        macro_rules! restore_all {
            () => {{
                self.statement_depth = old_statement_depth;
                restore_class_field_state!();
            }};
        }
        if body_type == FunctionBodyType::ArrowFunctionBodyExpression {
            if self.debugger_parse_data.is_some() {
                let parsed = self.parse_arrow_function_single_expression_body_source_elements(context);
                fail_if_false!(self, @hook { restore_all!(); }, parsed.is_some(), "Cannot parse body of this arrow function");
            } else {
                let parsed = self.parse_arrow_function_single_expression_body_source_elements(syntax_checker);
                fail_if_false!(self, @hook { restore_all!(); }, parsed.is_some(), "Cannot parse body of this arrow function");
            }
        } else if self.debugger_parse_data.is_some() {
            let parsed = self.parse_source_elements(context, SourceElementsMode::CheckForStrictMode);
            fail_if_false!(self, @hook { restore_all!(); }, parsed.is_some(), if body_type == FunctionBodyType::StandardFunctionBodyBlock { "Cannot parse body of this function" } else { "Cannot parse body of this arrow function" });
        } else {
            let parsed = self.parse_source_elements(syntax_checker, SourceElementsMode::CheckForStrictMode);
            fail_if_false!(self, @hook { restore_all!(); }, parsed.is_some(), if body_type == FunctionBodyType::StandardFunctionBodyBlock { "Cannot parse body of this function" } else { "Cannot parse body of this arrow function" });
        }
        // An expression body ends at its last token (the current token is already past it), as the SourceProviderCache path records it.
        let end_location = if is_arrow_function_body_expression { self.last_token_location } else { self.token_location() };
        let end_column = end_location.start_offset.wrapping_sub(end_location.line_start_offset);
        let current = self.current_scope();
        let function_super_binding = adjust_super_binding_for_base_constructor_scope(constructor_kind, super_binding, self.source_parse_mode(), &self.scope_stack[current]);
        let mut implementation_visibility = self.implementation_visibility();
        if is_async_function_wrapper_parse_mode(self.source_parse_mode()) && self.scope_stack[current].uses_await() {
            // std::max(ImplementationVisibility::Private, implementationVisibility)
            if (ImplementationVisibility::Private as u8) >= (implementation_visibility as u8) {
                implementation_visibility = ImplementationVisibility::Private;
            }
            self.scope_stack[current].set_implementation_visibility(implementation_visibility);
        }
        let metadata = context.create_function_metadata(start_location, &end_location, start_column as u32, end_column, function_start, function_name_start, parameters_start, implementation_visibility, self.lexically_scoped_features(), constructor_kind, function_super_binding, parameter_count, self.source_parse_mode(), is_arrow_function_body_expression);
        restore_all!();
        metadata
    }

    /// `template <class TreeBuilder, class FunctionInfoType> FormalParameterList parseFunctionParameters(TreeBuilder&, FunctionInfoType&)`.
    pub(crate) fn parse_function_parameters<B: TreeBuilder, F: HasParameterCount>(&mut self, context: &mut B, function_info: &mut F) -> B::FormalParameterList {
        let mode = self.source_parse_mode();
        assert!(!SourceParseModeSet::new(&[SourceParseMode::ProgramMode, SourceParseMode::ModuleAnalyzeMode, SourceParseMode::ModuleEvaluateMode]).contains(mode));
        let parameter_list = context.create_formal_parameter_list();
        if mode == SourceParseMode::ClassStaticBlockMode {
            return parameter_list;
        }
        // SetForScope functionParsePhasePoisoner: restaurado depois do corpo.
        let old_function_parse_phase = self.parser_state.function_parse_phase;
        self.parser_state.function_parse_phase = FunctionParsePhase::Parameters;

        let result: Option<B::FormalParameterList> = (|this: &mut Parser<T>| -> Option<B::FormalParameterList> {
            if SourceParseModeSet::new(&[SourceParseMode::ArrowFunctionMode, SourceParseMode::AsyncArrowFunctionMode]).contains(mode) {
                if !this.match_spec_identifier() && !this.match_(OPENPAREN) {
                    semantic_failure_due_to_keyword!(this, string_for_function_mode(mode), " name");
                    fail_with_message!(this, "Expected an arrow function input parameter");
                }

                if this.consume(OPENPAREN) {
                    if this.match_(CLOSEPAREN) {
                        *function_info.parameter_count_mut() = 0;
                    } else {
                        let is_arrow_function = true;
                        let is_method = false;
                        fail_if_false!(this, this.parse_formal_parameters(context, &parameter_list, is_arrow_function, is_method, function_info.parameter_count_mut()), "Cannot parse parameters for this ", string_for_function_mode(mode));
                    }

                    consume_or_fail!(this, CLOSEPAREN, "Expected a ')' or a ',' after a parameter declaration");
                } else {
                    *function_info.parameter_count_mut() = 1;
                    let parameter = this
                        .parse_destructuring_pattern(context, DestructuringKind::DestructureToParameters, ExportType::NotExported, None, None, AssignmentContext::DeclarationStatement, 0)
                        .unwrap_or_default();
                    fail_if_false!(this, parameter != B::DestructuringPattern::default(), "Cannot parse parameter pattern");
                    context.append_parameter(&parameter_list, parameter, Default::default());
                }

                return Some(parameter_list);
            }

            if !this.consume(OPENPAREN) {
                semantic_failure_due_to_keyword!(this, string_for_function_mode(mode), " name");
                fail_with_message!(this, "Expected an opening '(' before a ", string_for_function_mode(mode), "'s parameter list");
            }

            if mode == SourceParseMode::GetterMode {
                consume_or_fail!(this, CLOSEPAREN, "getter functions must have no parameters");
                *function_info.parameter_count_mut() = 0;
            } else if mode == SourceParseMode::SetterMode {
                fail_if_true!(this, this.match_(CLOSEPAREN), "setter functions must have one parameter");
                let mut duplicate_parameter: Option<Identifier> = None;
                let mut has_destructuring_pattern = false;
                let parameter = this
                    .parse_destructuring_pattern(context, DestructuringKind::DestructureToParameters, ExportType::NotExported, Some(&mut duplicate_parameter), Some(&mut has_destructuring_pattern), AssignmentContext::DeclarationStatement, 0)
                    .unwrap_or_default();
                fail_if_false!(this, parameter != B::DestructuringPattern::default(), "setter functions must have one parameter");
                let default_value = this.parse_default_value_for_destructuring_pattern(context);
                propagate_error!(this);
                if default_value.is_some() || has_destructuring_pattern {
                    if let Some(duplicate) = duplicate_parameter.clone() {
                        semantic_fail!(this, "Duplicate parameter '", duplicate, "' not allowed in function with non-simple parameter list");
                    }
                    let current = this.current_scope();
                    this.scope_stack[current].set_has_non_simple_parameter_list();
                }
                let has_default_value = default_value.is_some();
                context.append_parameter(&parameter_list, parameter, default_value.unwrap_or_default());
                *function_info.parameter_count_mut() = if has_default_value { 0 } else { 1 };
                fail_if_true!(this, this.match_(COMMA), "setter functions must have one parameter");
                consume_or_fail!(this, CLOSEPAREN, "Expected a ')' after a parameter declaration");
            } else {
                if this.match_(CLOSEPAREN) {
                    *function_info.parameter_count_mut() = 0;
                } else {
                    let is_arrow_function = false;
                    let is_method = is_method_parse_mode(mode);
                    fail_if_false!(this, this.parse_formal_parameters(context, &parameter_list, is_arrow_function, is_method, function_info.parameter_count_mut()), "Cannot parse parameters for this ", string_for_function_mode(mode));
                }
                consume_or_fail!(this, CLOSEPAREN, "Expected a ')' or a ',' after a parameter declaration");
            }

            Some(parameter_list)
        })(self);

        self.parser_state.function_parse_phase = old_function_parse_phase;
        result.unwrap_or_default()
    }

    /// `template <class TreeBuilder> FormalParameterList createGeneratorParameters(TreeBuilder&, unsigned&)`.
    pub(crate) fn create_generator_parameters<B: TreeBuilder>(&mut self, context: &mut B, parameter_count: &mut u32) -> B::FormalParameterList {
        let parameters = context.create_formal_parameter_list();

        let location = self.token_location();
        let position = *self.token_start_position();

        // `auto addParameter = [&](const Identifier& name)`, chamado nesta ordem:
        // @generator, @generatorState, @generatorValue, @generatorResumeMode, @generatorFrame.
        let names = [
            self.vm.property_names.generator_private_name.clone(),
            self.vm.property_names.generator_state_private_name.clone(),
            self.vm.property_names.generator_value_private_name.clone(),
            self.vm.property_names.generator_resume_mode_private_name.clone(),
            self.vm.property_names.generator_frame_private_name.clone(),
        ];
        for name in &names {
            self.declare_parameter(name);
            let binding = context.create_binding_location(&location, name, position, position, AssignmentContext::DeclarationStatement);
            context.append_parameter(&parameters, binding, Default::default());
            *parameter_count += 1;
        }

        parameters
    }

    /// `template <class TreeBuilder> bool parseFunctionInfo(TreeBuilder&, FunctionNameRequirements, bool, ConstructorKind, SuperBinding, unsigned, ParserFunctionInfo<TreeBuilder>&, FunctionDefinitionType, std::optional<int>)`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn parse_function_info<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        requirements: FunctionNameRequirements,
        name_is_in_containing_scope: bool,
        constructor_kind: ConstructorKind,
        expected_super_binding: SuperBinding,
        function_start: u32,
        function_info: &mut ParserFunctionInfo<B>,
        function_definition_type: FunctionDefinitionType,
        function_constructor_parameters_end_position: Option<i32>,
    ) -> bool {
        let mode = self.source_parse_mode();
        assert!(is_function_parse_mode(mode));

        let parent_scope = self.current_scope();

        let function_name_is_await = self.is_possibly_escaped_await(&self.token);
        let is_disallowed_await_function_name_reason: Option<&'static str> = if function_name_is_await && !self.can_use_identifier_await() { Some(self.disallowed_identifier_await_reason()) } else { None };

        let pushed = self.push_scope();
        let mut function_scope = AutoPopScope::new(pushed);
        let function_scope_ref = function_scope.scope();

        self.scope_stack[function_scope_ref].set_source_parse_mode(mode);
        self.reset_implementation_visibility_if_needed();

        self.scope_stack[function_scope_ref].set_expected_super_binding(expected_super_binding);
        self.scope_stack[function_scope_ref].set_constructor_kind(constructor_kind);

        // SetForScope functionParsePhasePoisoner(m_parserState.functionParsePhase, FunctionParsePhase::Body)
        let old_function_parse_phase = self.parser_state.function_parse_phase;
        self.parser_state.function_parse_phase = FunctionParsePhase::Body;
        let function_name_start = self.token.start_position.offset;
        let last_function_name = self.parser_state.last_function_name.take();

        // Per function, so the SourceProviderCache can replay it for a skipped body (it decides the enclosing code's NoEvalCacheFeature).
        let enclosing_code_contains_tagged_template = std::mem::replace(&mut self.seen_tagged_template_in_non_reparsing_function_mode, false);

        let result: bool = (|this: &mut Parser<T>| -> bool {
            let mut parameters_start: i32 = -1;
            let mut start_location = JSTokenLocation::default();
            let mut start_column: i32 = -1;
            let function_body_type: FunctionBodyType;

            // `auto tryLoadCachedFunction = [&] () -> bool { ... }`, um macro expressão: o `return` do
            // lambda vira `break 'cached`.
            macro_rules! try_load_cached_function {
                () => {
                    'cached: {
                        if !Options::use_source_provider_cache() {
                            break 'cached false;
                        }

                        if this.debugger_parse_data.is_some() {
                            break 'cached false;
                        }

                        // If we know about this function already, we can use the cached info and skip the parser to the end of the function.
                        let cached_info = if B::CAN_USE_FUNCTION_CACHE { this.find_cached_function_info(parameters_start) } else { None };
                        if let Some(cached_info) = cached_info {
                            // If we're in a strict context, the cached function info must say it was strict too.
                            let mut end_location = JSTokenLocation::default();

                            let cached_constructor_kind = cached_info.constructor_kind;
                            let cached_expected_super_binding = cached_info.expected_super_binding;

                            end_location.line = cached_info.last_token_line as i32;
                            end_location.start_offset = cached_info.last_token_start_offset;
                            end_location.line_start_offset = cached_info.last_token_line_start_offset;

                            let end_column_is_on_start_line = end_location.line == function_info.start_line;
                            let current_line_start_offset = this.lexer.current_line_start_offset();
                            let body_end_column = if end_column_is_on_start_line { end_location.start_offset.wrapping_sub(current_line_start_offset as u32) } else { end_location.start_offset.wrapping_sub(end_location.line_start_offset) };

                            let cached_function_body_type = if SourceParseModeSet::new(&[SourceParseMode::ArrowFunctionMode, SourceParseMode::AsyncArrowFunctionMode]).contains(mode) {
                                if cached_info.is_body_arrow_expression { FunctionBodyType::ArrowFunctionBodyExpression } else { FunctionBodyType::ArrowFunctionBodyBlock }
                            } else {
                                FunctionBodyType::StandardFunctionBodyBlock
                            };

                            let function_super_binding = adjust_super_binding_for_base_constructor(cached_constructor_kind, cached_expected_super_binding, mode, cached_info.needs_super_binding, cached_info.uses_eval, cached_info.inner_arrow_function_features);

                            function_info.body = context.create_function_metadata(
                                &start_location,
                                &end_location,
                                start_column as u32,
                                body_end_column,
                                function_start,
                                function_name_start,
                                parameters_start,
                                cached_info.implementation_visibility,
                                cached_info.lexically_scoped_features(),
                                cached_constructor_kind,
                                function_super_binding,
                                cached_info.parameter_count,
                                mode,
                                cached_function_body_type == FunctionBodyType::ArrowFunctionBodyExpression,
                            );
                            function_info.end_offset = cached_info.end_function_offset;
                            function_info.parameter_count = cached_info.parameter_count;
                            this.seen_tagged_template_in_non_reparsing_function_mode = cached_info.contains_tagged_template;

                            this.scope_stack[function_scope_ref].restore_from_source_provider_cache(&cached_info);
                            this.pop_scope_auto(&mut function_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);

                            this.token = cached_info.end_function_token();

                            if end_column_is_on_start_line {
                                this.token.start_position.line_start_offset = current_line_start_offset;
                            }
                            if this.token.end_position.line == function_info.start_line {
                                this.token.end_position.line_start_offset = current_line_start_offset;
                            }

                            // Resume where the last token ended; a template literal ending an expression body spans lines.
                            this.lexer.set_offset(this.token.end_position.offset, this.token.end_position.line_start_offset);
                            this.lexer.set_line_number(this.token.end_position.line);

                            match cached_function_body_type {
                                FunctionBodyType::ArrowFunctionBodyExpression => {
                                    this.next(LexerFlagSet::empty());
                                    context.set_end_offset(&function_info.body, this.lexer.current_offset());
                                }
                                FunctionBodyType::ArrowFunctionBodyBlock | FunctionBodyType::StandardFunctionBodyBlock => {
                                    context.set_end_offset(&function_info.body, this.lexer.current_offset());
                                    this.next(LexerFlagSet::empty());
                                }
                            }
                            function_info.end_line = this.last_token_location.line;
                            break 'cached true;
                        }

                        false
                    }
                };
            }

            let vm = this.vm.clone();
            let mut syntax_checker = SyntaxChecker::new(&vm);

            let old_state: ParserState;
            if SourceParseModeSet::new(&[SourceParseMode::ArrowFunctionMode, SourceParseMode::AsyncArrowFunctionMode]).contains(mode) {
                start_location = this.token_location();
                function_info.start_line = this.token_line();
                start_column = this.token_column();

                parameters_start = this.token.start_position.offset;
                function_info.start_offset = parameters_start as u32;
                function_info.parameters_start_column = start_column as u32;

                if try_load_cached_function!() {
                    return true;
                }

                this.parser_state.last_function_name = last_function_name.clone();
                old_state = this.internal_save_parser_state(context);
                {
                    // Parse formal parameters with [+Yield] parameterization, in order to ban YieldExpressions
                    // in ArrowFormalParameters, per ES6 #sec-arrow-function-definitions-static-semantics-early-errors.
                    // Scope::MaybeParseAsGeneratorFunctionForScope parseAsGeneratorFunction(functionScope.scope(), parentScope->isGeneratorFunction())
                    let old_is_generator_function = this.scope_stack[function_scope_ref].is_generator_function;
                    this.scope_stack[function_scope_ref].is_generator_function = this.scope_stack[parent_scope].is_generator_function();
                    // SetForScope overrideAllowAwait
                    let old_allow_await = this.parser_state.allow_await;
                    this.parser_state.allow_await = !this.scope_stack[parent_scope].is_async_function() && !is_async_function_parse_mode(mode);
                    this.parse_function_parameters(&mut syntax_checker, function_info);
                    propagate_error!(this, @hook {
                        this.parser_state.allow_await = old_allow_await;
                        this.scope_stack[function_scope_ref].is_generator_function = old_is_generator_function;
                    });
                    this.parser_state.allow_await = old_allow_await;
                    this.scope_stack[function_scope_ref].is_generator_function = old_is_generator_function;
                }

                match_or_fail!(this, ARROWFUNCTION, "Expected a '=>' after arrow function parameter declaration");

                if this.lexer.has_line_terminator_before_token() {
                    fail_due_to_unexpected_token!(this);
                }

                // Check if arrow body start with {. If it true it mean that arrow function is Fat arrow function
                // and we need use common approach to parse function body
                this.next(LexerFlagSet::empty());
                function_body_type = if this.match_(OPENBRACE) { FunctionBodyType::ArrowFunctionBodyBlock } else { FunctionBodyType::ArrowFunctionBodyExpression };
            } else {
                // http://ecma-international.org/ecma-262/6.0/#sec-function-definitions
                // FunctionExpression :
                //     function BindingIdentifieropt ( FormalParameters ) { FunctionBody }
                //
                // FunctionDeclaration[Yield, Default] :
                //     function BindingIdentifier[?Yield] ( FormalParameters ) { FunctionBody }
                //     [+Default] function ( FormalParameters ) { FunctionBody }
                //
                // GeneratorDeclaration[Yield, Default] :
                //     function * BindingIdentifier[?Yield] ( FormalParameters[Yield] ) { GeneratorBody }
                //     [+Default] function * ( FormalParameters[Yield] ) { GeneratorBody }
                //
                // GeneratorExpression :
                //     function * BindingIdentifier[Yield]opt ( FormalParameters[Yield] ) { GeneratorBody }
                //
                // The name of FunctionExpression and AsyncFunctionExpression can accept "yield" even in the context of generator.
                let mut can_use_yield = !this.strict_mode();
                if !(function_definition_type == FunctionDefinitionType::Expression && SourceParseModeSet::new(&[SourceParseMode::NormalFunctionMode, SourceParseMode::AsyncFunctionMode]).contains(mode)) {
                    can_use_yield &= !this.scope_stack[parent_scope].is_generator_function();
                }

                if requirements != FunctionNameRequirements::Unnamed {
                    if this.match_spec_identifier_with(can_use_yield, function_name_is_await) {
                        function_info.name = this.token.data.ident.clone();
                        this.parser_state.last_function_name = function_info.name.clone();
                        if let Some(reason) = is_disallowed_await_function_name_reason {
                            semantic_fail_if_true!(this, function_definition_type == FunctionDefinitionType::Declaration || is_async_function_or_async_generator_wrapper_parse_mode(mode), "Cannot declare function named 'await' ", reason);
                        } else if is_async_function_or_async_generator_wrapper_parse_mode(mode) && this.match_(AWAIT) && function_definition_type == FunctionDefinitionType::Expression {
                            semantic_fail!(this, "Cannot declare ", string_for_function_mode(mode), " named 'await'");
                        } else if is_generator_or_async_generator_wrapper_parse_mode(mode) && this.match_(YIELD) && function_definition_type == FunctionDefinitionType::Expression {
                            semantic_fail!(this, "Cannot declare ", string_for_function_mode(mode), " named 'yield'");
                        }
                        this.next(LexerFlagSet::empty());
                        if !name_is_in_containing_scope {
                            if let Some(name) = function_info.name.clone() {
                                fail_if_true_if_strict!(this, (this.scope_stack[function_scope_ref].declare_callee(&name) & DeclarationResult::INVALID_STRICT_MODE) != 0, "'", name, "' is not a valid ", string_for_function_mode(mode), " name in strict mode");
                            }
                        }
                    } else if requirements == FunctionNameRequirements::Named {
                        if this.match_(OPENPAREN) {
                            semantic_fail_if_true!(this, mode == SourceParseMode::NormalFunctionMode, "Function statements must have a name");
                            semantic_fail_if_true!(this, mode == SourceParseMode::AsyncFunctionMode, "Async function statements must have a name");
                        }
                        semantic_failure_due_to_keyword!(this, string_for_function_mode(mode), " name");
                        fail_due_to_unexpected_token!(this);
                    }
                }

                start_location = this.token_location();
                function_info.start_line = this.token_line();
                start_column = this.token_column();
                function_info.parameters_start_column = start_column as u32;

                parameters_start = this.token.start_position.offset;
                function_info.start_offset = parameters_start as u32;

                if try_load_cached_function!() {
                    return true;
                }

                this.parser_state.last_function_name = last_function_name.clone();
                old_state = this.internal_save_parser_state(context);
                {
                    // SetForScope overrideAllowAwait
                    let old_allow_await = this.parser_state.allow_await;
                    this.parser_state.allow_await = !is_async_function_parse_mode(mode);
                    this.parse_function_parameters(&mut syntax_checker, function_info);
                    propagate_error!(this, @hook { this.parser_state.allow_await = old_allow_await; });
                    this.parser_state.allow_await = old_allow_await;
                }

                match_or_fail!(this, OPENBRACE, "Expected an opening '{' at the start of a ", string_for_function_mode(mode), " body");

                // If the code is invoked from function constructor, we need to ensure that parameters are only composed by the string offered as parameters.
                if let Some(end_position) = function_constructor_parameters_end_position {
                    semantic_fail_if_false!(this, this.last_token_end_position().offset == end_position, "Parameters should match arguments offered as parameters in Function constructor");
                }

                // BytecodeGenerator emits code to throw TypeError when a class constructor is "call"ed.
                // Set ConstructorKind to None for non-constructor methods of classes.

                function_body_type = FunctionBodyType::StandardFunctionBodyBlock;
            }

            // FIXME: https://bugs.webkit.org/show_bug.cgi?id=156962
            // This loop collects the set of capture candidates that aren't
            // part of the set of this function's declared parameters. We will
            // figure out which parameters are captured for this function when
            // we actually generate code for it. For now, we just propagate to
            // our parent scopes which variables we might have closed over that
            // belong to them. This is necessary for correctness when using
            // the source provider cache because we can't close over a variable
            // that we don't claim to close over. The source provider cache must
            // know this information to properly cache this function.
            // This might work itself out nicer if we declared a different
            // Scope struct for the parameters (because they are indeed implemented
            // as their own scope).
            let mut non_local_captures_from_parameter_expressions = UniquedStringImplPtrSet::default();
            // `forEachUsedVariable` muta o escopo pai, que vive na mesma pilha que o escopo da função:
            // as variáveis usadas são coletadas primeiro (mesma ordem) e depois percorridas.
            let mut used_variables: Vec<UniquedKey> = Vec::new();
            this.scope_stack[function_scope_ref].for_each_used_variable(|impl_| {
                used_variables.push(impl_.clone());
                IterationStatus::Continue
            });
            for impl_ in &used_variables {
                if !this.scope_stack[function_scope_ref].has_declared_parameter(impl_) {
                    non_local_captures_from_parameter_expressions.add(impl_, ());
                    if B::NEEDS_FREE_VARIABLE_INFO {
                        this.scope_stack[parent_scope].add_closed_variable_candidate_unconditionally(impl_);
                    }
                }
            }

            // `auto performParsingFunctionBody = [&] { ... }`
            macro_rules! perform_parsing_function_body {
                () => {
                    this.parse_function_body(context, &mut syntax_checker, &start_location, start_column, function_start, function_name_start, parameters_start, constructor_kind, expected_super_binding, function_body_type, function_info.parameter_count)
                };
            }

            let mut implementation_visibility = this.implementation_visibility();
            if is_generator_or_async_function_wrapper_parse_mode(mode) {
                let pushed = this.push_scope();
                let mut generator_body_scope = AutoPopScope::new(pushed);
                let generator_body_scope_ref = generator_body_scope.scope();
                let inner_parse_mode = if is_async_function_or_async_generator_wrapper_parse_mode(mode) { get_async_function_body_parse_mode(mode) } else { SourceParseMode::GeneratorBodyMode };

                this.scope_stack[generator_body_scope_ref].set_source_parse_mode(inner_parse_mode);
                this.reset_implementation_visibility_if_needed();

                this.scope_stack[generator_body_scope_ref].set_constructor_kind(ConstructorKind::None);
                this.scope_stack[generator_body_scope_ref].set_expected_super_binding(expected_super_binding);

                // Disallow 'use strict' directives in the implicit inner function if
                // needed.
                if this.scope_stack[function_scope_ref].has_non_simple_parameter_list() {
                    this.scope_stack[generator_body_scope_ref].set_has_non_simple_parameter_list();
                }

                function_info.body = perform_parsing_function_body!();

                // When a generator has a "use strict" directive, a generator function wrapping it should be strict mode.
                if this.scope_stack[generator_body_scope_ref].strict_mode() {
                    this.scope_stack[function_scope_ref].set_strict_mode();
                }

                implementation_visibility = this.scope_stack[generator_body_scope_ref].implementation_visibility();
                this.pop_scope_auto(&mut generator_body_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);
                generator_body_scope.cleanup(this);
            } else {
                function_info.body = perform_parsing_function_body!();
            }

            this.restore_parser_state(context, &old_state);
            fail_if_false!(this, function_info.body != B::FunctionBody::default(), "Cannot parse the body of this ", string_for_function_mode(mode));
            context.set_end_offset(&function_info.body, this.lexer.current_offset());
            if this.scope_stack[function_scope_ref].strict_mode() && requirements != FunctionNameRequirements::Unnamed {
                assert!(
                    SourceParseModeSet::new(&[
                        SourceParseMode::NormalFunctionMode,
                        SourceParseMode::MethodMode,
                        SourceParseMode::ArrowFunctionMode,
                        SourceParseMode::GeneratorBodyMode,
                        SourceParseMode::GeneratorWrapperFunctionMode,
                        SourceParseMode::ClassStaticBlockMode,
                    ])
                    .contains(mode)
                        || is_async_function_or_async_generator_wrapper_parse_mode(mode)
                );
                if let Some(name) = function_info.name.clone() {
                    semantic_fail_if_true!(this, this.vm.property_names.arguments == name, "'", name, "' is not a valid function name in strict mode");
                    semantic_fail_if_true!(this, this.vm.property_names.eval == name, "'", name, "' is not a valid function name in strict mode");
                    semantic_fail_if_true!(this, this.vm.property_names.yield_keyword == name, "'", name, "' is not a valid function name in strict mode");
                }
            }

            let mut location = this.token.location();
            let mut last_token_end_position = this.token.end_position;
            function_info.end_offset = this.token.data.offset;

            if function_body_type == FunctionBodyType::ArrowFunctionBodyExpression {
                location = this.location_before_last_token();
                last_token_end_position = this.last_token_end_position;
                function_info.end_offset = location.end_offset;
            } else {
                this.record_function_entry_location(JSTextPosition::new(start_location.line, start_location.start_offset as i32, start_location.line_start_offset as i32));
                this.record_function_leave_location(JSTextPosition::new(location.line, location.start_offset as i32, location.line_start_offset as i32));
            }

            // Cache the tokenizer state and the function scope the first time the function is parsed.
            // Any future reparsing can then skip the function.
            // For arrow function is 8 = x=>x + 4 symbols;
            // For ordinary function is 16  = function(){} + 4 symbols
            let minimum_source_length_to_cache: i32 = if function_body_type == FunctionBodyType::StandardFunctionBodyBlock { 16 } else { 8 };
            let mut new_info: Option<Rc<SourceProviderCacheItem>> = None;
            let source_length = function_info.end_offset as i32 - function_info.start_offset as i32;
            let mut parameters = SourceProviderCacheItemCreationParameters::default();
            let mut has_precomputed_free_variables = false;
            if B::CAN_USE_FUNCTION_CACHE && this.function_cache.is_some() && source_length > minimum_source_length_to_cache {
                parameters.end_function_offset = function_info.end_offset;
                parameters.last_token_line = location.line as u32;
                parameters.last_token_start_offset = location.start_offset;
                parameters.last_token_end_offset = location.end_offset;
                parameters.last_token_line_start_offset = location.line_start_offset;
                parameters.last_token_end_line = last_token_end_position.line as u32;
                parameters.last_token_end_line_start_offset = last_token_end_position.line_start_offset as u32;
                parameters.parameter_count = function_info.parameter_count;
                parameters.constructor_kind = constructor_kind;
                parameters.expected_super_binding = expected_super_binding;
                parameters.implementation_visibility = implementation_visibility;
                parameters.contains_tagged_template = this.seen_tagged_template_in_non_reparsing_function_mode;
                if function_body_type == FunctionBodyType::ArrowFunctionBodyExpression {
                    parameters.is_body_arrow_expression = true;
                    parameters.token_type = this.token.type_;
                }
                this.scope_stack[function_scope_ref].fill_parameters_for_source_provider_cache(&mut parameters, &non_local_captures_from_parameter_expressions);
                has_precomputed_free_variables = true;
                new_info = Some(SourceProviderCacheItem::create(&parameters));
            }

            this.pop_scope_auto(&mut function_scope, B::NEEDS_FREE_VARIABLE_INFO, has_precomputed_free_variables, parameters.free_variables());

            if function_body_type != FunctionBodyType::ArrowFunctionBodyExpression {
                consume_or_fail!(this, CLOSEBRACE, "Expected a closing '}' after a ", string_for_function_mode(mode), " body");
            } else {
                // We need to lex the last token again because the last token is lexed under the different context because of the following possibilities.
                // 1. which may have different strict mode.
                // 2. which may not build strings for tokens.
                // But (1) is not possible because we do not recognize the string literal in ArrowFunctionBodyExpression as directive and this is correct in terms of the spec (`value => "use strict"`).
                // So we only check TreeBuilder's type here.
                // `if constexpr (!std::is_same_v<TreeBuilder, SyntaxChecker>)`: só o `SyntaxChecker` não cria AST.
                if B::CREATES_AST {
                    this.lex_current_token_again_under_current_context(context);
                }
            }

            if let Some(new_info) = new_info {
                if let Some(function_cache) = &this.function_cache {
                    function_cache.borrow_mut().add(function_info.start_offset as i32, new_info);
                }
            }

            function_info.end_line = this.last_token_location.line;
            true
        })(self);

        // Destrutores, na ordem inversa de construção: `propagateContainsTaggedTemplate`,
        // `functionParsePhasePoisoner` e `functionScope`.
        self.seen_tagged_template_in_non_reparsing_function_mode |= enclosing_code_contains_tagged_template;
        self.parser_state.function_parse_phase = old_function_parse_phase;
        function_scope.cleanup(self);
        result
    }
}
