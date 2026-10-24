//! Tradução de `runtime/FunctionExecutable.h`, `FunctionExecutable.cpp` e `FunctionExecutableInlines.h`.
//!
//! Vale `USE(BUN_JSC_ADDITIONS)` ligado (`toStringSlow` testa `isPrivateBuiltinFunction()`).
//!
//! DIVERGÊNCIAS (ver `executable.rs` e `script_executable.rs`):
//!
//! - `m_topLevelExecutable` é `WriteBarrier<ScriptExecutable>` e o construtor grava `this` quando o
//!   argumento é nulo. Um `Rc` para si mesmo seria um ciclo, então o campo é
//!   `Option<ScriptExecutableRef>`: `None` é "este próprio executável", e o
//!   `ScriptExecutableRef::top_level_executable` devolve o `Rc` que o chamador já tem.
//! - `m_singleton` (`InferredValue<JSFunction>`), `m_polyProtoWatchpoint` (`Box<InlineWatchpointSet>`,
//!   compartilhado por contagem de referência) e `m_cachedPolyProtoStructureID` mantêm os tipos do
//!   C++ nos caminhos `crate::runtime::inferred_value`, `crate::bytecode::watchpoint` e
//!   `crate::runtime::structure`. O dono do `WriteBarrier` (o `this` de `set(vm, this, ...)`) some.
//! - `FunctionCodeBlock*` é o `CodeBlockRef` comum (o `bit_cast` não muda a identidade).
//! - Sem GC: `visitChildren`, `visitOutputConstraints`, `reconcileWeakReferencesAtGCEnd`,
//!   `shouldKeepInConstraintSet` e o `outputConstraintsSet` do heap não existem; `subspaceFor`,
//!   `createStructure`, `destroy`, `offsetOf...` e `DECLARE_INFO` também não.
//! - `fromGlobalCode` devolve `None` com `exception` preenchida, como o `nullptr` + `JSObject*&`.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use crate::bytecode::code_block::CodeBlockRef;
use crate::bytecode::executable_info::DerivedContextType;
use crate::bytecode::unlinked_function_executable::{UnlinkedFunctionExecutable, UnlinkedFunctionExecutableRef};
use crate::bytecode::watchpoint::{InlineWatchpointSet, WatchpointState};
use crate::parser::parser_modes::{
    is_async_generator_parse_mode, is_generator_parse_mode, CodeFeatures, FunctionConstructionMode, FunctionMode,
    JSParserScriptMode, LexicallyScopedFeatures, SourceParseMode, SourceParseModeSet,
};
use crate::parser::source_code::{make_source, SourceCode};
use crate::parser::source_provider::SourceProviderSourceType;
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::construct_ability::ConstructAbility;
use crate::runtime::function_overrides::FunctionOverrideInfo;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::inferred_value::InferredValue;
use crate::runtime::inline_attribute::InlineAttribute;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{JSFunction, JSFunctionRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectHandle;
use crate::runtime::js_string::{js_string, JSStringRef};
use crate::runtime::js_string_builder::js_make_nontrivial_string;
use crate::runtime::js_type::JSType;
use crate::runtime::script_executable::{ScriptExecutable, ScriptExecutableRef, TemplateObjectMap};
use crate::runtime::source_origin::SourceOrigin;
use crate::runtime::structure::StructureRef;
use crate::runtime::throw_scope::ThrowScope;
use crate::runtime::type_set::TypeSet;
use crate::runtime::vm::VM;
use crate::wtf::text::text_position::TextPosition;
use crate::wtf::text::wtf_string::String as WtfString;

/// `FunctionExecutable::overrideLineNumberNotFound`.
pub const OVERRIDE_LINE_NUMBER_NOT_FOUND: i32 = -1;

/// `FunctionExecutable::RareData`.
pub struct RareData {
    pub return_statement_type_set: Option<Rc<TypeSet>>,
    pub line_count: u32,
    pub end_column: u32,
    pub override_line_number: Option<i32>,
    pub parameters_start_offset: u32,
    pub cached_poly_proto_structure_id: Option<StructureRef>,
    pub template_object_map: Option<Box<TemplateObjectMap>>,
    pub as_string: Option<JSStringRef>,
    pub function_start: u32,
    pub function_end: u32,
}

impl Default for RareData {
    /// Os inicializadores dos membros do C++ (`m_parametersStartOffset { 0 }`, `m_functionStart { UINT_MAX }`,
    /// `m_functionEnd { UINT_MAX }`); `m_lineCount` e `m_endColumn` ficam sem valor no C++ e o
    /// `ensureRareDataSlow` os atribui logo depois.
    fn default() -> RareData {
        RareData {
            return_statement_type_set: None,
            line_count: 0,
            end_column: 0,
            override_line_number: None,
            parameters_start_offset: 0,
            cached_poly_proto_structure_id: None,
            template_object_map: None,
            as_string: None,
            function_start: u32::MAX,
            function_end: u32::MAX,
        }
    }
}

/// `class FunctionExecutable`.
pub struct FunctionExecutable {
    base: ScriptExecutable,
    rare_data: Option<Box<RareData>>,
    /// `None` é `m_topLevelExecutable == this` (ver o topo do módulo).
    top_level_executable: Option<ScriptExecutableRef>,
    unlinked_executable: UnlinkedFunctionExecutableRef,
    code_block_for_call: Option<CodeBlockRef>,
    code_block_for_construct: Option<CodeBlockRef>,
    singleton: InferredValue<JSFunction>,
    poly_proto_watchpoint: Option<Rc<RefCell<InlineWatchpointSet>>>,
}

crate::parser::nodes::inherit!(FunctionExecutable => ScriptExecutable);

impl FunctionExecutable {
    /// `FunctionExecutable(VM&, ScriptExecutable* topLevelExecutable, const SourceCode&, UnlinkedFunctionExecutable*,
    /// Intrinsic, bool isInsideOrdinaryFunction)` (privado) mais `create`/`finishCreation`.
    /// `top_level_executable` nulo (`nullptr`) é `None`, que o campo lê como "este executável".
    pub fn create(
        _vm: &VM,
        top_level_executable: Option<ScriptExecutableRef>,
        source: &SourceCode,
        unlinked_executable: &UnlinkedFunctionExecutableRef,
        intrinsic: Intrinsic,
        is_inside_ordinary_function: bool,
    ) -> Rc<RefCell<FunctionExecutable>> {
        let (lexically_scoped_features, derived_context_type, is_arrow_function) = {
            let unlinked = unlinked_executable.borrow();
            (unlinked.lexically_scoped_features(), unlinked.derived_context_type(), unlinked.is_arrow_function())
        };
        let base = ScriptExecutable::new(
            JSType::FunctionExecutableType,
            source,
            lexically_scoped_features,
            derived_context_type,
            false,
            is_inside_ordinary_function || !is_arrow_function,
            crate::bytecode::executable_info::EvalContextType::None,
            intrinsic,
        );
        assert!(!source.is_null());
        debug_assert!(source.length() != 0);
        Rc::new(RefCell::new(FunctionExecutable {
            base,
            rare_data: None,
            top_level_executable,
            unlinked_executable: Rc::clone(unlinked_executable),
            code_block_for_call: None,
            code_block_for_construct: None,
            singleton: InferredValue::default(),
            poly_proto_watchpoint: None,
        }))
    }

    /// `static fromGlobalCode(const Identifier& name, JSGlobalObject*, String&& program, const SourceOrigin&,
    /// SourceTaintedOrigin, const String& sourceURL, const TextPosition&, LexicallyScopedFeatures, JSObject*& exception,
    /// int overrideLineNumber, std::optional<int> functionConstructorParametersEndPosition, FunctionConstructionMode)`.
    #[allow(clippy::too_many_arguments)]
    pub fn from_global_code(
        name: &Identifier,
        global_object: &JSGlobalObject,
        program: WtfString,
        source_origin: &SourceOrigin,
        tainted_origin: SourceTaintedOrigin,
        source_url: &WtfString,
        position: &TextPosition,
        lexically_scoped_features: LexicallyScopedFeatures,
        exception: &mut Option<JSObjectHandle>,
        override_line_number: i32,
        function_constructor_parameters_end_position: Option<i32>,
        function_construction_mode: FunctionConstructionMode,
    ) -> Option<Rc<RefCell<FunctionExecutable>>> {
        if override_line_number == OVERRIDE_LINE_NUMBER_NOT_FOUND {
            if let Some(executable) = global_object.try_get_cached_function_executable_for_function_constructor(
                name,
                &program,
                source_origin,
                tainted_origin,
                source_url,
                position,
                lexically_scoped_features,
                function_construction_mode,
            ) {
                return Some(executable);
            }
        }

        let source = make_source(
            &program,
            source_origin,
            tainted_origin,
            source_url.clone(),
            *position,
            SourceProviderSourceType::Program,
        );
        let unlinked_executable = UnlinkedFunctionExecutable::from_global_code(
            name,
            global_object,
            &source,
            lexically_scoped_features,
            exception,
            override_line_number,
            function_constructor_parameters_end_position,
        )?;

        let executable = UnlinkedFunctionExecutable::link(
            &unlinked_executable,
            &global_object.vm(),
            None,
            &source,
            Some(override_line_number),
            Intrinsic::NoIntrinsic,
            false,
        );
        if override_line_number == OVERRIDE_LINE_NUMBER_NOT_FOUND {
            global_object.cached_function_executable_for_function_constructor(&executable);
        }
        Some(executable)
    }

    /// `unlinkedExecutable()`.
    pub fn unlinked_executable(&self) -> &UnlinkedFunctionExecutableRef {
        &self.unlinked_executable
    }

    /// `eitherCodeBlock()`: returns either call or construct bytecode. This can be appropriate for
    /// answering questions that that don't vary between call and construct -- for example,
    /// argumentsRegister().
    pub fn either_code_block(&self) -> Option<CodeBlockRef> {
        if let Some(result) = self.code_block_for_call() {
            return Some(result);
        }
        self.code_block_for_construct()
    }

    /// `isGeneratedForCall()`.
    pub fn is_generated_for_call(&self) -> bool {
        self.code_block_for_call().is_some()
    }

    /// `codeBlockForCall()`.
    pub fn code_block_for_call(&self) -> Option<CodeBlockRef> {
        self.code_block_for_call.clone()
    }

    /// `isGeneratedForConstruct()`.
    pub fn is_generated_for_construct(&self) -> bool {
        self.code_block_for_construct().is_some()
    }

    /// `codeBlockForConstruct()`.
    pub fn code_block_for_construct(&self) -> Option<CodeBlockRef> {
        self.code_block_for_construct.clone()
    }

    /// `isGeneratedFor(CodeSpecializationKind)`.
    pub fn is_generated_for(&self, kind: CodeSpecializationKind) -> bool {
        match kind {
            CodeSpecializationKind::CodeForCall => self.is_generated_for_call(),
            CodeSpecializationKind::CodeForConstruct => self.is_generated_for_construct(),
        }
    }

    /// `codeBlockFor(CodeSpecializationKind)`.
    pub fn code_block_for(&self, kind: CodeSpecializationKind) -> Option<CodeBlockRef> {
        match kind {
            CodeSpecializationKind::CodeForCall => self.code_block_for_call(),
            CodeSpecializationKind::CodeForConstruct => self.code_block_for_construct(),
        }
    }

    /// `replaceCodeBlockWith(VM&, CodeSpecializationKind, CodeBlock*)` (FunctionExecutableInlines.h).
    pub fn replace_code_block_with(
        &mut self,
        _vm: &VM,
        kind: CodeSpecializationKind,
        new_code_block: Option<CodeBlockRef>,
    ) -> Option<CodeBlockRef> {
        match kind {
            CodeSpecializationKind::CodeForCall => {
                let old_code_block = self.code_block_for_call();
                self.code_block_for_call = new_code_block;
                old_code_block
            }
            CodeSpecializationKind::CodeForConstruct => {
                let old_code_block = self.code_block_for_construct();
                self.code_block_for_construct = new_code_block;
                old_code_block
            }
        }
    }

    /// `m_codeBlockForCall.clear()`.
    pub fn clear_code_block_for_call(&mut self) {
        self.code_block_for_call = None;
    }

    /// `m_codeBlockForConstruct.clear()`.
    pub fn clear_code_block_for_construct(&mut self) {
        self.code_block_for_construct = None;
    }

    /// `returnStatementTypeSet()`.
    pub fn return_statement_type_set(&mut self) -> Rc<TypeSet> {
        let rare_data = self.ensure_rare_data();
        Rc::clone(rare_data.return_statement_type_set.get_or_insert_with(TypeSet::create))
    }

    pub fn function_mode(&self) -> FunctionMode {
        self.unlinked_executable.borrow().function_mode()
    }

    pub fn implementation_visibility(&self) -> ImplementationVisibility {
        self.unlinked_executable.borrow().implementation_visibility()
    }

    pub fn is_builtin_function(&self) -> bool {
        self.unlinked_executable.borrow().is_builtin_function()
    }

    pub fn is_private_builtin_function(&self) -> bool {
        let hidden = self.source.provider().is_some_and(|provider| provider.is_hidden());
        hidden
            || self.is_builtin_function()
                && match self.source.provider() {
                    None => true,
                    Some(provider) => provider.source_url().is_null(),
                }
    }

    pub fn construct_ability(&self) -> ConstructAbility {
        self.unlinked_executable.borrow().construct_ability()
    }

    pub fn inline_attribute(&self) -> InlineAttribute {
        self.unlinked_executable.borrow().inline_attribute()
    }

    pub fn is_class(&self) -> bool {
        self.unlinked_executable.borrow().is_class()
    }

    pub fn is_arrow_function(&self) -> bool {
        self.parse_mode() == SourceParseMode::ArrowFunctionMode
    }

    pub fn is_getter(&self) -> bool {
        self.parse_mode() == SourceParseMode::GetterMode
    }

    pub fn is_setter(&self) -> bool {
        self.parse_mode() == SourceParseMode::SetterMode
    }

    pub fn is_generator(&self) -> bool {
        is_generator_parse_mode(self.parse_mode())
    }

    pub fn is_async_generator(&self) -> bool {
        is_async_generator_parse_mode(self.parse_mode())
    }

    pub fn is_method(&self) -> bool {
        self.parse_mode() == SourceParseMode::MethodMode
    }

    pub fn has_prototype_property(&self) -> bool {
        SourceParseModeSet::new(&[
            SourceParseMode::NormalFunctionMode,
            SourceParseMode::GeneratorBodyMode,
            SourceParseMode::GeneratorWrapperFunctionMode,
            SourceParseMode::GeneratorWrapperMethodMode,
            SourceParseMode::AsyncGeneratorWrapperFunctionMode,
            SourceParseMode::AsyncGeneratorWrapperMethodMode,
            SourceParseMode::AsyncGeneratorBodyMode,
        ])
        .contains(self.parse_mode())
            || self.is_class()
    }

    pub fn derived_context_type(&self) -> DerivedContextType {
        self.unlinked_executable.borrow().derived_context_type()
    }

    pub fn is_class_constructor_function(&self) -> bool {
        self.unlinked_executable.borrow().is_class_constructor_function()
    }

    /// `m_unlinkedExecutable->isBuiltinDefaultClassConstructor()`: o construtor de classe sem `constructor` escrito.
    pub fn is_builtin_default_class_constructor(&self) -> bool {
        self.unlinked_executable.borrow().is_builtin_default_class_constructor()
    }

    pub fn name(&self) -> Identifier {
        self.unlinked_executable.borrow().name()
    }

    pub fn ecma_name(&self) -> Identifier {
        self.unlinked_executable.borrow().ecma_name().clone()
    }

    /// `parameterCount()`: excluding 'this'!
    pub fn parameter_count(&self) -> u32 {
        self.unlinked_executable.borrow().parameter_count()
    }

    pub fn parse_mode(&self) -> SourceParseMode {
        self.unlinked_executable.borrow().parse_mode()
    }

    pub fn script_mode(&self) -> JSParserScriptMode {
        self.unlinked_executable.borrow().script_mode()
    }

    pub fn class_source(&self) -> SourceCode {
        self.unlinked_executable.borrow().class_source()
    }

    /// `setOverrideLineNumber(int)`.
    pub fn set_override_line_number(&mut self, override_line_number: i32) {
        if override_line_number == OVERRIDE_LINE_NUMBER_NOT_FOUND {
            if let Some(rare_data) = self.rare_data.as_mut() {
                rare_data.override_line_number = None;
            }
            return;
        }
        self.ensure_rare_data().override_line_number = Some(override_line_number);
    }

    /// `overrideLineNumber()`.
    pub fn override_line_number(&self) -> Option<i32> {
        if let Some(rare_data) = &self.rare_data {
            return rare_data.override_line_number;
        }
        None
    }

    /// `lineCount()`.
    pub fn line_count(&self) -> i32 {
        if let Some(rare_data) = &self.rare_data {
            return rare_data.line_count as i32;
        }
        self.unlinked_executable.borrow().line_count() as i32
    }

    /// `endColumn()`.
    pub fn end_column(&self) -> i32 {
        if let Some(rare_data) = &self.rare_data {
            return rare_data.end_column as i32;
        }
        self.unlinked_executable.borrow().linked_end_column(self.source.start_column().one_based_int() as u32) as i32
    }

    /// `firstLine()`.
    pub fn first_line(&self) -> i32 {
        self.source().first_line().one_based_int()
    }

    /// `lastLine()`.
    pub fn last_line(&self) -> i32 {
        self.first_line() + self.line_count()
    }

    /// `functionEnd()`.
    pub fn function_end(&self) -> u32 {
        if let Some(rare_data) = &self.rare_data {
            return rare_data.function_end;
        }
        self.unlinked_executable.borrow().unlinked_function_end()
    }

    /// `functionStart()`.
    pub fn function_start(&self) -> u32 {
        if let Some(rare_data) = &self.rare_data {
            return rare_data.function_start;
        }
        self.unlinked_executable.borrow().unlinked_function_start()
    }

    /// `parametersStartOffset()`.
    pub fn parameters_start_offset(&self) -> u32 {
        if let Some(rare_data) = &self.rare_data {
            return rare_data.parameters_start_offset;
        }
        self.unlinked_executable.borrow().parameters_start_offset()
    }

    /// `recordParse(CodeFeatures, LexicallyScopedFeatures, bool)` (herdado, protegido na base).
    pub fn record_parse(
        &mut self,
        features: CodeFeatures,
        lexically_scoped_features: LexicallyScopedFeatures,
        has_captured_variables: bool,
    ) {
        self.base.record_parse(features, lexically_scoped_features, has_captured_variables);
    }

    /// `overrideInfo(const FunctionOverrideInfo&)`.
    pub fn override_info(&mut self, override_info: &FunctionOverrideInfo) {
        let rare_data = self.ensure_rare_data();
        rare_data.line_count = override_info.line_count;
        rare_data.end_column = override_info.end_column;
        rare_data.parameters_start_offset = override_info.parameters_start_offset;
        rare_data.function_start = override_info.function_start;
        rare_data.function_end = override_info.function_end;
        self.base.source = override_info.source_code.clone();
    }

    /// `singleton()`.
    pub fn singleton(&mut self) -> &mut InferredValue<JSFunction> {
        &mut self.singleton
    }

    /// `notifyCreation(VM&, JSFunction*, const char* reason)` (FunctionExecutableInlines.h).
    pub fn notify_creation(&mut self, vm: &VM, function: &JSFunctionRef, reason: &str) {
        self.singleton.notify_write_with_reason(vm, function, reason);
        if self.singleton.has_been_invalidated() {
            self.unlinked_executable.borrow_mut().set_singleton_has_been_invalidated();
        }
    }

    /// `cachedPolyProtoStructure()`: cached poly proto structure for the result of constructing this executable.
    pub fn cached_poly_proto_structure(&self) -> Option<StructureRef> {
        if let Some(rare_data) = &self.rare_data {
            return rare_data.cached_poly_proto_structure_id.clone();
        }
        None
    }

    /// `setCachedPolyProtoStructure(VM&, Structure*)`.
    pub fn set_cached_poly_proto_structure(&mut self, _vm: &VM, structure: StructureRef) {
        self.ensure_rare_data().cached_poly_proto_structure_id = Some(structure);
    }

    /// `ensurePolyProtoWatchpoint()`.
    pub fn ensure_poly_proto_watchpoint(&mut self) -> Rc<RefCell<InlineWatchpointSet>> {
        Rc::clone(
            self.poly_proto_watchpoint
                .get_or_insert_with(|| Rc::new(RefCell::new(InlineWatchpointSet::new(WatchpointState::IsWatched)))),
        )
    }

    /// `sharedPolyProtoWatchpoint()`.
    pub fn shared_poly_proto_watchpoint(&self) -> Option<Rc<RefCell<InlineWatchpointSet>>> {
        self.poly_proto_watchpoint.clone()
    }

    /// `topLevelExecutable()`: `None` é este próprio executável (ver o topo do módulo).
    pub fn top_level_executable(&self) -> Option<ScriptExecutableRef> {
        self.top_level_executable.clone()
    }

    /// `ensureTemplateObjectMap(VM&)`.
    pub fn ensure_template_object_map(&mut self, _vm: &VM) -> &mut TemplateObjectMap {
        let rare_data = self.ensure_rare_data();
        ScriptExecutable::ensure_template_object_map_impl(&mut rare_data.template_object_map)
    }

    /// `asStringConcurrently()`.
    pub fn as_string_concurrently(&self) -> Option<JSStringRef> {
        match &self.rare_data {
            None => None,
            Some(rare_data) => rare_data.as_string.clone(),
        }
    }

    /// `ensureRareData()` e `ensureRareDataSlow()`.
    fn ensure_rare_data(&mut self) -> &mut RareData {
        if self.rare_data.is_none() {
            let rare_data = RareData {
                line_count: self.line_count() as u32,
                end_column: self.end_column() as u32,
                parameters_start_offset: self.parameters_start_offset(),
                function_start: self.function_start(),
                function_end: self.function_end(),
                ..RareData::default()
            };
            self.rare_data = Some(Box::new(rare_data));
        }
        self.rare_data.as_mut().expect("m_rareData")
    }

    /// Fixa o texto que `toString` devolve (o `m_asString` que `toStringSlow` calcularia do fonte). O bun mostra o fonte
    /// empacotado dos módulos embutidos; ver `api::builtin_source_texts`.
    pub fn override_as_string(this: &Rc<RefCell<FunctionExecutable>>, text: JSStringRef) {
        this.borrow_mut().ensure_rare_data().as_string = Some(text);
    }

    /// `toString(JSGlobalObject*)` (FunctionExecutableInlines.h). `None` é o `nullptr` com exceção pendente.
    pub fn to_string(this: &Rc<RefCell<FunctionExecutable>>, global_object: &JSGlobalObject) -> Option<JSStringRef> {
        let as_string = this.borrow_mut().ensure_rare_data().as_string.clone();
        match as_string {
            None => FunctionExecutable::to_string_slow(this, global_object),
            Some(as_string) => Some(as_string),
        }
    }

    /// `toStringSlow(JSGlobalObject*)` (privado).
    fn to_string_slow(this: &Rc<RefCell<FunctionExecutable>>, global_object: &JSGlobalObject) -> Option<JSStringRef> {
        let vm = global_object.vm();
        debug_assert!(this.borrow().rare_data.as_ref().is_some_and(|rare_data| rare_data.as_string.is_none()));

        let throw_scope = ThrowScope::new(&vm);

        let cache = |as_string: JSStringRef| -> Option<JSStringRef> {
            this.borrow_mut().ensure_rare_data().as_string = Some(Rc::clone(&as_string));
            Some(as_string)
        };

        if this.borrow().is_private_builtin_function() {
            // O método e o acessor escondidos não têm `has_name`: o nome deles é o `ecmaName` (a chave da propriedade).
            let name = if this.borrow().source.provider().is_some_and(|provider| provider.is_hidden()) {
                this.borrow().ecma_name()
            } else {
                this.borrow().name()
            };
            // O acessor nativo do navegador mostra o prefixo (`function get x() { [native code] }`); as builtins do
            // próprio JSC não.
            let hidden = this.borrow().source.provider().is_some_and(|provider| provider.is_hidden());
            let prefix = match this.borrow().parse_mode() {
                SourceParseMode::GetterMode if hidden => "get ",
                SourceParseMode::SetterMode if hidden => "set ",
                _ => "",
            };
            let value = js_make_nontrivial_string(
                global_object,
                &[&"function ", &prefix, &name.string().string(), &"() { [native code] }"],
            );
            if throw_scope.exception().is_some() {
                return None;
            }
            return cache(value.expect("jsMakeNontrivialString sem exceção devolveu nulo"));
        }

        if this.borrow().is_class() {
            // O empréstimo imutável não pode viver até dentro de `cache` (que faz `borrow_mut`).
            let class_source = js_string(&vm, &this.borrow().class_source().view());
            return cache(class_source);
        }

        let src = {
            let executable = this.borrow();
            let start = executable.function_start() as i32;
            let end = (executable.parameters_start_offset() as i32).wrapping_add(executable.source().length());
            executable.source().provider().expect("FunctionExecutable sem SourceProvider").get_range(start, end)
        };

        let value = js_make_nontrivial_string(global_object, &[&src]);
        if throw_scope.exception().is_some() {
            return None;
        }
        cache(value.expect("jsMakeNontrivialString sem exceção devolveu nulo"))
    }
}
