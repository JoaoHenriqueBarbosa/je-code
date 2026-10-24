// Parte 3 de bytecompiler/BytecodeGenerator.h (linhas 1000 a 1480 do .h). Juntada por include!.
// Convenção: `RegisterID*` e `RefPtr<RegisterID>` do gerador são `RegisterRef` (anulável: `Option<RegisterRef>`).
// O armazenamento (`parameters`, `constant_pool_registers`, `callee_locals`) guarda `Rc<RefCell<RegisterID>>`
// sem contagem; os acessores devolvem `RegisterRef::new(&armazenamento)`. Os membros da base `BytecodeGeneratorBase<JSGeneratorTraits>`
// (`m_codeBlock`, `m_writer`, `m_calleeLocals`, `m_lastInstruction`, `m_lastOpcodeID`, `m_labels`)
// vivem no campo `base`; o `Deref`/`DerefMut` da struct para a base é a herança do C++ (o acesso
// `self.writer` e a chamada `self.new_label()` resolvem na superclasse, como em `m_writer`/`newLabel()`).
// O ramo `USE(BUN_JSC_ADDITIONS)` não é compilado: vale o `#else` (`isPrivateBuiltinFunction` repassa
// `isBuiltinFunction`) e o campo `m_isPrivateBuiltinFunction` não existe.

/// `ScopeType` (BytecodeGenerator.h:1034).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ScopeType {
    CatchScope,
    CatchScopeWithSimpleParameter,
    LetConstScope,
    FunctionNameScope,
    ClassScope,
}

/// `TDZCheckOptimization` (BytecodeGenerator.h:1130).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TDZCheckOptimization {
    Optimize,
    DoNotOptimize,
}

/// `NestedScopeType` (BytecodeGenerator.h:1131).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NestedScopeType {
    IsNested,
    IsNotNested,
}

/// `TDZRequirement` (privado).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TDZRequirement {
    UnderTDZ,
    NotUnderTDZ,
}

/// `ScopeRegisterType` (privado).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeRegisterType {
    Var,
    Block,
}

/// `TDZNecessityLevel` (privado).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TDZNecessityLevel {
    NotNeeded,
    Optimize,
    DoNotOptimize,
}

/// `FunctionVariableType` (privado).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FunctionVariableType {
    NormalFunctionVariable,
    TopLevelFunctionVariable,
}

/// `TDZMap`: `UncheckedKeyHashMap<RefPtr<UniquedStringImpl>, TDZNecessityLevel, IdentifierRepHash>`.
pub type TDZMap = std::collections::HashMap<
    crate::wtf::text::string_impl::UniquedKey,
    TDZNecessityLevel,
>;

/// `TDZStackEntry`.
pub type TDZStackEntry = (
    TDZMap,
    Option<std::rc::Rc<crate::parser::variable_environment::TDZEnvironmentLink>>,
);

/// `BigIntMapEntry`.
pub type BigIntMapEntry = (Option<crate::wtf::text::string_impl::UniquedKey>, u8, bool);

/// `class PreservedTDZStack` (BytecodeGenerator.h:1294). `friend class BytecodeGenerator`.
#[derive(Default)]
pub struct PreservedTDZStack {
    pub(crate) preserved_tdz_stack: Vec<TDZStackEntry>,
}

/// `struct LexicalScopeStackEntry` (privado).
#[derive(Clone)]
pub struct LexicalScopeStackEntry {
    pub symbol_table: Option<std::rc::Rc<std::cell::RefCell<crate::runtime::symbol_table::SymbolTable>>>,
    pub scope: Option<crate::bytecompiler::register_id::RegisterRef>,
    pub is_with_scope: bool,
    pub symbol_table_constant_index: i32,
}

/// `struct AsyncFuncParametersTryCatchInfo` (privado).
#[derive(Default, Clone)]
pub struct AsyncFuncParametersTryCatchInfo {
    pub catch_start_label: Option<crate::bytecompiler::label::LabelRef>,
    pub thrown_value: Option<crate::bytecompiler::register_id::RegisterRef>,
}

/// `struct CatchEntry` (privado).
pub struct CatchEntry {
    pub try_data: std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::TryData>>,
    pub exception_register: crate::bytecode::virtual_register::VirtualRegister,
    pub thrown_value_register: crate::bytecode::virtual_register::VirtualRegister,
    pub completion_type_register: crate::bytecode::virtual_register::VirtualRegister,
}

/// O `struct { JSTextPosition position; DebugHookType type { DidExecuteProgram }; } m_lastDebugHook`.
pub struct LastDebugHook {
    pub position: crate::parser::parser_tokens::JSTextPosition,
    pub type_: crate::interpreter::interpreter::DebugHookType,
}

/// `class BytecodeGenerator` (BytecodeGenerator.h:1337 a 1437, os campos). Todos os campos do C++, em
/// snake_case do `m_` correspondente. `RegisterID*` anulável vira `Option<RegisterRef>`;
/// `SegmentedVector<T, N>` vira `Vec<T>` (os ponteiros do C++ para dentro dele viram `Rc`).
pub struct BytecodeGenerator {
    /// A superclasse `BytecodeGeneratorBase<JSGeneratorTraits>` (`m_codeBlock`, `m_writer`, `m_labels`,
    /// `m_calleeLocals`, `m_lastInstruction`, `m_lastOpcodeID`), alcançada por `Deref`/`DerefMut`.
    pub base: crate::bytecompiler::bytecode_generator_base::BytecodeGeneratorBase<
        crate::bytecompiler::bytecode_generator::JSGeneratorTraits,
    >,

