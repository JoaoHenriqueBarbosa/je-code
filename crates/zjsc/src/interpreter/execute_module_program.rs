//! `Interpreter::executeModuleProgram` (`interpreter/Interpreter.cpp`): a entrada do corpo de um módulo.
//!
//! DIVERGÊNCIAS: o `VMEntryScope`, o `DeferTraps` e o `clobberizeValidator` não existem; o
//! `JSModuleRecord*` do C++ é o `AbstractModuleRecord` por composição (ver `runtime/js_module_record.rs`) e
//! entra como `JSValue` pelo registro de células. A lacuna do porte (`Unported`) volta como `Err`, e a
//! exceção pendente como `Err(Thrown)`, como em `execute_program`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::interpreter::interpreter::Interpreter;
use crate::interpreter::proto_call_frame::ProtoCallFrame;
use crate::llint::llint_jit_code::LLIntEntry;
use crate::llint::slow_paths::throw_stack_overflow_error;
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::abstract_module_record::{AbstractModuleRecordRef, Argument, Field, State};
use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::js_callee::JSCallee;
use crate::runtime::js_global_object::JSGlobalObjectRef;
use crate::runtime::js_module_environment::JSModuleEnvironmentRef;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_value::{js_number_i32, JSValue};
use crate::runtime::module_program_executable::ModuleProgramExecutable;
use crate::runtime::script_executable::ScriptExecutableRef;

impl Interpreter {
    /// `executeModuleProgram(record, executable, lexicalGlobalObject, scope, sentValue, resumeMode)`.
    pub fn execute_module_program(
        &mut self,
        record: &AbstractModuleRecordRef,
        executable: &Rc<RefCell<ModuleProgramExecutable>>,
        global_object: &JSGlobalObjectRef,
        scope: &JSModuleEnvironmentRef,
        sent_value: JSValue,
        resume_mode: JSValue,
    ) -> LLIntResult<JSValue> {
        let vm = global_object.vm();
        if !vm.is_safe_to_recurse() {
            return Err(throw_stack_overflow_error(global_object));
        }

        let scope_ref = JSScopeRef::ModuleEnvironment(Rc::clone(scope));
        let callee = JSCallee::create(vm, global_object, scope_ref.clone());

        let mut code_block = None;
        ScriptExecutableRef::ModuleProgram(Rc::clone(executable)).prepare_for_execution(
            vm,
            None,
            &scope_ref,
            CodeSpecializationKind::CodeForCall,
            &mut code_block,
        );
        if vm.exception().is_some() {
            return Err(LLIntFailure::Thrown);
        }
        let code_block = code_block.expect("ASSERT(codeBlock): prepareForExecution sem exceção pendente devolve um CodeBlock");
        let number_of_arguments = Argument::NUMBER_OF_ARGUMENTS;
        let num_parameters = code_block.borrow().num_parameters();
        debug_assert!(num_parameters as usize == number_of_arguments + 1);

        let code_block_id = self.register_code_block(&code_block);
        let jit_code = code_block.borrow().jit_code();
        let entry = LLIntEntry::from_code_ptr(jit_code.address_for_call(ArityCheckMode::ArityCheckNotRequired))
            .expect("RELEASE_ASSERT(JIT desligado): o JITCode do módulo é o ponto de entrada do LLInt");

        // `record, state, sentValue, resumeMode, scope` (`AbstractModuleRecord::Argument`, sem o `this`).
        let args = vec![
            record.as_value().encode(),
            record.internal_field(Field::State).encode(),
            sent_value.encode(),
            resume_mode.encode(),
            scope_ref.into_js_value().encode(),
        ];

        let global_object_cell = JSScopeRef::GlobalObject(Rc::clone(global_object)).cell_id();
        let mut proto_call_frame = ProtoCallFrame::default();
        // The |this| of the module is always `undefined`.
        proto_call_frame.init(
            Some(code_block_id),
            num_parameters,
            global_object_cell,
            callee.cell_id(),
            JSValue::undefined(),
            None,
            number_of_arguments as i32 + 1,
            args,
        );

        record.set_internal_field(Field::State, js_number_i32(State::Executing as i32));

        self.vm_entry_to_javascript(global_object, proto_call_frame, entry)
    }
}
