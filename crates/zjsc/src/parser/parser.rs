//! Tradução de `parser/Parser.h`, primeira fatia: linhas 1 a 800, mais o fim da `struct Scope`
//! (linhas 920 a 1081: setters privados, campos e `ScopeStack`), porque a struct não pode ficar
//! partida entre arquivos. O resto do `.h` e os `.cpp` vêm em `parser_part2.rs` etc. por `include!`.
//!
//! Decisões do porte:
//!
//! - `Scope*` (`m_containingScope`, parâmetros `Scope* nestedScope`/`parentScope`) vira índice
//!   `ScopeRef = usize` na pilha `ScopeStack = Vec<Scope>` (o `SegmentedVector<Scope, 20, 10>` do C++
//!   existe para os ponteiros não se invalidarem; com índices o `Vec` basta). Funções que no C++ recebem
//!   outro `Scope*` recebem `&Scope`/`&mut Scope`; quem chama divide o empréstimo da pilha
//!   (`split_at_mut`), porque `nested` e `self` são sempre escopos distintos.
//! - `const VM&` vira `Rc<VM>`. `const Identifier*` vira `&Identifier`. `UniquedStringImpl*` vira
//!   `UniquedKey` (identidade por ponteiro do `Rc<StringImpl>`).
//! - `UniquedStringImplPtrSet` (`SmallSet`), `UncheckedKeyHashSet<UniquedStringImpl*>` e `IdentifierSet`
//!   viram `OrderedKeyMap<()>` (ordem de inserção, ver `variable_environment`).
//! - Campos de bit (`bool m_x : 1`) viram `bool`.
//! - As macros `TreeStatement`, `TreeExpression` etc. (linhas 60 a 72) são `typename TreeBuilder::X`:
//!   no porte são os tipos associados do trait `TreeBuilder`, e somem aqui.
//! - `NeedsDuplicateDeclarationCheck` e `PrivateAccessorType`, aninhados em `Scope`, ficam no módulo.
//!   `addSloppyModeFunctionHoistingCandidate<needsCheck>` recebe o `needsCheck` como parâmetro.
//! - `verifyLayout` (só `static_assert` de offset em ARM64 sem asserts) some.

use std::rc::Rc;
use std::sync::atomic::AtomicU32;

use crate::bytecode::executable_info::{DerivedContextType, EvalContextType};
use crate::parser::nodes::{ClassElementTag, FunctionStack};
use crate::parser::nodes::FunctionMetadataNode;
use crate::parser::parser_modes::{
    InnerArrowFunctionCodeFeatures, LexicallyScopedFeatures, SourceParseMode, SuperBinding,
    ARGUMENTS_INNER_ARROW_FUNCTION_FEATURE, EVAL_INNER_ARROW_FUNCTION_FEATURE,
    NEW_TARGET_INNER_ARROW_FUNCTION_FEATURE, STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
    SUPER_CALL_INNER_ARROW_FUNCTION_FEATURE, SUPER_PROPERTY_INNER_ARROW_FUNCTION_FEATURE,
    TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE, THIS_INNER_ARROW_FUNCTION_FEATURE,
};
use crate::parser::parser_tokens::{
    THISTOKEN, AUTOMINUSMINUS, DIVEQUAL, DIVIDE, MINUSMINUS, UNTERMINATED_IDENTIFIER_ESCAPE_ERRORTOK, UNTERMINATED_IDENTIFIER_UNICODE_ESCAPE_ERRORTOK,
    JSTextPosition, JSToken, JSTokenLocation, JSTokenType, FIRST_CONTEXTUAL_KEYWORD_TOKEN, IDENT, KEYWORD_TOKEN_FLAG,
    LAST_CONTEXTUAL_KEYWORD_TOKEN, LAST_UNTAGGED_TOKEN,
    AND, ANDEQUAL, ARROWFUNCTION, AUTOPLUSPLUS, AWAIT, BACKQUOTE, BIGINT, BINARY_OP_TOKEN_ALLOWS_IN_PRECEDENCE_ADDITIONAL_SHIFT, BINARY_OP_TOKEN_PRECEDENCE_MASK, BREAK, CAN_BE_ERROR_TOKEN_FLAG, CASE, CATCH, CLASSTOKEN, CLOSEBRACE, CLOSEBRACKET, CLOSEPAREN, COALESCE, COLON, COMMA, CONSTTOKEN, CONTINUE, DEBUGGER, DEFAULT, DO, DOT, DOTDOTDOT, DOUBLE, ELSE, EOFTOK, EQUAL, ERRORTOK, ESCAPED_KEYWORD, EXTENDS, FINALLY, FOR, FUNCTION, IF, IMPORT, INTEGER, INTOKEN, LET, NEW, OPENBRACE, OPENBRACKET, OPENPAREN, OR, PLUSPLUS, POW, PRIVATENAME, QUESTION, QUESTIONDOT, REGEXP, RESERVED, RESERVED_IF_STRICT, RETURN, SEMICOLON, STRING, SUPER, SWITCH, TEMPLATE, THROW, TIMES, TRY, UNTERMINATED_CAN_BE_ERROR_TOKEN_FLAG, UNTERMINATED_MULTILINE_COMMENT_ERRORTOK, UNTERMINATED_TEMPLATE_LITERAL_ERRORTOK, VAR, WHILE, WITH, YIELD,
};
use crate::parser::parser_tokens::*;
use crate::parser::lexer::{LexerFlagSet, RawStringsBuildMode};
use crate::parser::nodes::{AssignmentContext, Operator, PropertyNode};
use crate::parser::parser_error::{ErrorType, ParserError, SyntaxErrorType};
use crate::parser::parser_function_info::ParserFunctionInfo;
use crate::parser::parser_modes::{is_async_function_or_async_generator_wrapper_parse_mode, is_module_parse_mode, FunctionMode, JSParserBuiltinMode, JSParserScriptMode, SourceParseModeSet};
use crate::parser::source_code::SourceCode;
use crate::parser::syntax_checker::SyntaxChecker;
use crate::parser::tree_builder::TreeBuilder;
use crate::runtime::options::Options;
use crate::wtf::text::string_builder::StringBuilder;
use crate::wtf::text::string_impl::CharType;
use crate::wtf::text::wtf_string::String as WtfString;
use std::cell::RefCell;
use crate::bytecode::parse_hash::ParseHash;
use crate::bytecode::unlinked_function_executable::{ClassElementDefinition, ClassElementDefinitionKind};
use crate::debugger::debugger_parse_data::DebuggerParseData;
use crate::parser::nodes::ProgramNode;
use crate::parser::source_provider_cache::SourceProviderCache;
use crate::parser::source_provider_cache_item::{SourceProviderCacheItem, SourceProviderCacheItemCreationParameters};
use crate::wtf::fixed_vector::FixedVector;
use crate::wtf::monotonic_time::MonotonicTime;
use crate::parser::ast_builder::{ASTBuilder, Link};
use crate::parser::lexer::{Lexer, LexerFlags};
use crate::parser::module_scope_data::ModuleScopeData;
use crate::parser::nodes::{DefineFieldType, DotType, FunctionParameters, PropertyNodeType, SourceElements};
use crate::parser::parser_arena::ParserArena;
use crate::parser::parser_function_info::ParserClassInfo;
use crate::parser::parser_modes::{
    function_name_is_in_scope, is_async_function_parse_mode, is_async_function_wrapper_parse_mode,
    is_async_generator_wrapper_parse_mode, is_async_method_parse_mode, is_function_parse_mode,
    is_generator_method_parse_mode, is_generator_or_async_function_body_parse_mode,
    is_generator_or_async_function_wrapper_parse_mode, is_generator_or_async_generator_wrapper_parse_mode,
    is_generator_wrapper_parse_mode, is_method_parse_mode, CodeFeatures, ARGUMENTS_FEATURE,
    ASYNC_FUNCTION_WITHOUT_AWAIT_FEATURE, AWAIT_FEATURE, IMPORT_META_FEATURE, NON_SIMPLE_PARAMETER_LIST_FEATURE,
    NO_EVAL_CACHE_FEATURE, NO_LEXICALLY_SCOPED_FEATURES, SHADOWS_ARGUMENTS_FEATURE,
};
use crate::parser::parser_tokens::{is_unary_op, is_update_op};
use crate::parser::variable_environment::PrivateNameEnvironment;
use crate::wtf::text::string_impl::{LChar, UChar};
use crate::wtf::text::wtf_string::make_string_by_joining;
use crate::parser::variable_environment::{
    OrderedKeyMap, PrivateDeclarationResult, PrivateNameEntry, VariableEnvironment,
};
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::UniquedKey;

