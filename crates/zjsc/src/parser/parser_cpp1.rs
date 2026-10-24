// Primeira fatia de `parser/Parser.cpp` (linhas 1 a 720), incluída por `include!` em `parser.rs`.
// Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Convenções desta fatia (valem para as próximas fatias de `Parser.cpp`):
//
// - `return 0` das macros de erro vira `return Default::default()`: `false` num método que devolve
//   `bool`, `None` num que devolve `Option<_>`. Todo `TreeStatement`/`TreeExpression`/... que o C++
//   devolve (nulo = falha) vira `Option<B::Statement>`/`Option<B::Expression>`/...; um
//   `TreeStatement statement = 0` vira `None`.
// - As macros do `.cpp` recebem o parser como primeiro argumento (`macro_rules!` não enxerga `self`).
//   Quando o C++ tem um destrutor RAII vivo no ponto da falha (`AutoPopScope`, `SetForScope`), a
//   macro aceita `@hook { ... },` logo depois do parser: o bloco roda depois de montar a mensagem de
//   erro e antes do `return`, na ordem inversa de construção, como o destrutor rodaria.
// - O texto da mensagem é montado como no C++: `StringPrintStream::print(args..., ".")`, com o
//   `printUnexpectedTokenText` e ". " na frente quando `shouldPrintToken`. Cada argumento
//   implementa `ParserPrintArg`.
// - `SetForScope` vira salvar o valor antigo e restaurá-lo em cada saída (inclusive por erro).
// - `JSToken::dump` (`out.print(*m_data.cooked)`) só serve a `dump` de depuração e não se porta.
// - `std::atomic<unsigned> globalParseCount` vira `GLOBAL_PARSE_COUNT`.
// - `Scope::MaybeParseAsGeneratorFunctionForScope` e `DepthManager` (linhas 175 a 200) não são
//   usados em nenhum ponto de `Parser.cpp`; ver o relatório (dependem de `Scope::m_isGeneratorFunction`
//   privado, a classe fica em `parser_cpp2` junto do primeiro uso).


/// Argumento de `logError(shouldPrintToken, args...)`: o que o `PrintStream::print` do C++ aceita
/// neste arquivo (literais, `String`, `Identifier`, `UniquedStringImpl*`, inteiros).
pub trait ParserPrintArg {
    fn print_arg(&self, out: &mut StringBuilder);
}

impl ParserPrintArg for str {
    fn print_arg(&self, out: &mut StringBuilder) {
        out.append_ascii_literal(self);
    }
}

impl<A: ParserPrintArg + ?Sized> ParserPrintArg for &A {
    fn print_arg(&self, out: &mut StringBuilder) {
        (**self).print_arg(out);
    }
}

impl ParserPrintArg for WtfString {
    fn print_arg(&self, out: &mut StringBuilder) {
        out.append_string(self);
    }
}

impl ParserPrintArg for Identifier {
    fn print_arg(&self, out: &mut StringBuilder) {
        out.append_atom_string(self.string());
    }
}

impl ParserPrintArg for UniquedKey {
    fn print_arg(&self, out: &mut StringBuilder) {
        out.append_string(&WtfString::from(self.0.clone()));
    }
}

impl ParserPrintArg for i32 {
    fn print_arg(&self, out: &mut StringBuilder) {
        out.append_number_i32(*self);
    }
}

impl ParserPrintArg for u32 {
    fn print_arg(&self, out: &mut StringBuilder) {
        out.append_number_u32(*self);
    }
}

// ---------------------------------------------------------------------------------------------
// Macros do topo de `Parser.cpp` (linhas 39 a 85)
// ---------------------------------------------------------------------------------------------

/// `logError(shouldPrintToken, args...)` (variádico). Montagem idêntica à do C++.
macro_rules! log_error {
    ($p:expr, $should_print_token:expr, $($arg:expr),+ $(,)?) => {{
        if !$p.has_error() {
            let mut out = StringBuilder::new();
            if $should_print_token {
                $p.print_unexpected_token_text(&mut out);
                out.append_ascii_literal(". ");
            }
            $( ParserPrintArg::print_arg(&$arg, &mut out); )+
            out.append_ascii_literal(".");
            let message = out.to_string().clone();
            $p.set_error_message(&message);
        }
    }};
}

/// `#define propagateError()`, com o gancho dos destrutores vivos.
macro_rules! propagate_error {
    ($p:expr) => { propagate_error!($p, @hook {}) };
    ($p:expr, @hook $h:block) => {
        if $p.has_error() {
            $h
            return Default::default();
        }
    };
}

/// `#define updateErrorMessage(shouldPrintToken, ...)`.
macro_rules! update_error_message {
    ($p:expr, @hook $h:block, $should_print_token:expr, $($arg:expr),+ $(,)?) => {{
        propagate_error!($p, @hook $h);
        log_error!($p, $should_print_token, $($arg),+);
    }};
    ($p:expr, $should_print_token:expr, $($arg:expr),+ $(,)?) => {
        update_error_message!($p, @hook {}, $should_print_token, $($arg),+)
    };
}

/// `#define internalFailWithMessage(shouldPrintToken, ...)`.
macro_rules! internal_fail_with_message {
    ($p:expr, @hook $h:block, $should_print_token:expr, $($arg:expr),+ $(,)?) => {{
        update_error_message!($p, @hook $h, $should_print_token, $($arg),+);
        $h
        return Default::default();
    }};
    ($p:expr, $should_print_token:expr, $($arg:expr),+ $(,)?) => {
        internal_fail_with_message!($p, @hook {}, $should_print_token, $($arg),+)
    };
}

/// `#define failDueToUnexpectedToken()`.
macro_rules! fail_due_to_unexpected_token {
    ($p:expr) => { fail_due_to_unexpected_token!($p, @hook {}) };
    ($p:expr, @hook $h:block) => {{
        log_error_unexpected_token!($p);
        $h
        return Default::default();
    }};
}

/// `logError(bool)` sem argumentos: só o texto do token inesperado.
macro_rules! log_error_unexpected_token {
    ($p:expr) => {{
        if !$p.has_error() {
            let mut out = StringBuilder::new();
            $p.print_unexpected_token_text(&mut out);
            let message = out.to_string().clone();
            $p.set_error_message(&message);
        }
    }};
}

