// Segunda fatia de `parser/Parser.h` (linhas 801 a 1603), incluída por `include!` no fim de `parser.rs`.
// Sem `use` próprio: os imports ficam em `parser.rs`.
//
// Convenções desta fatia:
// - `Scope*` vira `ScopeRef` (índice em `Parser::scope_stack`); `nullptr` vira `Option<ScopeRef>`.
// - Onde o C++ usa dois `Scope*` ao mesmo tempo (pai e filho), o empréstimo é dividido com
//   `split_at_mut` (o pai sempre fica abaixo do filho na pilha).
// - Os destrutores RAII (`AutoPopScope`, `AutoCleanupLexicalScope`, `AllowInOverride`,
//   `CallOrApplyDepthScope`) não têm `Drop` em Rust (precisariam de `&mut Parser`). Cada um ganha o
//   método `cleanup`/`restore`/`finish`, que o chamador invoca exatamente onde o C++ destruiria o
//   objeto (inclusive nos retornos por erro de sintaxe).
// - Sobrecargas de `popScope` viram `pop_scope`, `pop_scope_auto` e `pop_scope_cleanup`.

// ---------------------------------------------------------------------------------------------
// Fim de `class Scope` (Parser.h 801 a 1000)
// ---------------------------------------------------------------------------------------------

impl Scope {
    pub fn finalize_sloppy_mode_function_hoisting(&mut self) {
        debug_assert!(self.allows_var_declarations());
        debug_assert!(!self.is_simple_catch_parameter_scope());

        for (metadata, _) in self.sloppy_mode_function_hoisting_candidates.iter() {
            // ES6 Annex B.3.3. The only time we can't hoist a function is if a syntax error would
            // be caused by declaring a var with that function's name or if we have a parameter with
            // that function's name. Note that we would only cause a syntax error if we had a let/const/class
            // variable with the same name.
            let function = match metadata.ident.borrow().impl_() {
                Some(function) => function,
                None => continue,
            };
            if !self.lexical_variables.contains(&function) {
                let add_result = self.declared_variables.add(&function);
                if add_result.is_new_entry {
                    add_result.value.set_is_sloppy_mode_hoisted_function();
                } else if add_result.value.is_parameter() {
                    continue;
                }

                add_result.value.set_is_var();
                metadata.is_sloppy_mode_hoisted_function.set(true);
            }
        }
    }

    /// `NEVER_INLINE`. `parent_scope` é o escopo que contém este (o chamador divide a pilha).
    pub fn bubble_sloppy_mode_function_hoisting_candidates(&self, parent_scope: &mut Scope) {
        for (metadata, check) in self.sloppy_mode_function_hoisting_candidates.iter() {
            let needs_check = *check == NeedsDuplicateDeclarationCheck::Yes;
            let in_lexical = match metadata.ident.borrow().impl_() {
                Some(key) => self.lexical_variables.contains(&key),
                None => false,
            };
            if !needs_check || !in_lexical || self.is_simple_catch_parameter_scope() {
                parent_scope.add_sloppy_mode_function_hoisting_candidate(metadata.clone(), NeedsDuplicateDeclarationCheck::Yes);
            }
        }
    }

    pub fn get_captured_vars(&self, captured_variables: &mut IdentifierSet) {
        if self.needs_full_activation || self.uses_eval {
            for (key, _) in self.declared_variables.iter() {
                captured_variables.add(key, ());
            }
            return;
        }
        for (impl_, _) in self.closed_variable_candidates.iter() {
            // We refer to m_declaredVariables here directly instead of a hasDeclaredVariable because we want to mark the callee as captured.
            if !self.declared_variables.contains(impl_) {
                continue;
            }
            captured_variables.add(impl_, ());
        }
    }

    pub fn lexically_scoped_features(&self) -> LexicallyScopedFeatures {
        self.lexically_scoped_features
    }
    pub fn set_lexically_scoped_features(&mut self, features: LexicallyScopedFeatures) {
        self.lexically_scoped_features = features;
    }
    // `setStrictMode`, `setTaintedByWithScope`, `strictMode` e `shadowsArguments` (linhas 850 a 856)
    // já estão em parser.rs.
    pub fn is_valid_strict_mode(&self) -> bool {
        self.is_valid_strict_mode
    }
    pub fn set_has_non_simple_parameter_list(&mut self) {
        self.is_valid_strict_mode = false;
        self.has_non_simple_parameter_list = true;
    }
    pub fn has_non_simple_parameter_list(&self) -> bool {
        self.has_non_simple_parameter_list
    }

