// Terceira fatia de `parser/Parser.h` (linhas 1605 a 2415), incluída por `include!` no fim de `parser.rs`.
// Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Convenções desta fatia (as de `parser_part2.rs` valem):
// - `Scope*` vira `ScopeRef` (índice em `Parser::scope_stack`).
// - As declarações sem corpo do `.h` (`parseStatement`, `logError`, `isBinaryOperator`,
//   `allowAutomaticSemicolon`, `printUnexpectedTokenText`, `recordPauseLocation`, `declareRestOrNormalParameter`
//   etc.) têm o corpo em `Parser.cpp` e não entram aqui.
// - `match` é palavra reservada do Rust: `match_`. As sobrecargas viram nomes distintos:
//   `get_token`/`get_token_for`, `match_spec_identifier`/`match_spec_identifier_with`.
// - `OptionSet<LexerFlags> flags = { }` não tem argumento padrão em Rust: o chamador passa
//   `LexerFlagSet::empty()`.
// - `internalSaveState(context, savePoint&)` devolve o `SavePoint` por valor (o C++ preenche um
//   parâmetro de saída; o `SavePointWithError` precisa do `SavePoint` embutido já montado).
// - `ParsedNode` (`ProgramNode`, `ModuleProgramNode`, `EvalNode`, `FunctionNode`...) é o trait
//   `ParsedNode`, definido abaixo.

/// `enum class ExportType { Exported, NotExported }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportType {
    Exported,
    NotExported,
}

/// `enum class FunctionDeclarationType { Declaration, Statement }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionDeclarationType {
    Declaration,
    Statement,
}

/// `enum class BlockType : uint8_t { Normal, CatchBlock, StaticBlock }`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockType {
    Normal,
    CatchBlock,
    StaticBlock,
}

/// `enum VarDeclarationListContext { ForLoopContext, VarDeclarationContext }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarDeclarationListContext {
    ForLoopContext,
    VarDeclarationContext,
}

/// `enum class ImportSpecifierType { NamespaceImport, NamedImport, DefaultImport }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportSpecifierType {
    NamespaceImport,
    NamedImport,
    DefaultImport,
}

/// `enum class FunctionDefinitionType { Expression, Declaration, Method }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionDefinitionType {
    Expression,
    Declaration,
    Method,
}

/// `isFunctionMetadataNode(ScopeNode*)` (false) e `isFunctionMetadataNode(FunctionMetadataNode*)`
/// (true): a sobrecarga por tipo do nó vira constante associada.
pub trait IsFunctionMetadataNode {
    const IS_FUNCTION_METADATA_NODE: bool = false;
}

/// O que `Parser::parse<ParsedNode>` exige do nó raiz: `ParsedNode::scopeIsFunction`, o
/// construtor chamado por `makeUnique<ParsedNode>(...)` (Parser.h 2230 a 2245; cada nó ignora os
/// argumentos que o seu construtor ignora) e `setLoc`/`setEndOffset`.
pub trait ParsedNode: IsEvalNode + IsFunctionMetadataNode + Sized {
    /// `static constexpr bool scopeIsFunction`.
    const SCOPE_IS_FUNCTION: bool;

    #[allow(clippy::too_many_arguments)]
    fn create(
        parser_arena: &mut ParserArena,
        start_location: &JSTokenLocation,
        end_location: &JSTokenLocation,
        start_column: u32,
        end_column: u32,
        source_elements: Link<SourceElements>,
        var_declarations: VariableEnvironment,
        function_declarations: FunctionStack,
        lexical_variables: VariableEnvironment,
        parameters: Link<FunctionParameters>,
        source: &SourceCode,
        features: CodeFeatures,
        lexically_scoped_features: LexicallyScopedFeatures,
        inner_arrow_function_features: InnerArrowFunctionCodeFeatures,
        num_constants: i32,
        module_scope_data: Option<Rc<ModuleScopeData>>,
    ) -> Box<Self>;

    fn set_loc(&mut self, first_line: u32, last_line: u32, start_offset: i32, line_start_offset: i32);
    fn set_end_offset(&mut self, offset: i32);
}

impl<T: CharType> Parser<T> {
    #[inline(always)]
    fn next_token_is_colon(&mut self) -> bool {
        self.lexer.next_token_is_colon()
    }

    #[inline(always)]
    fn consume(&mut self, expected: JSTokenType) -> bool {
        self.consume_with_flags(expected, LexerFlagSet::empty())
    }

    /// `consume(expected, flags)`.
    #[inline(always)]
    fn consume_with_flags(&mut self, expected: JSTokenType, flags: LexerFlagSet) -> bool {
        let result = self.token.type_ == expected;
        if result {
            self.next(flags);
        }
        result
    }

