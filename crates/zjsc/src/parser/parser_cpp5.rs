// Quinta fatia de `parser/Parser.cpp` (linhas 2946 a 3743), incluída por `include!` em `parser.rs`.
// Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Convenções: as de `parser_cpp1.rs` (retorno `Option<B::X>`, macros com o parser como primeiro
// argumento, `@hook` para os destrutores RAII, `impl<T: CharType> Parser<T>` com `<B: TreeBuilder>`).
// Acréscimos desta fatia:
//
// - `AutoPopScope` e `SetForScope` vivos no ponto da falha entram no `@hook`, na ordem inversa de
//   construção. As macros do topo do `.cpp` que não aceitam `@hook` (`semanticFail*`, `matchOrFail`,
//   `failIfTrueIfStrict`, `semanticFailureDueToKeyword`) ganham aqui uma variante `_hooked`, com a
//   mesma expansão e o gancho no mesmo ponto.
// - `DepthManager` (salva `m_statementDepth` e o restaura) vira salvar e restaurar o valor.
// - Variável do C++ que guarda `TreeExpression`/`TreePropertyList` e nasce em `0` vira o valor
//   `Default::default()` do tipo associado; o teste `if (x)` vira `x != Default::default()`.
// - `const Identifier*` que nunca é nulo (e tem `ASSERT`) vira `Identifier`.

/// Prefixos dos nomes privados dos campos com nome computado.
const INSTANCE_COMPUTED_NAME_PREFIX: &str = "instanceComputedName";
const STATIC_COMPUTED_NAME_PREFIX: &str = "staticComputedName";

/// `semanticFailIfTrue(cond, ...)` com o gancho dos destrutores vivos.
macro_rules! semantic_fail_if_true_hooked {
    ($p:expr, @hook $h:block, $cond:expr, $($arg:expr),+ $(,)?) => {
        if $cond {
            internal_fail_with_message!($p, @hook $h, false, $($arg),+);
        }
    };
}

/// `semanticFailIfFalse(cond, ...)` com o gancho dos destrutores vivos.
macro_rules! semantic_fail_if_false_hooked {
    ($p:expr, @hook $h:block, $cond:expr, $($arg:expr),+ $(,)?) => {
        if !($cond) {
            internal_fail_with_message!($p, @hook $h, false, $($arg),+);
        }
    };
}

/// `failIfTrueIfStrict(cond, ...)` com o gancho dos destrutores vivos.
macro_rules! fail_if_true_if_strict_hooked {
    ($p:expr, @hook $h:block, $cond:expr, $($arg:expr),+ $(,)?) => {
        if ($cond) && $p.strict_mode() {
            internal_fail_with_message!($p, @hook $h, false, $($arg),+);
        }
    };
}

/// `matchOrFail(tokenType, ...)` com o gancho dos destrutores vivos.
macro_rules! match_or_fail_hooked {
    ($p:expr, @hook $h:block, $token_type:expr, $($arg:expr),+ $(,)?) => {
        if !$p.match_($token_type) {
            handle_error_token!($p, @hook $h);
            internal_fail_with_message!($p, @hook $h, true, $($arg),+);
        }
    };
}

/// `semanticFailureDueToKeyword(...)` (sobre `m_token`) com o gancho dos destrutores vivos.
macro_rules! semantic_failure_due_to_keyword_hooked {
    ($p:expr, @hook $h:block, $($arg:expr),+ $(,)?) => {{
        let keyword_token: JSToken = $p.token.clone();
        semantic_fail_if_true_hooked!($p, @hook $h, $p.strict_mode() && keyword_token.type_ == RESERVED_IF_STRICT, "Cannot use the reserved word '", $p.get_token_for(&keyword_token), "' as a ", $($arg,)+ " in strict mode");
        semantic_fail_if_true_hooked!($p, @hook $h, keyword_token.type_ == RESERVED || keyword_token.type_ == RESERVED_IF_STRICT, "Cannot use the reserved word '", $p.get_token_for(&keyword_token), "' as a ", $($arg),+);
        if (keyword_token.type_ & KEYWORD_TOKEN_FLAG) != 0 {
            semantic_fail_if_false_hooked!($p, @hook $h, is_contextual_keyword(&keyword_token), "Cannot use the keyword '", $p.get_token_for(&keyword_token), "' as a ", $($arg),+);
            semantic_fail_if_true_hooked!($p, @hook $h, keyword_token.type_ == LET && $p.strict_mode(), "Cannot use 'let' as a ", $($arg,)+ " ", $p.disallowed_identifier_let_reason());
            semantic_fail_if_true_hooked!($p, @hook $h, keyword_token.type_ == AWAIT && !$p.can_use_identifier_await(), "Cannot use 'await' as a ", $($arg,)+ " ", $p.disallowed_identifier_await_reason());
            semantic_fail_if_true_hooked!($p, @hook $h, keyword_token.type_ == YIELD && !$p.can_use_identifier_yield(), "Cannot use 'yield' as a ", $($arg,)+ " ", $p.disallowed_identifier_yield_reason());
        }
    }};
}

/// `struct LabelInfo` (`const Identifier*` vira cópia do `Identifier`, que é um handle contado).
struct LabelInfo {
    ident: Identifier,
    start: JSTextPosition,
    end: JSTextPosition,
}

