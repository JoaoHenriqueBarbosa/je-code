//! Tradução de `runtime/ProgramExecutable.h`, `ProgramExecutable.cpp` e `ProgramExecutableInlines.h`.
//!
//! Vale `USE(BUN_JSC_ADDITIONS)` ligado (`initializeGlobalProperties` com o bloco `precompiled`) e
//! `PLATFORM(COCOA)` desligado (`requiresCanDeclareGlobalFunctionQuirk()` é sempre falso).
//!
//! DIVERGÊNCIAS (ver `executable.rs`, `script_executable.rs` e `global_executable.rs`):
//!
//! - `initializeGlobalProperties` recebe o `Rc<RefCell<ProgramExecutable>>` (o `this` do C++) porque o
//!   cache de código chama `recordParse` no mesmo executável e nenhum empréstimo pode estar vivo nessa
//!   hora. O `ParserError`/`JSObject*` devolvido é `Option<JSObjectHandle>` (`nullptr` é `None`).
//! - Os loops `for (auto& entry : environment)` percorrem pares `(chave, entrada)` do
//!   `VariableEnvironment`; `entry.key.get()` é a `UniquedKey` e `entry.value` a
//!   `VariableEnvironmentEntry`.
//! - O `ConcurrentJSLocker locker(symbolTable->m_lock)` some (uma thread). `DeferTermination` e
//!   `BatchedTransitionOptimizer` são guardas RAII com `Drop`.
//! - `#if ENABLE(DFG_JIT)` vale o ramo `#else` (JIT fora do porte), então o disparo do
//!   `WatchpointSet` de `getReferencedPropertyWatchpointSet` não existe.
//! - `createGlobalVarBinding<BindingCreationContext::Global>` é o método com o contexto como argumento.
//! - `subspaceFor`, `createStructure`, `visitChildren`, `destroy` e `DECLARE_INFO` são maquinaria de heap.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use crate::bytecode::executable_info::{DerivedContextType, EvalContextType};
use crate::bytecode::unlinked_code_block::UnlinkedProgramCodeBlock;
use crate::parser::parser_error::ParserError;
use crate::parser::parser_modes::NO_LEXICALLY_SCOPED_FEATURES;
use crate::parser::source_code::SourceCode;
use crate::parser::source_provider::SourceProviderSourceType;
use crate::runtime::batched_transition_optimizer::BatchedTransitionOptimizer;
use crate::runtime::code_cache::record_parse_from_unlinked_code_block;
use crate::runtime::exception_helpers::{
    create_error_for_duplicate_global_variable_declaration, create_error_for_invalid_global_function_declaration,
    create_error_for_invalid_global_var_declaration,
};
use crate::runtime::global_executable::GlobalExecutable;
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::{BindingCreationContext, JSGlobalObject};
use crate::runtime::js_object::{JSObject, JSObjectHandle};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::js_tdz_value;
use crate::runtime::property_attribute::PropertyAttribute;
use crate::runtime::property_name::PropertyName;
use crate::runtime::script_executable::{ScriptExecutable, ScriptExecutableRef, TemplateObjectMap};
use crate::runtime::symbol_table::SymbolTableEntry;
use crate::runtime::throw_scope::ThrowScope;
use crate::runtime::var_offset::VarOffset;
use crate::runtime::vm::{DeferTermination, VM};
use crate::wtf::text::string_concatenate::make_string_dyn;
use crate::wtf::text::wtf_string::String as WtfString;

/// `class ProgramExecutable`.
pub struct ProgramExecutable {
    base: GlobalExecutable<UnlinkedProgramCodeBlock>,
    template_object_map: Option<Box<TemplateObjectMap>>,
}

crate::parser::nodes::inherit!(ProgramExecutable => GlobalExecutable<UnlinkedProgramCodeBlock>);

/// `enum class GlobalPropertyLookUpStatus` (http://www.ecma-international.org/ecma-262/6.0/index.html#sec-hasrestrictedglobalproperty).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlobalPropertyLookUpStatus {
    NotFound,
    Configurable,
    NonConfigurable,
}

/// `static GlobalPropertyLookUpStatus hasRestrictedGlobalProperty(JSGlobalObject*, PropertyName)`.
fn has_restricted_global_property(global_object: &JSGlobalObject, property_name: &PropertyName) -> GlobalPropertyLookUpStatus {
    let mut descriptor = crate::runtime::property_descriptor::PropertyDescriptor::default();
    if !global_object.get_own_property_descriptor(global_object, property_name, &mut descriptor) {
        return GlobalPropertyLookUpStatus::NotFound;
    }
    if descriptor.configurable() {
        return GlobalPropertyLookUpStatus::Configurable;
    }
    GlobalPropertyLookUpStatus::NonConfigurable
}