    #[inline(always)]
    fn get_token(&self) -> WtfString {
        self.lexer.get_token(&self.token)
    }

    #[inline(always)]
    fn get_token_for(&self, token: &JSToken) -> WtfString {
        self.lexer.get_token(token)
    }

    #[inline(always)]
    fn match_(&self, expected: JSTokenType) -> bool {
        self.token.type_ == expected
    }

    #[inline(always)]
    fn match_and_update(&mut self, expected: JSTokenType, token: &JSToken) -> bool {
        if self.match_(expected) {
            self.token = token.clone();
            return true;
        }

        false
    }

    #[inline(always)]
    fn match_contextual_keyword(&self, identifier: &Identifier) -> bool {
        self.token.type_ == IDENT && self.token.data.ident.as_ref() == Some(identifier) && !self.token.data.escaped
    }

    #[inline(always)]
    fn match_identifier_or_keyword(&self) -> bool {
        is_identifier_or_keyword(&self.token)
    }

    #[inline(always)]
    fn token_start(&self) -> u32 {
        self.token.start_position.offset as u32
    }

    #[inline(always)]
    fn token_start_position(&self) -> &JSTextPosition {
        &self.token.start_position
    }

    #[inline(always)]
    fn token_line(&self) -> i32 {
        self.token.start_position.line
    }

    #[inline(always)]
    fn token_column(&self) -> i32 {
        self.token_start().wrapping_sub(self.token_line_start()) as i32
    }

    #[inline(always)]
    fn token_end_position(&self) -> &JSTextPosition {
        &self.token.end_position
    }

    #[inline(always)]
    fn token_line_start(&self) -> u32 {
        self.token.start_position.line_start_offset as u32
    }

    #[inline(always)]
    fn token_location(&self) -> JSTokenLocation {
        self.token.location()
    }

    fn set_error_message(&mut self, message: &WtfString) {
        debug_assert!(!message.is_empty(), "Attempted to set the empty string as an error message. Likely caused by invalid UTF8 used when creating the message.");
        self.error_message = message.clone();
        if self.error_message.is_empty() {
            self.error_message = WtfString::from_latin1(b"Unparseable script");
        }
    }

    /// `NEVER_INLINE`.
    fn update_error_with_name_and_message(&mut self, before_message: &str, name: &WtfString, after_message: &str) {
        // makeString(beforeMessage, " '"_s, name, "' "_s, afterMessage)
        self.error_message = make_string_by_joining(
            &[
                WtfString::from_latin1(before_message.as_bytes()),
                WtfString::from_latin1(b" '"),
                name.clone(),
                WtfString::from_latin1(b"' "),
                WtfString::from_latin1(after_message.as_bytes()),
            ],
            &WtfString::default(),
        );
    }

    /// `NEVER_INLINE`.
    fn update_error_message(&mut self, msg: &str) {
        self.error_message = WtfString::from_latin1(msg.as_bytes());
        debug_assert!(!self.error_message.is_null());
    }

    fn start_loop(&mut self) {
        let scope = self.current_scope();
        self.scope_stack[scope].start_loop();
    }

    fn end_loop(&mut self) {
        let scope = self.current_scope();
        self.scope_stack[scope].end_loop();
    }

    fn start_switch(&mut self) {
        let scope = self.current_scope();
        self.scope_stack[scope].start_switch();
    }

    fn end_switch(&mut self) {
        let scope = self.current_scope();
        self.scope_stack[scope].end_switch();
    }

    fn implementation_visibility(&self) -> ImplementationVisibility {
        self.scope_stack[self.current_scope()].implementation_visibility()
    }

    fn lexically_scoped_features(&self) -> LexicallyScopedFeatures {
        self.scope_stack[self.current_scope()].lexically_scoped_features()
    }

    fn set_strict_mode(&mut self) {
        let scope = self.current_scope();
        self.scope_stack[scope].set_strict_mode();
    }

    fn strict_mode(&self) -> bool {
        self.scope_stack[self.current_scope()].strict_mode()
    }

    fn is_valid_strict_mode(&self) -> bool {
        let current = self.current_scope();
        if !self.scope_stack[current].is_valid_strict_mode() {
            return false;
        }

        // In the case of Generator or Async function bodies, also check the wrapper function, whose name or
        // arguments may be invalid.
        let scope = &self.scope_stack[current];
        if scope.is_generator_function_boundary() || scope.is_async_function_boundary() {
            if let Some(containing) = scope.containing_scope() {
                return self.scope_stack[containing].is_valid_strict_mode();
            }
        }
        true
    }

    fn declare_parameter(&mut self, ident: &Identifier) -> DeclarationResultMask {
        let scope = self.current_scope();
        self.scope_stack[scope].declare_parameter(ident)
    }