    pub fn has_sloppy_mode_function_hoisting_candidates(&self) -> bool {
        !self.sloppy_mode_function_hoisting_candidates.is_empty()
    }

    pub fn copy_captured_variables_to_vector(&self, used_variables: &UniquedStringImplPtrSet, vector: &mut Vec<UniquedKey>) {
        for (impl_, _) in used_variables.iter() {
            if self.declared_variables.contains(impl_) || self.lexical_variables.contains(impl_) {
                continue;
            }
            vector.push(impl_.clone());
        }
    }

    pub fn fill_parameters_for_source_provider_cache(
        &self,
        parameters: &mut SourceProviderCacheItemCreationParameters,
        captures_from_parameter_expressions: &UniquedStringImplPtrSet,
    ) {
        debug_assert!(self.is_function);
        debug_assert!(parameters.used_variables.is_empty());
        parameters.uses_eval = self.uses_eval;
        parameters.uses_import_meta = self.uses_import_meta;
        parameters.lexically_scoped_features = self.lexically_scoped_features;
        parameters.needs_full_activation = self.needs_full_activation;
        parameters.inner_arrow_function_features = self.inner_arrow_function_features;
        parameters.needs_super_binding = self.needs_super_binding;
        for set in self.used_variables.iter() {
            self.copy_captured_variables_to_vector(set, &mut parameters.used_variables);
        }
        parameters.free_variable_count = parameters.used_variables.len() as u32;

        // FIXME: https://bugs.webkit.org/show_bug.cgi?id=156962
        // We add these unconditionally because we currently don't keep a separate
        // declaration scope for a function's parameters and its var/let/const declarations.
        // This is somewhat unfortunate and we should refactor to do this at some point
        // because parameters logically form a parent scope to var/let/const variables.
        // But because we don't do this, we must grab capture candidates from a parameter
        // list before we parse the body of a function because the body's declarations
        // might make us believe something isn't actually a capture candidate when it really
        // is.
        for (impl_, _) in captures_from_parameter_expressions.iter() {
            parameters.used_variables.push(impl_.clone());
        }
        debug_assert!(parameters.free_variable_count as usize + captures_from_parameter_expressions.len() as usize == parameters.used_variables.len());
    }

    pub fn restore_from_source_provider_cache(&mut self, info: &SourceProviderCacheItem) {
        debug_assert!(self.is_function);
        self.uses_eval = info.uses_eval;
        self.uses_import_meta = info.uses_import_meta;
        self.lexically_scoped_features = info.lexically_scoped_features();
        self.inner_arrow_function_features = info.inner_arrow_function_features;
        self.implementation_visibility = info.implementation_visibility;
        self.needs_full_activation = info.needs_full_activation;
        self.needs_super_binding = info.needs_super_binding;
        if let Some(dest_set) = self.used_variables.last_mut() {
            for variable in info.used_variables().iter() {
                dest_set.add(variable, ());
            }
        }
    }

}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgumentType {
    Normal,
    Spread,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsingContext {
    Normal,
    FunctionConstructor,
}

// ---------------------------------------------------------------------------------------------
// `template <typename LexerType> class Parser`
// ---------------------------------------------------------------------------------------------

/// `Parser::ParserState`.
#[derive(Clone, Debug)]
pub struct ParserState {
    pub assignment_count: i32,
    pub non_lhs_count: i32,
    pub non_trivial_expression_count: i32,
    pub return_statement_count: i32,
    pub unary_token_stack_depth: i32,
    pub function_parse_phase: FunctionParsePhase,
    pub last_identifier: Option<Identifier>,
    pub last_function_name: Option<Identifier>,
    pub last_private_name: Option<Identifier>,
    pub allow_await: bool,
    pub is_parsing_class_field_initializer: bool,
    pub class_field_init_masks_async: bool,
}

impl Default for ParserState {
    fn default() -> Self {
        ParserState {
            assignment_count: 0,
            non_lhs_count: 0,
            non_trivial_expression_count: 0,
            return_statement_count: 0,
            unary_token_stack_depth: 0,
            function_parse_phase: FunctionParsePhase::Body,
            last_identifier: None,
            last_function_name: None,
            last_private_name: None,
            allow_await: true,
            is_parsing_class_field_initializer: false,
            class_field_init_masks_async: false,
        }
    }
}

/// `enum class FunctionParsePhase { Parameters, Body }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionParsePhase {
    Parameters,
    Body,
}

/// `Parser::LexerState`.
#[derive(Clone, Debug)]
pub struct LexerState {
    pub start_offset: i32,
    pub old_line_start_offset: u32,
    pub last_token_location: JSTokenLocation,
    pub last_token_end_position: JSTextPosition,
    pub old_line_number: u32,
    pub has_line_terminator_before_token: bool,
    pub last_token_type: JSTokenType,
}