    // Campos da própria classe.
    pub code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
    pub lexical_scope_stack: Vec<LexicalScopeStackEntry>,
    pub cached_parent_tdz: Option<std::rc::Rc<crate::parser::variable_environment::TDZEnvironmentLink>>,
    pub generator_or_async_wrapper_function_parameter_names: Option<std::rc::Rc<Vec<crate::runtime::identifier::Identifier>>>,
    pub tdz_stack: Vec<TDZStackEntry>,
    pub private_names_stack: Vec<crate::parser::variable_environment::PrivateNameEnvironment>,
    pub var_scope_lexical_scope_stack_index: Option<usize>,
    pub scope_node: crate::parser::nodes::ScopeNodeRef,
    pub functions: std::collections::HashSet<crate::wtf::text::string_impl::UniquedKey>,
    pub ignored_result_register: crate::bytecompiler::register_id::RegisterID,
    pub this_register: crate::bytecompiler::register_id::RegisterID,
    /// `RegisterID m_calleeRegister`: por valor no C++, mas o gerador entrega o registrador por
    /// identidade (`&m_calleeRegister`), então aqui é `RegisterRef` (os usos fazem `borrow_mut`/`clone`).
    pub callee_register: crate::bytecompiler::register_id::RegisterID,
    pub scope_register: Option<crate::bytecompiler::register_id::RegisterRef>,
    // Só Eval e Module configuram este registrador.
    pub top_level_scope_register: Option<crate::bytecompiler::register_id::RegisterRef>,
    pub arguments_register: Option<crate::bytecompiler::register_id::RegisterRef>,
    pub lexical_environment_register: Option<crate::bytecompiler::register_id::RegisterRef>,
    pub generator_register: Option<crate::bytecompiler::register_id::RegisterRef>,
    pub empty_value_register: Option<crate::bytecompiler::register_id::RegisterRef>,
    pub new_target_register: Option<crate::bytecompiler::register_id::RegisterRef>,
    // Grafia do C++ preservada: `m_isDerivedConstuctor`.
    pub is_derived_constuctor: Option<crate::bytecompiler::register_id::RegisterRef>,
    pub link_time_constant_registers: std::collections::HashMap<
        crate::bytecode::link_time_constant::LinkTimeConstant,
        crate::bytecompiler::register_id::RegisterRef,
    >,
    pub arrow_function_context_lexical_environment_register: Option<crate::bytecompiler::register_id::RegisterRef>,
    pub promise_register: Option<crate::bytecompiler::register_id::RegisterRef>,
    pub current_finally_context: Option<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::FinallyContext>>>,
    pub parameters: Vec<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::register_id::RegisterID>>>,
    pub label_scopes: Vec<std::rc::Rc<crate::bytecompiler::label_scope::LabelScope>>,
    pub constant_pool_registers: Vec<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::register_id::RegisterID>>>,
    pub finally_depth: u32,
    pub local_scope_depth: u32,
    pub local_scope_count: u32,
    pub code_type: crate::bytecode::code_type::CodeType,
    pub control_flow_scope_stack: Vec<crate::bytecompiler::bytecode_generator::ControlFlowScope>,
    pub switch_context_stack: Vec<crate::parser::nodes::SwitchInfo>,
    pub for_in_context_stack: Vec<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::ForInContext>>>,
    pub try_context_stack: Vec<crate::bytecompiler::bytecode_generator::TryContext>,
    pub using_scope_stack: Vec<crate::bytecompiler::bytecode_generator::UsingScope>,
    pub yield_points: u32,
    pub needs_generatorification: bool,
    pub generator_frame_symbol_table: Option<std::rc::Rc<std::cell::RefCell<crate::runtime::symbol_table::SymbolTable>>>,
    pub generator_frame_symbol_table_index: i32,
    pub functions_to_initialize: Vec<(crate::parser::nodes::FunctionMetadataNodeRef, FunctionVariableType)>,
    pub need_to_initialize_arguments: bool,
    pub rest_parameter: Option<crate::parser::nodes::RestParameterNodeRef>,
    pub async_func_parameters_try_catch_info: Option<AsyncFuncParametersTryCatchInfo>,
    pub try_ranges: Vec<crate::bytecompiler::bytecode_generator::TryRange>,
    pub try_data: Vec<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::TryData>>>,
    pub optional_chain_target_stack: Vec<crate::bytecompiler::label::LabelRef>,
    pub next_constant_offset: i32,
    // Constant pool.
    pub identifier_map: crate::bytecompiler::bytecode_generator::IdentifierMap,
    pub js_value_map: std::collections::HashMap<
        crate::runtime::js_value::EncodedJSValueWithRepresentation,
        u32,
    >,
    pub string_map: std::collections::HashMap<
        Option<crate::wtf::text::string_impl::UniquedKey>,
        crate::runtime::js_string::JSStringRef,
    >,
    pub big_int_map: std::collections::HashMap<BigIntMapEntry, crate::runtime::js_value::JSValue>,
    pub template_object_descriptor_set: std::collections::HashSet<
        std::rc::Rc<crate::runtime::template_object_descriptor::TemplateObjectDescriptor>,
    >,
    pub template_descriptor_map: std::collections::HashMap<
        u64,
        crate::runtime::js_template_object_descriptor::JSTemplateObjectDescriptorRef,
    >,
    pub static_property_analyzer: crate::bytecompiler::static_property_analyzer::StaticPropertyAnalyzer,
    pub vm: std::rc::Rc<crate::runtime::vm::VM>,
    pub default_allow_call_ignore_result_optimization: bool,
    pub uses_exceptions: bool,
    pub expression_too_deep: bool,
    pub is_builtin_function: bool,
    pub is_builtin_default_class_constructor: bool,
    pub uses_sloppy_eval: bool,
    pub allow_tail_call_optimization: bool,
    pub allow_call_ignore_result_optimization: bool,
    // Campos de bit (`: 1`).
    pub needs_to_update_arrow_function_context: bool,
    pub needs_arguments: bool,
    pub ecma_mode: crate::runtime::ecma_mode::ECMAMode,
    pub derived_context_type: crate::bytecode::executable_info::DerivedContextType,
    pub exception_handlers_to_emit: Vec<CatchEntry>,
    pub last_debug_hook: LastDebugHook,
}