    fn break_is_valid(&self) -> bool {
        let mut current = self.current_scope();
        while !self.scope_stack[current].break_is_valid() {
            if !self.scope_stack[current].has_containing_scope() || self.scope_stack[current].is_static_block_boundary() {
                return false;
            }
            current = self.scope_stack[current].containing_scope().expect("containingScope nulo"); // Invariante: o laço só sobe enquanto o escopo não é o da raiz, que sempre tem pai.
        }
        true
    }

    fn continue_is_valid(&self) -> bool {
        let mut current = self.current_scope();
        while !self.scope_stack[current].continue_is_valid() {
            if !self.scope_stack[current].has_containing_scope() || self.scope_stack[current].is_static_block_boundary() {
                return false;
            }
            current = self.scope_stack[current].containing_scope().expect("containingScope nulo"); // Invariante: o laço só sobe enquanto o escopo não é o da raiz, que sempre tem pai.
        }
        true
    }

    fn push_label(&mut self, label: &Identifier, is_loop: bool) {
        let scope = self.current_scope();
        self.scope_stack[scope].push_label(label, is_loop);
    }

    /// `popLabel(Scope*)`.
    fn pop_label(&mut self, scope: ScopeRef) {
        self.scope_stack[scope].pop_label();
    }

    fn get_label(&self, label: &Identifier) -> Option<&ScopeLabelInfo> {
        let mut current = self.current_scope();
        loop {
            if let Some(result) = self.scope_stack[current].get_label(label) {
                return Some(result);
            }
            if !self.scope_stack[current].has_containing_scope() {
                return None;
            }
            current = self.scope_stack[current].containing_scope().expect("containingScope nulo"); // Invariante: o laço só sobe enquanto o escopo não é o da raiz, que sempre tem pai.
        }
    }

    #[inline(always)]
    fn match_spec_identifier(&self) -> bool {
        self.match_(IDENT) || self.is_allowed_identifier_let(&self.token) || self.is_allowed_identifier_yield(&self.token) || self.is_possibly_escaped_await(&self.token)
    }

    /// Special case where some information is already known.
    #[inline(always)]
    fn match_spec_identifier_with(&self, can_use_yield: bool, is_await: bool) -> bool {
        is_await || self.match_(IDENT) || self.is_allowed_identifier_let(&self.token) || (can_use_yield && self.is_possibly_escaped_yield(&self.token))
    }

    #[inline(always)]
    fn match_identifier_or_possibly_escaped_contextual_keyword(&self) -> bool {
        self.match_(IDENT) || self.is_possibly_escaped_let(&self.token) || self.is_possibly_escaped_yield(&self.token) || self.is_possibly_escaped_await(&self.token)
    }

    fn auto_semi_colon(&mut self) -> bool {
        if self.token.type_ == SEMICOLON {
            self.next(LexerFlagSet::empty());
            return true;
        }
        self.allow_automatic_semicolon()
    }

    fn last_token_end_position(&self) -> JSTextPosition {
        JSTextPosition::new(self.last_token_location.line, self.last_token_location.end_offset as i32, self.last_token_location.line_start_offset as i32)
    }

    fn has_error(&self) -> bool {
        !self.error_message.is_null()
    }

    fn is_allowed_identifier_let(&self, token: &JSToken) -> bool {
        self.is_possibly_escaped_let(token) && !self.strict_mode()
    }

    #[inline(always)]
    fn is_possibly_escaped_let(&self, token: &JSToken) -> bool {
        if token.type_ == LET {
            return true;
        }
        if token.type_ == ESCAPED_KEYWORD && token.data.ident.as_ref() == Some(&self.vm.property_names.let_keyword) {
            return true;
        }
        false
    }

    fn is_disallowed_identifier_await(&self, token: &JSToken) -> bool {
        self.is_possibly_escaped_await(token) && !self.can_use_identifier_await()
    }

    fn is_allowed_identifier_await(&self, token: &JSToken) -> bool {
        self.is_possibly_escaped_await(token) && self.can_use_identifier_await()
    }

    #[inline(always)]
    fn is_possibly_escaped_await(&self, token: &JSToken) -> bool {
        if token.type_ == AWAIT {
            return true;
        }
        if token.type_ == ESCAPED_KEYWORD && token.data.ident.as_ref() == Some(&self.vm.property_names.await_keyword) {
            return true;
        }
        false
    }

    #[inline(always)]
    fn can_use_identifier_await(&self) -> bool {
        let scope = &self.scope_stack[self.current_scope()];
        self.parser_state.allow_await && !scope.is_async_function() && !scope.is_static_block() && self.script_mode != JSParserScriptMode::Module
    }