/// `Parser::SavePoint`.
#[derive(Clone, Debug)]
pub struct SavePoint {
    pub parser_state: ParserState,
    pub lexer_state: LexerState,
}

/// `Parser::SavePointWithError : public SavePoint`.
#[derive(Clone, Debug)]
pub struct SavePointWithError {
    pub save_point: SavePoint,
    pub lexer_error: bool,
    pub lexer_error_message: WtfString,
    pub parser_error_message: WtfString,
}

/// `Parser::ParseInnerResult`.
pub struct ParseInnerResult {
    pub parameters: Link<FunctionParameters>,
    pub source_elements: Link<SourceElements>,
    pub function_declarations: FunctionStack,
    pub var_declarations: VariableEnvironment,
    pub lexical_variables: VariableEnvironment,
    pub features: CodeFeatures,
    pub num_constants: i32,
}

/// `Parser::CallOrApplyDepthScope`. O C++ encadeia por ponteiro para o pai; aqui o pai é o
/// elemento anterior de `Parser::call_or_apply_depth_scopes` (pilha), e o destrutor é `finish`.
#[derive(Clone, Copy, Debug)]
pub struct CallOrApplyDepthScope {
    depth: usize,
    depth_of_innermost_child: usize,
}

impl CallOrApplyDepthScope {
    pub fn distance_to_innermost_child(&self) -> usize {
        debug_assert!(self.depth_of_innermost_child >= self.depth);
        self.depth_of_innermost_child - self.depth
    }
}

/// `Parser::AllowInOverride`.
pub struct AllowInOverride {
    old_allows_in: bool,
}

impl AllowInOverride {
    pub fn new<T: CharType>(parser: &mut Parser<T>) -> AllowInOverride {
        let old_allows_in = parser.allows_in;
        parser.allows_in = true;
        AllowInOverride { old_allows_in }
    }

    /// Destrutor.
    pub fn restore<T: CharType>(self, parser: &mut Parser<T>) {
        parser.allows_in = self.old_allows_in;
    }
}

/// `Parser::AutoPopScope`.
pub struct AutoPopScope {
    scope: ScopeRef,
    popped: bool,
}

impl AutoPopScope {
    pub fn new(scope: ScopeRef) -> AutoPopScope {
        AutoPopScope { scope, popped: false }
    }

    pub fn set_popped(&mut self) {
        self.popped = true;
    }

    /// `operator->` e `scope()`.
    pub fn scope(&self) -> ScopeRef {
        self.scope
    }

    /// Destrutor: só desempilha se ninguém chamou `set_popped`.
    pub fn cleanup<T: CharType>(self, parser: &mut Parser<T>) {
        if !self.popped {
            parser.pop_scope(self.scope, false);
        }
    }
}

/// `Parser::AutoCleanupLexicalScope`.
pub struct AutoCleanupLexicalScope {
    scope: Option<ScopeRef>,
    valid: bool,
}

impl AutoCleanupLexicalScope {
    // We can allocate this object on the stack without actually knowing beforehand if we're
    // going to create a new lexical scope. If we decide to create a new lexical scope, we
    // can pass the scope into this obejct and it will take care of the cleanup for us if the parse fails.
    // This is helpful if we may fail from syntax errors after creating a lexical scope conditionally.
    pub fn new() -> AutoCleanupLexicalScope {
        AutoCleanupLexicalScope { scope: None, valid: false }
    }

    pub fn set_is_valid<T: CharType>(&mut self, scope: ScopeRef, parser: &Parser<T>) {
        assert!(parser.scope_stack[scope].is_lexical_scope());
        self.scope = Some(scope);
        self.valid = true;
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn set_popped(&mut self) {
        self.valid = false;
    }

    pub fn scope(&self) -> Option<ScopeRef> {
        self.scope
    }

    /// Destrutor. This should only ever be called if we fail from a syntax error. Otherwise
    /// it's the intention that a user of this class pops this scope manually on a
    /// successful parse.
    pub fn cleanup<T: CharType>(self, parser: &mut Parser<T>) {
        if self.is_valid() {
            if let Some(scope) = self.scope {
                parser.pop_scope(scope, false);
            }
        }
    }
}

/// `template <typename LexerType> class Parser`, com `LexerType` = `Lexer<T>`.
/// Campos na ordem do C++ (Parser.h 2127 a 2163); os que o `.h` guarda por ponteiro viram
/// `Option`/`Rc`.
pub struct Parser<T: CharType> {
    // Fields up to m_parserState are arranged according to access frequency and affinity;
    // do not rearrange without careful analysis.
    pub(crate) vm: Rc<VM>,
    pub(crate) token: JSToken,
    pub(crate) source: SourceCode,
    pub(crate) parser_arena: ParserArena,
    pub(crate) lexer: Box<Lexer<T>>,
    pub(crate) last_token_location: JSTokenLocation,
    pub(crate) current_scope: Option<ScopeRef>,
    pub(crate) error_message: WtfString,
    pub(crate) debugger_parse_data: Option<Rc<RefCell<DebuggerParseData>>>,
    pub(crate) last_token_type: JSTokenType,
    pub(crate) statement_depth: i32,
    pub(crate) function_mode: FunctionMode,
    pub(crate) allows_in: bool,
    pub(crate) immediate_parent_allows_function_declaration_in_statement: bool,
    pub(crate) implementation_visibility: ImplementationVisibility,
    pub(crate) inside_switch_case_body: bool,