impl ProgramExecutable {
    /// `ProgramExecutable(JSGlobalObject*, const SourceCode&)` (privado).
    fn new(vm: &VM, source: &SourceCode) -> ProgramExecutable {
        let base = ScriptExecutable::new(
            JSType::ProgramExecutableType,
            source,
            NO_LEXICALLY_SCOPED_FEATURES,
            DerivedContextType::None,
            false,
            false,
            EvalContextType::None,
            Intrinsic::NoIntrinsic,
        );
        debug_assert!(
            source.provider().expect("ProgramExecutable sem SourceProvider").source_type() == SourceProviderSourceType::Program
        );
        let executable = ProgramExecutable { base: GlobalExecutable::new(base), template_object_map: None };
        if vm.type_profiler().is_some() || vm.control_flow_profiler().is_some() {
            vm.function_has_executed_cache().insert_unexecuted_range(
                executable.source_id(),
                // `typeProfilingStartOffset()` e `typeProfilingEndOffset()` de um ProgramExecutable
                // (ScriptExecutable.cpp): 0 e `source().length() - 1`.
                0,
                (executable.source().length() as u32).wrapping_sub(1),
            );
        }
        executable
    }

    /// `static create(JSGlobalObject*, const SourceCode&)`.
    pub fn create(global_object: &JSGlobalObject, source: &SourceCode) -> Rc<RefCell<ProgramExecutable>> {
        let vm = global_object.vm();
        Rc::new(RefCell::new(ProgramExecutable::new(&vm, source)))
    }