// Campos: code_generation_mode: OptionSet<CodeGenerationMode>
// Campos: lexical_scope_stack: Vector<LexicalScopeStackEntry>
// Campos: cached_parent_tdz: RefPtr<TDZEnvironmentLink>
// Campos: generator_or_async_wrapper_function_parameter_names: *const FixedVector<Identifier> (anulável)
// Campos: tdz_stack: Vector<TDZStackEntry>
// Campos: private_names_stack: Vector<PrivateNameEnvironment>
// Campos: var_scope_lexical_scope_stack_index: Option<usize>
// Campos: scope_node: ScopeNode* const
// Campos: functions: HashSet<RefPtr<UniquedStringImpl>>
// Campos: ignored_result_register, this_register, callee_register: RegisterID
// Campos: scope_register, top_level_scope_register, arguments_register, lexical_environment_register,
//         generator_register, empty_value_register, new_target_register, is_derived_constuctor,
//         arrow_function_context_lexical_environment_register, promise_register: Option<RegisterRef>
// Campos: link_time_constant_registers: HashMap<LinkTimeConstant, RegisterRef>
// Campos: current_finally_context: Option<Rc<RefCell<FinallyContext>>>
// Campos: parameters, constant_pool_registers: SegmentedVector<RegisterID, 32>
// Campos: label_scopes: SegmentedVector<LabelScope, 32>
// Campos: finally_depth, local_scope_depth, local_scope_count, yield_points: u32
// Campos: code_type: CodeType (const)
// Campos: control_flow_scope_stack: SegmentedVector<ControlFlowScope, 16>
// Campos: switch_context_stack: Vector<SwitchInfo>; for_in_context_stack: Vector<Ref<ForInContext>>
// Campos: try_context_stack: Vector<TryContext>; using_scope_stack: SegmentedVector<UsingScope, 8>
// Campos: needs_generatorification: bool
// Campos: generator_frame_symbol_table: Strong<SymbolTable>; generator_frame_symbol_table_index: i32
// Campos: functions_to_initialize: Vector<(FunctionMetadataNode*, FunctionVariableType)>
// Campos: need_to_initialize_arguments: bool; rest_parameter: RestParameterNode*
// Campos: async_func_parameters_try_catch_info: Option<AsyncFuncParametersTryCatchInfo>
// Campos: try_ranges: Vector<TryRange>; try_data: SegmentedVector<TryData, 8>
// Campos: optional_chain_target_stack: Vector<Ref<Label>>; next_constant_offset: i32
// Campos: identifier_map, js_value_map, string_map, big_int_map, template_object_descriptor_set,
//         template_descriptor_map (constant pool)
// Campos: static_property_analyzer; vm; flags bool (default_allow_call_ignore_result_optimization,
//         uses_exceptions, expression_too_deep, is_builtin_function, is_builtin_default_class_constructor,
//         uses_sloppy_eval, allow_tail_call_optimization, allow_call_ignore_result_optimization,
//         needs_to_update_arrow_function_context, needs_arguments); ecma_mode; derived_context_type
// Campos: exception_handlers_to_emit: Vector<CatchEntry>; last_debug_hook

impl std::ops::Deref for BytecodeGenerator {
    type Target = crate::bytecompiler::bytecode_generator_base::BytecodeGeneratorBase<
        crate::bytecompiler::bytecode_generator::JSGeneratorTraits,
    >;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl std::ops::DerefMut for BytecodeGenerator {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

/// `class StrictModeScope : private SetForScope<ECMAMode>` (BytecodeGenerator.h:1441). Guarda o valor
/// anterior e o restaura em `Drop`; o gerador é acessado pelo chamador via `generator()`.
pub struct StrictModeScope<'a> {
    generator: &'a mut BytecodeGenerator,
    saved: crate::runtime::ecma_mode::ECMAMode,
}

impl<'a> StrictModeScope<'a> {
    pub fn new(generator: &'a mut BytecodeGenerator) -> StrictModeScope<'a> {
        let saved = generator.ecma_mode;
        generator.ecma_mode = crate::runtime::ecma_mode::ECMAMode::strict();
        StrictModeScope { generator, saved }
    }

    pub fn generator(&mut self) -> &mut BytecodeGenerator {
        self.generator
    }
}

impl<'a> Drop for StrictModeScope<'a> {
    fn drop(&mut self) {
        self.generator.ecma_mode = self.saved;
    }
}

impl BytecodeGenerator {
    pub const CURRENT_LEXICAL_SCOPE_INDEX: i32 = -2;
    pub const OUTERMOST_LEXICAL_SCOPE_INDEX: i32 = -1;