    pub(crate) parser_state: ParserState,
    pub(crate) parse_mode: SourceParseMode,
    pub(crate) constructor_kind_for_top_level_function_expressions: ConstructorKind,
    pub(crate) is_inside_ordinary_function: bool,
    pub(crate) seen_tagged_template_in_non_reparsing_function_mode: bool,
    pub(crate) seen_private_name_use_in_non_reparsing_function_mode: bool,
    pub(crate) seen_arguments_dot_length: bool,
    pub(crate) parsing_builtin: bool,
    pub(crate) is_eval_context: bool,
    /// on a later line than m_lastTokenLocation when that token is a template literal
    pub(crate) last_token_end_position: JSTextPosition,

    pub(crate) function_cache: Option<Rc<RefCell<SourceProviderCache>>>,
    /// `m_callOrApplyDepthScope`: a pilha de `CallOrApplyDepthScope` ativos (o topo é o atual).
    pub(crate) call_or_apply_depth_scopes: Vec<CallOrApplyDepthScope>,
    pub(crate) module_scope_data: Option<Rc<ModuleScopeData>>,
    pub(crate) script_mode: JSParserScriptMode,
    pub(crate) super_binding: SuperBinding,
    pub(crate) has_stack_overflow: bool,
    pub(crate) scope_stack: ScopeStack,
}

impl<T: CharType> Parser<T> {
    pub fn override_constructor_kind_for_top_level_function_expressions(&mut self, constructor_kind: ConstructorKind) {
        self.constructor_kind_for_top_level_function_expressions = constructor_kind;
    }

    pub fn position_before_last_newline(&self) -> JSTextPosition {
        self.lexer.position_before_last_newline()
    }

    pub fn location_before_last_token(&self) -> JSTokenLocation {
        self.last_token_location
    }

    /// `CallOrApplyDepthScope(Parser*)`: empilha.
    pub fn push_call_or_apply_depth_scope(&mut self) {
        let depth = match self.call_or_apply_depth_scopes.last() {
            Some(parent) => parent.depth + 1,
            None => 0,
        };
        self.call_or_apply_depth_scopes.push(CallOrApplyDepthScope { depth, depth_of_innermost_child: depth });
    }

    /// `~CallOrApplyDepthScope()`: desempilha e devolve o escopo para o chamador consultar
    /// `distance_to_innermost_child` (o C++ consulta antes do destrutor, então o chamador deve
    /// ler `current_call_or_apply_depth_scope` antes de chamar isto).
    pub fn pop_call_or_apply_depth_scope(&mut self) -> CallOrApplyDepthScope {
        let finished = self.call_or_apply_depth_scopes.pop().expect("CallOrApplyDepthScope vazio");
        if let Some(parent) = self.call_or_apply_depth_scopes.last_mut() {
            parent.depth_of_innermost_child = std::cmp::max(finished.depth_of_innermost_child, parent.depth_of_innermost_child);
        }
        finished
    }

    /// `m_callOrApplyDepthScope`.
    pub fn current_call_or_apply_depth_scope(&self) -> Option<&CallOrApplyDepthScope> {
        self.call_or_apply_depth_scopes.last()
    }

    #[inline(always)]
    fn destructuring_kind_from_declaration_type(&self, type_: DeclarationType) -> DestructuringKind {
        match type_ {
            DeclarationType::VarDeclaration => DestructuringKind::DestructureToVariables,
            DeclarationType::LetDeclaration => DestructuringKind::DestructureToLet,
            DeclarationType::ConstDeclaration => DestructuringKind::DestructureToConst,
            DeclarationType::UsingDeclaration | DeclarationType::AwaitUsingDeclaration => {
                // Invariante: destructuring de using/await using é rejeitado antes, na gramática de `using`.
                panic!("RELEASE_ASSERT_NOT_REACHED")
            }
        }
    }

