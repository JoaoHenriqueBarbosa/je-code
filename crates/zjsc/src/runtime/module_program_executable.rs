//! Tradução de `runtime/ModuleProgramExecutable.h`, `ModuleProgramExecutable.cpp` e
//! `ModuleProgramExecutableInlines.h`.
//!
//! DIVERGÊNCIAS (ver `executable.rs` e `global_executable.rs`): `getUnlinkedCodeBlock` e `tryCreate`
//! recebem o `Rc<RefCell<ModuleProgramExecutable>>` em vez de usar `this`, porque o cache de código
//! chama `recordParse` no mesmo executável (`recordParseFromUnlinkedCodeBlock`) e nenhum empréstimo
//! pode estar vivo nessa hora. `createStructure`, `subspaceFor`, `visitChildren`, `destroy` e
//! `DECLARE_INFO` são maquinaria de heap e não existem. `vm.typeProfiler()`,
//! `vm.controlFlowProfiler()` e `vm.functionHasExecutedCache()` são do `TypeProfiler`, ainda sem
//! porte: o `if` do construtor vai escrito como no C++, chamando esses nomes.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use crate::bytecode::executable_info::{DerivedContextType, EvalContextType};
use crate::bytecode::unlinked_code_block::UnlinkedModuleProgramCodeBlock;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::parser::parser_error::ParserError;
use crate::parser::parser_modes::{AWAIT_FEATURE, STRICT_MODE_LEXICALLY_SCOPED_FEATURE};
use crate::parser::source_code::SourceCode;
use crate::parser::source_provider::SourceProviderSourceType;
use crate::runtime::global_executable::GlobalExecutable;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_type::JSType;
use crate::runtime::script_executable::{ScriptExecutable, TemplateObjectMap};
use crate::runtime::symbol_table::{PropagateCloneInvalidationToOriginal, SymbolTable, SymbolTableRef};
use crate::runtime::throw_scope::{throw_vm_error, ThrowScope};
use crate::runtime::vm::VM;

/// `class ModuleProgramExecutable`.
pub struct ModuleProgramExecutable {
    base: GlobalExecutable<UnlinkedModuleProgramCodeBlock>,
    module_environment_symbol_table: Option<SymbolTableRef>,
    template_object_map: Option<Box<TemplateObjectMap>>,
}

crate::parser::nodes::inherit!(ModuleProgramExecutable => GlobalExecutable<UnlinkedModuleProgramCodeBlock>);

impl ModuleProgramExecutable {
    /// `ModuleProgramExecutable(JSGlobalObject*, const SourceCode&)` (privado).
    fn new(vm: &VM, source: &SourceCode) -> ModuleProgramExecutable {
        let base = ScriptExecutable::new(
            JSType::ModuleProgramExecutableType,
            source,
            STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
            DerivedContextType::None,
            false,
            false,
            EvalContextType::None,
            Intrinsic::NoIntrinsic,
        );
        let source_type = source.provider().expect("ModuleProgramExecutable sem SourceProvider").source_type();
        debug_assert!(
            source_type == SourceProviderSourceType::Module || source_type == SourceProviderSourceType::BunTranspiledModule
        );
        let executable = ModuleProgramExecutable {
            base: GlobalExecutable::new(base),
            module_environment_symbol_table: None,
            template_object_map: None,
        };
        if vm.type_profiler().is_some() || vm.control_flow_profiler().is_some() {
            vm.function_has_executed_cache().insert_unexecuted_range(
                executable.source_id(),
                // `typeProfilingStartOffset()` e `typeProfilingEndOffset()` de um ModuleProgramExecutable
                // (ScriptExecutable.cpp): 0 e `source().length() - 1`.
                0,
                (executable.source().length() as u32).wrapping_sub(1),
            );
        }
        executable
    }

    /// `getUnlinkedCodeBlock(JSGlobalObject*)`: `nullptr` com exceção pendente é `None`.
    pub fn get_unlinked_code_block(
        this: &Rc<RefCell<ModuleProgramExecutable>>,
        global_object: &JSGlobalObject,
    ) -> Option<Rc<RefCell<UnlinkedModuleProgramCodeBlock>>> {
        let vm = global_object.vm_rc();
        let mut throw_scope = ThrowScope::new(&vm);

        let existing = this.borrow().unlinked_code_block().cloned();
        if let Some(unlinked_module_program_code) = existing {
            return Some(unlinked_module_program_code);
        }

        let mut error = ParserError::new();
        let code_generation_mode = global_object.default_code_generation_mode();
        let source = this.borrow().source().clone();
        let unlinked_module_program_code =
            vm.code_cache().get_unlinked_module_program_code_block(&vm, this, &source, code_generation_mode, &mut error);

        if global_object.has_debugger() {
            global_object.debugger().source_parsed(
                global_object,
                source.provider().expect("ModuleProgramExecutable sem SourceProvider"),
                error.line(),
                error.message(),
            );
        }

        if error.is_valid() {
            throw_vm_error(
                global_object,
                &mut throw_scope,
                error.to_error_object(global_object, &source).expect("ParserError válido"),
            );
            return None;
        }

        let unlinked_module_program_code =
            unlinked_module_program_code.expect("getUnlinkedModuleProgramCodeBlock sem erro devolveu nulo");
        let symbol_table = {
            let mut executable = this.borrow_mut();
            executable.set_unlinked_code_block(Rc::clone(&unlinked_module_program_code));
            let symbol_table_reg = VirtualRegister::new(
                unlinked_module_program_code.borrow().module_environment_symbol_table_constant_register_offset(),
            );
            let constant = unlinked_module_program_code.borrow().base_ref().borrow().constant_register(symbol_table_reg);
            SymbolTable::from_cell_id(constant.as_cell()).expect("constante do ambiente do módulo não é SymbolTable")
        };
        let cloned = symbol_table.borrow().clone_scope_part(&vm, PropagateCloneInvalidationToOriginal::Yes);
        this.borrow_mut().module_environment_symbol_table = Some(cloned);
        Some(unlinked_module_program_code)
    }

    /// `tryCreate(JSGlobalObject*, const SourceCode&)`.
    pub fn try_create(global_object: &JSGlobalObject, source: &SourceCode) -> Option<Rc<RefCell<ModuleProgramExecutable>>> {
        let vm = global_object.vm();
        let throw_scope = ThrowScope::new(&vm);

        let executable = Rc::new(RefCell::new(ModuleProgramExecutable::new(&vm, source)));
        // This generates and binds unlinked code block.
        if ModuleProgramExecutable::get_unlinked_code_block(&executable, global_object).is_none() {
            return None;
        }
        if throw_scope.exception().is_some() {
            return None;
        }
        Some(executable)
    }

    /// `isAsync()`.
    pub fn is_async(&self) -> bool {
        self.features() & AWAIT_FEATURE != 0
    }

    /// `moduleEnvironmentSymbolTable()`.
    pub fn module_environment_symbol_table(&self) -> Option<SymbolTableRef> {
        self.module_environment_symbol_table.clone()
    }

    /// `m_moduleEnvironmentSymbolTable.clear()`.
    pub fn clear_module_environment_symbol_table(&mut self) {
        self.module_environment_symbol_table = None;
    }

    /// `ensureTemplateObjectMap(VM&)`.
    pub fn ensure_template_object_map(&mut self, _vm: &VM) -> &mut TemplateObjectMap {
        ScriptExecutable::ensure_template_object_map_impl(&mut self.template_object_map)
    }
}