    fn is_disallowed_identifier_yield(&self, token: &JSToken) -> bool {
        self.is_possibly_escaped_yield(token) && !self.can_use_identifier_yield()
    }

    fn is_allowed_identifier_yield(&self, token: &JSToken) -> bool {
        self.is_possibly_escaped_yield(token) && self.can_use_identifier_yield()
    }

    #[inline(always)]
    fn is_possibly_escaped_yield(&self, token: &JSToken) -> bool {
        if token.type_ == YIELD {
            return true;
        }
        if token.type_ == ESCAPED_KEYWORD && token.data.ident.as_ref() == Some(&self.vm.property_names.yield_keyword) {
            return true;
        }
        false
    }

    #[inline(always)]
    fn can_use_identifier_yield(&self) -> bool {
        !self.strict_mode() && !self.scope_stack[self.current_scope()].is_generator_function()
    }

    fn match_allowed_escaped_contextual_keyword(&self) -> bool {
        debug_assert!(self.token.type_ == ESCAPED_KEYWORD);
        let ident = self.token.data.ident.as_ref();
        (ident == Some(&self.vm.property_names.let_keyword) && !self.strict_mode())
            || (ident == Some(&self.vm.property_names.await_keyword) && self.can_use_identifier_await())
            || (ident == Some(&self.vm.property_names.yield_keyword) && self.can_use_identifier_yield())
    }