    #[inline(always)]
    fn declaration_type_to_variable_kind(&self, type_: DeclarationType) -> &'static str {
        match type_ {
            DeclarationType::VarDeclaration => "variable name",
            DeclarationType::LetDeclaration | DeclarationType::ConstDeclaration => "lexical variable name",
            DeclarationType::UsingDeclaration | DeclarationType::AwaitUsingDeclaration => "using variable name",
        }
    }

    #[inline(always)]
    fn assignment_context_from_declaration_type(&self, type_: DeclarationType) -> AssignmentContext {
        match type_ {
            DeclarationType::ConstDeclaration => AssignmentContext::ConstDeclarationStatement,
            DeclarationType::UsingDeclaration => AssignmentContext::UsingDeclarationStatement,
            DeclarationType::AwaitUsingDeclaration => AssignmentContext::AwaitUsingDeclarationStatement,
            _ => AssignmentContext::DeclarationStatement,
        }
    }

    #[inline(always)]
    pub fn source_parse_mode(&self) -> SourceParseMode {
        self.parse_mode
    }

    #[inline(always)]
    pub fn function_mode(&self) -> FunctionMode {
        self.function_mode
    }

    #[inline(always)]
    fn is_eval_or_arguments(&self, ident: &Identifier) -> bool {
        is_eval_or_arguments_identifier(&self.vm, ident)
    }

    fn upper_scope(&self, n: usize) -> ScopeRef {
        debug_assert!(self.scope_stack.len() >= 1 + n);
        self.scope_stack.len() - 1 - n
    }

    pub fn current_scope(&self) -> ScopeRef {
        self.current_scope.expect("sem ASSERT em Parser.h:1270: currentScope() devolve m_currentScope e o chamador o desreferencia (UB no C++ se nulo)")
    }

    fn current_variable_scope(&self) -> ScopeRef {
        let mut scope = self.current_scope();
        while !self.scope_stack[scope].allows_var_declarations() {
            scope = self.scope_stack[scope].containing_scope().expect("sem ASSERT em Parser.h:1277: currentVariableScope desreferencia containingScope() sem conferir nulo (UB no C++); a raiz sempre allowsVarDeclarations");
        }
        scope
    }

    fn current_lexical_declaration_scope(&self) -> ScopeRef {
        let mut scope = self.current_scope();
        while !self.scope_stack[scope].allows_lexical_declarations() {
            scope = self.scope_stack[scope].containing_scope().expect("sem ASSERT em Parser.h:1285: currentLexicalDeclarationScope desreferencia containingScope() sem conferir nulo (UB no C++); a raiz sempre allowsLexicalDeclarations");
        }
        scope
    }

    fn current_function_scope(&self) -> ScopeRef {
        let mut scope = self.current_scope();
        while let Some(containing) = self.scope_stack[scope].containing_scope() {
            if self.scope_stack[scope].is_function_boundary() {
                break;
            }
            scope = containing;
        }
        // When reaching the top level scope (it can be non function scope), we return it.
        scope
    }

    fn find_private_name_scope(&self) -> Option<ScopeRef> {
        let mut scope = self.current_scope();
        while let Some(containing) = self.scope_stack[scope].containing_scope() {
            if self.scope_stack[scope].is_private_name_scope() {
                break;
            }
            scope = containing;
        }
        if self.scope_stack[scope].is_private_name_scope() {
            return Some(scope);
        }
        None
    }

    fn closest_parent_ordinary_function_non_lexical_scope(&self) -> ScopeRef {
        let mut scope = self.current_scope();
        while let Some(containing) = self.scope_stack[scope].containing_scope() {
            let s = &self.scope_stack[scope];
            if s.is_function_boundary() && !s.is_generator_function_boundary() && !s.is_async_function_boundary() && !s.is_arrow_function_boundary() {
                break;
            }
            scope = containing;
        }
        // When reaching the top level scope (it can be non ordinary function scope), we return it.
        scope
    }

    // Walk to the closest scope that has its own `arguments` binding (or the top-level
    // scope). Arrow functions and lexical scopes are transparent for `arguments`.
    fn closest_scope_owning_arguments(&self) -> ScopeRef {
        let mut scope = self.current_scope();
        while let Some(containing) = self.scope_stack[scope].containing_scope() {
            if self.scope_stack[scope].is_function_boundary() && !self.scope_stack[scope].is_arrow_function_boundary() {
                break;
            }
            scope = containing;
        }
        scope
    }