    /// O que o C++ faz antes do corpo de qualquer construtor do `BytecodeGenerator`: o
    /// `BytecodeGeneratorBase(makeUnique<UnlinkedCodeBlockGenerator>(vm, codeBlock),
    /// CodeBlock::llintBaselineCalleeSaveSpaceAsVirtualRegisters())` e o inicializador de membros do .h
    /// (`m_codeGenerationMode`, `m_scopeNode`, `m_thisRegister(thisArgumentOffset)`, `m_codeType` e
    /// os `{ false }`/`{ nullptr }`/`{ 0 }` dos demais). `code_block` é a base (`base_ref()`) do
    /// subtipo `Unlinked*CodeBlock`; os construtores só sobrescrevem o que diverge do padrão.
    pub fn with_defaults(
        vm: &std::rc::Rc<crate::runtime::vm::VM>,
        code_block: std::rc::Rc<std::cell::RefCell<crate::bytecode::unlinked_code_block::UnlinkedCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        scope_node: crate::parser::nodes::ScopeNodeRef,
        code_type: crate::bytecode::code_type::CodeType,
    ) -> BytecodeGenerator {
        let generator = crate::bytecode::unlinked_code_block_generator::UnlinkedCodeBlockGenerator::new(vm, code_block);
        let base = crate::bytecompiler::bytecode_generator_base::BytecodeGeneratorBase::<
            crate::bytecompiler::bytecode_generator::JSGeneratorTraits,
        >::new(
            generator,
            crate::bytecode::code_block::llint_baseline_callee_save_space_as_virtual_registers(),
        );
        BytecodeGenerator {
            base,
            code_generation_mode,
            lexical_scope_stack: Vec::new(),
            cached_parent_tdz: None,
            generator_or_async_wrapper_function_parameter_names: None,
            tdz_stack: Vec::new(),
            private_names_stack: Vec::new(),
            var_scope_lexical_scope_stack_index: None,
            scope_node,
            functions: std::collections::HashSet::new(),
            ignored_result_register: crate::bytecompiler::register_id::RegisterID::default(),
            this_register: crate::bytecompiler::register_id::RegisterID::from_index(
                crate::interpreter::call_frame::this_argument_offset(),
            ),
            callee_register: crate::bytecompiler::register_id::RegisterID::default(),
            scope_register: None,
            top_level_scope_register: None,
            arguments_register: None,
            lexical_environment_register: None,
            generator_register: None,
            empty_value_register: None,
            new_target_register: None,
            is_derived_constuctor: None,
            link_time_constant_registers: std::collections::HashMap::new(),
            arrow_function_context_lexical_environment_register: None,
            promise_register: None,
            current_finally_context: None,
            parameters: Vec::new(),
            label_scopes: Vec::new(),
            constant_pool_registers: Vec::new(),
            finally_depth: 0,
            local_scope_depth: 0,
            local_scope_count: 0,
            code_type,
            control_flow_scope_stack: Vec::new(),
            switch_context_stack: Vec::new(),
            for_in_context_stack: Vec::new(),
            try_context_stack: Vec::new(),
            using_scope_stack: Vec::new(),
            yield_points: 0,
            needs_generatorification: false,
            generator_frame_symbol_table: None,
            generator_frame_symbol_table_index: 0,
            functions_to_initialize: Vec::new(),
            need_to_initialize_arguments: false,
            rest_parameter: None,
            async_func_parameters_try_catch_info: None,
            try_ranges: Vec::new(),
            try_data: Vec::new(),
            optional_chain_target_stack: Vec::new(),
            next_constant_offset: 0,
            identifier_map: Default::default(),
            js_value_map: std::collections::HashMap::new(),
            string_map: std::collections::HashMap::new(),
            big_int_map: std::collections::HashMap::new(),
            template_object_descriptor_set: std::collections::HashSet::new(),
            template_descriptor_map: std::collections::HashMap::new(),
            static_property_analyzer: crate::bytecompiler::static_property_analyzer::StaticPropertyAnalyzer::default(),
            vm: vm.clone(),
            default_allow_call_ignore_result_optimization: false,
            uses_exceptions: false,
            expression_too_deep: false,
            is_builtin_function: false,
            is_builtin_default_class_constructor: false,
            uses_sloppy_eval: false,
            allow_tail_call_optimization: false,
            allow_call_ignore_result_optimization: false,
            needs_to_update_arrow_function_context: false,
            needs_arguments: false,
            ecma_mode: crate::runtime::ecma_mode::ECMAMode::sloppy(),
            derived_context_type: crate::bytecode::executable_info::DerivedContextType::None,
            exception_handlers_to_emit: Vec::new(),
            last_debug_hook: LastDebugHook {
                position: crate::parser::parser_tokens::JSTextPosition::default(),
                type_: crate::interpreter::interpreter::DebugHookType::DidExecuteProgram,
            },
        }
    }

    // BytecodeGenerator.h:1004 (declaração)
    // pub fn try_resolve_variable(&mut self, node: &Expression) -> Option<Variable>;  // .cpp

    // BytecodeGenerator.h:1011
    pub fn current_lexical_scope_index(&self) -> i32 {
        let size = self.lexical_scope_stack.len() as i32;
        if size == 0 {
            return Self::OUTERMOST_LEXICAL_SCOPE_INDEX;
        }
        size - 1
    }

    // pub fn emit_out_of_line_exception_handler(&mut self, exception_register: &RegisterRef, thrown_value_register: &RegisterRef, completion_type_register: &RegisterRef, try_data: &Rc<RefCell<TryData>>);  // .cpp
    // pub fn emit_construct_impl<ConstructOp>(&mut self, dst: Option<RegisterRef>, func: &RegisterRef, lazy_this: Option<RegisterRef>, expected: ExpectedFunction, args: &mut CallArguments, divot: &JSTextPosition, divot_start: &JSTextPosition, divot_end: &JSTextPosition, is_default_derived_constructor_call: bool) -> Option<RegisterRef>;  // .cpp
    // pub fn restore_scope_register(&mut self);  // .cpp
    // pub fn restore_scope_register_at(&mut self, lexical_scope_index: i32);  // .cpp
    // pub fn label_scope_depth_to_lexical_scope_index(&mut self, label_scope_depth: i32) -> i32;  // .cpp
    // pub fn emit_throw(&mut self, value: &RegisterRef);  // .cpp
    // pub fn emit_argument_count(&mut self, dst: Option<RegisterRef>) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_throw_static_error_register(&mut self, ty: ErrorTypeWithExtension, message: &RegisterRef);  // .cpp
    // pub fn emit_throw_static_error(&mut self, ty: ErrorTypeWithExtension, message: &Identifier);  // .cpp (a sobrecarga de Register é `emit_throw_static_error_register`)
    // pub fn emit_throw_reference_error(&mut self, message: &str);  // .cpp
    // pub fn emit_throw_type_error(&mut self, message: &str);  // .cpp
    // pub fn emit_throw_type_error_identifier(&mut self, message: &Identifier);  // .cpp
    // pub fn emit_throw_range_error(&mut self, message: &Identifier);  // .cpp
    // pub fn emit_throw_out_of_memory_error(&mut self);  // .cpp
    // pub fn emit_push_catch_scope(&mut self, env: &mut VariableEnvironment, ty: ScopeType);  // .cpp
    // pub fn emit_pop_catch_scope(&mut self, env: &mut VariableEnvironment);  // .cpp
    // pub fn emit_push_with_scope(&mut self, object_scope: &RegisterRef) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_pop_with_scope(&mut self);  // .cpp
    // pub fn emit_put_this_to_arrow_function_context_scope(&mut self);  // .cpp
    // pub fn emit_put_new_target_to_arrow_function_context_scope(&mut self);  // .cpp
    // pub fn emit_put_derived_constructor_to_arrow_function_context_scope(&mut self);  // .cpp
    // pub fn emit_load_derived_constructor_from_arrow_function_lexical_environment(&mut self) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_load_derived_constructor(&mut self) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_debug_hook(&mut self, ty: DebugHookType, position: &JSTextPosition, data: Option<RegisterRef>);  // .cpp
    // pub fn emit_debug_hook_statement_data(&mut self, node: &Statement, data: Option<RegisterRef>);  // .cpp
    // pub fn emit_debug_hook_expression_data(&mut self, node: &Expression, data: Option<RegisterRef>);  // .cpp
    // pub fn emit_will_leave_call_frame_debug_hook(&mut self);  // .cpp