    fn disallowed_identifier_let_reason(&self) -> &'static str {
        debug_assert!(self.strict_mode());
        "in strict mode"
    }

    fn disallowed_identifier_await_reason(&self) -> &'static str {
        let scope = &self.scope_stack[self.current_scope()];
        if !self.parser_state.allow_await || scope.is_async_function() {
            return "in an async function";
        }
        if scope.is_static_block() {
            return "in a static block";
        }
        if self.script_mode == JSParserScriptMode::Module {
            return "in a module";
        }
        panic!("RELEASE_ASSERT_NOT_REACHED");
    }

    fn disallowed_identifier_yield_reason(&self) -> &'static str {
        if self.strict_mode() {
            return "in strict mode";
        }
        if self.scope_stack[self.current_scope()].is_generator_function() {
            return "in a generator function";
        }
        panic!("RELEASE_ASSERT_NOT_REACHED");
    }

    #[inline(always)]
    fn is_arguments_identifier(&self) -> bool {
        self.token.data.ident.as_ref() == Some(&self.vm.property_names.arguments)
    }

    // If you're using this directly, you probably should be using
    // createSavePoint() instead.
    #[inline(always)]
    fn internal_save_parser_state<TB: TreeBuilder>(&self, context: &TB) -> ParserState {
        let mut parser_state = self.parser_state.clone();
        parser_state.unary_token_stack_depth = context.unary_token_stack_depth();
        parser_state
    }

    #[inline(always)]
    fn restore_parser_state<TB: TreeBuilder>(&mut self, context: &mut TB, state: &ParserState) {
        self.parser_state = state.clone();
        context.set_unary_token_stack_depth(self.parser_state.unary_token_stack_depth);
    }

    // If you're using this directly, you probably should be using
    // createSavePoint() instead.
    // i.e, if you parse any kind of AssignmentExpression between
    // saving/restoring, you should definitely not be using this directly.
    #[inline(always)]
    fn internal_save_lexer_state(&self) -> LexerState {
        let result = LexerState {
            start_offset: self.token.start_position.offset,
            old_line_start_offset: self.token.start_position.line_start_offset as u32,
            last_token_location: self.last_token_location,
            last_token_end_position: self.last_token_end_position,
            old_line_number: self.token.start_position.line as u32,
            // Why is this reading from Lexer fine while we are re-lexing the same token?
            // This is because this flag is updated and indicating whether we have a line
            // terminator before the lexed token, and based on that, we already moved startOffset.
            // So getting this flag and setting it before lexing this token is right.
            has_line_terminator_before_token: self.lexer.has_line_terminator_before_token(),
            last_token_type: self.last_token_type,
        };
        debug_assert!(result.start_offset as u32 >= result.old_line_start_offset);
        result
    }

    #[inline(always)]
    fn restore_lexer_state(&mut self, lexer_state: &LexerState) {
        // setOffset clears lexer errors.
        self.lexer.set_offset(lexer_state.start_offset, lexer_state.old_line_start_offset as i32);
        self.lexer.set_line_number(lexer_state.old_line_number as i32);
        self.lexer.set_has_line_terminator_before_token(lexer_state.has_line_terminator_before_token);
        self.last_token_type = lexer_state.last_token_type;
        self.token.type_ = lexer_state.last_token_type;
        self.token.start_position.line = lexer_state.last_token_location.line;
        self.token.start_position.offset = lexer_state.last_token_location.start_offset as i32;
        self.token.start_position.line_start_offset = lexer_state.last_token_location.line_start_offset as i32;
        self.token.end_position = lexer_state.last_token_end_position;
        self.next_without_clearing_line_terminator(LexerFlagSet::empty());
    }

    #[inline(always)]
    fn internal_save_state<TB: TreeBuilder>(&self, context: &TB) -> SavePoint {
        SavePoint { parser_state: self.internal_save_parser_state(context), lexer_state: self.internal_save_lexer_state() }
    }

    #[inline(always)]
    fn swap_save_point_for_error<TB: TreeBuilder>(&mut self, context: &mut TB, old_save_point: &SavePoint) -> SavePointWithError {
        let mut save_point = SavePointWithError {
            save_point: self.internal_save_state(context),
            lexer_error: self.lexer.saw_error(),
            lexer_error_message: self.lexer.get_error_message(),
            parser_error_message: self.error_message.clone(),
        };
        // Make sure we set our new savepoints unary stack to what oldSavePoint had as it currently may contain stale info.
        save_point.save_point.parser_state.unary_token_stack_depth = old_save_point.parser_state.unary_token_stack_depth;
        self.restore_save_point(context, old_save_point);
        save_point
    }

    #[inline(always)]
    fn create_save_point<TB: TreeBuilder>(&self, context: &TB) -> SavePoint {
        debug_assert!(!self.has_error());
        self.internal_save_state(context)
    }

    #[inline(always)]
    fn internal_restore_state<TB: TreeBuilder>(&mut self, context: &mut TB, save_point: &SavePoint) {
        self.restore_lexer_state(&save_point.lexer_state);
        self.restore_parser_state(context, &save_point.parser_state);
    }

    #[inline(always)]
    fn restore_save_point_with_error<TB: TreeBuilder>(&mut self, context: &mut TB, save_point: &SavePointWithError) {
        self.internal_restore_state(context, &save_point.save_point);
        self.lexer.set_saw_error(save_point.lexer_error);
        self.lexer.set_error_message(&save_point.lexer_error_message);
        self.error_message = save_point.parser_error_message.clone();
    }

    #[inline(always)]
    fn restore_save_point<TB: TreeBuilder>(&mut self, context: &mut TB, save_point: &SavePoint) {
        self.internal_restore_state(context, save_point);
        self.error_message = WtfString::default();
    }

    /// `Parser<LexerType>::parse<ParsedNode>` (Parser.h 2192 a 2282).
    pub fn parse<P: ParsedNode>(
        &mut self,
        error: &mut ParserError,
        callee_name: &Identifier,
        parsing_context: ParsingContext,
        function_constructor_parameters_end_position: Option<i32>,
        parent_scope_private_names: Option<&PrivateNameEnvironment>,
        class_element_definitions: Option<&FixedVector<ClassElementDefinition>>,
    ) -> Option<Box<P>> {
        let mut err_line: i32;
        let mut err_msg: WtfString;
        let parse_mode = self.source_parse_mode();

        if P::SCOPE_IS_FUNCTION {
            self.lexer.set_is_reparsing_function();
        }

        err_line = -1;
        err_msg = WtfString::default();

        let start_location = self.token_location();
        debug_assert!(self.source.start_column().zero_based_int() >= 0);
        let start_column = self.source.start_column().zero_based_int() as u32;

        let parse_result = self.parse_inner(callee_name, parsing_context, function_constructor_parameters_end_position, class_element_definitions, parent_scope_private_names);

        let line_number = self.lexer.line_number();
        let lex_error = self.lexer.saw_error();
        let lex_error_message = if lex_error { self.lexer.get_error_message() } else { WtfString::default() };
        debug_assert!(lex_error_message.is_null() != lex_error);
        self.lexer.clear();

        if parse_result.is_err() || lex_error {
            err_line = line_number;
            err_msg = if !lex_error_message.is_null() {
                lex_error_message
            } else {
                parse_result.as_ref().err().cloned().unwrap_or_default()
            };
        }

        let mut result: Option<Box<P>> = None;
        match parse_result {
            Ok(value) => {
                let mut end_location = JSTokenLocation::default();
                end_location.line = self.lexer.line_number();
                end_location.line_start_offset = self.lexer.current_line_start_offset() as u32;
                end_location.start_offset = self.lexer.current_offset() as u32;
                let end_column = end_location.start_offset - end_location.line_start_offset;
                let current = self.current_scope();
                let lexically_scoped_features = self.scope_stack[current].lexically_scoped_features();
                let inner_arrow_function_features = self.scope_stack[current].inner_arrow_function_features();
                let mut node = P::create(
                    &mut self.parser_arena,
                    &start_location,
                    &end_location,
                    start_column,
                    end_column,
                    value.source_elements,
                    value.var_declarations,
                    value.function_declarations,
                    value.lexical_variables,
                    value.parameters,
                    &self.source,
                    value.features,
                    lexically_scoped_features,
                    inner_arrow_function_features,
                    value.num_constants,
                    self.module_scope_data.take(),
                );
                node.set_loc(self.source.first_line().one_based_int() as u32, self.lexer.line_number() as u32, self.lexer.current_offset(), self.lexer.current_line_start_offset());
                node.set_end_offset(self.lexer.current_offset());

                if !is_function_parse_mode(parse_mode) {
                    if let Some(provider) = self.source.provider() {
                        provider.set_source_url_directive(&self.lexer.source_url_directive());
                        provider.set_source_mapping_url_directive(&self.lexer.source_mapping_url_directive());
                    }
                }
                result = Some(node);
            }
            Err(_) => {
                // We can never see a syntax error when reparsing a function, since we should have
                // reported the error when parsing the containing program or eval code. So if we're
                // parsing a function body node, we assume that what actually happened here is that
                // we ran out of stack while parsing. If we see an error while parsing eval or program
                // code we assume that it was a syntax error since running out of stack is much less
                // likely, and we are currently unable to distinguish between the two cases.
                if P::IS_FUNCTION_METADATA_NODE || self.has_stack_overflow {
                    *error = ParserError::with_token(ErrorType::StackOverflow, SyntaxErrorType::SyntaxErrorNone, self.token.clone());
                } else {
                    let mut error_type = SyntaxErrorType::SyntaxErrorIrrecoverable;
                    if self.token.type_ == EOFTOK {
                        error_type = SyntaxErrorType::SyntaxErrorRecoverable;
                    } else if self.token.type_ & UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG != 0 {
                        // Treat multiline capable unterminated literals as recoverable.
                        if self.token.type_ == UNTERMINATED_MULTILINE_COMMENT_ERRORTOK || self.token.type_ == UNTERMINATED_TEMPLATE_LITERAL_ERRORTOK {
                            error_type = SyntaxErrorType::SyntaxErrorRecoverable;
                        } else {
                            error_type = SyntaxErrorType::SyntaxErrorUnterminatedLiteral;
                        }
                    }

                    if is_eval_node::<P>() {
                        *error = ParserError::with_message(ErrorType::EvalError, error_type, self.token.clone(), &err_msg, err_line);
                    } else {
                        *error = ParserError::with_message(ErrorType::SyntaxError, error_type, self.token.clone(), &err_msg, err_line);
                    }
                }
            }
        }

        result
    }
}