/// `#define handleErrorToken()`.
macro_rules! handle_error_token {
    ($p:expr) => { handle_error_token!($p, @hook {}) };
    ($p:expr, @hook $h:block) => {
        if $p.token.type_ == EOFTOK || ($p.token.type_ & CAN_BE_ERROR_TOKEN_FLAG) != 0 {
            fail_due_to_unexpected_token!($p, @hook $h);
        }
    };
}

/// `#define failWithMessage(...)`.
macro_rules! fail_with_message {
    ($p:expr, @hook $h:block, $($arg:expr),+ $(,)?) => {{
        handle_error_token!($p, @hook $h);
        update_error_message!($p, @hook $h, true, $($arg),+);
        $h
        return Default::default();
    }};
    ($p:expr, $($arg:expr),+ $(,)?) => { fail_with_message!($p, @hook {}, $($arg),+) };
}

/// `#define failWithStackOverflow()`.
macro_rules! fail_with_stack_overflow {
    ($p:expr) => {{
        update_error_message!($p, false, "Stack exhausted");
        $p.has_stack_overflow = true;
        return Default::default();
    }};
}

/// `#define failIfFalse(cond, ...)`.
macro_rules! fail_if_false {
    ($p:expr, @hook $h:block, $cond:expr, $($arg:expr),+ $(,)?) => {
        if !($cond) {
            handle_error_token!($p, @hook $h);
            internal_fail_with_message!($p, @hook $h, true, $($arg),+);
        }
    };
    ($p:expr, $cond:expr, $($arg:expr),+ $(,)?) => {
        fail_if_false!($p, @hook {}, $cond, $($arg),+)
    };
}

/// `#define failIfTrue(cond, ...)`.
macro_rules! fail_if_true {
    ($p:expr, @hook $h:block, $cond:expr, $($arg:expr),+ $(,)?) => {
        if $cond {
            handle_error_token!($p, @hook $h);
            internal_fail_with_message!($p, @hook $h, true, $($arg),+);
        }
    };
    ($p:expr, $cond:expr, $($arg:expr),+ $(,)?) => {
        fail_if_true!($p, @hook {}, $cond, $($arg),+)
    };
}

/// `#define failIfTrueIfStrict(cond, ...)`.
macro_rules! fail_if_true_if_strict {
    ($p:expr, $cond:expr, $($arg:expr),+ $(,)?) => {
        if ($cond) && $p.strict_mode() {
            internal_fail_with_message!($p, false, $($arg),+);
        }
    };
}

/// `#define failIfFalseIfStrict(cond, ...)`.
macro_rules! fail_if_false_if_strict {
    ($p:expr, $cond:expr, $($arg:expr),+ $(,)?) => {
        if !($cond) && $p.strict_mode() {
            internal_fail_with_message!($p, false, $($arg),+);
        }
    };
}

/// `#define consumeOrFail(tokenType, ...)`.
macro_rules! consume_or_fail {
    ($p:expr, @hook $h:block, $token_type:expr, $($arg:expr),+ $(,)?) => {
        if !$p.consume($token_type) {
            handle_error_token!($p, @hook $h);
            internal_fail_with_message!($p, @hook $h, true, $($arg),+);
        }
    };
    ($p:expr, $token_type:expr, $($arg:expr),+ $(,)?) => {
        consume_or_fail!($p, @hook {}, $token_type, $($arg),+)
    };
}

/// `#define consumeOrFailWithFlags(tokenType, flags, ...)`.
macro_rules! consume_or_fail_with_flags {
    ($p:expr, $token_type:expr, $flags:expr, $($arg:expr),+ $(,)?) => {
        if !$p.consume_with_flags($token_type, $flags) {
            handle_error_token!($p);
            internal_fail_with_message!($p, true, $($arg),+);
        }
    };
}

/// `#define matchOrFail(tokenType, ...)`.
macro_rules! match_or_fail {
    ($p:expr, $token_type:expr, $($arg:expr),+ $(,)?) => {
        if !$p.match_($token_type) {
            handle_error_token!($p);
            internal_fail_with_message!($p, true, $($arg),+);
        }
    };
}

/// `#define failIfStackOverflow()`.
macro_rules! fail_if_stack_overflow {
    ($p:expr) => {
        fail_if_stack_overflow!($p, crate::runtime::vm::stack_cost::PARSER_LEVEL);
    };
    ($p:expr, $cost:expr) => {
        // O frame lógico fica reservado até o fim do bloco que expandiu a macro (o nível de recursão inteiro).
        let _logical_stack_frame = match $p.vm.enter_logical_frame($cost) {
            Some(frame) => frame,
            None => fail_with_stack_overflow!($p),
        };
    };
}

/// `#define semanticFail(...)`.
macro_rules! semantic_fail {
    ($p:expr, $($arg:expr),+ $(,)?) => {{ internal_fail_with_message!($p, false, $($arg),+); }};
}

/// `#define semanticFailIfTrue(cond, ...)`.
macro_rules! semantic_fail_if_true {
    ($p:expr, $cond:expr, $($arg:expr),+ $(,)?) => {
        if $cond {
            internal_fail_with_message!($p, false, $($arg),+);
        }
    };
}

/// `#define semanticFailIfFalse(cond, ...)`.
macro_rules! semantic_fail_if_false {
    ($p:expr, $cond:expr, $($arg:expr),+ $(,)?) => {
        if !($cond) {
            internal_fail_with_message!($p, false, $($arg),+);
        }
    };
}

/// `#define regexFail(failure)`.
macro_rules! regex_fail {
    ($p:expr, $failure:expr) => {{
        $p.set_error_message($failure);
        return Default::default();
    }};
}

/// `#define handleProductionOrFail(token, tokenString, operation, production)`.
macro_rules! handle_production_or_fail {
    ($p:expr, $token:expr, $token_string:expr, $operation:expr, $production:expr) => {
        consume_or_fail!($p, $token, "Expected '", $token_string, "' to ", $operation, " a ", $production);
    };
}