impl<T: CharType> Parser<T> {
    /// `template <class TreeBuilder> TreeStatement parseFunctionDeclaration(...)`.
    pub(crate) fn parse_function_declaration<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        declaration_type: FunctionDeclarationType,
        export_type: ExportType,
        declaration_default_context: DeclarationDefaultContext,
        function_constructor_parameters_end_position: Option<i32>,
    ) -> Option<B::Statement> {
        debug_assert!(self.match_(FUNCTION));
        let location = self.token_location();
        let function_start = self.token_start();
        self.next(LexerFlagSet::empty());
        let mut parse_mode = SourceParseMode::NormalFunctionMode;
        if self.match_(TIMES) {
            fail_if_true!(self, declaration_type == FunctionDeclarationType::Statement, "Cannot use generator function declaration in single-statement context");
            self.next(LexerFlagSet::empty());
            parse_mode = SourceParseMode::GeneratorWrapperFunctionMode;
        }
        // SetForScope innerParseMode: restaurado em cada saída.
        let old_parse_mode = self.parse_mode;
        self.parse_mode = parse_mode;

        let mut function_info = ParserFunctionInfo::<B>::default();
        let mut requirements = FunctionNameRequirements::Named;
        if declaration_default_context == DeclarationDefaultContext::ExportDefault {
            // Under the "export default" context, function declaration does not require the function name.
            //
            //     ExportDeclaration:
            //         ...
            //         export default HoistableDeclaration[~Yield, +Default]
            //         ...
            //
            //     HoistableDeclaration[Yield, Default]:
            //         FunctionDeclaration[?Yield, ?Default]
            //         GeneratorDeclaration[?Yield, ?Default]
            //
            //     FunctionDeclaration[Yield, Default]:
            //         ...
            //         [+Default] function ( FormalParameters[~Yield] ) { FunctionBody[~Yield] }
            //
            //     GeneratorDeclaration[Yield, Default]:
            //         ...
            //         [+Default] function * ( FormalParameters[+Yield] ) { GeneratorBody }
            //
            // In this case, we use "*default*" as this function declaration's name.
            requirements = FunctionNameRequirements::None;
            function_info.name = Some(self.vm.property_names.star_default_private_name.clone());
        }

        fail_if_false!(self, @hook { self.parse_mode = old_parse_mode; }, self.parse_function_info(context, requirements, true, ConstructorKind::None, SuperBinding::NotNeeded, function_start, &mut function_info, FunctionDefinitionType::Declaration, function_constructor_parameters_end_position), "Cannot parse this function");
        debug_assert!(function_info.name.is_some());
        let name = function_info.name.clone().expect("ASSERT(functionInfo.name)");

        let (declaration_result, declaration_scope) = self.declare_function(&name);
        fail_if_true_if_strict_hooked!(self, @hook { self.parse_mode = old_parse_mode; }, (declaration_result & DeclarationResult::INVALID_STRICT_MODE) != 0, "Cannot declare a function named '", name, "' in strict mode");
        semantic_fail_if_true_hooked!(self, @hook { self.parse_mode = old_parse_mode; }, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Cannot declare a function that shadows a let/const/class/function variable '", name, "'");
        if export_type == ExportType::Exported {
            debug_assert!(declaration_default_context != DeclarationDefaultContext::ExportDefault, "Export default case will export the name and binding in the caller.");
            semantic_fail_if_false_hooked!(self, @hook { self.parse_mode = old_parse_mode; }, self.export_name(&name), "Cannot export a duplicate function name: '", name, "'");
            if let Some(data) = &self.module_scope_data {
                data.export_binding_same_name(&name);
            }
        }

        let result = context.create_func_decl_statement(&location, &function_info);
        if B::CREATES_AST {
            let metadata = B::get_metadata(&function_info);
            self.scope_stack[declaration_scope].append_function(metadata.clone());
            let is_sloppy_mode_hoisting_candidate = self.statement_depth != 1 && !self.strict_mode() && self.parse_mode == SourceParseMode::NormalFunctionMode;
            if is_sloppy_mode_hoisting_candidate {
                // Functions declared inside a function inside a nested block scope in sloppy mode are subject to this
                // crazy rule defined inside Annex B.3.2 in the ECMA-262 spec. It basically states that we will create
                // the function as a local block scoped variable, but when we evaluate the block that the function is
                // contained in, we will assign the function to a "var" variable only if declaring such a "var" wouldn't
                // be a syntax error and if there isn't a parameter with the same name. (It would only be a syntax error if
                // there are is a let/class/const with the same name). Note that this mean we only do the "var" hoisting
                // binding if the block evaluates. For example, this means we wont won't perform the binding if it's inside
                // the untaken branch of an if statement.
                self.scope_stack[declaration_scope].add_sloppy_mode_function_hoisting_candidate(metadata, NeedsDuplicateDeclarationCheck::No);
            }
        }
        self.parse_mode = old_parse_mode;
        Some(result)
    }

    /// `template <class TreeBuilder> TreeStatement parseAsyncFunctionDeclaration(...)`.
    pub(crate) fn parse_async_function_declaration<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        function_start: u32,
        export_type: ExportType,
        declaration_default_context: DeclarationDefaultContext,
        function_constructor_parameters_end_position: Option<i32>,
    ) -> Option<B::Statement> {
        debug_assert!(self.match_(FUNCTION));
        let location = self.token_location();
        self.next(LexerFlagSet::empty());
        let mut function_info = ParserFunctionInfo::<B>::default();
        let mut parse_mode = SourceParseMode::AsyncFunctionMode;
        if self.consume(TIMES) {
            parse_mode = SourceParseMode::AsyncGeneratorWrapperFunctionMode;
        }
        // SetForScope innerParseMode: restaurado em cada saída.
        let old_parse_mode = self.parse_mode;
        self.parse_mode = parse_mode;

        let mut requirements = FunctionNameRequirements::Named;
        if declaration_default_context == DeclarationDefaultContext::ExportDefault {
            // Under the "export default" context, function declaration does not require the function name.
            //
            //     ExportDeclaration:
            //         ...
            //         export default HoistableDeclaration[~Yield, +Default]
            //         ...
            //
            //     HoistableDeclaration[Yield, Default]:
            //         FunctionDeclaration[?Yield, ?Default]
            //         GeneratorDeclaration[?Yield, ?Default]
            //
            //     FunctionDeclaration[Yield, Default]:
            //         ...
            //         [+Default] function ( FormalParameters[~Yield] ) { FunctionBody[~Yield] }
            //
            //     GeneratorDeclaration[Yield, Default]:
            //         ...
            //         [+Default] function * ( FormalParameters[+Yield] ) { GeneratorBody }
            //
            // In this case, we use "*default*" as this function declaration's name.
            requirements = FunctionNameRequirements::None;
            function_info.name = Some(self.vm.property_names.star_default_private_name.clone());
        }

        fail_if_false!(self, @hook { self.parse_mode = old_parse_mode; }, self.parse_function_info(context, requirements, true, ConstructorKind::None, SuperBinding::NotNeeded, function_start, &mut function_info, FunctionDefinitionType::Declaration, function_constructor_parameters_end_position), "Cannot parse this async function");
        fail_if_false!(self, @hook { self.parse_mode = old_parse_mode; }, function_info.name.is_some(), "Async function statements must have a name");
        let name = function_info.name.clone().expect("failIfFalse(functionInfo.name)");

        let (declaration_result, declaration_scope) = self.declare_function(&name);
        fail_if_true_if_strict_hooked!(self, @hook { self.parse_mode = old_parse_mode; }, (declaration_result & DeclarationResult::INVALID_STRICT_MODE) != 0, "Cannot declare an async function named '", name, "' in strict mode");
        semantic_fail_if_true_hooked!(self, @hook { self.parse_mode = old_parse_mode; }, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Cannot declare an async function that shadows a let/const/class/function variable '", name, "'");
        if export_type == ExportType::Exported {
            semantic_fail_if_false_hooked!(self, @hook { self.parse_mode = old_parse_mode; }, self.export_name(&name), "Cannot export a duplicate function name: '", name, "'");
            if let Some(data) = &self.module_scope_data {
                data.export_binding_same_name(&name);
            }
        }

        let result = context.create_func_decl_statement(&location, &function_info);
        if B::CREATES_AST {
            let metadata = B::get_metadata(&function_info);
            self.scope_stack[declaration_scope].append_function(metadata);
        }
        self.parse_mode = old_parse_mode;
        Some(result)
    }

    /// `template <class TreeBuilder> TreeStatement parseClassDeclaration(...)`.
    pub(crate) fn parse_class_declaration<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        export_type: ExportType,
        declaration_default_context: DeclarationDefaultContext,
    ) -> Option<B::Statement> {
        debug_assert!(self.match_(CLASSTOKEN));
        let location = self.token_location();
        let class_start = self.token_start_position().clone();
        let class_start_line = self.token_line() as u32;

        let mut info = ParserClassInfo::<B>::default();
        let mut requirements = FunctionNameRequirements::Named;
        if declaration_default_context == DeclarationDefaultContext::ExportDefault {
            // Under the "export default" context, class declaration does not require the class name.
            //
            //     ExportDeclaration:
            //         ...
            //         export default ClassDeclaration[~Yield, +Default]
            //         ...
            //
            //     ClassDeclaration[Yield, Default]:
            //         ...
            //         [+Default] class ClassTail[?Yield]
            //
            // In this case, we use "*default*" as this class declaration's name.
            requirements = FunctionNameRequirements::None;
            info.class_name = Some(self.vm.property_names.star_default_private_name.clone());
        }

        let class_expr = self.parse_class(context, requirements, &mut info);
        fail_if_false!(self, class_expr.is_some(), "Failed to parse class");
        let class_expr = class_expr.unwrap_or_default();
        debug_assert!(info.class_name.is_some());
        let class_name = info.class_name.clone().expect("ASSERT(info.className)");

        let declaration_result = self.declare_variable(&class_name, DeclarationType::LetDeclaration, DeclarationImportType::NotImported);
        semantic_fail_if_true!(self, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Cannot declare a class twice: '", class_name, "'");
        if export_type == ExportType::Exported {
            debug_assert!(declaration_default_context != DeclarationDefaultContext::ExportDefault, "Export default case will export the name and binding in the caller.");
            semantic_fail_if_false!(self, self.export_name(&class_name), "Cannot export a duplicate class name: '", class_name, "'");
            if let Some(data) = &self.module_scope_data {
                data.export_binding_same_name(&class_name);
            }
        }

        let class_end = self.last_token_end_position();
        let class_end_line = self.token_line() as u32;

        Some(context.create_class_decl_statement(&location, class_expr, class_start, class_end, class_start_line, class_end_line))
    }

    /// `template <class TreeBuilder> TreeClassExpression parseClass(...)`.
    pub(crate) fn parse_class<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        requirements: FunctionNameRequirements,
        info: &mut ParserClassInfo<B>,
    ) -> Option<B::ClassExpression> {
        debug_assert!(self.match_(CLASSTOKEN));
        let vm = self.vm.clone();
        let start = self.token_start_position().clone();
        let location = self.token_location();
        info.start_line = location.line;
        info.start_column = self.token_column() as u32;
        info.start_offset = location.start_offset;

        // We have a subtle problem here. Class heritage evaluation should find class declaration's constructor name, but should not find private name evaluation.
        // For example,
        //
        //     class A extends (
        //         class {
        //             constructor() {
        //                 print(A); // This is OK.
        //                 print(A.#test); // This is SyntaxError.
        //             }
        //         }) {
        //         static #test = 42;
        //     }
        //
        // We need to create two scopes here since private name lookup will traverse scope at linking time in CodeBlock.
        // This classHeadScope is similar to functionScope in FunctionExpression with name.
        let class_head_pushed = self.push_scope();
        let mut class_head_scope = AutoPopScope::new(class_head_pushed);
        self.scope_stack[class_head_scope.scope()].set_is_lexical_scope();
        self.scope_stack[class_head_scope.scope()].prevent_var_declarations();
        self.scope_stack[class_head_scope.scope()].set_strict_mode();
        self.next(LexerFlagSet::empty());
        semantic_fail_if_true_hooked!(self, @hook { class_head_scope.cleanup(self); }, self.scope_stack[self.current_scope()].is_static_block() && self.match_(AWAIT), "Cannot use 'await' as a class name within static block");

        debug_assert!(requirements != FunctionNameRequirements::Unnamed, "Currently, there is no caller that uses FunctionNameRequirements::Unnamed for class syntax.");
        debug_assert!(!(requirements == FunctionNameRequirements::None && info.class_name.is_none()), "When specifying FunctionNameRequirements::None, we need to initialize info.className with the default value in the caller side.");
        if self.match_(IDENT) || self.is_allowed_identifier_await(&self.token) {
            let class_name = self.token.data.ident.clone().expect("ASSERT(m_token.m_data.ident)"); // Invariante: o lexer sempre preenche `ident` nos tokens IDENT, STRING e PRIVATENAME já conferidos acima.
            info.class_name = Some(class_name.clone());
            self.next(LexerFlagSet::empty());
            let declaration_result = self.scope_stack[class_head_scope.scope()].declare_lexical_variable(&class_name, true, DeclarationImportType::NotImported, false, false);
            fail_if_true!(self, @hook { class_head_scope.cleanup(self); }, (declaration_result & DeclarationResult::INVALID_STRICT_MODE) != 0, "'", class_name, "' is not a valid class name");
        } else if requirements == FunctionNameRequirements::Named {
            semantic_fail_if_true_hooked!(self, @hook { class_head_scope.cleanup(self); }, self.match_(OPENBRACE), "Class statements must have a name");
            semantic_failure_due_to_keyword_hooked!(self, @hook { class_head_scope.cleanup(self); }, "class name");
            fail_due_to_unexpected_token!(self, @hook { class_head_scope.cleanup(self); });
        }
        debug_assert!(info.class_name.is_some());

        let mut divot = start;
        let mut parent_class: B::Expression = Default::default();
        if self.consume(EXTENDS) {
            divot = self.token_start_position().clone();
            let parsed = self.parse_member_expression(context);
            fail_if_false!(self, @hook { class_head_scope.cleanup(self); }, parsed.is_some(), "Cannot parse the parent class name");
            parent_class = parsed.unwrap_or_default();
        }
        let constructor_kind = if parent_class != B::Expression::default() { ConstructorKind::Extends } else { ConstructorKind::Base };

        let class_head_end = self.last_token_end_position();
        consume_or_fail!(self, @hook { class_head_scope.cleanup(self); }, OPENBRACE, "Expected opening '{' at the start of a class body");

        let class_pushed = self.push_scope();
        let mut class_scope = AutoPopScope::new(class_pushed);
        self.scope_stack[class_scope.scope()].set_is_lexical_scope();
        self.scope_stack[class_scope.scope()].prevent_var_declarations();
        self.scope_stack[class_scope.scope()].set_strict_mode();
        self.scope_stack[class_scope.scope()].set_is_class_scope();

        // Destrutores vivos de `classScope` e `classHeadScope`, na ordem inversa de construção.
        macro_rules! class_cleanup {
            ($p:expr) => {{
                class_scope.cleanup($p);
                class_head_scope.cleanup($p);
            }};
        }

        let mut declares_private_method = false;
        let mut declares_private_accessor = false;
        let mut declares_static_private_method = false;
        let mut declares_static_private_accessor = false;

        let mut constructor: B::Expression = Default::default();
        let mut class_elements: B::PropertyList = Default::default();
        let mut class_elements_tail: B::PropertyList = Default::default();
        let mut next_instance_computed_field_id: u32 = 0;
        let mut next_static_computed_field_id: u32 = 0;
        while !self.match_(CLOSEBRACE) {
            if self.consume(SEMICOLON) {
                continue;
            }

            let method_location = self.token_location();
            let mut function_start = self.token_start();

            // For backwards compatibility, "static" is a non-reserved keyword in non-strict mode.
            let mut tag = ClassElementTag::Instance;
            let mut parse_mode = SourceParseMode::MethodMode;
            let mut type_: PropertyNodeType = PropertyNode::CONSTANT;
            if self.match_(RESERVED_IF_STRICT) && self.token.data.ident.as_ref() == Some(&vm.property_names.static_keyword) {
                let save_point = self.create_save_point(context);
                self.next(LexerFlagSet::empty());
                if self.match_(OPENPAREN) || self.match_(SEMICOLON) || self.match_(EQUAL) {
                    // Reparse "static()" as a method or "static" as a class field.
                    self.restore_save_point(context, &save_point);
                } else {
                    tag = ClassElementTag::Static;
                    function_start = self.token_start();
                    if self.match_(OPENBRACE) {
                        parse_mode = SourceParseMode::ClassStaticBlockMode;
                    }
                }
            }

            // FIXME: Figure out a way to share more code with parseProperty.
            let mut ident = vm.property_names.null_identifier.clone();
            let mut computed_property_name: B::Expression = Default::default();
            let mut is_getter = false;
            let mut is_setter = false;
            if self.consume(TIMES) {
                parse_mode = SourceParseMode::GeneratorWrapperMethodMode;
            }

            // `parseMethod:` e o `goto parseMethod` do `async` viram `continue 'parse_method`; o `break` do
            // `switch` é a saída do `match`, seguida do `break 'parse_method` no fim do laço.
            'parse_method: loop {
                // `namedKeyword:` é o corpo de `case STRING:`, alcançado também pelo `default`.
                let mut take_named_keyword = false;
                match self.token.type_ {
                    STRING => take_named_keyword = true,
                    BIGINT => {
                        let big_int_string = self.token.data.big_int_string.clone().expect("ASSERT(m_token.m_data.bigIntString)");
                        let radix = self.token.data.radix;
                        let made = self.parser_arena.identifier_arena().borrow_mut().make_big_int_decimal_identifier(&self.vm, &big_int_string, radix);
                        fail_if_false!(self, @hook { class_cleanup!(self); }, made.is_some(), "Cannot parse big int property name");
                        if let Some(made_ident) = made {
                            ident = made_ident;
                        }
                        self.next(LexerFlagSet::empty());
                    }
                    ESCAPED_KEYWORD | IDENT | AWAIT => 'ident_case: {
                        if self.token.type_ != AWAIT && self.token.data.ident.as_ref() == Some(&vm.property_names.r#async) && !self.token.data.escaped {
                            if !is_generator_method_parse_mode(parse_mode) && !is_async_method_parse_mode(parse_mode) {
                                self.next(LexerFlagSet::empty());
                                // We match SEMICOLON as a special case for a field called 'async' without initializer.
                                if self.match_(OPENPAREN) || self.match_(COLON) || self.match_(SEMICOLON) || self.match_(EQUAL) || self.lexer.has_line_terminator_before_token() {
                                    ident = vm.property_names.r#async.clone();
                                    break 'ident_case;
                                }
                                if self.consume(TIMES) {
                                    parse_mode = SourceParseMode::AsyncGeneratorWrapperMethodMode;
                                } else {
                                    parse_mode = SourceParseMode::AsyncMethodMode;
                                }
                                continue 'parse_method;
                            }
                        }
                        // `[[fallthrough]]` para `case AWAIT:`.
                        ident = self.token.data.ident.clone().expect("ASSERT(ident)"); // Invariante: o lexer preenche `ident` em IDENT, PRIVATENAME, STRING e palavras-chave usadas como nome.
                        let escaped = self.token.data.escaped;
                        self.next(LexerFlagSet::empty());
                        if parse_mode == SourceParseMode::MethodMode && !escaped && (self.match_identifier_or_keyword() || self.match_(STRING) || self.match_(DOUBLE) || self.match_(INTEGER) || self.match_(BIGINT) || self.match_(OPENBRACKET) || self.match_(PRIVATENAME)) {
                            is_getter = ident == vm.property_names.get;
                            is_setter = ident == vm.property_names.set;
                        }
                    },
                    DOUBLE | INTEGER => {
                        ident = self.parser_arena.identifier_arena().borrow_mut().make_numeric_identifier(&self.vm, self.token.data.double_value);
                        self.next(LexerFlagSet::empty());
                    }
                    OPENBRACKET => {
                        self.next(LexerFlagSet::empty());
                        semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, self.scope_stack[self.current_scope()].is_static_block() && self.match_(IDENT) && self.is_arguments_identifier(), "Cannot use 'arguments' as an identifier in static block");
                        let parsed = self.parse_assignment_expression(context);
                        type_ |= PropertyNode::COMPUTED;
                        fail_if_false!(self, @hook { class_cleanup!(self); }, parsed.is_some(), "Cannot parse computed property name");
                        computed_property_name = parsed.unwrap_or_default();
                        consume_or_fail!(self, @hook { class_cleanup!(self); }, CLOSEBRACKET, "Expected '", "]", "' to ", "end", " a ", "computed property name");
                    }
                    PRIVATENAME => {
                        ident = self.token.data.ident.clone().expect("ASSERT(ident)"); // Invariante: o lexer preenche `ident` em IDENT, PRIVATENAME, STRING e palavras-chave usadas como nome.
                        fail_if_true!(self, @hook { class_cleanup!(self); }, is_getter || is_setter, "Cannot parse class method with private name");
                        self.next(LexerFlagSet::empty());
                        if self.match_(OPENPAREN) {
                            let declaration_result = self.scope_stack[class_scope.scope()].declare_private_method(&ident, tag);
                            semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Cannot declare private method twice");
                            semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, ident == vm.property_names.constructor_private_field, "Cannot declare a private method named '#constructor'");

                            if tag == ClassElementTag::Static {
                                declares_static_private_method = true;
                            } else {
                                declares_private_method = true;
                            }

                            type_ |= PropertyNode::PRIVATE_METHOD;
                            break 'parse_method;
                        }

                        fail_if_true!(self, @hook { class_cleanup!(self); }, self.match_(OPENPAREN), "Cannot parse class method with private name");
                        let declaration_result = self.scope_stack[class_scope.scope()].declare_private_field(&ident);
                        semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Cannot declare private field twice");
                        type_ |= PropertyNode::PRIVATE_FIELD;
                    }
                    OPENBRACE => {
                        fail_if_false!(self, @hook { class_cleanup!(self); }, parse_mode == SourceParseMode::ClassStaticBlockMode, "Cannot parse static block without 'static'");
                        type_ |= PropertyNode::BLOCK;
                    }
                    _ => {
                        if (self.token.type_ & KEYWORD_TOKEN_FLAG) != 0 {
                            take_named_keyword = true;
                        } else {
                            fail_due_to_unexpected_token!(self, @hook { class_cleanup!(self); });
                        }
                    }
                }
                if take_named_keyword {
                    ident = self.token.data.ident.clone().expect("ASSERT(ident)"); // Invariante: o lexer preenche `ident` em IDENT, PRIVATENAME, STRING e palavras-chave usadas como nome.
                    self.next(LexerFlagSet::empty());
                }
                break;
            }

            let property: B::Property;
            if is_getter || is_setter {
                if self.match_(PRIVATENAME) {
                    ident = self.token.data.ident.clone().expect("ASSERT(m_token.m_data.ident)"); // Invariante: o lexer sempre preenche `ident` nos tokens IDENT, STRING e PRIVATENAME já conferidos acima.

                    let declaration_result = if is_setter {
                        self.scope_stack[class_scope.scope()].declare_private_setter(&ident, tag)
                    } else {
                        self.scope_stack[class_scope.scope()].declare_private_getter(&ident, tag)
                    };
                    semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, (declaration_result & DeclarationResult::INVALID_DUPLICATE_DECLARATION) != 0, "Declared private setter with an already used name");
                    if tag == ClassElementTag::Static {
                        semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, (declaration_result & DeclarationResult::INVALID_PRIVATE_STATIC_NON_STATIC) != 0, "Cannot declare a private static ", (if is_setter { "setter" } else { "getter" }), " if there is a non-static private ", (if is_setter { "getter" } else { "setter" }), " with used name");
                        declares_static_private_accessor = true;
                    } else {
                        semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, (declaration_result & DeclarationResult::INVALID_PRIVATE_STATIC_NON_STATIC) != 0, "Cannot declare a private non-static ", (if is_setter { "setter" } else { "getter" }), " if there is a static private ", (if is_setter { "getter" } else { "setter" }), " with used name");
                        declares_private_accessor = true;
                    }

                    if is_setter {
                        type_ |= PropertyNode::PRIVATE_SETTER;
                    } else {
                        type_ |= PropertyNode::PRIVATE_GETTER;
                    }
                } else {
                    type_ &= !PropertyNode::CONSTANT;
                    type_ |= if is_getter { PropertyNode::GETTER } else { PropertyNode::SETTER };
                }
                let parsed = self.parse_getter_setter(context, type_, function_start, ConstructorKind::None, tag);
                fail_if_false!(self, @hook { class_cleanup!(self); }, parsed.is_some(), "Cannot parse this method");
                property = parsed.unwrap_or_default();
            } else if !self.match_(OPENPAREN) && parse_mode == SourceParseMode::MethodMode {
                debug_assert!(!is_getter && !is_setter);
                semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, ident == vm.property_names.constructor, "Cannot declare class field named 'constructor'");
                semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, ident == vm.property_names.constructor_private_field, "Cannot declare private class field named '#constructor'");
                if tag == ClassElementTag::Static {
                    semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, ident == vm.property_names.prototype, "Cannot declare a static field named 'prototype'");
                }

                if computed_property_name != B::Expression::default() {
                    if tag == ClassElementTag::Instance {
                        let field_id = next_instance_computed_field_id;
                        next_instance_computed_field_id += 1;
                        ident = self.parser_arena.identifier_arena().borrow_mut().make_private_identifier(&self.vm, INSTANCE_COMPUTED_NAME_PREFIX, field_id);
                    } else {
                        let field_id = next_static_computed_field_id;
                        next_static_computed_field_id += 1;
                        ident = self.parser_arena.identifier_arena().borrow_mut().make_private_identifier(&self.vm, STATIC_COMPUTED_NAME_PREFIX, field_id);
                    }
                    let declaration_result = self.scope_stack[class_scope.scope()].declare_lexical_variable(&ident, true, DeclarationImportType::NotImported, false, false);
                    debug_assert!(declaration_result == DeclarationResult::VALID);
                    self.scope_stack[class_scope.scope()].use_variable_identifier(&ident, false);
                    self.scope_stack[class_scope.scope()].add_closed_variable_candidate_unconditionally(&uid(&ident));
                }

                let mut initializer: B::Expression = Default::default();
                if self.consume(EQUAL) {
                    let current = self.current_scope();
                    let used_variables_size = self.scope_stack[current].current_used_variables_size();
                    self.scope_stack[current].push_used_variable_set();
                    // SetForScope overrideParsingClassFieldInitializer e maskAsync: restaurados em cada saída.
                    let old_is_parsing_class_field_initializer = self.parser_state.is_parsing_class_field_initializer;
                    self.parser_state.is_parsing_class_field_initializer = true;
                    let old_class_field_init_masks_async = self.parser_state.class_field_init_masks_async;
                    self.parser_state.class_field_init_masks_async = true;
                    self.scope_stack[class_scope.scope()].set_expected_super_binding(SuperBinding::Needed);
                    let parsed = self.parse_assignment_expression(context);
                    self.scope_stack[class_scope.scope()].set_expected_super_binding(SuperBinding::NotNeeded);
                    fail_if_false!(self, @hook { self.parser_state.class_field_init_masks_async = old_class_field_init_masks_async; self.parser_state.is_parsing_class_field_initializer = old_is_parsing_class_field_initializer; class_cleanup!(self); }, parsed.is_some(), "Cannot parse initializer for class field");
                    initializer = parsed.unwrap_or_default();
                    self.scope_stack[class_scope.scope()].mark_last_used_variables_set_as_captured(used_variables_size);
                    self.parser_state.class_field_init_masks_async = old_class_field_init_masks_async;
                    self.parser_state.is_parsing_class_field_initializer = old_is_parsing_class_field_initializer;
                }
                fail_if_false!(self, @hook { class_cleanup!(self); }, self.auto_semi_colon(), "Expected a ';' following a class field");
                let infer_name = if initializer != B::Expression::default() { InferName::Allowed } else { InferName::Disallowed };
                if computed_property_name != B::Expression::default() {
                    property = context.create_property_identifier_computed(&ident, computed_property_name, initializer, type_, SuperBinding::NotNeeded, tag);
                } else {
                    property = context.create_property_named(Some(&ident), initializer, type_, SuperBinding::NotNeeded, infer_name, tag);
                }
            } else if parse_mode == SourceParseMode::ClassStaticBlockMode {
                match_or_fail_hooked!(self, @hook { class_cleanup!(self); }, OPENBRACE, "Expected block statement for class static block");
                let current = self.current_scope();
                let used_variables_size = self.scope_stack[current].current_used_variables_size();
                self.scope_stack[current].push_used_variable_set();
                // DepthManager statementDepth: restaurado em cada saída.
                let old_statement_depth = self.statement_depth;
                self.statement_depth = 0;
                let parsed = self.parse_block_statement(context, BlockType::StaticBlock);
                fail_if_false!(self, @hook { self.statement_depth = old_statement_depth; class_cleanup!(self); }, parsed.is_some(), "Cannot parse class static block");
                // `makeIdentifier(vm, SymbolImpl*)` só guarda o `Identifier::fromUid(*symbol)` na arena e
                // o devolve; o `Identifier` (handle contado) já vale por si.
                ident = vm.property_names.builtin_names().static_initializer_block_private_name();
                property = context.create_property_identifier(&ident, type_, SuperBinding::Needed, tag);
                self.scope_stack[class_scope.scope()].mark_last_used_variables_set_as_captured(used_variables_size);
                self.statement_depth = old_statement_depth;
            } else {
                let mut method_info = ParserFunctionInfo::<B>::default();
                let is_constructor = tag == ClassElementTag::Instance && ident == vm.property_names.constructor;
                semantic_fail_if_true_hooked!(self, @hook { class_cleanup!(self); }, is_constructor && parse_mode != SourceParseMode::MethodMode,
                    "Cannot declare ", string_article_for_function_mode(parse_mode), string_for_function_mode(parse_mode), " named 'constructor'");

                method_info.name = if is_constructor { info.class_name.clone() } else { Some(ident.clone()) };
                // SetForScope innerParseMode: restaurado em cada saída.
                let old_parse_mode = self.parse_mode;
                self.parse_mode = parse_mode;
                fail_if_false!(self, @hook { self.parse_mode = old_parse_mode; class_cleanup!(self); }, self.parse_function_info(context, FunctionNameRequirements::Unnamed, false, if is_constructor { constructor_kind } else { ConstructorKind::None }, SuperBinding::Needed, function_start, &mut method_info, FunctionDefinitionType::Method, None), "Cannot parse this method");

                let method = context.create_method_definition(&method_location, &method_info);
                if is_constructor {
                    semantic_fail_if_true_hooked!(self, @hook { self.parse_mode = old_parse_mode; class_cleanup!(self); }, constructor != B::Expression::default(), "Cannot declare multiple constructors in a single class");
                    constructor = method;
                    self.parse_mode = old_parse_mode;
                    continue;
                }

                semantic_fail_if_true_hooked!(self, @hook { self.parse_mode = old_parse_mode; class_cleanup!(self); }, tag == ClassElementTag::Static && method_info.name.as_ref().is_some_and(|name| *name == vm.property_names.prototype),
                    "Cannot declare a static method named 'prototype'");

                if computed_property_name != B::Expression::default() {
                    property = context.create_property_computed(computed_property_name, method, type_, SuperBinding::Needed, tag);
                } else {
                    property = context.create_property_named(method_info.name.as_ref(), method, type_, SuperBinding::Needed, InferName::Allowed, tag);
                }
                self.parse_mode = old_parse_mode;
            }

            if class_elements_tail != B::PropertyList::default() {
                class_elements_tail = context.create_property_list_append(&method_location, property, class_elements_tail);
            } else {
                class_elements_tail = context.create_property_list(&method_location, property);
                class_elements = class_elements_tail.clone();
            }
        }

        info.end_offset = self.token_location().end_offset.wrapping_sub(1);
        consume_or_fail!(self, @hook { class_cleanup!(self); }, CLOSEBRACE, "Expected a closing '}' after a class body");

        if declares_private_method || declares_private_accessor || declares_static_private_method || declares_static_private_accessor {
            {
                let private_brand_identifier = vm.property_names.builtin_names().private_brand_private_name();
                let declaration_result = self.scope_stack[class_scope.scope()].declare_lexical_variable(&private_brand_identifier, true, DeclarationImportType::NotImported, false, false);
                debug_assert!(declaration_result == DeclarationResult::VALID);
                self.scope_stack[class_scope.scope()].use_variable_identifier(&private_brand_identifier, false);
                self.scope_stack[class_scope.scope()].add_closed_variable_candidate_unconditionally(&uid(&private_brand_identifier));
            }
            {
                let private_class_brand_identifier = vm.property_names.builtin_names().private_class_brand_private_name();
                let declaration_result = self.scope_stack[class_scope.scope()].declare_lexical_variable(&private_class_brand_identifier, true, DeclarationImportType::NotImported, false, false);
                debug_assert!(declaration_result == DeclarationResult::VALID);
                self.scope_stack[class_scope.scope()].use_variable_identifier(&private_class_brand_identifier, false);
                self.scope_stack[class_scope.scope()].add_closed_variable_candidate_unconditionally(&uid(&private_class_brand_identifier));
            }
        }

        // `if constexpr (std::is_same_v<TreeBuilder, ASTBuilder>)`: só o `ASTBuilder` cria a árvore.
        if B::CREATES_AST && class_elements != B::PropertyList::default() {
            context.set_has_private_accessors(&class_elements, declares_private_accessor || declares_static_private_accessor);
        }

        let (lexical_environment, function_declarations) = self.pop_scope_auto(&mut class_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);
        let (class_head_environment, class_head_function_declarations) = self.pop_scope_auto(&mut class_head_scope, B::NEEDS_FREE_VARIABLE_INFO, false, &[]);
        debug_assert!(function_declarations.is_empty());
        debug_assert!(class_head_function_declarations.is_empty());
        let result = context.create_class_expr(&location, info, class_head_environment, lexical_environment, constructor, parent_class, class_elements, start, divot, class_head_end);
        class_cleanup!(self);
        Some(result)
    }

    /// `template <class TreeBuilder> TreeSourceElements parseClassFieldInitializerSourceElements(...)`.
    pub(crate) fn parse_class_field_initializer_source_elements<B: TreeBuilder>(
        &mut self,
        context: &mut B,
        class_element_definitions: &FixedVector<ClassElementDefinition>,
    ) -> Option<B::SourceElements> {
        let source_elements = context.create_source_elements();
        let current = self.current_scope();
        self.scope_stack[current].set_is_class_scope();

        // Clear errors from parsing anything before the initializer expressions.
        self.lexer.clear_error_code_and_buffers();

        for definition in class_element_definitions.iter() {
            let position = definition.position.clone();
            let has_line_terminator_before_token = false;

            let statement: B::Statement;
            if definition.kind == ClassElementDefinitionKind::StaticInitializationBlock {
                {
                    let mut loc = JSTokenLocation::default();
                    loc.line = position.line;
                    loc.line_start_offset = position.line_start_offset as u32;
                    loc.start_offset = position.offset as u32;
                    loc.end_offset = position.offset as u32;
                    self.restore_lexer_state(&LexerState {
                        start_offset: position.offset,
                        old_line_start_offset: position.line_start_offset as u32,
                        last_token_location: loc,
                        last_token_end_position: position.clone(),
                        old_line_number: position.line as u32,
                        has_line_terminator_before_token,
                        last_token_type: ERRORTOK,
                    });
                }
                let start_location = self.token_location();
                let start_position = self.token_start_position().clone();
                let expression_start = self.token_start();

                debug_assert!(self.match_(RESERVED_IF_STRICT) && self.token.data.ident.as_ref() == Some(&self.vm.property_names.static_keyword));
                self.next(LexerFlagSet::empty());

                let mut function_info = ParserFunctionInfo::<B>::default();
                function_info.name = Some(self.vm.property_names.null_identifier.clone());
                // SetForScope setInnerParseMode: restaurado em cada saída.
                let old_parse_mode = self.parse_mode;
                self.parse_mode = SourceParseMode::ClassStaticBlockMode;
                fail_if_false!(self, @hook { self.parse_mode = old_parse_mode; }, self.parse_function_info(context, FunctionNameRequirements::None, false, ConstructorKind::None, SuperBinding::Needed, expression_start, &mut function_info, FunctionDefinitionType::Expression, None), "Cannot parse static block function");
                let expression = context.create_function_expr(&start_location, &function_info);

                let call_end_position = self.last_token_end_position();
                let expression = context.make_static_block_function_call_node(&start_location, expression, call_end_position.clone(), start_position.clone(), call_end_position);
                statement = context.create_expr_statement(&start_location, expression, start_position, self.last_token_location.line);
                self.parse_mode = old_parse_mode;
            } else {
                let mut location = JSTokenLocation::default();
                location.line = position.line;
                location.line_start_offset = position.line_start_offset as u32;
                location.start_offset = position.offset as u32;

                let mut initializer: B::Expression = Default::default();
                if let Some(initializer_position) = definition.initializer_position.clone() {
                    {
                        let mut loc = JSTokenLocation::default();
                        loc.line = initializer_position.line;
                        loc.line_start_offset = initializer_position.line_start_offset as u32;
                        loc.start_offset = initializer_position.offset as u32;
                        loc.end_offset = initializer_position.offset as u32;
                        self.restore_lexer_state(&LexerState {
                            start_offset: initializer_position.offset,
                            old_line_start_offset: initializer_position.line_start_offset as u32,
                            last_token_location: loc,
                            last_token_end_position: initializer_position.clone(),
                            old_line_number: initializer_position.line as u32,
                            has_line_terminator_before_token,
                            last_token_type: ERRORTOK,
                        });
                    }
                    // parseExpression() is more permissive way to parse AssignmentExpression than parseAssignmentExpression() that is used in parseClass().
                    // This is very intentional: we need to fail for `foo = 1, 2` but support reparsing `foo = (1, 2)`, which is tricky because open paren
                    // is skipped (meaning start offset points to `1`) by parsePrimaryExpression().
                    let parsed = self.parse_expression(context);
                    fail_if_false!(self, parsed.is_some(), "Cannot parse expression statement");
                    initializer = parsed.unwrap_or_default();
                }

                let mut type_ = DefineFieldType::Name;
                if definition.kind == ClassElementDefinitionKind::FieldWithComputedPropertyKey {
                    type_ = DefineFieldType::ComputedName;
                } else if definition.kind == ClassElementDefinitionKind::FieldWithPrivatePropertyKey {
                    type_ = DefineFieldType::PrivateName;
                    let current = self.current_scope();
                    self.scope_stack[current].use_variable_identifier(&definition.ident, false);
                }

                let define_field = context.create_define_field(&location, &definition.ident, initializer, type_);
                statement = define_field.into();
            }

            context.append_statement(&source_elements, statement);
        }

        debug_assert!(!self.has_error());
        // Trick parseInner() into believing we've parsed the entire SourceCode, in order to prevent it from producing an error.
        self.token.type_ = EOFTOK;
        Some(source_elements)
    }

    /// `template <class TreeBuilder> TreeStatement parseExpressionOrLabelStatement(TreeBuilder&, bool)`.
    pub(crate) fn parse_expression_or_label_statement<B: TreeBuilder>(&mut self, context: &mut B, allow_function_declaration_as_statement: bool) -> Option<B::Statement> {
        /* Expression and Label statements are ambiguous at LL(1), so we have a
         * special case that looks for a colon as the next character in the input.
         */
        let mut labels: Vec<LabelInfo> = Vec::new();
        let mut location = JSTokenLocation::default();
        loop {
            if !self.next_token_is_colon() {
                // If we hit this path we're making a expression statement, which
                // by definition can't make use of continue/break so we can just
                // ignore any labels we might have accumulated.
                return self.parse_expression_statement(context);
            }

            semantic_fail_if_true!(self, self.is_possibly_escaped_let(&self.token) && self.strict_mode(), "Cannot use 'let' as a label ", self.disallowed_identifier_let_reason());
            semantic_fail_if_true!(self, self.is_disallowed_identifier_await(&self.token), "Cannot use 'await' as a label ", self.disallowed_identifier_await_reason());
            semantic_fail_if_true!(self, self.is_disallowed_identifier_yield(&self.token), "Cannot use 'yield' as a label ", self.disallowed_identifier_yield_reason());

            let Some(ident) = self.token.data.ident.clone() else {
                fail_due_to_unexpected_token!(self);
            };
            let start = self.token_start_position().clone();
            let end = self.token_end_position().clone();
            location = self.token_location();
            self.next(LexerFlagSet::empty());
            consume_or_fail!(self, COLON, "Labels must be followed by a ':'");

            // This is O(N^2) over the current list of consecutive labels, but I
            // have never seen more than one label in a row in the real world.
            for i in 0..labels.len() {
                fail_if_true!(self, ident.impl_() == labels[i].ident.impl_(), "Attempted to redeclare the label '", ident, "'");
            }
            fail_if_true!(self, self.get_label(&ident).is_some(), "Cannot find scope for the label '", ident, "'");
            labels.push(LabelInfo { ident, start, end });
            if !self.match_spec_identifier() {
                break;
            }
        }
        let is_loop = matches!(self.token.type_, FOR | WHILE | DO);
        let mut unused: Option<Identifier> = None;
        let label_scope = self.current_scope();
        for label in &labels {
            self.push_label(&label.ident, is_loop);
        }
        self.immediate_parent_allows_function_declaration_in_statement = allow_function_declaration_as_statement;
        let statement = self.parse_statement(context, &mut unused, None);
        for _ in 0..labels.len() {
            self.pop_label(label_scope);
        }
        fail_if_false!(self, statement.is_some(), "Cannot parse statement");
        let mut statement = statement.unwrap_or_default();
        for i in 0..labels.len() {
            let info = &labels[labels.len() - i - 1];
            statement = context.create_label_statement(&location, &info.ident, statement, info.start.clone(), info.end.clone());
        }
        Some(statement)
    }

    /// `template <class TreeBuilder> TreeStatement parseExpressionStatement(TreeBuilder&)`.
    pub(crate) fn parse_expression_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        match self.token.type_ {
            // https://tc39.es/ecma262/#sec-expression-statement
            // Despite the spec's requirement to fail from FUNCTION token here, Annex B.3.1
            // permits a labelled FunctionDeclaration in sloppy mode for web compatibility
            // reasons. We implement this semantics in parseStatement().
            CLASSTOKEN => {
                fail_with_message!(self, "'class' declaration is not directly within a block statement");
            }
            LET => {
                let save_point = self.create_save_point(context);
                self.next(LexerFlagSet::empty());
                fail_if_true!(self, self.match_(OPENBRACKET), "Cannot use lexical declaration in single-statement context");
                self.restore_save_point(context, &save_point);
            }
            IDENT => {
                if self.token.data.ident.as_ref() == Some(&self.vm.property_names.r#async) && !self.token.data.escaped {
                    let save_point = self.create_save_point(context);
                    self.next(LexerFlagSet::empty());
                    fail_if_true!(self, self.match_(FUNCTION) && !self.lexer.has_line_terminator_before_token(), "Cannot use async function declaration in single-statement context");
                    self.restore_save_point(context, &save_point);
                }
            }
            _ => {}
        }

        let start = self.token_start_position().clone();
        let location = self.token_location();
        let expression = self.parse_expression(context);
        fail_if_false!(self, expression.is_some(), "Cannot parse expression statement");
        let expression = expression.unwrap_or_default();
        if !self.auto_semi_colon() {
            fail_due_to_unexpected_token!(self);
        }
        Some(context.create_expr_statement(&location, expression, start, self.last_token_location.line))
    }

    /// `template <class TreeBuilder> TreeStatement parseIfStatement(TreeBuilder&)`.
    pub(crate) fn parse_if_statement<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::Statement> {
        debug_assert!(self.match_(IF));
        let if_location = self.token_location();
        let start = self.token_line();
        self.next(LexerFlagSet::empty());
        handle_production_or_fail2!(self, OPENPAREN, "(", "start", "'if' condition");

        let condition = self.parse_expression(context);
        fail_if_false!(self, condition.is_some(), "Expected an expression as the condition for an if statement");
        let condition = condition.unwrap_or_default();
        self.record_pause_location(context.breakpoint_location(&condition));
        let end = self.token_line();
        handle_production_or_fail2!(self, CLOSEPAREN, ")", "end", "'if' condition");

        let mut unused: Option<Identifier> = None;
        self.immediate_parent_allows_function_declaration_in_statement = true;
        let true_block = self.parse_statement(context, &mut unused, None);
        fail_if_false!(self, true_block.is_some(), "Expected a statement as the body of an if block");
        let true_block = true_block.unwrap_or_default();

        if !self.match_(ELSE) {
            return Some(context.create_if_statement(&if_location, condition, true_block, Default::default(), start, end));
        }

        let mut expr_stack: Vec<(B::Expression, i32, i32, JSTokenLocation)> = Vec::new();
        let mut statement_stack: Vec<B::Statement> = Vec::new();
        let mut trailing_else = false;
        loop {
            let temp_location = self.token_location();
            self.next(LexerFlagSet::empty());
            if !self.match_(IF) {
                let mut unused: Option<Identifier> = None;
                self.immediate_parent_allows_function_declaration_in_statement = true;
                let block = self.parse_statement(context, &mut unused, None);
                fail_if_false!(self, block.is_some(), "Expected a statement as the body of an else block");
                statement_stack.push(block.unwrap_or_default());
                trailing_else = true;
                break;
            }
            let inner_start = self.token_line();
            self.next(LexerFlagSet::empty());

            handle_production_or_fail2!(self, OPENPAREN, "(", "start", "'if' condition");

            let inner_condition = self.parse_expression(context);
            fail_if_false!(self, inner_condition.is_some(), "Expected an expression as the condition for an if statement");
            let inner_condition = inner_condition.unwrap_or_default();
            self.record_pause_location(context.breakpoint_location(&inner_condition));
            let inner_end = self.token_line();
            handle_production_or_fail2!(self, CLOSEPAREN, ")", "end", "'if' condition");
            let mut unused: Option<Identifier> = None;
            self.immediate_parent_allows_function_declaration_in_statement = true;
            let inner_true_block = self.parse_statement(context, &mut unused, None);
            fail_if_false!(self, inner_true_block.is_some(), "Expected a statement as the body of an if block");
            expr_stack.push((inner_condition, inner_start, inner_end, temp_location));
            statement_stack.push(inner_true_block.unwrap_or_default());
            if !self.match_(ELSE) {
                break;
            }
        }

        if !trailing_else {
            let (condition, start, end, location) = expr_stack.pop().expect("takeLast em vetor vazio");
            let true_block = statement_stack.pop().expect("takeLast em vetor vazio");
            let if_statement = context.create_if_statement(&location, condition, true_block.clone(), Default::default(), start, end);
            let true_block_end_offset = context.end_offset(&true_block);
            context.set_end_offset(&if_statement, true_block_end_offset);
            statement_stack.push(if_statement);
        }

        while !expr_stack.is_empty() {
            let (condition, start, end, location) = expr_stack.pop().expect("takeLast em vetor vazio");
            let false_block = statement_stack.pop().expect("takeLast em vetor vazio");
            let true_block = statement_stack.pop().expect("takeLast em vetor vazio");
            let if_statement = context.create_if_statement(&location, condition, true_block, false_block.clone(), start, end);
            let false_block_end_offset = context.end_offset(&false_block);
            context.set_end_offset(&if_statement, false_block_end_offset);
            statement_stack.push(if_statement);
        }

        let last_statement = statement_stack.last().cloned().unwrap_or_default();
        Some(context.create_if_statement(&if_location, condition, true_block, last_statement, start, end))
    }

    /// `template <class TreeBuilder> typename TreeBuilder::ModuleName parseModuleName(TreeBuilder&)`.
    pub(crate) fn parse_module_name<B: TreeBuilder>(&mut self, context: &mut B) -> Option<B::ModuleName> {
        // ModuleName (ModuleSpecifier in the spec) represents the module name imported by the script.
        // http://www.ecma-international.org/ecma-262/6.0/#sec-imports
        // http://www.ecma-international.org/ecma-262/6.0/#sec-exports
        let specifier_location = self.token_location();
        fail_if_false!(self, self.match_(STRING), "Imported modules names must be string literals");
        let module_name = self.token.data.ident.clone().expect("ASSERT(m_token.m_data.ident)"); // Invariante: o lexer sempre preenche `ident` nos tokens IDENT, STRING e PRIVATENAME já conferidos acima.
        self.next(LexerFlagSet::empty());
        Some(context.create_module_name(&specifier_location, &module_name))
    }
}