/// O bloco `if (Options::reportParseTimes())` do fim de `parse`, `parseRootNode` e
/// `parseFunctionForFunctionConstructor` (idêntico nas três).
fn report_parse_times_if_needed(source: &SourceCode, succeeded: bool, before: Option<MonotonicTime>) {
    if Options::report_parse_times() {
        let before = before.expect("MonotonicTime de início ausente");
        let after = MonotonicTime::now();
        let hash = ParseHash::new(source);
        let message = if succeeded { "Parsed #" } else { "Failed to parse #" };
        crate::runtime::host::eprintln(&format!("{}{}/#{} in {} ms.", message, hash.hash_for_call(), hash.hash_for_construct(), (after - before).milliseconds()));
    }
}

/// `MonotonicTime before; if (Options::reportParseTimes()) before = MonotonicTime::now();`.
fn start_parse_timer() -> Option<MonotonicTime> {
    if Options::report_parse_times() {
        Some(MonotonicTime::now())
    } else {
        None
    }
}

/// O corpo de cada ramo (8 bits e 16 bits) de `parse`: o C++ duplica o ramo por tipo de caractere,
/// aqui é um genérico sobre `CharType`. `log_builtin_errors` só vale no ramo de 8 bits do C++
/// (Parser.h 2307 a 2313).
#[allow(clippy::too_many_arguments)]
fn parse_with_char_type<T: CharType, P: ParsedNode>(
    vm: &Rc<VM>,
    source: &SourceCode,
    name: &Identifier,
    implementation_visibility: ImplementationVisibility,
    builtin_mode: JSParserBuiltinMode,
    lexically_scoped_features: LexicallyScopedFeatures,
    script_mode: JSParserScriptMode,
    parse_mode: SourceParseMode,
    function_mode: FunctionMode,
    super_binding: SuperBinding,
    error: &mut ParserError,
    constructor_kind: ConstructorKind,
    derived_context_type: DerivedContextType,
    eval_context_type: EvalContextType,
    parent_scope_private_names: Option<&PrivateNameEnvironment>,
    class_element_definitions: Option<&FixedVector<ClassElementDefinition>>,
    is_inside_ordinary_function: bool,
    log_builtin_errors: bool,
) -> Option<Box<P>> {
    let mut parser = Parser::<T>::new(
        vm.clone(),
        source,
        implementation_visibility,
        builtin_mode,
        lexically_scoped_features,
        script_mode,
        parse_mode,
        function_mode,
        super_binding,
        constructor_kind,
        derived_context_type,
        is_eval_node::<P>(),
        eval_context_type,
        None,
        is_inside_ordinary_function,
    );
    let result = parser.parse::<P>(error, name, ParsingContext::Normal, None, parent_scope_private_names, class_element_definitions);
    if log_builtin_errors && builtin_mode == JSParserBuiltinMode::Builtin && result.is_none() {
        debug_assert!(error.is_valid());
        if error.type_() != ErrorType::StackOverflow {
            crate::runtime::host::eprintln(&format!("Unexpected error compiling builtin: {} on line {} for function {}.", String::from_utf8_lossy(&error.message().utf8(crate::wtf::text::conversion_mode::ConversionMode::LenientConversion)), error.line(), String::from_utf8_lossy(&name.utf8())));
        }
    }
    result
}