/// `#define handleProductionOrFail2(token, tokenString, operation, production)`.
macro_rules! handle_production_or_fail2 {
    ($p:expr, $token:expr, $token_string:expr, $operation:expr, $production:expr) => {
        consume_or_fail!($p, $token, "Expected '", $token_string, "' to ", $operation, " an ", $production);
    };
}

/// `#define semanticFailureDueToKeywordCheckingToken(token, ...)`. `$token` é uma expressão que
/// produz um `JSToken`; o C++ lê `token.m_type` e `getToken(token)`.
macro_rules! semantic_failure_due_to_keyword_checking_token {
    ($p:expr, $token:expr, $($arg:expr),+ $(,)?) => {{
        let keyword_token: JSToken = ($token).clone();
        semantic_fail_if_true!($p, $p.strict_mode() && keyword_token.type_ == RESERVED_IF_STRICT, "Cannot use the reserved word '", $p.get_token_for(&keyword_token), "' as a ", $($arg,)+ " in strict mode");
        semantic_fail_if_true!($p, keyword_token.type_ == RESERVED || keyword_token.type_ == RESERVED_IF_STRICT, "Cannot use the reserved word '", $p.get_token_for(&keyword_token), "' as a ", $($arg),+);
        if (keyword_token.type_ & KEYWORD_TOKEN_FLAG) != 0 {
            semantic_fail_if_false!($p, is_contextual_keyword(&keyword_token), "Cannot use the keyword '", $p.get_token_for(&keyword_token), "' as a ", $($arg),+);
            semantic_fail_if_true!($p, keyword_token.type_ == LET && $p.strict_mode(), "Cannot use 'let' as a ", $($arg,)+ " ", $p.disallowed_identifier_let_reason());
            semantic_fail_if_true!($p, keyword_token.type_ == AWAIT && !$p.can_use_identifier_await(), "Cannot use 'await' as a ", $($arg,)+ " ", $p.disallowed_identifier_await_reason());
            semantic_fail_if_true!($p, keyword_token.type_ == YIELD && !$p.can_use_identifier_yield(), "Cannot use 'yield' as a ", $($arg,)+ " ", $p.disallowed_identifier_yield_reason());
        }
    }};
}

/// `#define semanticFailureDueToKeyword(...)`: o mesmo, sobre `m_token`.
macro_rules! semantic_failure_due_to_keyword {
    ($p:expr, $($arg:expr),+ $(,)?) => {
        semantic_failure_due_to_keyword_checking_token!($p, $p.token, $($arg),+)
    };
}

// ---------------------------------------------------------------------------------------------
// `namespace JSC` de `Parser.cpp`
// ---------------------------------------------------------------------------------------------

/// `ALWAYS_INLINE static SourceParseMode getAsyncFunctionBodyParseMode(SourceParseMode)`.
#[inline(always)]
fn get_async_function_body_parse_mode(parse_mode: SourceParseMode) -> SourceParseMode {
    if is_async_generator_wrapper_parse_mode(parse_mode) {
        return SourceParseMode::AsyncGeneratorBodyMode;
    }

    if parse_mode == SourceParseMode::AsyncArrowFunctionMode {
        return SourceParseMode::AsyncArrowFunctionBodyMode;
    }

    SourceParseMode::AsyncFunctionBodyMode
}

/// `static ALWAYS_INLINE bool isPrivateFieldName(UniquedStringImpl*)`.
#[inline(always)]
fn is_private_field_name(uid: &UniquedKey) -> bool {
    uid.0.length() != 0 && uid.0.char_at(0) == u16::from(b'#')
}

impl<T: CharType> Parser<T> {
    /// `Parser<LexerType>::logError(bool)`, para as chamadas fora de macro.
    pub(crate) fn log_error_unexpected_token(&mut self) {
        log_error_unexpected_token!(self);
    }