    fn closest_class_scope_or_top_level_scope(&self) -> ScopeRef {
        let mut scope = self.current_scope();
        while let Some(containing) = self.scope_stack[scope].containing_scope() {
            if self.scope_stack[scope].is_class_scope() {
                break;
            }
            scope = containing;
        }
        scope
    }

    fn push_scope(&mut self) -> ScopeRef {
        let mut implementation_visibility = self.implementation_visibility;
        let mut lexically_scoped_features = NO_LEXICALLY_SCOPED_FEATURES;
        let mut is_function = false;
        let mut is_generator_function = false;
        let mut is_arrow_function = false;
        let mut is_async_function = false;
        let mut is_static_block = false;
        let parent_scope = self.current_scope;
        if let Some(parent) = parent_scope {
            let parent = &self.scope_stack[parent];
            implementation_visibility = parent.implementation_visibility();
            lexically_scoped_features = parent.lexically_scoped_features();
            is_function = parent.is_function();
            is_generator_function = parent.is_generator_function();
            is_arrow_function = parent.is_arrow_function();
            is_async_function = parent.is_async_function();
            is_static_block = parent.is_static_block();
        }
        self.scope_stack.push(Scope::new(
            self.vm.clone(),
            parent_scope,
            implementation_visibility,
            lexically_scoped_features,
            is_function,
            is_generator_function,
            is_arrow_function,
            is_async_function,
            is_static_block,
        ));
        let pushed = self.scope_stack.len() - 1;
        self.current_scope = Some(pushed);
        pushed
    }

    fn reset_implementation_visibility_if_needed(&mut self) {
        // Find the closest function boundary that is not the current scope (if the current scope
        // is also a function boundary). If the implementation visibility of that scope is not
        // recursive, reset the implementation visibility of the current scope.
        let current = self.current_scope();
        if !self.scope_stack[current].is_function_boundary() {
            return;
        }

        let mut scope = self.scope_stack[current].containing_scope();
        while let Some(s) = scope {
            if !self.scope_stack[s].is_function_boundary() {
                scope = self.scope_stack[s].containing_scope();
                continue;
            }

            if self.scope_stack[s].implementation_visibility() != ImplementationVisibility::PrivateRecursive {
                self.scope_stack[current].reset_implementation_visibility();
            }
            break;
        }
    }

    /// `popScopeInternal`. `has_precomputed_free_variables` e `precomputed_free_variables` formam
    /// o `std::optional<std::span>` do C++, mantidos separados como lá.
    fn pop_scope_internal(
        &mut self,
        scope: ScopeRef,
        should_track_closed_variables: bool,
        has_precomputed_free_variables: bool,
        precomputed_free_variables: &[UniquedKey],
    ) -> (VariableEnvironment, FunctionStack) {
        debug_assert!(Some(scope) == self.current_scope);
        debug_assert!(self.scope_stack.len() > 1);
        let last = self.current_scope();
        let parent = self.scope_stack[last].containing_scope().expect("ASSERT(m_scopeStack.size() > 1) em Parser.h:1386: popScopeInternal só roda com ao menos dois escopos, então o atual tem containingScope (Parser.h:1388)");

        self.scope_stack[last].finalize_lexical_environment();

        // O pai fica abaixo do filho na pilha: divide o empréstimo.
        {
            let (below, above) = self.scope_stack.split_at_mut(last);
            let last_scope = &mut above[0];
            let parent_scope = &mut below[parent];

            parent_scope.collect_free_variables_from(last_scope, should_track_closed_variables, has_precomputed_free_variables, precomputed_free_variables);

            if last_scope.has_sloppy_mode_function_hoisting_candidates() {
                last_scope.bubble_sloppy_mode_function_hoisting_candidates(parent_scope);
            }

            if last_scope.is_arrow_function() {
                last_scope.set_inner_arrow_function_uses_eval_and_use_arguments_if_needed();
            }

            if !(last_scope.is_function_boundary() && !last_scope.is_arrow_function_boundary()) {
                parent_scope.merge_inner_arrow_function_features(last_scope.inner_arrow_function_features());
            }

            if !last_scope.is_function_boundary() && last_scope.needs_full_activation() {
                parent_scope.set_needs_full_activation();
            }
        }
        let result = (self.scope_stack[last].take_lexical_environment(), self.scope_stack[last].take_function_declarations());
        self.scope_stack.pop();
        self.current_scope = Some(parent);
        result
    }

    #[inline(always)]
    fn pop_scope(&mut self, scope: ScopeRef, should_track_closed_variables: bool) -> (VariableEnvironment, FunctionStack) {
        self.pop_scope_internal(scope, should_track_closed_variables, false, &[])
    }