    /// `initializeGlobalProperties(VM&, JSGlobalObject*, JSScope*, UnlinkedProgramCodeBlock* precompiled = nullptr)`.
    ///
    /// A block precompiled from source() earlier (by any global object; like the CodeCache, this only
    /// cares about the CodeGenerationMode) stands in for the CodeCache lookup of source(). If it was
    /// generated under a different mode than globalObject compiles with now (a debugger attached
    /// since), it is ignored and the lookup happens as usual. The global declaration instantiation is
    /// done for globalObject either way, so one block can initialize any number of realms; each realm
    /// still needs its own ProgramExecutable, which links exactly once.
    pub fn initialize_global_properties(
        this: &Rc<RefCell<ProgramExecutable>>,
        vm: &VM,
        global_object: &JSGlobalObject,
        scope: &JSScopeRef,
        precompiled: Option<&Rc<RefCell<UnlinkedProgramCodeBlock>>>,
    ) -> Option<JSObjectHandle> {
        let _defer_scope = DeferTermination::new(vm);
        let throw_scope = ThrowScope::new(vm);
        debug_assert!(std::ptr::eq(global_object as *const JSGlobalObject, Rc::as_ptr(&scope.realm())));
        debug_assert!(std::ptr::eq(global_object.vm() as *const VM, vm as *const VM));

        let mut error = ParserError::new();
        let code_generation_mode = global_object.default_code_generation_mode();
        let source = this.borrow().source().clone();
        // `UnlinkedProgramCodeBlock*`: `nullptr` (`None`) acompanha um `ParserError` válido, e o C++ só
        // desreferencia o ponteiro depois do `if (error.isValid())`.
        let unlinked_code_block: Option<Rc<RefCell<UnlinkedProgramCodeBlock>>>;
        let precompiled_matches = precompiled
            .is_some_and(|precompiled| precompiled.borrow().base_ref().borrow().code_generation_mode() == code_generation_mode.to_raw());
        if precompiled_matches {
            let precompiled = precompiled.expect("precompiled");
            record_parse_from_unlinked_code_block(
                &ScriptExecutableRef::Program(Rc::clone(this)),
                &source,
                &precompiled.borrow(),
            );
            unlinked_code_block = Some(Rc::clone(precompiled));
        } else {
            unlinked_code_block =
                vm.code_cache().get_unlinked_program_code_block(&global_object.vm_rc(), this, &source, code_generation_mode, &mut error);
        }

        if global_object.has_debugger() {
            global_object.debugger().source_parsed(
                global_object,
                source.provider().expect("ProgramExecutable sem SourceProvider"),
                error.line(),
                error.message(),
            );
        }

        if error.is_valid() {
            return Some(error.to_error_object(global_object, &source).expect("ParserError válido").as_object());
        }
        let unlinked_code_block = unlinked_code_block.expect("getUnlinkedProgramCodeBlock sem erro devolveu nulo");

        let mut next_prototype = global_object.get_prototype_direct();
        while let Some(prototype) = JSObject::from_value(&next_prototype) {
            if prototype.type_() == JSType::ProxyObjectType {
                return Some(crate::runtime::error::create_type_error(
                    global_object,
                    &WtfString::from_latin1(b"Proxy is not allowed in the global prototype chain."),
                ));
            }
            next_prototype = prototype.get_prototype_direct();
        }

        let global_lexical_environment = global_object.global_lexical_environment();
        let has_global_lexical_declarations = !global_lexical_environment.is_empty();
        let unlinked = unlinked_code_block.borrow();
        let variable_declarations = unlinked.variable_declarations();
        let lexical_declarations = unlinked.lexical_declarations();
        let number_of_functions = unlinked.base_ref().borrow().number_of_function_decls();
        let is_in_strict_context = this.borrow().is_in_strict_context();
        // The ES6 spec says that no vars/global properties/let/const can be duplicated in the global scope.
        // This carried out section 15.1.8 of the ES6 spec: http://www.ecma-international.org/ecma-262/6.0/index.html#sec-globaldeclarationinstantiation
        {
            // Check for intersection of "var" and "let"/"const"/"class"
            // Check if any new "let"/"const"/"class" will shadow any pre-existing global property names (with configurable = false), or "var"/"let"/"const" variables.
            // It's an error to introduce a shadow.
            for (key, value) in lexical_declarations.iter() {
                if has_global_lexical_declarations {
                    let has_property = global_lexical_environment.has_property(vm, &PropertyName::from_uid(Some(key.clone()), false));
                    if throw_scope.exception().is_some() {
                        return None;
                    }
                    if has_property {
                        if vm.allow_redeclaring_symbols() {
                            continue;
                        }
                        if value.is_const() && !vm.global_const_redeclaration_should_throw() && !is_in_strict_context {
                            // We only allow "const" duplicate declarations under this setting.
                            // For example, we don't allow "let" variables to be overridden by "const" variables.
                            if global_lexical_environment.is_const_variable(key) {
                                continue;
                            }
                        }
                        return Some(create_error_for_duplicate_global_variable_declaration(global_object, key));
                    }
                }

                // The ES6 spec says that RestrictedGlobalProperty can't be shadowed.
                let status = has_restricted_global_property(global_object, &PropertyName::from_uid(Some(key.clone()), false));
                if throw_scope.exception().is_some() {
                    return None;
                }
                match status {
                    GlobalPropertyLookUpStatus::NonConfigurable => {
                        return Some(crate::runtime::error::create_syntax_error(
                            global_object,
                            &make_string_dyn(&[
                                &"Can't create duplicate variable that shadows a global property: '",
                                &WtfString::from(Rc::clone(&key.0)),
                                &'\'',
                            ]),
                        ));
                    }
                    GlobalPropertyLookUpStatus::Configurable => {
                        // Lexical bindings can shadow global properties if the given property's attribute is configurable.
                        // https://tc39.github.io/ecma262/#sec-globaldeclarationinstantiation step 5-c, `hasRestrictedGlobal` becomes false
                        // However we may emit GlobalProperty look up in bytecodes already and it may cache the value for the global scope.
                        // To make it invalid,
                        // 1. In LLInt and Baseline, we bump the global lexical binding epoch and it works.
                        // 3. In DFG and FTL, we watch the watchpoint and jettison once it is fired.
                    }
                    GlobalPropertyLookUpStatus::NotFound => {}
                }
            }

            // Check if any new "var"s will shadow any previous "let"/"const"/"class" names.
            // It's an error to introduce a shadow.
            if has_global_lexical_declarations {
                for (key, value) in variable_declarations.iter() {
                    if value.is_sloppy_mode_hoisted_function() {
                        continue;
                    }
                    let has_property = global_lexical_environment.has_property(vm, &PropertyName::from_uid(Some(key.clone()), false));
                    if throw_scope.exception().is_some() {
                        return None;
                    }
                    if has_property {
                        return Some(create_error_for_duplicate_global_variable_declaration(global_object, key));
                    }
                }
            }

            for i in 0..number_of_functions {
                let unlinked_function_executable = Rc::clone(unlinked.base_ref().borrow().function_decl(i));
                debug_assert!(!unlinked_function_executable.borrow().name().is_empty());
                let name = unlinked_function_executable.borrow().name();
                let can_declare = global_object.can_declare_global_function(&name);
                if throw_scope.exception().is_some() {
                    return None;
                }
                if !can_declare {
                    // `if (requiresCanDeclareGlobalFunctionQuirk())` (apagar a propriedade e seguir) só vale em
                    // `PLATFORM(COCOA)` com SDK antigo: a função devolve sempre `false` aqui e o ramo some.
                    return Some(create_error_for_invalid_global_function_declaration(global_object, &name));
                }
            }

            if !global_object.is_structure_extensible() {
                for (key, value) in variable_declarations.iter() {
                    if value.is_function() || value.is_sloppy_mode_hoisted_function() {
                        continue;
                    }
                    debug_assert!(value.is_var());
                    let ident = Identifier::from_uid(vm, Some(key));
                    let can_declare = global_object.can_declare_global_var(&ident);
                    if throw_scope.exception().is_some() {
                        return None;
                    }
                    if !can_declare {
                        return Some(create_error_for_invalid_global_var_declaration(global_object, &ident));
                    }
                }
            }
        }

        this.borrow_mut().set_unlinked_code_block(Rc::clone(&unlinked_code_block));

        let _optimizer = BatchedTransitionOptimizer::new(vm, global_object);

        // https://tc39.es/ecma262/#sec-web-compat-globaldeclarationinstantiation (excluding last step)
        if !is_in_strict_context {
            for (key, value) in variable_declarations.iter() {
                if !value.is_sloppy_mode_hoisted_function() {
                    continue;
                }

                let ident = Identifier::from_uid(vm, Some(key));

                if has_global_lexical_declarations {
                    let has_property = global_lexical_environment.has_property(vm, &PropertyName::from_identifier(&ident));
                    if throw_scope.exception().is_some() {
                        return None;
                    }
                    if has_property {
                        continue;
                    }
                }

                let can_declare = global_object.can_declare_global_var(&ident);
                if throw_scope.exception().is_some() {
                    return None;
                }
                if !can_declare {
                    continue;
                }

                global_object.create_global_var_binding(BindingCreationContext::Global, &ident);
                if throw_scope.exception().is_some() {
                    return None;
                }
            }
        }

        for i in 0..number_of_functions {
            let unlinked_function_executable = Rc::clone(unlinked.base_ref().borrow().function_decl(i));
            debug_assert!(!unlinked_function_executable.borrow().name().is_empty());
            let name = unlinked_function_executable.borrow().name();
            global_object.create_global_function_binding(BindingCreationContext::Global, &name);
            if throw_scope.exception().is_some() {
                return None;
            }
            if vm.type_profiler().is_some() || vm.control_flow_profiler().is_some() {
                let executable = unlinked_function_executable.borrow();
                vm.function_has_executed_cache().insert_unexecuted_range(
                    this.borrow().source_id(),
                    executable.unlinked_function_start(),
                    executable.unlinked_function_end(),
                );
            }
        }

        for (key, value) in variable_declarations.iter() {
            if value.is_function() || value.is_sloppy_mode_hoisted_function() {
                continue;
            }
            debug_assert!(value.is_var());
            global_object.create_global_var_binding(BindingCreationContext::Global, &Identifier::from_uid(vm, Some(key)));
            if throw_scope.exception().is_some() {
                return None;
            }
        }

        {
            let symbol_table = global_lexical_environment.symbol_table();
            for (key, value) in lexical_declarations.iter() {
                if vm.allow_redeclaring_symbols() && symbol_table.borrow().contains(key) {
                    continue;
                }
                if value.is_const() && !vm.global_const_redeclaration_should_throw() && !is_in_strict_context {
                    if symbol_table.borrow().contains(key) {
                        continue;
                    }
                }
                let offset = symbol_table.borrow_mut().take_next_scope_offset();
                let attributes = if value.is_const() { PropertyAttribute::ReadOnly as u32 } else { PropertyAttribute::None as u32 };
                let mut new_entry = SymbolTableEntry::new(VarOffset::from_scope_offset(offset), attributes);
                new_entry.prepare_to_watch();
                symbol_table.borrow_mut().add(key.clone(), new_entry);

                let offset_for_assert = global_lexical_environment.add_variables(1, js_tdz_value());
                assert!(offset_for_assert == offset);
            }
        }
        if lexical_declarations.size() != 0 {
            // `#if ENABLE(DFG_JIT)` (disparo do WatchpointSet de `getReferencedPropertyWatchpointSet`): vale o
            // ramo `#else`, não há DFG.
            global_object.bump_global_lexical_binding_epoch(vm);
        }
        None
    }

    /// `ensureTemplateObjectMap(VM&)`.
    pub fn ensure_template_object_map(&mut self, _vm: &VM) -> &mut TemplateObjectMap {
        ScriptExecutable::ensure_template_object_map_impl(&mut self.template_object_map)
    }
}