    // BytecodeGenerator.h:1067
    pub fn emit_load_completion_type(
        &mut self,
        dst: Option<crate::bytecompiler::register_id::RegisterRef>,
        ty: crate::bytecompiler::bytecode_generator::CompletionType,
    ) -> Option<crate::bytecompiler::register_id::RegisterRef> {
        self.emit_load_js_value(dst, crate::runtime::js_value::js_number_i32(ty.0))
    }

    // BytecodeGenerator.h:1068
    pub fn emit_load_resume_mode(
        &mut self,
        dst: Option<crate::bytecompiler::register_id::RegisterRef>,
        mode: crate::runtime::js_generator::ResumeMode,
    ) -> Option<crate::bytecompiler::register_id::RegisterRef> {
        self.emit_load_js_value(dst, crate::runtime::js_value::js_number_i32(mode as i32))
    }

    // pub fn emit_jump_via_finally_if_needed(&mut self, target_label_scope_depth: i32, jump_target: &Rc<Label>) -> bool;  // .cpp
    // pub fn emit_return_via_finally_if_needed(&mut self, return_register: &RegisterRef) -> bool;  // .cpp
    // pub fn emit_finally_completion(&mut self, ctx: &mut FinallyContext, normal_completion_label: &Rc<Label>);  // .cpp
    // pub fn push_finally_control_flow_scope(&mut self, ctx: &Rc<RefCell<FinallyContext>>);  // .cpp
    // pub fn pop_finally_control_flow_scope(&mut self);  // .cpp

    // BytecodeGenerator.h:1081
    pub fn has_finally_scopes(&self) -> bool {
        self.current_finally_context.is_some()
    }

    // pub fn push_optional_chain_target(&mut self);  // .cpp
    // pub fn push_optional_chain_target_label(&mut self, label: &Rc<Label>);  // .cpp
    // pub fn pop_optional_chain_target(&mut self);  // .cpp
    // pub fn pop_optional_chain_target_dst(&mut self, dst: &RegisterRef, is_delete: bool);  // .cpp
    // pub fn discard_optional_chain_target(&mut self);  // .cpp
    // pub fn emit_optional_check(&mut self, src: &RegisterRef);  // .cpp
    // pub fn push_for_in_scope(&mut self, local: &RegisterRef, property_name: &RegisterRef, property_offset: &RegisterRef, enumerator: &RegisterRef, mode: &RegisterRef, base: Option<Variable>);  // .cpp
    // pub fn pop_for_in_scope(&mut self, local: &RegisterRef);  // .cpp
    // pub fn break_target(&mut self, name: &Identifier) -> Option<LabelScopeRef>;  // .cpp
    // pub fn continue_target(&mut self, name: &Identifier) -> Option<LabelScopeRef>;  // .cpp
    // pub fn begin_switch(&mut self, value: &RegisterRef, ty: SwitchInfo::SwitchType);  // .cpp
    // pub fn end_switch(&mut self, labels: &[Rc<Label>], nodes: &[Option<Expression>], default_label: &Rc<Label>, min: i32, range: i32);  // .cpp
    // pub fn emit_yield_point(&mut self, value: &RegisterRef, reason: AsyncGeneratorSuspendReason);  // .cpp
    // pub fn emit_generator_state_label(&mut self);  // .cpp
    // pub fn emit_generator_state_change(&mut self, state: i32);  // .cpp
    // pub fn emit_yield(&mut self, argument: &RegisterRef) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_await(&mut self, dst: Option<RegisterRef>, src: &RegisterRef, position: &JSTextPosition) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_delegate_yield(&mut self, argument: &RegisterRef, node: &ThrowableExpressionData) -> Option<RegisterRef>;  // .cpp

    // BytecodeGenerator.h:1111
    pub fn generator_state_register(&self) -> crate::bytecompiler::register_id::RegisterRef {
        crate::bytecompiler::register_id::RegisterRef::new(
            &self.parameters[crate::runtime::js_generator::Argument::State as usize],
        )
    }

    // BytecodeGenerator.h:1112
    pub fn generator_value_register(&self) -> crate::bytecompiler::register_id::RegisterRef {
        crate::bytecompiler::register_id::RegisterRef::new(
            &self.parameters[crate::runtime::js_generator::Argument::Value as usize],
        )
    }

    // BytecodeGenerator.h:1113
    pub fn generator_resume_mode_register(&self) -> crate::bytecompiler::register_id::RegisterRef {
        crate::bytecompiler::register_id::RegisterRef::new(
            &self.parameters[crate::runtime::js_generator::Argument::ResumeMode as usize],
        )
    }

    // BytecodeGenerator.h:1114
    pub fn generator_frame_register(&self) -> crate::bytecompiler::register_id::RegisterRef {
        crate::bytecompiler::register_id::RegisterRef::new(
            &self.parameters[crate::runtime::js_generator::Argument::Frame as usize],
        )
    }

    // BytecodeGenerator.h:1116
    pub fn code_type(&self) -> crate::bytecode::code_type::CodeType {
        self.code_type
    }

    // BytecodeGenerator.h:1118
    pub fn should_be_concerned_with_completion_value(&self) -> bool {
        !self.default_allow_call_ignore_result_optimization
    }

    // BytecodeGenerator.h:1120
    pub fn should_emit_debug_hooks(&self) -> bool {
        self.code_generation_mode.contains(crate::parser::parser_modes::CodeGenerationMode::Debugger)
            && !self.is_builtin_function()
    }

    // BytecodeGenerator.h:1121
    pub fn should_emit_type_profiler_hooks(&self) -> bool {
        self.code_generation_mode.contains(crate::parser::parser_modes::CodeGenerationMode::TypeProfiler)
    }