    // If hasPrecomputedFreeVariables is true, precomputedFreeVariables contains nestedScope's
    // free variables already computed by the caller. Conceptually these two parameters form an
    // std::optional<std::span>, but we keep them separate so they are passed in registers.
    // This code is hot enough that it makes a difference.
    /// `popScope(AutoPopScope&, ...)`.
    #[inline(always)]
    fn pop_scope_auto(
        &mut self,
        scope: &mut AutoPopScope,
        should_track_closed_variables: bool,
        has_precomputed_free_variables: bool,
        precomputed_free_variables: &[UniquedKey],
    ) -> (VariableEnvironment, FunctionStack) {
        scope.set_popped();
        self.pop_scope_internal(scope.scope(), should_track_closed_variables, has_precomputed_free_variables, precomputed_free_variables)
    }

    /// `popScope(AutoCleanupLexicalScope&, ...)`.
    #[inline(always)]
    fn pop_scope_cleanup(&mut self, cleanup_scope: &mut AutoCleanupLexicalScope, should_track_closed_variables: bool) -> (VariableEnvironment, FunctionStack) {
        assert!(cleanup_scope.is_valid());
        let scope = cleanup_scope.scope().expect("RELEASE_ASSERT(cleanupScope.isValid()) em Parser.h:1428: um AutoCleanupLexicalScope válido guarda o escopo que popScope lê em Parser.h:1429");
        cleanup_scope.set_popped();
        self.pop_scope_internal(scope, should_track_closed_variables, false, &[])
    }

    /// `NEVER_INLINE`.
    fn declare_hoisted_variable(&mut self, ident: &Identifier) -> DeclarationResultMask {
        let mut scope = self.current_scope();
        loop {
            // Annex B.3.5 exempts `try {} catch (e) { var e; }` from being a syntax error.
            if self.scope_stack[scope].has_lexically_declared_variable_identifier(ident) && !self.scope_stack[scope].is_simple_catch_parameter_scope() {
                return DeclarationResult::INVALID_DUPLICATE_DECLARATION;
            }

            if self.scope_stack[scope].allows_var_declarations() {
                return self.scope_stack[scope].declare_variable(ident);
            }

            self.scope_stack[scope].add_variable_being_hoisted(ident);
            scope = self.scope_stack[scope].containing_scope().expect("sem ASSERT em Parser.h:1446: declareVariable sobe containingScope() sem conferir nulo (UB no C++); a raiz sempre allowsVarDeclarations e encerra o laço");
        }
    }

    /// `declareVariable(ident, type = VarDeclaration, importType = NotImported)`.
    fn declare_variable(&mut self, ident: &Identifier, type_: DeclarationType, import_type: DeclarationImportType) -> DeclarationResultMask {
        if type_ == DeclarationType::VarDeclaration {
            return self.declare_hoisted_variable(ident);
        }

        debug_assert!(
            type_ == DeclarationType::LetDeclaration
                || type_ == DeclarationType::ConstDeclaration
                || type_ == DeclarationType::UsingDeclaration
                || type_ == DeclarationType::AwaitUsingDeclaration
        );
        // Lexical variables declared at a top level scope that shadow arguments or vars are not allowed.
        if !self.lexer.is_reparsing_function() && self.statement_depth == 1 && (self.has_declared_parameter(ident) || self.has_declared_variable(ident)) {
            return DeclarationResult::INVALID_DUPLICATE_DECLARATION;
        }

        let scope = self.current_lexical_declaration_scope();
        if self.scope_stack[scope].is_catch_block_scope() {
            let containing = self.scope_stack[scope].containing_scope().expect("sem ASSERT em Parser.h:1461: scope->containingScope()->hasLexicallyDeclaredVariable desreferencia sem conferir nulo (UB no C++); um escopo de catch sempre tem pai");
            if self.scope_stack[containing].has_lexically_declared_variable_identifier(ident) {
                return DeclarationResult::INVALID_DUPLICATE_DECLARATION;
            }
        }

        let is_await_using = type_ == DeclarationType::AwaitUsingDeclaration;
        let is_using = type_ == DeclarationType::UsingDeclaration || is_await_using;
        self.scope_stack[scope].declare_lexical_variable(ident, type_ == DeclarationType::ConstDeclaration || is_using, import_type, is_using, is_await_using)
    }