    /// `Parser::Parser(...)`. O `Scope*` devolvido por `pushScope()` é o índice na pilha.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        vm: Rc<VM>,
        source: &SourceCode,
        implementation_visibility: ImplementationVisibility,
        builtin_mode: JSParserBuiltinMode,
        lexically_scoped_features: LexicallyScopedFeatures,
        script_mode: JSParserScriptMode,
        parse_mode: SourceParseMode,
        function_mode: FunctionMode,
        super_binding: SuperBinding,
        constructor_kind: ConstructorKind,
        derived_context_type: DerivedContextType,
        is_eval_context: bool,
        eval_context_type: EvalContextType,
        debugger_parse_data: Option<Rc<RefCell<DebuggerParseData>>>,
        is_inside_ordinary_function: bool,
    ) -> Parser<T> {
        let function_cache = source.provider().map(|provider| vm.add_source_provider_cache(provider));
        let mut parser = Parser {
            vm: vm.clone(),
            token: JSToken::default(),
            source: source.clone(),
            parser_arena: ParserArena::new(),
            lexer: Box::new(Lexer::new(vm, builtin_mode, script_mode)),
            last_token_location: JSTokenLocation::default(),
            current_scope: None,
            error_message: WtfString::default(),
            debugger_parse_data,
            last_token_type: ERRORTOK,
            statement_depth: 0,
            function_mode,
            allows_in: true,
            immediate_parent_allows_function_declaration_in_statement: false,
            implementation_visibility,
            inside_switch_case_body: false,
            parser_state: ParserState::default(),
            parse_mode,
            constructor_kind_for_top_level_function_expressions: ConstructorKind::None,
            is_inside_ordinary_function,
            seen_tagged_template_in_non_reparsing_function_mode: false,
            seen_private_name_use_in_non_reparsing_function_mode: false,
            seen_arguments_dot_length: false,
            parsing_builtin: builtin_mode == JSParserBuiltinMode::Builtin,
            is_eval_context: false,
            last_token_end_position: JSTextPosition::default(),
            function_cache,
            call_or_apply_depth_scopes: Vec::new(),
            module_scope_data: None,
            script_mode,
            super_binding,
            has_stack_overflow: false,
            scope_stack: ScopeStack::new(),
        };
        parser.lexer.set_code(&parser.source, &mut parser.parser_arena);
        parser.token.start_position.line = source.first_line().one_based_int();
        parser.token.start_position.offset = source.start_offset() as i32;
        parser.token.start_position.line_start_offset = source.start_offset() as i32;
        parser.token.end_position.offset = source.start_offset() as i32;

        let scope = parser.push_scope();
        parser.scope_stack[scope].set_lexically_scoped_features(lexically_scoped_features);
        parser.scope_stack[scope].set_source_parse_mode(parse_mode);
        parser.scope_stack[scope].set_is_eval_context(is_eval_context);
        if is_eval_context {
            parser.scope_stack[scope].set_eval_context_type(eval_context_type);
        }

        if parser.scope_stack[scope].is_function() {
            parser.scope_stack[scope].set_constructor_kind(constructor_kind);
        } else {
            debug_assert!(constructor_kind == ConstructorKind::None);
        }

        parser.scope_stack[scope].set_derived_context_type(derived_context_type);
        if derived_context_type != DerivedContextType::None {
            parser.scope_stack[scope].set_expected_super_binding(SuperBinding::Needed);
        }

        if is_module_parse_mode(parse_mode) {
            parser.module_scope_data = Some(ModuleScopeData::create());
        }

        parser.next(LexerFlagSet::empty());
        parser
    }

    /// `Parser<LexerType>::parseInner`. O `ASTBuilder` é concreto aqui, como no C++.
    pub(crate) fn parse_inner(
        &mut self,
        callee_name: &Identifier,
        parsing_context: ParsingContext,
        function_constructor_parameters_end_position: Option<i32>,
        class_element_definitions: Option<&FixedVector<ClassElementDefinition>>,
        parent_scope_private_names: Option<&PrivateNameEnvironment>,
    ) -> Result<ParseInnerResult, WtfString> {
        let mut context = ASTBuilder::new(self.vm.clone(), &mut self.parser_arena, &self.source);
        let parse_mode = self.source_parse_mode();
        let scope = self.current_scope();
        self.scope_stack[scope].set_is_lexical_scope();

        let has_private_names = self.scope_stack[scope].is_eval_context()
            && parent_scope_private_names.is_some_and(|names| names.len() != 0);

        if has_private_names {
            self.scope_stack[scope].set_is_private_name_scope();
            self.scope_stack[scope].lexical_variables().add_private_names_from(parent_scope_private_names);
        }

        // SetForScope functionParsePhasePoisoner: restaurado em cada saída.
        let old_function_parse_phase = self.parser_state.function_parse_phase;
        self.parser_state.function_parse_phase = FunctionParsePhase::Body;
        macro_rules! restore_function_parse_phase {
            () => {
                self.parser_state.function_parse_phase = old_function_parse_phase;
            };
        }

        let mut parameters = Link::<FunctionParameters>::default();
        let mut is_arrow_function_body_expression = parse_mode == SourceParseMode::AsyncArrowFunctionBodyMode && !self.match_(OPENBRACE);
        if self.lexer.is_reparsing_function() {
            let mut function_info = ParserFunctionInfo::<ASTBuilder>::default();
            if is_generator_or_async_function_body_parse_mode(parse_mode) {
                parameters = self.create_generator_parameters(&mut context, &mut function_info.parameter_count);
            } else if parse_mode == SourceParseMode::ClassFieldInitializerMode {
                parameters = context.create_formal_parameter_list();
            } else {
                parameters = self.parse_function_parameters(&mut context, &mut function_info);
            }

            if SourceParseModeSet::new(&[SourceParseMode::ArrowFunctionMode, SourceParseMode::AsyncArrowFunctionMode]).contains(parse_mode) && !self.has_error() {
                // FIXME:
                // Logically, this should be an assert, since we already successfully parsed the arrow
                // function when syntax checking. So logically, we should see the arrow token here.
                // But we're seeing crashes in the wild when making this an assert. Instead, we'll just
                // handle it as an error in release builds, and an assert on debug builds, with the hopes
                // of fixing it in the future.
                // https://bugs.webkit.org/show_bug.cgi?id=221633
                if !self.match_(ARROWFUNCTION) {
                    restore_function_parse_phase!();
                    return Err(WtfString::from_utf8(b"Parser error"));
                }
                self.next(LexerFlagSet::empty());
                is_arrow_function_body_expression = !self.match_(OPENBRACE);
            }
        }

        if function_name_is_in_scope(callee_name, self.function_mode()) {
            self.scope_stack[scope].declare_callee(callee_name);
        }

        if self.lexer.is_reparsing_function() {
            self.statement_depth -= 1;
        }

        let mut source_elements = None;
        // The only way we can error this early is if we reparse a function and we run out of stack space.
        if !self.has_error() {
            if is_async_function_wrapper_parse_mode(parse_mode) {
                source_elements = self.parse_async_function_source_elements(&mut context, callee_name, is_arrow_function_body_expression, SourceElementsMode::CheckForStrictMode);
            } else if is_arrow_function_body_expression {
                source_elements = self.parse_arrow_function_single_expression_body_source_elements(&mut context);
            } else if is_module_parse_mode(parse_mode) {
                source_elements = self.parse_module_source_elements(&mut context);
            } else if is_generator_wrapper_parse_mode(parse_mode) {
                source_elements = self.parse_generator_function_source_elements(&mut context, callee_name, SourceElementsMode::CheckForStrictMode);
            } else if is_async_generator_wrapper_parse_mode(parse_mode) {
                source_elements = self.parse_async_generator_function_source_elements(&mut context, callee_name, is_arrow_function_body_expression, SourceElementsMode::CheckForStrictMode);
            } else if parsing_context == ParsingContext::FunctionConstructor {
                source_elements = self.parse_single_function(&mut context, function_constructor_parameters_end_position);
            } else if parse_mode == SourceParseMode::ClassFieldInitializerMode {
                debug_assert!(class_element_definitions.is_some_and(|definitions| !definitions.is_empty()));
                source_elements = match class_element_definitions {
                    Some(definitions) => self.parse_class_field_initializer_source_elements(&mut context, definitions),
                    None => None,
                };
            } else {
                source_elements = self.parse_source_elements(&mut context, SourceElementsMode::CheckForStrictMode);
            }
        }

        let valid_ending = self.consume(EOFTOK);
        let source_elements = match source_elements {
            Some(source_elements) if valid_ending => source_elements,
            _ => {
                restore_function_parse_phase!();
                return Err(if self.has_error() { self.error_message.clone() } else { WtfString::from_utf8(b"Parser error") });
            }
        };

        if !self.lexer.is_reparsing_function() && self.seen_private_name_use_in_non_reparsing_function_mode {
            let mut error_message = WtfString::default();
            self.scope_stack[scope].for_each_used_variable(|impl_| {
                if !is_private_field_name(impl_) {
                    return IterationStatus::Continue;
                }
                if parent_scope_private_names.is_some_and(|names| names.contains(impl_)) {
                    return IterationStatus::Continue;
                }
                if self.scope_stack[scope].has_lexically_declared_variable(impl_) {
                    return IterationStatus::Continue;
                }
                let mut out = StringBuilder::new();
                out.append_ascii_literal("Cannot reference undeclared private names: \"");
                out.append_string(&WtfString::from(impl_.0.clone()));
                out.append_latin1_character(b'"');
                error_message = out.to_string().clone();
                IterationStatus::Done
            });
            if !error_message.is_null() {
                restore_function_parse_phase!();
                return Err(error_message);
            }
        }

        // It's essential to finalize the hoisting before computing captured variables.
        self.scope_stack[scope].finalize_sloppy_mode_function_hoisting();

        let mut captured_variables = IdentifierSet::default();
        self.scope_stack[scope].get_captured_vars(&mut captured_variables);

        for (entry, _) in captured_variables.iter() {
            self.scope_stack[scope].declared_variables().mark_variable_as_captured(entry);
        }
        self.scope_stack[scope].finalize_lexical_environment();

        if is_generator_wrapper_parse_mode(parse_mode) || is_async_function_or_async_generator_wrapper_parse_mode(parse_mode) {
            if let Some(arguments) = self.vm.property_names.arguments.impl_() {
                if self.scope_stack[scope].used_variables_contains(&arguments) {
                    context.propagate_arguments_use();
                }
            }
        }

        let mut features = context.features() as CodeFeatures;
        if self.scope_stack[scope].shadows_arguments() {
            features |= SHADOWS_ARGUMENTS_FEATURE;
        }
        if self.seen_tagged_template_in_non_reparsing_function_mode {
            features |= NO_EVAL_CACHE_FEATURE;
        }
        if self.scope_stack[scope].has_non_simple_parameter_list() {
            features |= NON_SIMPLE_PARAMETER_LIST_FEATURE;
        }
        if self.scope_stack[scope].uses_import_meta() {
            features |= IMPORT_META_FEATURE;
        }
        if self.seen_arguments_dot_length && self.scope_stack[scope].has_declared_global_arguments() {
            features |= ARGUMENTS_FEATURE;
        }
        if self.scope_stack[scope].async_function_body_does_not_use_await() {
            features |= ASYNC_FUNCTION_WITHOUT_AWAIT_FEATURE;
        }
        if self.scope_stack[scope].uses_await() {
            features |= AWAIT_FEATURE;
        }

        // O bloco `#if ASSERT_ENABLED` (checagem de captura global em builtins) não existe em release.

        let function_declarations = self.scope_stack[scope].take_function_declarations();
        let var_declarations = self.scope_stack[scope].take_declared_variables();
        let lexical_variables = self.scope_stack[scope].take_lexical_environment();
        restore_function_parse_phase!();
        Ok(ParseInnerResult {
            parameters,
            source_elements,
            function_declarations,
            var_declarations,
            lexical_variables,
            features,
            num_constants: context.num_constants(),
        })
    }

    /// `template <class TreeBuilder> bool isArrowFunctionParameters(TreeBuilder&)`.
    pub(crate) fn is_arrow_function_parameters<B: TreeBuilder>(&mut self, context: &mut B) -> bool {
        if self.match_(OPENPAREN) {
            let save_arrow_function_point = self.create_save_point(context);
            self.next(LexerFlagSet::empty());
            let mut is_arrow_function = false;
            if self.consume(CLOSEPAREN) {
                is_arrow_function = self.match_(ARROWFUNCTION);
            } else {
                let vm = self.vm.clone();
                let mut syntax_checker = SyntaxChecker::new(&vm);
                // We make fake scope, otherwise parseFormalParameters will add variable to current scope that lead to errors
                let pushed = self.push_scope();
                let mut fake_scope = AutoPopScope::new(pushed);

                self.scope_stack[fake_scope.scope()].set_source_parse_mode(SourceParseMode::ArrowFunctionMode);
                self.reset_implementation_visibility_if_needed();

                let mut parameters_count: u32 = 0;
                let is_arrow_function_parameter_list = true;
                let is_method = false;
                let parameter_list = syntax_checker.create_formal_parameter_list();
                is_arrow_function = self.parse_formal_parameters(&mut syntax_checker, &parameter_list, is_arrow_function_parameter_list, is_method, &mut parameters_count)
                    && self.consume(CLOSEPAREN)
                    && self.match_(ARROWFUNCTION);
                propagate_error!(self, @hook { fake_scope.cleanup(self); });
                self.pop_scope_auto(&mut fake_scope, SyntaxChecker::NEEDS_FREE_VARIABLE_INFO, false, &[]);
                fake_scope.cleanup(self);
            }
            self.restore_save_point(context, &save_arrow_function_point);
            return is_arrow_function;
        }

        if self.match_spec_identifier() {
            semantic_fail_if_true!(self, self.is_disallowed_identifier_await(&self.token), "Cannot use 'await' as a parameter name ", self.disallowed_identifier_await_reason());
            let save_arrow_function_point = self.create_save_point(context);
            self.next(LexerFlagSet::empty());
            let is_arrow_function = self.match_(ARROWFUNCTION);
            self.restore_save_point(context, &save_arrow_function_point);
            return is_arrow_function;
        }

        false
    }

    /// `bool Parser<LexerType>::allowAutomaticSemicolon()`.
    pub(crate) fn allow_automatic_semicolon(&self) -> bool {
        self.match_(CLOSEBRACE) || self.match_(EOFTOK) || self.lexer.has_line_terminator_before_token()
    }

    /// `template <class TreeBuilder> TreeSourceElements parseSourceElements(TreeBuilder&, SourceElementsMode)`.
    pub(crate) fn parse_source_elements<B: TreeBuilder>(&mut self, context: &mut B, mode: SourceElementsMode) -> Option<B::SourceElements> {
        const LENGTH_OF_USE_STRICT_LITERAL: u32 = 12; // "use strict".length
        let mut source_elements = context.create_source_elements();
        let mut directive: Option<Identifier> = None;
        let mut directive_literal_length: u32 = 0;
        let mut save_point: Option<SavePoint> = None;
        let mut should_check_for_use_strict = mode == SourceElementsMode::CheckForStrictMode;
        if should_check_for_use_strict {
            save_point = Some(self.create_save_point(context));
        }

        while let Some(statement) = self.parse_statement_list_item(context, &mut directive, Some(&mut directive_literal_length)) {
            if should_check_for_use_strict {
                if let Some(found_directive) = directive.clone() {
                    // "use strict" must be the exact literal without escape sequences or line continuation.
                    if directive_literal_length == LENGTH_OF_USE_STRICT_LITERAL && self.vm.property_names.use_strict_identifier == found_directive {
                        self.set_strict_mode();
                        should_check_for_use_strict = false; // We saw "use strict", there is no need to keep checking for it.
                        if !self.is_valid_strict_mode() {
                            if let Some(last_function_name) = self.parser_state.last_function_name.clone() {
                                semantic_fail_if_true!(self, self.vm.property_names.arguments == last_function_name, "Cannot name a function 'arguments' in strict mode");
                                semantic_fail_if_true!(self, self.vm.property_names.eval == last_function_name, "Cannot name a function 'eval' in strict mode");
                            }
                            let arguments_name = self.vm.property_names.arguments.clone();
                            let eval_name = self.vm.property_names.eval.clone();
                            semantic_fail_if_true!(self, self.has_declared_variable(&arguments_name), "Cannot declare a variable named 'arguments' in strict mode");
                            semantic_fail_if_true!(self, self.has_declared_variable(&eval_name), "Cannot declare a variable named 'eval' in strict mode");
                            let current = self.current_scope();
                            semantic_fail_if_true!(self, self.scope_stack[current].has_non_simple_parameter_list(), "'use strict' directive not allowed inside a function with a non-simple parameter list");
                            semantic_fail_if_false!(self, self.is_valid_strict_mode(), "Invalid parameters or function name in strict mode");
                        }
                        // Since strict mode is changed, restoring lexer state by calling next() may cause errors.
                        if let Some(save_point) = &save_point {
                            self.restore_save_point(context, save_point);
                        }
                        propagate_error!(self);
                        continue;
                    }

                    // We saw a directive, but it wasn't "use strict". We reset our state to
                    // see if the next statement we parse is also a directive.
                    directive = None;
                } else {
                    // We saw a statement that wasn't in the form of a directive. The spec says that "use strict"
                    // is only allowed as the first statement, or after a sequence of directives before it, but
                    // not after non-directive statements.
                    should_check_for_use_strict = false;
                }
            }
            context.append_statement(&mut source_elements, statement);
        }

        propagate_error!(self);
        Some(source_elements)
    }

    /// `template <class TreeBuilder> TreeSourceElements parseModuleSourceElements(TreeBuilder&)`.
    pub(crate) fn parse_module_source_elements<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::SourceElements> {
        let mut source_elements = context.create_source_elements();
        let vm = self.vm.clone();
        let mut syntax_checker = SyntaxChecker::new(&vm);

        // `goto end` vira `break`: o rótulo `end:` fica logo depois do laço.
        loop {
            let mut statement: Option<B::Statement> = None;
            // O `case IMPORT` cai no `default` (`[[fallthrough]]`) quando não é declaração de import.
            let mut take_default = false;
            match self.token.type_ {
                EXPORT_ => {
                    statement = self.parse_export_declaration(context);
                    if let Some(statement) = &statement {
                        self.record_pause_location(context.breakpoint_location(statement));
                    }
                }

                IMPORT => {
                    let save_point = self.create_save_point(context);
                    self.next(LexerFlagSet::empty());
                    let is_import_declaration = !self.match_(OPENPAREN) && !self.match_(DOT);
                    self.restore_save_point(context, &save_point);
                    if is_import_declaration {
                        statement = self.parse_import_declaration(context);
                        if let Some(statement) = &statement {
                            self.record_pause_location(context.breakpoint_location(statement));
                        }
                    } else {
                        // This is `import("...")` call or `import.meta` meta property case.
                        take_default = true;
                    }
                }

                _ => take_default = true,
            }

            if take_default {
                let mut directive: Option<Identifier> = None;
                let mut directive_literal_length: u32 = 0;
                if self.source_parse_mode() == SourceParseMode::ModuleAnalyzeMode {
                    if self.parse_statement_list_item(&mut syntax_checker, &mut directive, Some(&mut directive_literal_length)).is_none() {
                        break;
                    }
                    continue;
                }
                statement = self.parse_statement_list_item(context, &mut directive, Some(&mut directive_literal_length));
            }

            match statement {
                Some(statement) => context.append_statement(&mut source_elements, statement),
                None => break,
            }
        }

        // end:
        propagate_error!(self);

        let exported_bindings: Vec<Option<UniquedKey>> = match &self.module_scope_data {
            Some(data) => data.exported_bindings().keys().cloned().collect(),
            None => Vec::new(),
        };
        for uid in exported_bindings {
            let uid = match uid {
                Some(uid) => uid,
                None => continue,
            };
            let current = self.current_scope();
            if self.scope_stack[current].has_declared_variable(&uid) {
                self.scope_stack[current].declared_variables().mark_variable_as_exported(&uid);
                continue;
            }

            if self.scope_stack[current].has_lexically_declared_variable(&uid) {
                self.scope_stack[current].lexical_variables().mark_variable_as_exported(&uid);
                continue;
            }

            semantic_fail!(self, "Exported binding '", uid, "' needs to refer to a top-level declared variable");
        }

        Some(source_elements)
    }

    /// `template <class TreeBuilder> TreeSourceElements parseGeneratorFunctionSourceElements(...)`.
    pub(crate) fn parse_generator_function_source_elements<B: TreeBuilder>(&mut self, context: &mut B, name: &Identifier, mode: SourceElementsMode) -> Option<B::SourceElements> {
        let mut source_elements = context.create_source_elements();

        let function_start = self.token_start();
        let start_location = self.token_location();
        let start = *self.token_start_position();
        let start_column = self.token_column() as u32;
        let function_name_start = self.token.start_position.offset;
        let parameters_start = function_name_start;

        let mut info = ParserFunctionInfo::<B>::default();
        info.name = Some(self.vm.property_names.null_identifier.clone());
        self.create_generator_parameters(context, &mut info.parameter_count);
        info.start_offset = parameters_start as u32;
        info.start_line = self.token_line();

        {
            let pushed = self.push_scope();
            let mut generator_body_scope = AutoPopScope::new(pushed);

            self.scope_stack[generator_body_scope.scope()].set_source_parse_mode(SourceParseMode::GeneratorBodyMode);
            self.reset_implementation_visibility_if_needed();

            self.scope_stack[generator_body_scope.scope()].set_constructor_kind(ConstructorKind::None);
            let super_binding = self.super_binding;
            self.scope_stack[generator_body_scope.scope()].set_expected_super_binding(super_binding);

            let vm = self.vm.clone();
            let mut generator_function_context = SyntaxChecker::new(&vm);
            let parsed = self.parse_source_elements(&mut generator_function_context, mode);
            fail_if_false!(self, @hook { generator_body_scope.cleanup(self); }, parsed.is_some(), "Cannot parse the body of a generator");
            self.pop_scope_auto(&mut generator_body_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);
            generator_body_scope.cleanup(self);
        }
        info.body = context.create_function_metadata(
            &start_location,
            &self.token_location(),
            start_column,
            self.token_column() as u32,
            function_start,
            function_name_start,
            parameters_start,
            self.implementation_visibility(),
            self.lexically_scoped_features(),
            ConstructorKind::None,
            self.super_binding,
            info.parameter_count,
            SourceParseMode::GeneratorBodyMode,
            false,
        );

        info.end_line = self.token_line();
        info.end_offset = self.token.data.offset as u32;
        info.parameters_start_column = start_column;

        let function_expr = context.create_generator_function_body(&start_location, &info, name);
        let statement = context.create_expr_statement(&start_location, function_expr, start, self.last_token_location.line);
        context.append_statement(&mut source_elements, statement);

        Some(source_elements)
    }

    /// `template <class TreeBuilder> TreeSourceElements parseAsyncFunctionSourceElements(...)`.
    pub(crate) fn parse_async_function_source_elements<B: TreeBuilder>(&mut self, context: &mut B, callee_name: &Identifier, is_arrow_function_body_expression: bool, mode: SourceElementsMode) -> Option<B::SourceElements> {
        debug_assert!(is_async_function_or_async_generator_wrapper_parse_mode(self.source_parse_mode()));

        let function_start = self.token_start();
        let start_location = self.token_location();
        let start = *self.token_start_position();
        let start_column = self.token_column() as u32;
        let function_name_start = self.token.start_position.offset;
        let parameters_start = function_name_start;
        let start_line = self.token_line();

        let body_parse_mode = get_async_function_body_parse_mode(self.source_parse_mode());
        // SetForScope innerParseMode: restaurado em cada saída.
        let old_parse_mode = self.parse_mode;
        self.parse_mode = body_parse_mode;

        let mut body_uses_await = false;
        let body_save_point = self.create_save_point(context);
        {
            let pushed = self.push_scope();
            let mut async_function_body_scope = AutoPopScope::new(pushed);

            let source_parse_mode = self.source_parse_mode();
            self.scope_stack[async_function_body_scope.scope()].set_source_parse_mode(source_parse_mode);
            self.reset_implementation_visibility_if_needed();

            let vm = self.vm.clone();
            let mut syntax_checker = SyntaxChecker::new(&vm);
            if is_arrow_function_body_expression {
                if self.debugger_parse_data.is_some() {
                    let parsed = self.parse_arrow_function_single_expression_body_source_elements(context);
                    fail_if_false!(self, @hook { async_function_body_scope.cleanup(self); self.parse_mode = old_parse_mode; }, parsed.is_some(), "Cannot parse the body of async arrow function");
                } else {
                    let parsed = self.parse_arrow_function_single_expression_body_source_elements(&mut syntax_checker);
                    fail_if_false!(self, @hook { async_function_body_scope.cleanup(self); self.parse_mode = old_parse_mode; }, parsed.is_some(), "Cannot parse the body of async arrow function");
                }
            } else if self.debugger_parse_data.is_some() {
                let parsed = self.parse_source_elements(context, mode);
                fail_if_false!(self, @hook { async_function_body_scope.cleanup(self); self.parse_mode = old_parse_mode; }, parsed.is_some(), "Cannot parse the body of async function");
            } else {
                let parsed = self.parse_source_elements(&mut syntax_checker, mode);
                fail_if_false!(self, @hook { async_function_body_scope.cleanup(self); self.parse_mode = old_parse_mode; }, parsed.is_some(), "Cannot parse the body of async function");
            }
            body_uses_await = self.scope_stack[async_function_body_scope.scope()].uses_await();

            // When body doesn't use await, we'll inline it directly in the wrapper.
            // In this case, there's no body function, so we don't need to track closed variables
            // (which would unnecessarily mark parameters as captured).
            self.pop_scope_auto(&mut async_function_body_scope, if body_uses_await { B::NEEDS_FREE_VARIABLE_INFO } else { false }, false, &[]);
            async_function_body_scope.cleanup(self);
        }

        // If the body doesn't use await, we can inline it directly into the wrapper.
        // without creating a separate body function or generator infrastructure.
        // For example,
        //
        //     async function test(a, b) {
        //         return 42;
        //     }
        //
        if !body_uses_await {
            // Re-parse with ASTBuilder to get the actual body AST.
            // Parse directly in the wrapper's scope (not a separate body scope) so that
            // lexical variables (let, const) are registered in the wrapper's scope.
            self.restore_save_point(context, &body_save_point);
            let function_scope = self.current_function_scope();
            self.scope_stack[function_scope].set_async_function_body_does_not_use_await();
            let result = if is_arrow_function_body_expression {
                self.parse_arrow_function_single_expression_body_source_elements(context)
            } else {
                self.parse_source_elements(context, mode)
            };
            self.parse_mode = old_parse_mode;
            return result;
        }

        // Full async function path (has await) - create body function with generator parameters.
        let mut source_elements = context.create_source_elements();

        let mut info = ParserFunctionInfo::<B>::default();
        info.name = Some(self.vm.property_names.null_identifier.clone());
        self.create_generator_parameters(context, &mut info.parameter_count);
        info.start_offset = parameters_start as u32;
        info.start_line = start_line;

        let mut implementation_visibility = self.implementation_visibility();
        if implementation_visibility == ImplementationVisibility::Private {
            implementation_visibility = ImplementationVisibility::Public;
        }

        info.body = context.create_function_metadata(
            &start_location,
            &self.token_location(),
            start_column,
            self.token_column() as u32,
            function_start,
            function_name_start,
            parameters_start,
            implementation_visibility,
            self.lexically_scoped_features(),
            ConstructorKind::None,
            self.super_binding,
            info.parameter_count,
            self.source_parse_mode(),
            is_arrow_function_body_expression,
        );

        if !callee_name.is_empty() && !callee_name.is_symbol() {
            B::set_function_body_ecma_name(&info.body, callee_name);
        }
        info.end_line = self.token_line();
        info.end_offset = if is_arrow_function_body_expression { self.token_location().end_offset } else { self.token.data.offset as u32 };
        info.parameters_start_column = start_column;

        let function_expr = context.create_async_function_body(&start_location, &info, body_parse_mode, callee_name);
        let statement = context.create_expr_statement(&start_location, function_expr, start, self.last_token_location.line);
        context.append_statement(&mut source_elements, statement);

        self.parse_mode = old_parse_mode;
        Some(source_elements)
    }

    /// `template <class TreeBuilder> TreeSourceElements parseAsyncGeneratorFunctionSourceElements(...)`.
    pub(crate) fn parse_async_generator_function_source_elements<B: TreeBuilder>(&mut self, context: &mut B, callee_name: &Identifier, is_arrow_function_body_expression: bool, mode: SourceElementsMode) -> Option<B::SourceElements> {
        debug_assert!(is_async_generator_wrapper_parse_mode(self.source_parse_mode()));
        let mut source_elements = context.create_source_elements();

        let function_start = self.token_start();
        let start_location = self.token_location();
        let start = *self.token_start_position();
        let start_column = self.token_column() as u32;
        let function_name_start = self.token.start_position.offset;
        let parameters_start = function_name_start;

        let mut info = ParserFunctionInfo::<B>::default();
        info.name = Some(self.vm.property_names.null_identifier.clone());
        self.create_generator_parameters(context, &mut info.parameter_count);
        info.start_offset = parameters_start as u32;
        info.start_line = self.token_line();

        let parse_mode = SourceParseMode::AsyncGeneratorBodyMode;
        // SetForScope innerParseMode: restaurado em cada saída.
        let old_parse_mode = self.parse_mode;
        self.parse_mode = parse_mode;
        {
            let pushed = self.push_scope();
            let mut async_function_body_scope = AutoPopScope::new(pushed);

            let source_parse_mode = self.source_parse_mode();
            self.scope_stack[async_function_body_scope.scope()].set_source_parse_mode(source_parse_mode);
            self.reset_implementation_visibility_if_needed();

            let vm = self.vm.clone();
            let mut syntax_checker = SyntaxChecker::new(&vm);
            if is_arrow_function_body_expression {
                if self.debugger_parse_data.is_some() {
                    let parsed = self.parse_arrow_function_single_expression_body_source_elements(context);
                    fail_if_false!(self, @hook { async_function_body_scope.cleanup(self); self.parse_mode = old_parse_mode; }, parsed.is_some(), "Cannot parse the body of async arrow function");
                } else {
                    let parsed = self.parse_arrow_function_single_expression_body_source_elements(&mut syntax_checker);
                    fail_if_false!(self, @hook { async_function_body_scope.cleanup(self); self.parse_mode = old_parse_mode; }, parsed.is_some(), "Cannot parse the body of async arrow function");
                }
            } else if self.debugger_parse_data.is_some() {
                let parsed = self.parse_source_elements(context, mode);
                fail_if_false!(self, @hook { async_function_body_scope.cleanup(self); self.parse_mode = old_parse_mode; }, parsed.is_some(), "Cannot parse the body of async function");
            } else {
                let parsed = self.parse_source_elements(&mut syntax_checker, mode);
                fail_if_false!(self, @hook { async_function_body_scope.cleanup(self); self.parse_mode = old_parse_mode; }, parsed.is_some(), "Cannot parse the body of async function");
            }
            self.pop_scope_auto(&mut async_function_body_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);
            async_function_body_scope.cleanup(self);
        }
        info.body = context.create_function_metadata(
            &start_location,
            &self.token_location(),
            start_column,
            self.token_column() as u32,
            function_start,
            function_name_start,
            parameters_start,
            self.implementation_visibility(),
            self.lexically_scoped_features(),
            ConstructorKind::None,
            self.super_binding,
            info.parameter_count,
            parse_mode,
            is_arrow_function_body_expression,
        );

        if !callee_name.is_empty() && !callee_name.is_symbol() {
            B::set_function_body_ecma_name(&info.body, callee_name);
        }
        info.end_line = self.token_line();
        info.end_offset = if is_arrow_function_body_expression { self.token_location().end_offset } else { self.token.data.offset as u32 };
        info.parameters_start_column = start_column;

        let function_expr = context.create_async_function_body(&start_location, &info, parse_mode, callee_name);
        let statement = context.create_expr_statement(&start_location, function_expr, start, self.last_token_location.line);
        context.append_statement(&mut source_elements, statement);

        self.parse_mode = old_parse_mode;
        Some(source_elements)
    }
}