const _: () = assert!(LAST_UNTAGGED_TOKEN < 64, "Less than 64 untagged tokens");

/// `typedef SmallSet<UniquedStringImpl*> UniquedStringImplPtrSet`.
pub type UniquedStringImplPtrSet = OrderedKeyMap<()>;

/// `typedef HashSet<RefPtr<UniquedStringImpl>> IdentifierSet` (`Identifier.h`).
pub type IdentifierSet = OrderedKeyMap<()>;

/// `WTF::IterationStatus` (`wtf/IterationStatus.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IterationStatus {
    Continue,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceElementsMode {
    CheckForStrictMode,
    DontCheckForStrictMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionBodyType {
    ArrowFunctionBodyExpression,
    ArrowFunctionBodyBlock,
    StandardFunctionBodyBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionNameRequirements {
    None,
    Named,
    Unnamed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestructuringKind {
    DestructureToVariables,
    DestructureToLet,
    DestructureToConst,
    DestructureToCatchParameters,
    DestructureToParameters,
    DestructureToExpressions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarationType {
    VarDeclaration,
    LetDeclaration,
    ConstDeclaration,
    UsingDeclaration,
    AwaitUsingDeclaration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarationImportType {
    Imported,
    ImportedNamespace,
    NotImported,
}

/// `typedef uint8_t DeclarationResultMask`.
pub type DeclarationResultMask = u8;

/// `enum DeclarationResult` (enum sem classe, usado como máscara de bits).
pub struct DeclarationResult;

impl DeclarationResult {
    pub const VALID: DeclarationResultMask = 0;
    pub const INVALID_STRICT_MODE: DeclarationResultMask = 1 << 0;
    pub const INVALID_DUPLICATE_DECLARATION: DeclarationResultMask = 1 << 1;
    pub const INVALID_PRIVATE_STATIC_NON_STATIC: DeclarationResultMask = 1 << 2;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarationDefaultContext {
    Standard,
    ExportDefault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferName {
    Allowed,
    Disallowed,
}

/// `template <typename T> bool isEvalNode()`: `false` para todo tipo, `true` para `EvalNode`.
/// Cada tipo de nó raiz que o `Parser` instancia implementa o trait (o `EvalNode` entra com o
/// `Parser::parse`, em `parser_part2.rs`).
pub trait IsEvalNode {
    const IS_EVAL_NODE: bool = false;
}

pub fn is_eval_node<T: IsEvalNode>() -> bool {
    T::IS_EVAL_NODE
}

/// `struct ScopeLabelInfo`.
#[derive(Clone, Debug)]
pub struct ScopeLabelInfo {
    pub uid: UniquedKey,
    pub is_loop: bool,
}

/// `isArguments`.
pub fn is_arguments(vm: &VM, ident: &Identifier) -> bool {
    vm.property_names.arguments == *ident
}

/// `isEval`.
pub fn is_eval(vm: &VM, ident: &Identifier) -> bool {
    vm.property_names.eval == *ident
}

/// `isEvalOrArgumentsIdentifier`.
pub fn is_eval_or_arguments_identifier(vm: &VM, ident: &Identifier) -> bool {
    is_eval(vm, ident) || is_arguments(vm, ident)
}

/// `isIdentifierOrKeyword`.
pub fn is_identifier_or_keyword(token: &JSToken) -> bool {
    token.type_ == IDENT || token.type_ & KEYWORD_TOKEN_FLAG != 0
}

/// `isContextualKeyword`: "let", "yield" e "await" são palavra-chave ou identificador conforme o contexto.
pub fn is_contextual_keyword(token: &JSToken) -> bool {
    let type_: JSTokenType = token.type_;
    (FIRST_CONTEXTUAL_KEYWORD_TOKEN..=LAST_CONTEXTUAL_KEYWORD_TOKEN).contains(&type_)
}

/// `globalParseCount`.
pub static GLOBAL_PARSE_COUNT: AtomicU32 = AtomicU32::new(0);

/// `Scope*` do C++: índice na `ScopeStack`.
pub type ScopeRef = usize;

/// `typedef SegmentedVector<Scope, 20, 10> ScopeStack`.
pub type ScopeStack = Vec<Scope>;

/// `Scope::NeedsDuplicateDeclarationCheck`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedsDuplicateDeclarationCheck {
    No,
    Yes,
}

/// `Scope::PrivateAccessorType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateAccessorType {
    Setter,
    Getter,
}

/// `uid` do `Identifier::impl()`: o parser nunca pergunta por um `Identifier` nulo (o C++ desreferencia
/// o ponteiro sem checar).
fn uid(ident: &Identifier) -> UniquedKey {
    ident.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)")
}

/// `struct Scope`. Sem cópia (`WTF_MAKE_NONCOPYABLE`); movível.
pub struct Scope {
    vm: Rc<VM>,
    containing_scope: Option<ScopeRef>,
    function_declarations: FunctionStack,
    shadows_arguments: bool,
    uses_eval: bool,
    uses_import_meta: bool,
    needs_full_activation: bool,
    has_direct_super: bool,
    needs_super_binding: bool,
    allows_var_declarations: bool,
    allows_lexical_declarations: bool,
    is_function: bool,
    is_generator_function: bool,
    is_generator_function_boundary: bool,
    is_arrow_function: bool,
    is_arrow_function_boundary: bool,
    is_async_function: bool,
    is_async_function_boundary: bool,
    is_lexical_scope: bool,
    is_global_code: bool,
    is_module_code: bool,
    is_simple_catch_parameter_scope: bool,
    is_catch_block_scope: bool,
    is_static_block: bool,
    is_static_block_boundary: bool,
    is_function_boundary: bool,
    is_valid_strict_mode: bool,
    has_arguments: bool,
    is_eval_context: bool,
    has_non_simple_parameter_list: bool,
    is_class_scope: bool,
    async_function_body_does_not_use_await: bool,
    uses_await: bool,
    loop_depth: i32,
    expected_super_binding: SuperBinding,
    implementation_visibility: ImplementationVisibility,
    lexically_scoped_features: LexicallyScopedFeatures,
    constructor_kind: ConstructorKind,
    inner_arrow_function_features: InnerArrowFunctionCodeFeatures,
    /// `m_sloppyModeFunctionHoistingCandidates`, em ordem de declaração.
    sloppy_mode_function_hoisting_candidates: Vec<(Rc<FunctionMetadataNode>, NeedsDuplicateDeclarationCheck)>,
    lexical_variables: VariableEnvironment,
    declared_variables: VariableEnvironment,
    declared_parameters: UniquedStringImplPtrSet,
    variables_being_hoisted: UniquedStringImplPtrSet,
    /// `m_labels` (`std::unique_ptr<LabelStack>`; nulo e vazio se comportam igual em todo uso).
    labels: Vec<ScopeLabelInfo>,
    switch_depth: i32,
    eval_context_type: EvalContextType,
    derived_context_type: DerivedContextType,
    last_added_used_variable: Option<UniquedKey>,
    used_variables: Vec<UniquedStringImplPtrSet>,
    closed_variable_candidates: OrderedKeyMap<()>,
}

impl Scope {
    pub fn new(
        vm: Rc<VM>,
        containing_scope: Option<ScopeRef>,
        implementation_visibility: ImplementationVisibility,
        lexically_scoped_features: LexicallyScopedFeatures,
        is_function: bool,
        is_generator_function: bool,
        is_arrow_function: bool,
        is_async_function: bool,
        is_static_block: bool,
    ) -> Scope {
        Scope {
            vm,
            containing_scope,
            function_declarations: FunctionStack::new(),
            shadows_arguments: false,
            uses_eval: false,
            uses_import_meta: false,
            needs_full_activation: false,
            has_direct_super: false,
            needs_super_binding: false,
            allows_var_declarations: true,
            allows_lexical_declarations: true,
            is_function,
            is_generator_function,
            is_generator_function_boundary: false,
            is_arrow_function,
            is_arrow_function_boundary: false,
            is_async_function,
            is_async_function_boundary: false,
            is_lexical_scope: false,
            is_global_code: false,
            is_module_code: false,
            is_simple_catch_parameter_scope: false,
            is_catch_block_scope: false,
            is_static_block,
            is_static_block_boundary: false,
            is_function_boundary: false,
            is_valid_strict_mode: true,
            has_arguments: false,
            is_eval_context: false,
            has_non_simple_parameter_list: false,
            is_class_scope: false,
            async_function_body_does_not_use_await: false,
            uses_await: false,
            loop_depth: 0,
            expected_super_binding: SuperBinding::NotNeeded,
            implementation_visibility,
            lexically_scoped_features,
            constructor_kind: ConstructorKind::None,
            inner_arrow_function_features: 0,
            sloppy_mode_function_hoisting_candidates: Vec::new(),
            lexical_variables: VariableEnvironment::default(),
            declared_variables: VariableEnvironment::default(),
            declared_parameters: UniquedStringImplPtrSet::default(),
            variables_being_hoisted: UniquedStringImplPtrSet::default(),
            labels: Vec::new(),
            switch_depth: 0,
            eval_context_type: EvalContextType::None,
            derived_context_type: DerivedContextType::None,
            last_added_used_variable: None,
            used_variables: vec![UniquedStringImplPtrSet::default()],
            closed_variable_candidates: OrderedKeyMap::default(),
        }
    }

    pub fn implementation_visibility(&self) -> ImplementationVisibility {
        self.implementation_visibility
    }

    pub fn reset_implementation_visibility(&mut self) {
        self.set_implementation_visibility(ImplementationVisibility::Public);
    }

    pub fn set_implementation_visibility(&mut self, implementation_visibility: ImplementationVisibility) {
        self.implementation_visibility = implementation_visibility;
    }

    pub fn start_switch(&mut self) {
        self.switch_depth += 1;
    }

    pub fn end_switch(&mut self) {
        self.switch_depth -= 1;
    }

    pub fn start_loop(&mut self) {
        self.loop_depth += 1;
    }

    pub fn end_loop(&mut self) {
        self.loop_depth -= 1;
    }

    pub fn in_loop(&self) -> bool {
        self.loop_depth != 0
    }

    pub fn break_is_valid(&self) -> bool {
        self.loop_depth != 0 || self.switch_depth != 0
    }

    pub fn continue_is_valid(&self) -> bool {
        self.loop_depth != 0
    }

    pub fn push_label(&mut self, label: &Identifier, is_loop: bool) {
        self.labels.push(ScopeLabelInfo { uid: uid(label), is_loop });
    }

    pub fn pop_label(&mut self) {
        self.labels.pop();
    }

    pub fn get_label(&self, label: &Identifier) -> Option<&ScopeLabelInfo> {
        let key = uid(label);
        self.labels.iter().rev().find(|info| info.uid == key)
    }

    pub fn containing_scope(&self) -> Option<ScopeRef> {
        self.containing_scope
    }

    pub fn has_containing_scope(&self) -> bool {
        self.containing_scope.is_some() && !self.is_function_boundary()
    }

    pub fn set_source_parse_mode(&mut self, mode: SourceParseMode) {
        match mode {
            SourceParseMode::AsyncGeneratorBodyMode => self.set_is_async_generator_function_body(),
            SourceParseMode::AsyncArrowFunctionBodyMode => self.set_is_async_arrow_function_body(),
            SourceParseMode::AsyncFunctionBodyMode => self.set_is_async_function_body(),
            SourceParseMode::GeneratorBodyMode => self.set_is_generator_function_body(),
            SourceParseMode::GeneratorWrapperFunctionMode | SourceParseMode::GeneratorWrapperMethodMode => {
                self.set_is_generator_function()
            }
            SourceParseMode::AsyncGeneratorWrapperMethodMode | SourceParseMode::AsyncGeneratorWrapperFunctionMode => {
                self.set_is_async_generator_function()
            }
            SourceParseMode::NormalFunctionMode
            | SourceParseMode::GetterMode
            | SourceParseMode::SetterMode
            | SourceParseMode::MethodMode
            | SourceParseMode::ClassFieldInitializerMode => self.set_is_function(),
            SourceParseMode::ClassStaticBlockMode => {
                self.set_is_function();
                self.set_is_static_block();
            }
            SourceParseMode::ArrowFunctionMode => self.set_is_arrow_function(),
            SourceParseMode::AsyncFunctionMode | SourceParseMode::AsyncMethodMode => self.set_is_async_function(),
            SourceParseMode::AsyncArrowFunctionMode => self.set_is_async_arrow_function(),
            SourceParseMode::ProgramMode => self.set_is_global_code(),
            SourceParseMode::ModuleAnalyzeMode | SourceParseMode::ModuleEvaluateMode => self.set_is_module_code(),
        }
    }

    pub fn is_function(&self) -> bool {
        self.is_function
    }

    pub fn is_function_boundary(&self) -> bool {
        self.is_function_boundary
    }

    pub fn is_generator_function(&self) -> bool {
        self.is_generator_function
    }

    pub fn is_generator_function_boundary(&self) -> bool {
        self.is_generator_function_boundary
    }

    pub fn is_async_function(&self) -> bool {
        self.is_async_function
    }

    pub fn is_async_function_boundary(&self) -> bool {
        self.is_async_function_boundary
    }

    pub fn is_private_name_scope(&self) -> bool {
        self.is_class_scope
    }

    pub fn is_class_scope(&self) -> bool {
        self.is_class_scope
    }

    pub fn is_global_code(&self) -> bool {
        self.is_global_code
    }

    pub fn is_module_code(&self) -> bool {
        self.is_module_code
    }

    pub fn has_arguments(&self) -> bool {
        self.has_arguments
    }

    pub fn set_is_simple_catch_parameter_scope(&mut self) {
        self.is_simple_catch_parameter_scope = true;
    }

    pub fn is_simple_catch_parameter_scope(&self) -> bool {
        self.is_simple_catch_parameter_scope
    }

    pub fn set_is_catch_block_scope(&mut self) {
        self.is_catch_block_scope = true;
    }

    pub fn is_catch_block_scope(&self) -> bool {
        self.is_catch_block_scope
    }

    pub fn set_is_static_block(&mut self) {
        self.is_static_block = true;
        self.is_static_block_boundary = true;
    }

    pub fn is_static_block(&self) -> bool {
        self.is_static_block
    }

    pub fn is_static_block_boundary(&self) -> bool {
        self.is_static_block_boundary
    }

    pub fn set_is_lexical_scope(&mut self) {
        self.is_lexical_scope = true;
        self.allows_lexical_declarations = true;
    }

    pub fn set_is_private_name_scope(&mut self) {
        // FIXME do C++: hoje isPrivateNameScope é sinônimo de isClassScope, o que engana em
        // eval direto dentro de uma classe.
        self.set_is_class_scope();
    }

    pub fn set_is_class_scope(&mut self) {
        self.is_class_scope = true;
    }

    pub fn is_lexical_scope(&self) -> bool {
        self.is_lexical_scope
    }

    pub fn uses_eval(&self) -> bool {
        self.uses_eval
    }

    pub fn uses_import_meta(&self) -> bool {
        self.uses_import_meta
    }

    pub fn closed_variable_candidates(&self) -> &OrderedKeyMap<()> {
        &self.closed_variable_candidates
    }

    pub fn declared_variables(&mut self) -> &mut VariableEnvironment {
        &mut self.declared_variables
    }

    pub fn lexical_variables(&mut self) -> &mut VariableEnvironment {
        &mut self.lexical_variables
    }

    pub fn finalize_lexical_environment(&mut self) {
        if self.uses_eval || self.needs_full_activation {
            self.lexical_variables.mark_all_variables_as_captured();
        } else {
            self.compute_lexically_captured_variables_and_purge_candidates();
        }
    }

    pub fn take_lexical_environment(&mut self) -> VariableEnvironment {
        std::mem::take(&mut self.lexical_variables)
    }

    pub fn take_declared_variables(&mut self) -> VariableEnvironment {
        std::mem::take(&mut self.declared_variables)
    }

    pub fn compute_lexically_captured_variables_and_purge_candidates(&mut self) {
        // Because variables may be defined at any time in the range of a lexical scope, we must
        // track lexical variables that might be captured. Then, when we're preparing to pop the top
        // lexical scope off the stack, we should find which variables are truly captured, and which
        // variable still may be captured in a parent scope.
        if self.lexical_variables.size() != 0 && self.closed_variable_candidates.len() != 0 {
            for (impl_, _) in self.closed_variable_candidates.iter() {
                self.lexical_variables.mark_variable_as_captured_if_defined(impl_);
            }
        }

        // We can now purge values from the captured candidates because they're captured in this scope.
        for (key, value) in self.lexical_variables.iter() {
            if value.is_captured() {
                self.closed_variable_candidates.remove(key);
            }
        }
    }

    pub fn declare_callee(&mut self, ident: &Identifier) -> DeclarationResultMask {
        let add_result = self.declared_variables.add(&uid(ident));
        // We want to track if callee is captured, but we don't want to act like it's a 'var'
        // because that would cause the BytecodeGenerator to emit bad code.
        add_result.value.clear_is_var();

        let mut result = DeclarationResult::VALID;
        if is_eval_or_arguments_identifier(&self.vm, ident) {
            result |= DeclarationResult::INVALID_STRICT_MODE;
        }
        result
    }

    pub fn declare_variable(&mut self, ident: &Identifier) -> DeclarationResultMask {
        let mut result = DeclarationResult::VALID;
        let is_valid_strict_mode = !is_eval_or_arguments_identifier(&self.vm, ident);
        self.is_valid_strict_mode = self.is_valid_strict_mode && is_valid_strict_mode;
        let add_result = self.declared_variables.add(&uid(ident));
        add_result.value.set_is_var();
        if !is_valid_strict_mode {
            result |= DeclarationResult::INVALID_STRICT_MODE;
        }
        result
    }

    pub fn declare_function_as_var(&mut self, ident: &Identifier) -> DeclarationResultMask {
        let mut result = DeclarationResult::VALID;
        let is_valid_strict_mode = !is_eval_or_arguments_identifier(&self.vm, ident);
        if !is_valid_strict_mode {
            result |= DeclarationResult::INVALID_STRICT_MODE;
        }
        self.is_valid_strict_mode = self.is_valid_strict_mode && is_valid_strict_mode;

        let key = uid(ident);
        let add_result = self.declared_variables.add(&key);
        add_result.value.set_is_var();
        add_result.value.set_is_function();

        if self.lexical_variables.contains(&key) {
            result |= DeclarationResult::INVALID_DUPLICATE_DECLARATION;
        }
        result
    }

    pub fn declare_function_as_let(&mut self, ident: &Identifier, is_function_declaration: bool) -> DeclarationResultMask {
        let mut result = DeclarationResult::VALID;
        let is_valid_strict_mode = !is_eval_or_arguments_identifier(&self.vm, ident);
        if !is_valid_strict_mode {
            result |= DeclarationResult::INVALID_STRICT_MODE;
        }
        self.is_valid_strict_mode = self.is_valid_strict_mode && is_valid_strict_mode;

        let key = uid(ident);
        let strict_mode = self.strict_mode();
        // As duas consultas não dependem do `add` abaixo (outros mapas), então saem antes dele.
        let duplicates_var = self.declared_variables.contains(&key) || self.variables_being_hoisted.contains(&key);
        let add_result = self.lexical_variables.add(&key);
        if !add_result.is_new_entry
            && (strict_mode || !add_result.value.is_function_declaration() || !is_function_declaration)
        {
            result |= DeclarationResult::INVALID_DUPLICATE_DECLARATION;
        }
        if duplicates_var {
            result |= DeclarationResult::INVALID_DUPLICATE_DECLARATION;
        }

        add_result.value.set_is_let();
        add_result.value.set_is_function();
        if is_function_declaration {
            add_result.value.set_is_function_declaration();
        }

        result
    }

    pub fn add_variable_being_hoisted(&mut self, ident: &Identifier) {
        self.variables_being_hoisted.add(&uid(ident), ());
    }

    /// `addSloppyModeFunctionHoistingCandidate<needsCheck>`.
    pub fn add_sloppy_mode_function_hoisting_candidate(
        &mut self,
        node: Rc<FunctionMetadataNode>,
        needs_check: NeedsDuplicateDeclarationCheck,
    ) {
        self.sloppy_mode_function_hoisting_candidates.push((node, needs_check));
    }

    pub fn append_function(&mut self, node: Rc<FunctionMetadataNode>) {
        self.function_declarations.push(node);
    }

    pub fn take_function_declarations(&mut self) -> FunctionStack {
        std::mem::take(&mut self.function_declarations)
    }

    pub fn declare_lexical_variable(
        &mut self,
        ident: &Identifier,
        is_constant: bool,
        import_type: DeclarationImportType,
        is_using: bool,
        is_await_using: bool,
    ) -> DeclarationResultMask {
        let mut result = DeclarationResult::VALID;
        let is_valid_strict_mode = !is_eval_or_arguments_identifier(&self.vm, ident);
        self.is_valid_strict_mode = self.is_valid_strict_mode && is_valid_strict_mode;
        let key = uid(ident);
        let being_hoisted = self.variables_being_hoisted.contains(&key);
        let add_result = self.lexical_variables.add(&key);
        if is_constant {
            add_result.value.set_is_const();
        } else {
            add_result.value.set_is_let();
        }
        if is_using {
            add_result.value.set_is_using();
        }

        if import_type == DeclarationImportType::Imported {
            add_result.value.set_is_imported();
        } else if import_type == DeclarationImportType::ImportedNamespace {
            add_result.value.set_is_imported();
            add_result.value.set_is_imported_namespace();
        }

        if !add_result.is_new_entry || being_hoisted {
            result |= DeclarationResult::INVALID_DUPLICATE_DECLARATION;
        }
        if !is_valid_strict_mode {
            result |= DeclarationResult::INVALID_STRICT_MODE;
        }
        if is_await_using {
            self.lexical_variables.set_has_await_using_declaration();
        }

        result
    }

    pub fn has_declared_global_arguments(&self) -> bool {
        let ident = &self.vm.property_names.arguments;
        self.has_lexically_declared_variable_identifier(ident)
            || self.has_declared_variable_identifier(ident)
            || self.shadows_arguments()
    }

    pub fn has_declared_variable_identifier(&self, ident: &Identifier) -> bool {
        self.has_declared_variable(&uid(ident))
    }

    pub fn has_declared_variable(&self, ident: &UniquedKey) -> bool {
        match self.declared_variables.find(ident) {
            None => false,
            Some(entry) => entry.is_var(), // The callee isn't a "var".
        }
    }

    pub fn has_lexically_declared_variable_identifier(&self, ident: &Identifier) -> bool {
        self.has_lexically_declared_variable(&uid(ident))
    }

    pub fn has_lexically_declared_variable(&self, ident: &UniquedKey) -> bool {
        self.lexical_variables.contains(ident)
    }

    pub fn has_variable_being_hoisted(&self, ident: &UniquedKey) -> bool {
        self.variables_being_hoisted.contains(ident)
    }

    pub fn has_private_name(&self, ident: &Identifier) -> bool {
        self.lexical_variables.has_private_name(ident)
    }

    pub fn declare_private_method(&mut self, ident: &Identifier, tag: ClassElementTag) -> DeclarationResultMask {
        let mut result = DeclarationResult::VALID;
        let key = uid(ident);
        let traits = if tag == ClassElementTag::Static {
            PrivateNameEntry::IS_METHOD | PrivateNameEntry::IS_STATIC
        } else {
            PrivateNameEntry::NONE
        };
        let add_result = self.lexical_variables.declare_private_method(&key, traits);

        if !add_result {
            result |= DeclarationResult::INVALID_DUPLICATE_DECLARATION;
            return result;
        }

        result
    }

    pub fn declare_private_accessor(
        &mut self,
        ident: &Identifier,
        tag: ClassElementTag,
        accessor_type: PrivateAccessorType,
    ) -> DeclarationResultMask {
        let mut result = DeclarationResult::VALID;
        let key = uid(ident);
        let traits = if tag == ClassElementTag::Static { PrivateNameEntry::IS_STATIC } else { PrivateNameEntry::NONE };
        let add_result = if accessor_type == PrivateAccessorType::Setter {
            self.lexical_variables.declare_private_setter(&key, traits)
        } else {
            self.lexical_variables.declare_private_getter(&key, traits)
        };

        if add_result == PrivateDeclarationResult::DuplicatedName {
            result |= DeclarationResult::INVALID_DUPLICATE_DECLARATION;
        }

        if add_result == PrivateDeclarationResult::InvalidStaticNonStatic {
            result |= DeclarationResult::INVALID_PRIVATE_STATIC_NON_STATIC;
        }

        result
    }

    pub fn declare_private_setter(&mut self, ident: &Identifier, tag: ClassElementTag) -> DeclarationResultMask {
        self.declare_private_accessor(ident, tag, PrivateAccessorType::Setter)
    }

    pub fn declare_private_getter(&mut self, ident: &Identifier, tag: ClassElementTag) -> DeclarationResultMask {
        self.declare_private_accessor(ident, tag, PrivateAccessorType::Getter)
    }

    pub fn declare_private_field(&mut self, ident: &Identifier) -> DeclarationResultMask {
        let mut result = DeclarationResult::VALID;
        let add_result = self.lexical_variables.declare_private_field(&uid(ident));
        if !add_result.is_new_entry {
            result |= DeclarationResult::INVALID_DUPLICATE_DECLARATION;
        }
        result
    }

    pub fn has_declared_parameter_identifier(&self, ident: &Identifier) -> bool {
        self.has_declared_parameter(&uid(ident))
    }

    pub fn has_declared_parameter(&self, ident: &UniquedKey) -> bool {
        self.declared_parameters.contains(ident) || self.has_declared_variable(ident)
    }

    pub fn prevent_all_variable_declarations(&mut self) {
        self.allows_var_declarations = false;
        self.allows_lexical_declarations = false;
    }

    pub fn prevent_var_declarations(&mut self) {
        self.allows_var_declarations = false;
    }

    pub fn allows_var_declarations(&self) -> bool {
        self.allows_var_declarations
    }

    pub fn allows_lexical_declarations(&self) -> bool {
        self.allows_lexical_declarations
    }

    pub fn declare_parameter(&mut self, ident: &Identifier) -> DeclarationResultMask {
        let mut result = DeclarationResult::VALID;
        let is_arguments_ident = is_arguments(&self.vm, ident);
        let key = uid(ident);
        let add_result = self.declared_variables.add(&key);
        let is_duplicate_parameter = !add_result.is_new_entry && add_result.value.is_parameter();
        let is_valid_strict_mode = !is_duplicate_parameter && self.vm.property_names.eval != *ident && !is_arguments_ident;
        add_result.value.clear_is_var();
        add_result.value.set_is_parameter();
        self.is_valid_strict_mode = self.is_valid_strict_mode && is_valid_strict_mode;
        self.declared_parameters.add(&key, ());
        if !is_valid_strict_mode {
            result |= DeclarationResult::INVALID_STRICT_MODE;
        }
        if is_arguments_ident {
            self.shadows_arguments = true;
        }
        if is_duplicate_parameter {
            result |= DeclarationResult::INVALID_DUPLICATE_DECLARATION;
        }

        result
    }

    pub fn used_variables_contains(&self, impl_: &UniquedKey) -> bool {
        self.used_variables.iter().any(|set| set.contains(impl_))
    }

    pub fn for_each_used_variable<F: FnMut(&UniquedKey) -> IterationStatus>(&self, mut func: F) {
        for set in &self.used_variables {
            for (impl_, _) in set.iter() {
                if func(impl_) == IterationStatus::Done {
                    return;
                }
            }
        }
    }

    pub fn use_variable_identifier(&mut self, ident: &Identifier, is_eval: bool) {
        self.use_variable(&uid(ident), is_eval);
    }

    pub fn use_variable(&mut self, impl_: &UniquedKey, is_eval: bool) {
        self.uses_eval |= is_eval;
        if self.last_added_used_variable.as_ref() == Some(impl_) {
            // A failure indicates that m_usedVariables was changed (a set added or removed)
            // without clearing m_lastAddedUsedVariable.
            return;
        }
        self.last_added_used_variable = Some(impl_.clone());
        if let Some(last) = self.used_variables.last_mut() {
            last.add(impl_, ());
        }
    }

    pub fn use_private_name(&mut self, ident: &Identifier) {
        self.use_variable_identifier(ident, false);
    }

    pub fn set_uses_import_meta(&mut self) {
        self.uses_import_meta = true;
    }

    pub fn push_used_variable_set(&mut self) {
        self.used_variables.push(UniquedStringImplPtrSet::default());
        self.last_added_used_variable = None;
    }

    pub fn current_used_variables_size(&self) -> usize {
        self.used_variables.len()
    }

    pub fn revert_to_previous_used_variables(&mut self, size: usize) {
        self.used_variables.resize_with(size, UniquedStringImplPtrSet::default);
        self.last_added_used_variable = None;
    }

    pub fn set_needs_full_activation(&mut self) {
        self.needs_full_activation = true;
    }

    pub fn needs_full_activation(&self) -> bool {
        self.needs_full_activation
    }

    pub fn is_arrow_function_boundary(&self) -> bool {
        self.is_arrow_function_boundary
    }

    pub fn is_arrow_function(&self) -> bool {
        self.is_arrow_function
    }

    pub fn set_async_function_body_does_not_use_await(&mut self) {
        self.async_function_body_does_not_use_await = true;
    }

    pub fn async_function_body_does_not_use_await(&self) -> bool {
        self.async_function_body_does_not_use_await
    }

    pub fn set_uses_await(&mut self) {
        self.uses_await = true;
    }

    pub fn uses_await(&self) -> bool {
        self.uses_await
    }

    pub fn has_using_declaration(&self) -> bool {
        self.lexical_variables.has_using_declaration()
    }

    pub fn has_direct_super(&self) -> bool {
        self.has_direct_super
    }

    pub fn set_has_direct_super(&mut self) {
        self.has_direct_super = true;
    }

    pub fn needs_super_binding(&self) -> bool {
        self.needs_super_binding
    }

    pub fn set_needs_super_binding(&mut self) {
        self.needs_super_binding = true;
    }

    pub fn set_eval_context_type(&mut self, eval_context_type: EvalContextType) {
        self.eval_context_type = eval_context_type;
    }

    pub fn eval_context_type(&self) -> EvalContextType {
        self.eval_context_type
    }

    pub fn set_derived_context_type(&mut self, derived_context_type: DerivedContextType) {
        self.derived_context_type = derived_context_type;
    }

    pub fn derived_context_type(&self) -> DerivedContextType {
        self.derived_context_type
    }

    pub fn inner_arrow_function_features(&self) -> InnerArrowFunctionCodeFeatures {
        self.inner_arrow_function_features
    }

    pub fn set_expected_super_binding(&mut self, super_binding: SuperBinding) {
        self.expected_super_binding = super_binding;
    }

    pub fn expected_super_binding(&self) -> SuperBinding {
        self.expected_super_binding
    }

    pub fn set_constructor_kind(&mut self, constructor_kind: ConstructorKind) {
        self.constructor_kind = constructor_kind;
    }

    pub fn constructor_kind(&self) -> ConstructorKind {
        self.constructor_kind
    }

    pub fn set_inner_arrow_function_uses_super_call(&mut self) {
        self.inner_arrow_function_features |= SUPER_CALL_INNER_ARROW_FUNCTION_FEATURE;
    }

    pub fn set_inner_arrow_function_uses_super_property(&mut self) {
        self.inner_arrow_function_features |= SUPER_PROPERTY_INNER_ARROW_FUNCTION_FEATURE;
    }

    pub fn set_inner_arrow_function_uses_eval(&mut self) {
        self.inner_arrow_function_features |= EVAL_INNER_ARROW_FUNCTION_FEATURE;
    }

    pub fn set_inner_arrow_function_uses_this(&mut self) {
        self.inner_arrow_function_features |= THIS_INNER_ARROW_FUNCTION_FEATURE;
    }

    pub fn set_inner_arrow_function_uses_new_target(&mut self) {
        self.inner_arrow_function_features |= NEW_TARGET_INNER_ARROW_FUNCTION_FEATURE;
    }

    pub fn set_inner_arrow_function_uses_arguments(&mut self) {
        self.inner_arrow_function_features |= ARGUMENTS_INNER_ARROW_FUNCTION_FEATURE;
    }

    pub fn is_eval_context(&self) -> bool {
        self.is_eval_context
    }

    pub fn set_is_eval_context(&mut self, is_eval_context: bool) {
        self.is_eval_context = is_eval_context;
    }

    pub fn set_inner_arrow_function_uses_eval_and_use_arguments_if_needed(&mut self) {
        if self.uses_eval {
            self.set_inner_arrow_function_uses_eval();
        }

        let arguments = uid(&self.vm.property_names.arguments);
        if self.used_variables_contains(&arguments) {
            self.set_inner_arrow_function_uses_arguments();
        }
    }

    pub fn add_closed_variable_candidate_unconditionally(&mut self, impl_: &UniquedKey) {
        self.closed_variable_candidates.add(impl_, ());
    }

    pub fn mark_last_used_variables_set_as_captured(&mut self, from: usize) {
        for index in from..self.used_variables.len() {
            for (impl_, _) in self.used_variables[index].iter() {
                self.closed_variable_candidates.add(impl_, ());
            }
        }
    }

    /// `collectFreeVariablesFrom(Scope* nestedScope, ...)`: `nested_scope` é outro escopo da pilha.
    pub fn collect_free_variables_from(
        &mut self,
        nested_scope: &Scope,
        should_track_closed_variables: bool,
        has_precomputed_free_variables: bool,
        precomputed_free_variables: &[UniquedKey],
    ) {
        if nested_scope.uses_eval {
            self.uses_eval = true;
        }
        if nested_scope.uses_import_meta {
            self.uses_import_meta = true;
        }

        {
            // If nestedScope is a non-arrow function and there is an "arguments" reference,
            // we need to filter it because it should not propagate out.
            let arguments_identifier_or_null: Option<UniquedKey> =
                if nested_scope.is_function_boundary() && nested_scope.has_arguments() && !nested_scope.is_arrow_function_boundary() {
                    self.vm.property_names.arguments.impl_()
                } else {
                    None
                };
            // We don't want a declared variable that is used in an inner scope to be thought of as captured if
            // that inner scope is both a lexical scope and not a function. Only inner functions and "catch"
            // statements can cause variables to be captured.
            let do_track_closed_variables =
                should_track_closed_variables && (nested_scope.is_function_boundary || !nested_scope.is_lexical_scope);
            let Scope { used_variables, closed_variable_candidates, .. } = &mut *self;
            let mut destination_set = used_variables.last_mut();
            let mut propagate_free_variable = |impl_: &UniquedKey| {
                if arguments_identifier_or_null.as_ref() == Some(impl_) {
                    return;
                }
                if let Some(destination_set) = destination_set.as_deref_mut() {
                    destination_set.add(impl_, ());
                }
                if do_track_closed_variables {
                    closed_variable_candidates.add(impl_, ());
                }
            };

            if has_precomputed_free_variables {
                for impl_ in precomputed_free_variables {
                    propagate_free_variable(impl_);
                }
            } else {
                for used_variables_set in &nested_scope.used_variables {
                    for (impl_, _) in used_variables_set.iter() {
                        if nested_scope.declared_variables.contains(impl_) || nested_scope.lexical_variables.contains(impl_) {
                            continue;
                        }
                        propagate_free_variable(impl_);
                    }
                }
            }
        }
        // Propagate closed variable candidates downwards within the same function.
        // Cross function captures will be realized via m_usedVariables propagation.
        if should_track_closed_variables && !nested_scope.is_function_boundary && nested_scope.closed_variable_candidates.len() != 0 {
            for (impl_, _) in nested_scope.closed_variable_candidates.iter() {
                self.closed_variable_candidates.add(impl_, ());
            }
        }
    }

    pub fn merge_inner_arrow_function_features(&mut self, arrow_function_code_features: InnerArrowFunctionCodeFeatures) {
        self.inner_arrow_function_features |= arrow_function_code_features;
    }

    // Campos e funções privadas de `Scope` das linhas 920 a 1012 do `.h` (o `private:` da struct).

    fn set_is_function(&mut self) {
        self.is_function = true;
        self.is_function_boundary = true;
        self.has_arguments = true;
        self.set_is_lexical_scope();
        self.is_generator_function = false;
        self.is_generator_function_boundary = false;
        self.is_arrow_function_boundary = false;
        self.is_arrow_function = false;
        self.is_async_function = false;
        self.is_async_function_boundary = false;
        self.is_static_block = false;
        self.is_static_block_boundary = false;
    }

    fn set_is_generator_function(&mut self) {
        self.set_is_function();
        self.is_generator_function = true;
    }

    fn set_is_generator_function_body(&mut self) {
        self.set_is_function();
        self.has_arguments = false;
        self.is_generator_function = true;
        self.is_generator_function_boundary = true;
    }

    fn set_is_arrow_function(&mut self) {
        self.set_is_function();
        self.is_arrow_function_boundary = true;
        self.is_arrow_function = true;
    }

    fn set_is_async_arrow_function(&mut self) {
        self.set_is_arrow_function();
        self.is_async_function = true;
    }

    fn set_is_async_function(&mut self) {
        self.set_is_function();
        self.is_async_function = true;
    }

    fn set_is_async_generator_function(&mut self) {
        self.set_is_function();
        self.is_async_function = true;
        self.is_generator_function = true;
    }

    fn set_is_async_generator_function_body(&mut self) {
        self.set_is_function();
        self.has_arguments = false;
        self.is_generator_function = true;
        self.is_generator_function_boundary = true;
        self.is_async_function = true;
        self.is_async_function_boundary = true;
    }

    fn set_is_async_function_body(&mut self) {
        self.set_is_function();
        self.has_arguments = false;
        self.is_async_function = true;
        self.is_async_function_boundary = true;
    }

    fn set_is_async_arrow_function_body(&mut self) {
        self.set_is_arrow_function();
        self.has_arguments = false;
        self.is_async_function = true;
        self.is_async_function_boundary = true;
    }

    fn set_is_global_code(&mut self) {
        self.is_global_code = true;
    }

    fn set_is_module_code(&mut self) {
        self.set_is_global_code();
        self.is_module_code = true;
    }

    // As funções das linhas 801 a 916 (`finalizeSloppyModeFunctionHoisting`, `strictMode`,
    // `lexicallyScopedFeatures`, `getCapturedVars`, ...) vêm no `impl Scope` de `parser_part2.rs`.
    // `strict_mode` é usado acima (linhas 850 a 862 do `.h`):

    pub fn strict_mode(&self) -> bool {
        self.lexically_scoped_features & STRICT_MODE_LEXICALLY_SCOPED_FEATURE != 0
    }

    pub fn shadows_arguments(&self) -> bool {
        self.shadows_arguments
    }

    pub fn set_strict_mode(&mut self) {
        self.lexically_scoped_features |= STRICT_MODE_LEXICALLY_SCOPED_FEATURE;
    }

    pub fn set_tainted_by_with_scope(&mut self) {
        self.lexically_scoped_features |= TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE;
    }
}

// continua em parser_part2.rs
include!("parser_part2.rs");
include!("parser_part3.rs");
include!("parser_cpp1.rs");
include!("parser_cpp2.rs");
include!("parser_cpp3.rs");
include!("parser_cpp4.rs");
include!("parser_cpp5.rs");
include!("parser_cpp6.rs");
include!("parser_cpp7.rs");
include!("parser_cpp8.rs");