    fn declare_function(&mut self, ident: &Identifier) -> (DeclarationResultMask, ScopeRef) {
        if self.statement_depth == 1 && !self.scope_stack[self.current_scope()].is_module_code() {
            // Functions declared at the top-most scope (both in sloppy and strict mode) are declared as vars
            // for backwards compatibility, allowing us to declare functions with the same name more than once, except
            // Module code. Please see https://webkit.org/b/263269 for detailed explanation and ECMA-262 references.
            let variable_scope = self.current_variable_scope();
            return (self.scope_stack[variable_scope].declare_function_as_var(ident), variable_scope);
        }

        let lexical_variable_scope = self.current_lexical_declaration_scope();
        if self.scope_stack[lexical_variable_scope].is_catch_block_scope() {
            let containing = self.scope_stack[lexical_variable_scope].containing_scope().expect("sem ASSERT em Parser.h:1480: lexicalVariableScope->containingScope()->hasLexicallyDeclaredVariable desreferencia sem conferir nulo (UB no C++); um escopo de catch sempre tem pai");
            if self.scope_stack[containing].has_lexically_declared_variable_identifier(ident) {
                return (DeclarationResult::INVALID_DUPLICATE_DECLARATION, lexical_variable_scope);
            }
        }

        let is_function_declaration = self.parse_mode == SourceParseMode::NormalFunctionMode;
        (self.scope_stack[lexical_variable_scope].declare_function_as_let(ident, is_function_declaration), lexical_variable_scope)
    }

    /// `NEVER_INLINE`.
    fn has_declared_variable(&self, ident: &Identifier) -> bool {
        let scope = self.current_variable_scope();
        self.scope_stack[scope].has_declared_variable_identifier(ident)
    }

    /// `NEVER_INLINE`.
    fn has_declared_parameter(&self, ident: &Identifier) -> bool {
        // FIXME: hasDeclaredParameter() is not valid during reparsing of generator or async function bodies, because their formal
        // parameters are declared in a scope unavailable during reparsing. Note that it is redundant to call this function during
        // reparsing anyways, as the function is already guaranteed to be valid by the original parsing.
        // https://bugs.webkit.org/show_bug.cgi?id=164087
        debug_assert!(!self.lexer.is_reparsing_function());

        let mut scope = self.current_variable_scope();
        if self.scope_stack[scope].is_generator_function_boundary() || self.scope_stack[scope].is_async_function_boundary() {
            // The formal parameters which need to be verified for Generators and Async Function bodies occur
            // in the outer wrapper function, so pick the outer scope here.
            scope = self.scope_stack[scope].containing_scope().expect("sem ASSERT em Parser.h:1510: hasDeclaredParameter desreferencia containingScope() sem conferir nulo (UB no C++); o corpo de generator/async sempre tem o wrapper externo");
        }
        self.scope_stack[scope].has_declared_parameter_identifier(ident)
    }

    fn export_name(&mut self, ident: &Identifier) -> bool {
        debug_assert!(self.scope_stack[self.current_scope()].containing_scope().is_none());
        debug_assert!(self.module_scope_data.is_some());
        match &self.module_scope_data {
            Some(data) => data.export_name(ident),
            None => false,
        }
    }

    fn find_cached_function_info(&self, open_brace_pos: i32) -> Option<Rc<SourceProviderCacheItem>> {
        match &self.function_cache {
            Some(cache) => cache.borrow().get(open_brace_pos),
            None => None,
        }
    }

    // `bool isFunctionMetadataNode(ScopeNode*)` / `(FunctionMetadataNode*)`: sobrecarga por tipo do
    // nó, resolvida pelo trait `IsFunctionMetadataNode` (ver abaixo), implementado em `parser.rs`
    // para `ScopeNode` (false) e `FunctionMetadataNode` (true).

    #[inline(always)]
    fn next(&mut self, lexer_flags: LexerFlagSet) {
        self.last_token_location = self.token.location();
        self.last_token_end_position = self.token.end_position;
        self.last_token_type = self.token.type_;
        let strict = self.strict_mode();
        self.token.type_ = self.lexer.lex(&mut self.token, lexer_flags, strict);
    }

    #[inline(always)]
    fn next_without_clearing_line_terminator(&mut self, lexer_flags: LexerFlagSet) {
        self.last_token_location = self.token.location();
        self.last_token_end_position = self.token.end_position;
        self.last_token_type = self.token.type_;
        let strict = self.strict_mode();
        self.token.type_ = self.lexer.lex_without_clearing_line_terminator(&mut self.token, lexer_flags, strict);
    }

    #[inline(always)]
    fn lex_current_token_again_under_current_context<TB: TreeBuilder>(&mut self, context: &mut TB) {
        let save_point = self.create_save_point(context);
        self.restore_save_point(context, &save_point);
    }
}