    // BytecodeGenerator.h:1122
    pub fn should_emit_control_flow_profiler_hooks(&self) -> bool {
        self.code_generation_mode.contains(crate::parser::parser_modes::CodeGenerationMode::ControlFlowProfiler)
    }

    // BytecodeGenerator.h:1124
    pub fn ecma_mode(&self) -> crate::runtime::ecma_mode::ECMAMode {
        self.ecma_mode
    }

    // BytecodeGenerator.h:1125: `set_uses_checkpoints` é o método do `OpWriter` (bytecode_generator.rs).

    // BytecodeGenerator.h:1127
    pub fn parse_mode(&self) -> crate::parser::parser_modes::SourceParseMode {
        self.code_block.parse_mode()
    }

    // BytecodeGenerator.h:1129
    pub fn is_builtin_function(&self) -> bool {
        self.is_builtin_function
    }


    // BytecodeGenerator.h:1138
    pub fn is_builtin_default_class_constructor(&self) -> bool {
        self.is_builtin_default_class_constructor
    }

    // BytecodeGenerator.h:1140
    pub fn last_opcode_id(&self) -> crate::bytecode::opcode::OpcodeID {
        self.last_opcode_id
    }

    // BytecodeGenerator.h:1142
    pub fn is_derived_constructor_context(&self) -> bool {
        self.derived_context_type == crate::bytecode::executable_info::DerivedContextType::DerivedConstructorContext
    }

    // BytecodeGenerator.h:1143
    pub fn is_derived_class_context(&self) -> bool {
        self.derived_context_type == crate::bytecode::executable_info::DerivedContextType::DerivedMethodContext
    }

    // BytecodeGenerator.h:1144
    pub fn is_arrow_function(&self) -> bool {
        self.code_block.is_arrow_function()
    }

    // pub fn push_lexical_scope_internal(&mut self, env: &mut VariableEnvironment, tdz: TDZCheckOptimization, nested: NestedScopeType, constant_symbol_table_result: Option<&mut Option<RegisterRef>>, requirement: TDZRequirement, ty: ScopeType, register_type: ScopeRegisterType);  // .cpp
    // pub fn initialize_block_scoped_functions(&mut self, env: &mut VariableEnvironment, stack: &mut FunctionStack, constant_symbol_table: Option<RegisterRef>);  // .cpp
    // pub fn pop_lexical_scope_internal(&mut self, env: &mut VariableEnvironment);  // .cpp
    // pub fn instantiate_lexical_variables<F>(&mut self, env: &VariableEnvironment, ty: ScopeType, table: &mut SymbolTable, register_type: ScopeRegisterType, look_up_var_kind: F) -> bool;  // .cpp
    // pub fn emit_prefill_stack_tdz_variables(&mut self, env: &VariableEnvironment, table: &mut SymbolTable);  // .cpp
    // pub fn emit_get_parent_scope(&mut self, dst: Option<RegisterRef>, scope: &RegisterRef) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_push_function_name_scope(&mut self, property: &Identifier, value: &RegisterRef, is_captured: bool);  // .cpp
    // pub fn emit_new_function_expression_common(&mut self, dst: &RegisterRef, metadata: &FunctionMetadataNode);  // .cpp
    // pub fn is_new_target_used_in_inner_arrow_function(&mut self) -> bool;  // .cpp
    // pub fn is_arguments_used_in_inner_arrow_function(&mut self) -> bool;  // .cpp

    // BytecodeGenerator.h:1166
    pub fn emit_to_this_this_register(&mut self) {
        let this_register = self.this_register();
        self.emit_to_this(&this_register);
    }

    // pub fn emit_move(&mut self, dst: &RegisterRef, src: &RegisterRef) -> Option<RegisterRef>;  // .cpp

    // BytecodeGenerator.h:1172
    pub fn disable_peephole_optimization(&mut self) {
        self.last_opcode_id = crate::bytecode::opcode::OpcodeID::op_debug;
    }

    // BytecodeGenerator.h:1174
    pub fn can_do_peephole_optimization(&self) -> bool {
        self.last_opcode_id != crate::bytecode::opcode::OpcodeID::op_debug
    }

    // pub fn is_super_used_in_inner_arrow_function(&mut self) -> bool;  // .cpp
    // pub fn is_super_call_used_in_inner_arrow_function(&mut self) -> bool;  // .cpp
    // pub fn is_this_used_in_inner_arrow_function(&mut self) -> bool;  // .cpp
    // pub fn push_lexical_scope(&mut self, node: &VariableEnvironmentNode, ty: ScopeType, tdz: TDZCheckOptimization, nested: NestedScopeType /* padrão IsNotNested */, constant_symbol_table_result: Option<&mut Option<RegisterRef>> /* padrão None */, should_initialize_block_scoped_functions: bool /* padrão true */);  // .cpp
    // pub fn push_class_lexical_scope(&mut self, node: &VariableEnvironmentNode);  // .cpp
    // pub fn pop_lexical_scope(&mut self, node: &VariableEnvironmentNode);  // .cpp
    // pub fn prepare_lexical_scope_for_next_for_loop_iteration(&mut self, node: &VariableEnvironmentNode, loop_symbol_table: &RegisterRef);  // .cpp
    // pub fn label_scope_depth(&self) -> i32;  // .cpp
    // pub fn generate(&mut self, out: &mut u32) -> ParserError;  // .cpp
    // pub fn variable_for_local_entry(&mut self, ident: &Identifier, entry: &SymbolTableEntryFast, symbol_table_constant_index: i32, is_lexically_scoped: bool) -> Variable;  // .cpp

    // BytecodeGenerator.h:1197
    pub fn kill(
        &mut self,
        dst: &crate::bytecompiler::register_id::RegisterRef,
    ) -> crate::bytecompiler::register_id::RegisterRef {
        self.static_property_analyzer.kill_register(&dst.borrow());
        dst.clone()
    }