/// `parse<ParsedNode>(...)` (Parser.h 2284 a 2329).
#[allow(clippy::too_many_arguments)]
pub fn parse<P: ParsedNode>(
    vm: &Rc<VM>,
    source: &SourceCode,
    name: &Identifier,
    implementation_visibility: ImplementationVisibility,
    builtin_mode: JSParserBuiltinMode,
    lexically_scoped_features: LexicallyScopedFeatures,
    script_mode: JSParserScriptMode,
    parse_mode: SourceParseMode,
    function_mode: FunctionMode,
    super_binding: SuperBinding,
    error: &mut ParserError,
    constructor_kind: ConstructorKind,
    derived_context_type: DerivedContextType,
    eval_context_type: EvalContextType,
    parent_scope_private_names: Option<&PrivateNameEnvironment>,
    class_element_definitions: Option<&FixedVector<ClassElementDefinition>>,
    is_inside_ordinary_function: bool,
) -> Option<Box<P>> {
    let before = start_parse_timer();

    let is_8bit = match source.provider() {
        Some(provider) => provider.source().is_8bit(),
        None => panic!("SourceCode sem provider"),
    };
    let result = if is_8bit {
        parse_with_char_type::<LChar, P>(
            vm, source, name, implementation_visibility, builtin_mode, lexically_scoped_features, script_mode, parse_mode, function_mode, super_binding, error,
            constructor_kind, derived_context_type, eval_context_type, parent_scope_private_names, class_element_definitions, is_inside_ordinary_function, true,
        )
    } else {
        parse_with_char_type::<UChar, P>(
            vm, source, name, implementation_visibility, builtin_mode, lexically_scoped_features, script_mode, parse_mode, function_mode, super_binding, error,
            constructor_kind, derived_context_type, eval_context_type, parent_scope_private_names, class_element_definitions, is_inside_ordinary_function, false,
        )
    };

    if Options::count_parse_times() {
        GLOBAL_PARSE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    report_parse_times_if_needed(source, result.is_some(), before);

    result
}

/// O corpo de cada ramo de `parseRootNode`: o do 8 bits chama
/// `overrideConstructorKindForTopLevelFunctionExpressions` e preenche `positionBeforeLastNewline`;
/// o de 16 bits afirma que ambos são triviais (`ASSERT_WITH_MESSAGE`), então o mesmo corpo vale.
#[allow(clippy::too_many_arguments)]
fn parse_root_node_with_char_type<T: CharType, P: ParsedNode>(
    vm: &Rc<VM>,
    source: &SourceCode,
    implementation_visibility: ImplementationVisibility,
    builtin_mode: JSParserBuiltinMode,
    lexically_scoped_features: LexicallyScopedFeatures,
    script_mode: JSParserScriptMode,
    parse_mode: SourceParseMode,
    error: &mut ParserError,
    constructor_kind_for_top_level_function_expressions: ConstructorKind,
    position_before_last_newline: Option<&mut JSTextPosition>,
    debugger_parse_data: Option<Rc<RefCell<DebuggerParseData>>>,
) -> Option<Box<P>> {
    let name = Identifier::default();
    let is_eval_node = false;
    let is_inside_ordinary_function = false;
    let mut parser = Parser::<T>::new(
        vm.clone(),
        source,
        implementation_visibility,
        builtin_mode,
        lexically_scoped_features,
        script_mode,
        parse_mode,
        FunctionMode::None,
        SuperBinding::NotNeeded,
        ConstructorKind::None,
        DerivedContextType::None,
        is_eval_node,
        EvalContextType::None,
        debugger_parse_data,
        is_inside_ordinary_function,
    );
    parser.override_constructor_kind_for_top_level_function_expressions(constructor_kind_for_top_level_function_expressions);
    let result = parser.parse::<P>(error, &name, ParsingContext::Normal, None, None, None);
    if let Some(position) = position_before_last_newline {
        *position = parser.position_before_last_newline();
    }
    result
}

/// `parseRootNode<ParsedNode>(...)` (Parser.h 2331 a 2375). `ParsedNode` é `ProgramNode` ou
/// `ModuleProgramNode` (o `static_assert` vira a convenção de só instanciar com esses dois).
#[allow(clippy::too_many_arguments)]
pub fn parse_root_node<P: ParsedNode>(
    vm: &Rc<VM>,
    source: &SourceCode,
    implementation_visibility: ImplementationVisibility,
    builtin_mode: JSParserBuiltinMode,
    lexically_scoped_features: LexicallyScopedFeatures,
    script_mode: JSParserScriptMode,
    parse_mode: SourceParseMode,
    error: &mut ParserError,
    constructor_kind_for_top_level_function_expressions: ConstructorKind,
    position_before_last_newline: Option<&mut JSTextPosition>,
    debugger_parse_data: Option<Rc<RefCell<DebuggerParseData>>>,
) -> Option<Box<P>> {
    let before = start_parse_timer();

    let is_8bit = match source.provider() {
        Some(provider) => provider.source().is_8bit(),
        None => panic!("SourceCode sem provider"),
    };
    let result = if is_8bit {
        parse_root_node_with_char_type::<LChar, P>(
            vm, source, implementation_visibility, builtin_mode, lexically_scoped_features, script_mode, parse_mode, error,
            constructor_kind_for_top_level_function_expressions, position_before_last_newline, debugger_parse_data,
        )
    } else {
        parse_root_node_with_char_type::<UChar, P>(
            vm, source, implementation_visibility, builtin_mode, lexically_scoped_features, script_mode, parse_mode, error,
            constructor_kind_for_top_level_function_expressions, position_before_last_newline, debugger_parse_data,
        )
    };

    if Options::count_parse_times() {
        GLOBAL_PARSE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    report_parse_times_if_needed(source, result.is_some(), before);

    result
}

/// Corpo de cada ramo de `parseFunctionForFunctionConstructor` (os dois são idênticos no C++).
fn parse_function_for_function_constructor_with_char_type<T: CharType>(
    vm: &Rc<VM>,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    error: &mut ParserError,
    position_before_last_newline: Option<&mut JSTextPosition>,
    function_constructor_parameters_end_position: Option<i32>,
) -> Option<Box<ProgramNode>> {
    let name = Identifier::default();
    let is_eval_node = false;
    let mut parser = Parser::<T>::new(
        vm.clone(),
        source,
        ImplementationVisibility::Public,
        JSParserBuiltinMode::NotBuiltin,
        lexically_scoped_features,
        JSParserScriptMode::Classic,
        SourceParseMode::ProgramMode,
        FunctionMode::None,
        SuperBinding::NotNeeded,
        ConstructorKind::None,
        DerivedContextType::None,
        is_eval_node,
        EvalContextType::None,
        None,
        false,
    );
    let result = parser.parse::<ProgramNode>(error, &name, ParsingContext::FunctionConstructor, function_constructor_parameters_end_position, None, None);
    if let Some(position) = position_before_last_newline {
        *position = parser.position_before_last_newline();
    }
    result
}

/// `parseFunctionForFunctionConstructor` (Parser.h 2377 a 2410).
pub fn parse_function_for_function_constructor(
    vm: &Rc<VM>,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    error: &mut ParserError,
    position_before_last_newline: Option<&mut JSTextPosition>,
    function_constructor_parameters_end_position: Option<i32>,
) -> Option<Box<ProgramNode>> {
    let before = start_parse_timer();

    let is_8bit = match source.provider() {
        Some(provider) => provider.source().is_8bit(),
        None => panic!("SourceCode sem provider"),
    };
    let result = if is_8bit {
        parse_function_for_function_constructor_with_char_type::<LChar>(
            vm, source, lexically_scoped_features, error, position_before_last_newline, function_constructor_parameters_end_position,
        )
    } else {
        parse_function_for_function_constructor_with_char_type::<UChar>(
            vm, source, lexically_scoped_features, error, position_before_last_newline, function_constructor_parameters_end_position,
        )
    };

    if Options::count_parse_times() {
        GLOBAL_PARSE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    report_parse_times_if_needed(source, result.is_some(), before);

    result
}