    // pub fn retrieve_last_unary_op(&mut self, dst_index: &mut i32, src_index: &mut i32);  // .cpp
    // pub fn rewind(&mut self);  // .cpp
    // pub fn allocate_scope(&mut self);  // .cpp
    // pub fn set_target_for_jump_instruction<JumpOp>(&mut self, instruction: &mut MutableRef, target: i32);  // .cpp
    // pub fn emit_expected_function_snippet(&mut self, dst: Option<RegisterRef>, func: &RegisterRef, expected: ExpectedFunction, args: &mut CallArguments, done: &Rc<Label>) -> ExpectedFunction;  // .cpp
    // pub fn compute_features_for_call_direct_eval(&mut self) -> LexicallyScopedFeatures;  // .cpp
    // pub fn emit_call<CallOp>(&mut self, dst: Option<RegisterRef>, func: &RegisterRef, expected: ExpectedFunction, args: &mut CallArguments, divot: &JSTextPosition, divot_start: &JSTextPosition, divot_end: &JSTextPosition, debuggable: DebuggableCall) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_call_iterator(&mut self, iterator: &RegisterRef, argument: &RegisterRef, node: &ThrowableExpressionData) -> Option<RegisterRef>;  // .cpp
    // pub fn initialize_next_parameter(&mut self) -> Option<RegisterRef>;  // .cpp
    // pub fn visible_name_for_parameter(&mut self, pattern: &DestructuringPatternNode) -> Option<Rc<UniquedStringImpl>>;  // .cpp

    // BytecodeGenerator.h: `RegisterID* calleeRegister() { return &m_calleeRegister; }`
    pub fn callee_register(&self) -> crate::bytecompiler::register_id::RegisterRef {
        crate::bytecompiler::register_id::RegisterRef::new(&std::rc::Rc::new(std::cell::RefCell::new(
            crate::bytecompiler::register_id::RegisterID::from_virtual_register(self.callee_register.virtual_register()),
        )))
    }

    // BytecodeGenerator.h:1209
    pub fn register_for(
        &self,
        reg: crate::bytecode::virtual_register::VirtualRegister,
    ) -> crate::bytecompiler::register_id::RegisterRef {
        if reg.is_local() {
            return crate::bytecompiler::register_id::RegisterRef::new(&self.callee_locals[reg.to_local() as usize]);
        }

        if reg.offset() == crate::interpreter::call_frame::CallFrameSlot::Callee as i32 {
            // `&m_calleeRegister`: o campo é `RegisterID` por valor; o handle novo compartilha só o
            // índice virtual (igual a `this_register()`).
            return self.callee_register();
        }

        debug_assert!(!self.parameters.is_empty());
        crate::bytecompiler::register_id::RegisterRef::new(&self.parameters[reg.to_argument() as usize])
    }

    // pub fn has_constant(&self, ident: &Identifier) -> bool;  // .cpp
    // pub fn add_constant_identifier(&mut self, ident: &Identifier) -> u32;  // .cpp
    // pub fn add_constant_value(&mut self, value: JSValue, representation: SourceCodeRepresentation /* padrão Other */) -> Option<RegisterRef>;  // .cpp
    // pub fn add_constant_empty_value(&mut self) -> Option<RegisterRef>;  // .cpp

    // BytecodeGenerator.h:1226
    pub fn make_function(
        &mut self,
        metadata: &crate::parser::nodes::FunctionMetadataNode,
    ) -> crate::runtime::unlinked_function_executable::UnlinkedFunctionExecutableRef {
        use crate::bytecode::executable_info::{DerivedContextType, EvalContextType, NeedsClassFieldInitializer};
        use crate::parser::parser_modes::SourceParseMode;
        use crate::runtime::construct_ability::ConstructAbility;
        use crate::runtime::constructor_kind::ConstructorKind;

        let mut new_derived_context_type = DerivedContextType::None;
        let mut new_eval_context_type = EvalContextType::FunctionEvalContext;

        let mut needs_class_field_initializer = if metadata.is_constructor_and_needs_class_field_initializer() {
            NeedsClassFieldInitializer::Yes
        } else {
            NeedsClassFieldInitializer::No
        };
        let mut private_brand_requirement = metadata.private_brand_requirement.get();
        let metadata_parse_mode = metadata.parse_mode;
        if matches!(
            metadata_parse_mode,
            SourceParseMode::ArrowFunctionMode
                | SourceParseMode::AsyncArrowFunctionMode
                | SourceParseMode::AsyncArrowFunctionBodyMode
        ) {
            if self.constructor_kind() == ConstructorKind::Extends || self.is_derived_constructor_context() {
                new_derived_context_type = DerivedContextType::DerivedConstructorContext;
                needs_class_field_initializer = self.code_block.needs_class_field_initializer();
                private_brand_requirement = self.code_block.private_brand_requirement();
            } else if self.code_block.is_class_context() || self.is_derived_class_context() {
                new_derived_context_type = DerivedContextType::DerivedMethodContext;
            }
            new_eval_context_type = self.code_block.eval_context_type();
        }

        let optional_variables_under_tdz = self.get_variables_under_tdz();
        let mut generator_or_async_wrapper_function_parameter_names = Vec::new();
        let parent_private_name_environment = self.get_available_private_access_names();

        // FIXME do upstream: estes flags, ParserModes e a propagação para os XXXCodeBlocks deviam ser reorganizados.
        // https://bugs.webkit.org/show_bug.cgi?id=151547
        let parse_mode = metadata.parse_mode;
        let mut construct_ability = crate::parser::parser_modes::construct_ability_for_parse_mode(parse_mode);
        if parse_mode == SourceParseMode::MethodMode && metadata.constructor_kind != ConstructorKind::None {
            construct_ability = ConstructAbility::CanConstruct;
        }

        if crate::parser::parser_modes::is_generator_or_async_function_wrapper_parse_mode(self.code_block.parse_mode())
            && crate::parser::parser_modes::is_generator_or_async_function_body_parse_mode(parse_mode)
        {
            generator_or_async_wrapper_function_parameter_names = self.get_parameter_names();
        }

        crate::runtime::unlinked_function_executable::UnlinkedFunctionExecutable::create(
            &self.vm,
            &self.scope_node.borrow().source,
            metadata,
            if self.is_builtin_function() {
                crate::runtime::unlinked_function_executable::UnlinkedFunctionKind::UnlinkedBuiltinFunction
            } else {
                crate::runtime::unlinked_function_executable::UnlinkedFunctionKind::UnlinkedNormalFunction
            },
            construct_ability,
            crate::runtime::inline_attribute::InlineAttribute::None,
            self.script_mode(),
            optional_variables_under_tdz,
            generator_or_async_wrapper_function_parameter_names,
            parent_private_name_environment,
            new_derived_context_type,
            new_eval_context_type,
            needs_class_field_initializer,
            private_brand_requirement,
        )
    }

    // pub fn get_variables_under_tdz(&mut self) -> Option<Rc<TDZEnvironmentLink>>;  // .cpp
    // pub fn get_parameter_names(&self) -> Vec<Identifier>;  // .cpp
    // pub fn get_available_private_access_names(&mut self) -> Option<PrivateNameEnvironment>;  // .cpp
    // pub fn emit_construct_varargs(&mut self, dst: Option<RegisterRef>, func: &RegisterRef, this_register: Option<RegisterRef>, arguments: &RegisterRef, first_free_register: &RegisterRef, first_var_arg_offset: i32, divot: &JSTextPosition, divot_start: &JSTextPosition, divot_end: &JSTextPosition, debuggable: DebuggableCall) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_super_construct_varargs(&mut self, /* mesmos parâmetros de emit_construct_varargs */) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_call_varargs<CallOp>(&mut self, /* mesmos parâmetros de emit_construct_varargs */) -> Option<RegisterRef>;  // .cpp
    // pub fn emit_log_shadow_chicken_prologue_if_necessary(&mut self);  // .cpp
    // pub fn emit_log_shadow_chicken_tail_if_necessary(&mut self);  // .cpp
    // pub fn initialize_parameters(&mut self, parameters: &mut FunctionParameters);  // .cpp
    // pub fn initialize_var_lexical_environment(&mut self, symbol_table_constant_index: i32, function_symbol_table: &mut SymbolTable, has_captured_variables: bool);  // .cpp
    // pub fn initialize_default_parameter_values_and_setup_function_scope_stack(&mut self, parameters: &mut FunctionParameters, is_simple_parameter_list: bool, function_node: &FunctionNode, table: &mut SymbolTable, symbol_table_constant_index: i32, captures: &dyn Fn(&UniquedStringImpl) -> bool, should_create_arguments_variable_in_parameter_scope: bool);  // .cpp
    // pub fn initialize_arrow_function_context_scope_if_needed(&mut self, function_symbol_table: Option<&mut SymbolTable>, can_reuse_lexical_environment: bool /* padrão false */);  // .cpp
    // pub fn needs_derived_constructor_in_arrow_function_lexical_environment(&mut self) -> bool;  // .cpp
    // pub fn add_string_constant(&mut self, ident: &Identifier) -> JSStringRef;  // .cpp
    // pub fn add_big_int_constant(&mut self, ident: &Identifier, radix: u8, sign: bool) -> JSValue;  // .cpp
    // pub fn add_template_object_constant(&mut self, descriptor: Rc<TemplateObjectDescriptor>, index: i32) -> Option<RegisterRef>;  // .cpp

    // BytecodeGenerator.h:1290
    pub fn instructions(&self) -> &crate::bytecode::instruction_stream::JSInstructionStreamWriter {
        &self.writer
    }

    // pub fn emit_throw_expression_too_deep_exception(&mut self) -> Option<RegisterRef>;  // .cpp
    // pub fn preserve_tdz_stack(&mut self, preserved: &mut PreservedTDZStack);  // .cpp
    // pub fn restore_tdz_stack(&mut self, preserved: &PreservedTDZStack);  // .cpp

    // BytecodeGenerator.h:1304
    pub fn with_writer<F: FnOnce(&mut BytecodeGenerator)>(
        &mut self,
        writer: &mut crate::bytecode::instruction_stream::JSInstructionStreamWriter,
        func: F,
    ) {
        let prev_last_opcode_id = self.last_opcode_id;
        let prev_last_instruction = self.last_instruction.clone();
        self.writer.swap(writer);
        self.disable_peephole_optimization();
        self.last_instruction = self.writer.ref_();
        func(self);
        self.writer.swap(writer);
        self.last_opcode_id = prev_last_opcode_id;
        self.last_instruction = prev_last_instruction;
    }

    // pub fn get_private_traits(&mut self, ident: &Identifier) -> PrivateNameEntry;  // .cpp
    // pub fn push_private_access_names(&mut self, env: Option<&PrivateNameEnvironment>);  // .cpp
    // pub fn pop_private_access_names(&mut self);  // .cpp

    // BytecodeGenerator.h:1323
    pub fn needs_arguments(&self) -> bool {
        self.needs_arguments
    }

    // BytecodeGenerator.h:1324
    pub fn should_get_arguments_dot_length_fast(&self, node: &crate::parser::nodes::Expression) -> bool {
        use crate::parser::parser_modes::{
            is_arrow_function_parse_mode, is_generator_or_async_function_body_parse_mode,
        };
        self.is_function_node()
            && !self.needs_arguments()
            && !self.has_shadows_arguments_code_feature()
            && node.is_arguments_length_access(self.vm())
            && !is_arrow_function_parse_mode(self.parse_mode())
            && !is_generator_or_async_function_body_parse_mode(self.parse_mode())
    }

    // BytecodeGenerator.h:1333
    pub fn local_scope_count(&self) -> u32 {
        self.local_scope_count
    }

    // BytecodeGenerator.h:1384 (declarações)
    // pub fn local_scope_depth(&self) -> u32;  // .cpp
    // pub fn push_local_control_flow_scope(&mut self);  // .cpp
    // pub fn pop_local_control_flow_scope(&mut self);  // .cpp
    // pub fn push_tdz_variables(&mut self, env: &VariableEnvironment, tdz: TDZCheckOptimization, requirement: TDZRequirement);  // .cpp
    // pub fn async_func_parameters_try_catch_wrap<F>(&mut self, emit_bytecode: F);  // .cpp (template)
}
