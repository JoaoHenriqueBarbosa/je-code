// Parte 1 de bytecompiler/BytecodeGenerator.cpp (linhas 1 a 992). Juntada por include!.
// Convenções: `RefPtr<RegisterID>` é `Option<Rc<RefCell<RegisterID>>>` quando anulável; `Ref<Label>` é
// `LabelRef`. Os construtores sobrecarregados do C++ viram `new_program`, `new_function` e (na parte
// seguinte) `new_eval`; todos partem de `BytecodeGenerator::with_defaults(vm, code_block, ...)`, que
// a struct fornece com os valores de inicialização padrão dos membros. O `return` antecipado no meio
// do construtor do C++ vira `return this`.

/// `template<typename CallOp> struct VarArgsOp`: o opcode varargs de cada opcode de chamada.
pub trait VarArgsOp {
    type Type;
}

impl VarArgsOp for crate::bytecode::bytecode_ops::OpCall {
    type Type = crate::bytecode::bytecode_ops::OpCallVarargs;
}

impl VarArgsOp for crate::bytecode::bytecode_ops::OpCallIgnoreResult {
    type Type = crate::bytecode::bytecode_ops::OpCallVarargs;
}

impl VarArgsOp for crate::bytecode::bytecode_ops::OpCallDirectEval {
    type Type = crate::bytecode::bytecode_ops::OpCallVarargs;
}

impl VarArgsOp for crate::bytecode::bytecode_ops::OpTailCall {
    type Type = crate::bytecode::bytecode_ops::OpTailCallVarargs;
}

impl VarArgsOp for crate::bytecode::bytecode_ops::OpConstruct {
    type Type = crate::bytecode::bytecode_ops::OpConstructVarargs;
}

impl VarArgsOp for crate::bytecode::bytecode_ops::OpSuperConstruct {
    type Type = crate::bytecode::bytecode_ops::OpSuperConstructVarargs;
}

impl Variable {
    /// `Variable::dump(PrintStream&)`: devolve o texto que o C++ imprimiria.
    pub fn dump(&self) -> String {
        format!(
            "{{ident = {}, offset = {}, local = {}, attributes = {}, kind = {}, symbolTableConstantIndex = {}, isLexicallyScoped = {}}}",
            String::from_utf8_lossy(&self.ident.utf8()),
            format!("{:?}", self.offset),
            match &self.local {
                Some(local) => format!("{:p}", std::rc::Rc::as_ptr(local.get())),
                None => String::from("(nil)"),
            },
            self.attributes,
            self.kind as u32,
            self.symbol_table_constant_index,
            self.is_lexically_scoped,
        )
    }
}

impl FinallyContext {
    /// `FinallyContext::FinallyContext(BytecodeGenerator&, Label&)`.
    pub fn new(
        generator: &mut BytecodeGenerator,
        finally_label: crate::bytecompiler::label::LabelRef,
    ) -> FinallyContext {
        let mut context = FinallyContext {
            outer_context: generator.current_finally_context.clone(),
            finally_label: Some(finally_label),
            number_of_breaks_or_continues: 0,
            handles_returns: false,
            jumps: Vec::new(),
            completion_record: Default::default(),
        };
        context.completion_record.type_register = Some(generator.new_temporary());
        context.completion_record.value_register = Some(generator.new_temporary());
        let completion_type_register = context.completion_type_register();
        generator.emit_load_completion_type(completion_type_register, CompletionType::NORMAL);
        let completion_value_register = context.completion_value_register();
        generator.move_empty_value(completion_value_register);
        context
    }
}

/// `pattern->boundProperty()` de `BindingNode` (o C++ chama `as<BindingNode>`; o padrão já foi
/// conferido com `isBindingNode()`).
fn binding_bound_property(pattern: &crate::parser::nodes::DestructuringPatternNode) -> Identifier {
    match pattern {
        crate::parser::nodes::DestructuringPatternNode::Binding(binding) => {
            binding.borrow().bound_property.clone()
        }
        _ => unreachable!("o parâmetro simples é sempre um BindingNode"),
    }
}

impl BytecodeGenerator {
    /// `template<typename EmitBytecodeFunctor> void asyncFuncParametersTryCatchWrap(const EmitBytecodeFunctor&)`.
    pub fn async_func_parameters_try_catch_wrap<F: FnOnce(&mut BytecodeGenerator)>(
        &mut self,
        emit_bytecode: F,
    ) {
        let mut try_data = None;
        if let Some(info) = self.async_func_parameters_try_catch_info.clone() {
            debug_assert!(info.catch_start_label.is_some() && info.thrown_value.is_some());
            let try_start_label = self.new_emitted_label();
            try_data = Some(self.push_try(
                &try_start_label,
                info.catch_start_label.as_ref().unwrap(),
                crate::bytecode::handler_info::HandlerType::SynthesizedCatch,
            ));
        }

        emit_bytecode(self);

        if let Some(info) = self.async_func_parameters_try_catch_info.clone() {
            let try_end_label = self.new_emitted_label();
            self.pop_try(try_data.as_ref().unwrap(), &try_end_label);

            self.emit_out_of_line_catch_handler(info.thrown_value, None, try_data.as_ref());
        }
    }

    /// `ParserError BytecodeGenerator::generate(unsigned& size)`; `size` volta no segundo elemento.
    pub fn generate(&mut self) -> (crate::parser::parser_error::ParserError, u32) {
        use crate::parser::parser_error::{ErrorType, ParserError};
        let mut size = 0u32;

        if self.out_of_memory_during_construction {
            return (ParserError::with_type(ErrorType::OutOfMemory), size);
        }

        let mut calling_non_callable_constructor = false;
        match self.constructor_kind() {
            crate::runtime::constructor_kind::ConstructorKind::None => {}
            crate::runtime::constructor_kind::ConstructorKind::Naked
            | crate::runtime::constructor_kind::ConstructorKind::Base
            | crate::runtime::constructor_kind::ConstructorKind::Extends => {
                calling_non_callable_constructor = !self.is_constructor();
            }
        }

        let this_virtual_register = self.this_register.virtual_register();
        self.code_block.set_this_register(this_virtual_register);

        self.emit_log_shadow_chicken_prologue_if_necessary();

        if !calling_non_callable_constructor {
            // If we have declared a variable named "arguments" and we are using arguments then we should
            // perform that assignment now.
            if self.need_to_initialize_arguments {
                let arguments_variable = self.variable(&self.property_names().arguments.clone(), crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                let arguments_register = self.arguments_register.clone();
                self.initialize_variable(&arguments_variable, arguments_register);
            }

            {
                let should_hoist_in_eval = self.code_type == CodeType::EvalCode && !self.ecma_mode.is_strict();
                let mut top_level_scope: Option<RegisterRef> = None;
                let functions_to_initialize = self.functions_to_initialize.clone();
                for function_pair in functions_to_initialize.iter() {
                    let metadata = &function_pair.0;
                    let function_type = function_pair.1;
                    if function_type == FunctionVariableType::NormalFunctionVariable {
                        let var = self.variable(&metadata.ident.borrow().clone(), crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                        if let Some(local) = var.local() {
                            self.emit_new_function(Some(local), metadata);
                        } else {
                            let temp = self.new_temporary();
                            self.emit_new_function(Some(temp.clone()), metadata);
                            self.initialize_variable(&var, Some(temp));
                        }
                    } else if function_type == FunctionVariableType::TopLevelFunctionVariable {
                        let temp = self.new_temporary();
                        self.emit_new_function(Some(temp.clone()), metadata);
                        if top_level_scope.is_none() {
                            // We know this will resolve to the top level scope or global object because our parser/global initialization code
                            // doesn't allow let/const/class variables to have the same names as functions.
                            // This is a top level function, and it's an error to ever create a top level function
                            // name that would resolve to a lexical variable. E.g:
                            // ```
                            //     function f() {
                            //         {
                            //             let x;
                            //             {
                            //             //// error thrown here
                            //                  eval("function x(){}");
                            //             }
                            //         }
                            //     }
                            // ```
                            // Therefore, we're guaranteed to have this resolve to a top level variable.
                            let ident = metadata.ident.borrow().clone();
                            let top_level_object_scope = if should_hoist_in_eval {
                                self.emit_resolve_scope_for_hoisting_func_decl_in_eval(None, &ident)
                            } else {
                                self.emit_resolve_scope(None, &Variable::from_ident(&ident))
                            };

                            let new_scope = self.new_block_scope_variable();
                            self.move_register(Some(&new_scope), top_level_object_scope.as_ref().unwrap());
                            top_level_scope = Some(new_scope);
                        }
                        let ident = metadata.ident.borrow().clone();
                        if should_hoist_in_eval {
                            self.emit_put_to_scope_dynamic(
                                top_level_scope.clone(),
                                &ident,
                                Some(temp),
                                ResolveMode::ThrowIfNotFound,
                                InitializationMode::NotInitialization,
                            );
                        } else {
                            self.emit_put_to_scope(
                                top_level_scope.clone(),
                                &Variable::from_ident(&ident),
                                Some(temp),
                                ResolveMode::ThrowIfNotFound,
                                InitializationMode::NotInitialization,
                            );
                        }
                    } else {
                        unreachable!("RELEASE_ASSERT_NOT_REACHED");
                    }
                }
            }

            let scope_node = self.scope_node.clone();
            // `m_scopeNode->emitBytecode(*this)` é virtual no C++: despacho pela variante concreta.
            match &scope_node {
                crate::parser::nodes::ScopeNodeRef::Program(node) => node.borrow().emit_bytecode(self, None),
                crate::parser::nodes::ScopeNodeRef::Eval(node) => node.borrow().emit_bytecode(self, None),
                crate::parser::nodes::ScopeNodeRef::ModuleProgram(node) => node.borrow().emit_bytecode(self, None),
                crate::parser::nodes::ScopeNodeRef::Function(node) => node.borrow().emit_bytecode(self, None),
            }
        } else {
            // At this point we would have emitted an unconditional throw followed by some nonsense that's
            // just an artifact of how this generator is structured. That code never runs, but it confuses
            // bytecode analyses because it constitutes an unterminated basic block. So, we terminate the
            // basic block the strongest way possible.
            crate::bytecode::bytecode_ops::OpUnreachable::emit(self);
        }

        let handlers_to_emit = std::mem::take(&mut self.exception_handlers_to_emit);
        for handler in handlers_to_emit.iter() {
            let real_catch_target = self.new_label();
            let try_data = handler.try_data.clone();

            crate::bytecode::bytecode_ops::OpCatch::emit(
                self,
                handler.exception_register,
                handler.thrown_value_register,
            );
            let last_instruction_offset = self.last_instruction.offset();
            <crate::bytecompiler::bytecode_generator::JSGeneratorTraits as crate::bytecompiler::bytecode_generator_base::BytecodeGeneratorTraits>::set_label_location(
                self,
                &real_catch_target,
                last_instruction_offset,
            );
            if handler.completion_type_register.is_valid() {
                let completion_type_register = RegisterRef::new(&std::rc::Rc::new(std::cell::RefCell::new(
                    RegisterID::from_virtual_register(handler.completion_type_register),
                )));
                let handler_type = try_data.borrow().handler_type;
                let completion_type = if handler_type == HandlerType::Finally
                    || handler_type == HandlerType::SynthesizedFinally
                {
                    CompletionType::THROW
                } else {
                    CompletionType::NORMAL
                };
                self.emit_load_completion_type(Some(completion_type_register), completion_type);
            }

            let target = try_data.borrow().target.clone();
            self.emit_jump(&target);
            try_data.borrow_mut().target = real_catch_target;
        }
        self.exception_handlers_to_emit = handlers_to_emit;

        if let Some(info) = self.async_func_parameters_try_catch_info.clone() {
            debug_assert!(info.catch_start_label.is_some() && info.thrown_value.is_some());
            self.emit_label(info.catch_start_label.as_ref().unwrap());
            let scope_node = self.scope_node.clone();
            let scope_node = scope_node.borrow();
            let divot = crate::parser::parser_tokens::JSTextPosition::new(
                scope_node.first_line(),
                scope_node.start_offset(),
                scope_node.line_start_offset(),
            );
            if self.promise_register().is_some() {
                let reject_promise = self.move_link_time_constant(
                    None,
                    LinkTimeConstant::RejectPromiseWithFirstResolvingFunctionCallCheck,
                );
                let mut args = CallArguments::new(self, None, 2);
                let this_register = args.this_register();
                self.emit_load_js_value(this_register, crate::runtime::js_value::JSValue::Undefined);
                let promise_register = self.promise_register();
                self.move_register(args.argument_register(0).as_ref(), promise_register.as_ref().unwrap());
                self.move_register(args.argument_register(1).as_ref(), info.thrown_value.as_ref().unwrap());
                let result = self.new_temporary();
                self.emit_call_ignore_result(
                    Some(result),
                    reject_promise,
                    ExpectedFunction::NoExpectedFunction,
                    &mut args,
                    &divot,
                    &divot,
                    &divot,
                    DebuggableCall::No,
                );
                let promise_register = self.promise_register();
                self.emit_return(promise_register);
            } else {
                // If we are not creating a promise yet, we can just do `return @newRejectedPromise(thrownValue)`.
                let new_rejected_promise =
                    self.move_link_time_constant(None, LinkTimeConstant::NewRejectedPromise);
                let mut args = CallArguments::new(self, None, 1);
                let this_register = args.this_register();
                self.emit_load_js_value(this_register, crate::runtime::js_value::JSValue::Undefined);
                self.move_register(args.argument_register(0).as_ref(), info.thrown_value.as_ref().unwrap());
                let result = self.new_temporary();
                self.emit_call(
                    Some(result.clone()),
                    new_rejected_promise,
                    ExpectedFunction::NoExpectedFunction,
                    &mut args,
                    &divot,
                    &divot,
                    &divot,
                    DebuggableCall::No,
                );
                self.emit_return(Some(result));
            }
        }

        self.static_property_analyzer.kill();

        let try_ranges = self.try_ranges.clone();
        for range in try_ranges.iter() {
            let start = range.start.borrow_mut().bind().target(&*self);
            let end = range.end.borrow_mut().bind().target(&*self);

            // This will happen for empty try blocks and for some cases of finally blocks:
            //
            // try {
            //    try {
            //    } finally {
            //        return 42;
            //        // *HERE*
            //    }
            // } finally {
            //    print("things");
            // }
            //
            // The return will pop scopes to execute the outer finally block. But this includes
            // popping the try context for the inner try. The try context is live in the fall-through
            // part of the finally block not because we will emit a handler that overlaps the finally,
            // but because we haven't yet had a chance to plant the catch target. Then when we finish
            // emitting code for the outer finally block, we repush the try contex, this time with a
            // new start index. But that means that the start index for the try range corresponding
            // to the inner-finally-following-the-return (marked as "*HERE*" above) will be greater
            // than the end index of the try block. This is harmless since end < start handlers will
            // never get matched in our logic, but we do the runtime a favor and choose to not emit
            // such handlers at all.
            if end <= start {
                continue;
            }

            let target = range.try_data.borrow().target.clone();
            let handler_type = range.try_data.borrow().handler_type;
            let info = crate::bytecode::handler_info::UnlinkedHandlerInfo::new(
                start as u32,
                end as u32,
                target.borrow_mut().bind().target(&*self) as u32,
                handler_type,
            );
            self.code_block.add_exception_handler(info);
        }

        if self.needs_generatorification {
            crate::bytecompiler::bytecode_generatorification::perform_generatorification(
                self,
                self.generator_frame_symbol_table.clone(),
                self.generator_frame_symbol_table_index,
            );
        }

        assert!(
            (crate::bytecompiler::bytecode_generator_base::GeneratorCodeBlock::num_callee_locals(&self.code_block))
                < crate::bytecode::virtual_register::FIRST_CONSTANT_REGISTER_INDEX as u32
        );
        size = self.instructions().size_in_bytes() as u32;
        let finalized = self.writer.finalize();
        if !self.code_block.finalize(finalized) {
            return (ParserError::with_type(ErrorType::OutOfMemory), size);
        }

        // We limit total bytecode sequence size to int32_t so that we can use int32_t jump offsets.
        // Also, this allows us to use one bit of bytecode for some flag, including "ignore-result-flag".
        if size > i32::MAX as u32 {
            return (ParserError::with_type(ErrorType::OutOfMemory), size);
        }

        if self.expression_too_deep {
            return (ParserError::with_type(ErrorType::OutOfMemory), size);
        }
        (ParserError::with_type(ErrorType::ErrorNone), size)
    }

    /// `BytecodeGenerator::BytecodeGenerator(VM&, ProgramNode*, UnlinkedProgramCodeBlock*, ...)`.
    pub fn new_program(
        vm: &Rc<crate::runtime::vm::VM>,
        program_node: NodeRef<crate::parser::nodes::ProgramNode>,
        code_block: &Rc<std::cell::RefCell<crate::bytecode::unlinked_code_block::UnlinkedProgramCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<Rc<TDZEnvironmentLink>>,
        _generator_or_async_wrapper_function_parameter_names: Option<&Vec<Identifier>>,
        _parent_private_name_environment: Option<&PrivateNameEnvironment>,
    ) -> BytecodeGenerator {
        let base = code_block.borrow().base_ref();
        let mut this = BytecodeGenerator::with_defaults(
            vm,
            base,
            code_generation_mode,
            crate::parser::nodes::ScopeNodeRef::Program(program_node.clone()),
            CodeType::GlobalCode,
        );
        {
            let node = program_node.borrow();
            this.uses_exceptions = false;
            this.expression_too_deep = false;
            this.is_builtin_function = false;
            this.uses_sloppy_eval = false;
            this.allow_tail_call_optimization = false;
            this.allow_call_ignore_result_optimization = this.default_allow_call_ignore_result_optimization;
            this.needs_to_update_arrow_function_context = node.uses_arrow_function() || node.uses_eval();
            this.ecma_mode = ECMAMode::from_bool(node.is_strict_mode());
        }

        debug_assert!(parent_scope_tdz_variables.is_none());

        this.code_block.set_num_parameters(1); // Allocate space for "this"

        this.emit_enter();

        this.allocate_scope();

        let function_stack = program_node.borrow().base.variable_environment.function_stack.clone();

        for function in function_stack.iter() {
            this.functions_to_initialize
                .push((function.clone(), FunctionVariableType::TopLevelFunctionVariable));
        }

        if crate::runtime::options::Options::validate_bytecode() {
            for entry in program_node.borrow().base.var_declarations.iter() {
                assert!(entry.1.is_var());
            }
        }
        code_block
            .borrow_mut()
            .set_variable_declarations(program_node.borrow().base.var_declarations.clone());
        code_block
            .borrow_mut()
            .set_lexical_declarations(program_node.borrow().base.variable_environment.lexical_variables.clone());
        // Even though this program may have lexical variables that go under TDZ, when linking the get_from_scope/put_to_scope
        // operations we emit we will have ResolveTypes that implictly do TDZ checks. Therefore, we don't need
        // additional TDZ checks on top of those. This is why we can omit pushing programNode->lexicalVariables()
        // to the TDZ stack.

        if this.needs_to_update_arrow_function_context() {
            this.initialize_arrow_function_context_scope_if_needed(None, false);
            this.emit_put_this_to_arrow_function_context_scope();
        }
        this
    }

    /// `BytecodeGenerator::BytecodeGenerator(VM&, FunctionNode*, UnlinkedFunctionCodeBlock*, ...)`.
    pub fn new_function(
        vm: &Rc<crate::runtime::vm::VM>,
        function_node: NodeRef<crate::parser::nodes::FunctionNode>,
        code_block: &crate::bytecode::unlinked_code_block::UnlinkedFunctionCodeBlock,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<Rc<TDZEnvironmentLink>>,
        generator_or_async_wrapper_function_parameter_names: Option<&Vec<Identifier>>,
        parent_private_name_environment: Option<&PrivateNameEnvironment>,
    ) -> BytecodeGenerator {
        use crate::parser::parser_modes::SourceParseMode;
        use crate::runtime::constructor_kind::ConstructorKind;
        let base = code_block.base_ref();
        let mut this = BytecodeGenerator::with_defaults(
            vm,
            base.clone(),
            code_generation_mode,
            crate::parser::nodes::ScopeNodeRef::Function(function_node.clone()),
            CodeType::FunctionCode,
        );
        this.default_allow_call_ignore_result_optimization = !crate::runtime::options::Options::eval_mode();
        // FIXME: This should be a flag
        this.uses_exceptions = false;
        this.expression_too_deep = false;
        this.is_builtin_function = base.borrow().is_builtin_function();
        this.is_builtin_default_class_constructor = base.borrow().is_builtin_default_class_constructor();
        this.uses_sloppy_eval = function_node.borrow().uses_eval() && !function_node.borrow().is_strict_mode();
        // FIXME: We should be able to have tail call elimination with the profiler
        // enabled. This is currently not possible because the profiler expects
        // op_will_call / op_did_call pairs before and after a call, which are not
        // compatible with tail calls (we have no way of emitting op_did_call).
        // https://bugs.webkit.org/show_bug.cgi?id=148819
        //
        // Note that we intentionally enable tail call for naked constructors since it does not have special code for "return".
        this.allow_tail_call_optimization = crate::runtime::options::Options::use_tail_calls()
            && !this.is_constructor()
            && this.constructor_kind() == ConstructorKind::None
            && function_node.borrow().is_strict_mode();
        this.allow_call_ignore_result_optimization = this.default_allow_call_ignore_result_optimization;
        this.needs_to_update_arrow_function_context =
            function_node.borrow().uses_arrow_function() || function_node.borrow().uses_eval();
        this.ecma_mode = ECMAMode::from_bool(function_node.borrow().is_strict_mode());
        this.derived_context_type = base.borrow().derived_context_type();

        // `#if USE(BUN_JSC_ADDITIONS)` m_isPrivateBuiltinFunction: fora (BytecodeGenerator.h usa o ramo #else,
        // `is_private_builtin_function()` devolve `is_builtin_function`).
        let ecma_mode = this.ecma_mode;
        this.push_private_access_names(parent_private_name_environment);

        let function_symbol_table = SymbolTable::create(&this.vm);
        function_symbol_table.borrow_mut().set_uses_sloppy_eval(this.uses_sloppy_eval);
        let mut symbol_table_constant_index: i32 = 0;

        this.cached_parent_tdz = parent_scope_tdz_variables.clone();
        this.generator_or_async_wrapper_function_parameter_names =
            generator_or_async_wrapper_function_parameter_names.map(|names| Rc::new(names.clone()));

        let parameters = function_node.borrow().parameters.clone().expect("FunctionNode sem FunctionParameters");
        // http://www.ecma-international.org/ecma-262/6.0/index.html#sec-functiondeclarationinstantiation
        // This implements IsSimpleParameterList in the Ecma 2015 spec.
        // If IsSimpleParameterList is false, we will create a strict-mode like arguments object.
        // IsSimpleParameterList is false if the argument list contains any default parameter values,
        // a rest parameter, or any destructuring patterns.
        // If we do have default parameters, destructuring parameters, or a rest parameter, our parameters will be allocated in a different scope.
        let is_simple_parameter_list = parameters.borrow().is_simple_parameter_list;

        let parse_mode = base.borrow().parse_mode();

        let contains_arrow_or_eval_but_not_in_arrow_block = ((function_node.borrow().uses_arrow_function()
            && function_node.borrow().do_any_inner_arrow_functions_use_any_feature())
            || this.uses_eval())
            && !this.code_block.is_arrow_function();
        let mut should_capture_some_of_the_things = this.should_emit_debug_hooks()
            || function_node.borrow().needs_activation()
            || contains_arrow_or_eval_but_not_in_arrow_block;

        let mut should_capture_all_of_the_things = this.should_emit_debug_hooks() || this.uses_eval();
        this.needs_arguments = (|| {
            if parse_mode != SourceParseMode::ClassFieldInitializerMode {
                if !base.borrow().is_arrow_function() {
                    if function_node.borrow().uses_arrow_function() && this.is_arguments_used_in_inner_arrow_function() {
                        return true;
                    }
                    if function_node.borrow().uses_arguments() {
                        return true;
                    }
                    if this.should_emit_debug_hooks() {
                        return true;
                    }
                }
                if this.uses_eval() {
                    return true;
                }
            }
            false
        })();

        if is_generator_or_async_function_body_parse_mode(parse_mode) {
            this.needs_generatorification = true;
            // Generator and AsyncFunction never provides "arguments". "arguments" reference will be resolved in an upper generator function scope.
            this.needs_arguments = false;
        } else if is_generator_or_async_function_wrapper_parse_mode(parse_mode) {
            // Generator does not provide "arguments". Instead, wrapping GeneratorFunction provides "arguments".
            // This is because arguments of a generator should be evaluated before starting it.
            // To workaround it, we evaluate these arguments as arguments of a wrapping generator function, and reference it from a generator.
            //
            //    function *gen(a, b = hello())
            //    {
            //        return {
            //            @generatorNext: function (@generator, @generatorState, @generatorValue, @generatorResumeMode, @generatorFrame)
            //            {
            //                arguments;  // This `arguments` should reference to the gen's arguments.
            //                ...
            //            }
            //        }
            //    }
            //
            // For async functions without await, the body is inlined directly - no body function exists.
            // So we don't need to capture parameters for the body function to access them.
            if !(is_async_function_wrapper_parse_mode(parse_mode)
                && function_node.borrow().is_async_function_without_await())
            {
                if this.needs_arguments {
                    should_capture_some_of_the_things = true;
                }
                if parameters.borrow().patterns.len() != 0 {
                    should_capture_some_of_the_things = true;
                    should_capture_all_of_the_things = true;
                }
            }
        }

        if should_capture_all_of_the_things {
            function_node.borrow_mut().base.var_declarations.mark_all_variables_as_captured();
        }

        let needs_arguments = this.needs_arguments;
        let arguments_impl = this.property_names().arguments.impl_();
        let captures = |uid: &crate::wtf::text::string_impl::UniquedKey| -> bool {
            if !should_capture_some_of_the_things {
                return false;
            }
            if needs_arguments && Some(uid) == arguments_impl.as_ref() {
                // Actually, we only need to capture the arguments object when we "need full activation"
                // because of name scopes. But historically we did it this way, so for now we just preserve
                // the old behavior.
                // FIXME: https://bugs.webkit.org/show_bug.cgi?id=143072
                return true;
            }
            function_node.borrow().base.var_declarations.captures(uid)
        };
        let var_kind = |uid: &crate::wtf::text::string_impl::UniquedKey| -> VarKind {
            if captures(uid) {
                VarKind::Scope
            } else {
                VarKind::Stack
            }
        };

        this.callee_register.set_index(VirtualRegister::new(CallFrameSlot::CALLEE));

        this.initialize_parameters(&parameters.borrow());
        debug_assert!(!(is_simple_parameter_list && this.rest_parameter.is_some()));

        this.emit_enter();

        if is_generator_or_async_function_body_parse_mode(parse_mode) {
            this.generator_register = Some(RegisterRef::new(&this.parameters[crate::runtime::js_generator::Argument::Generator as usize]));
        }

        this.allocate_scope();

        match this.constructor_kind() {
            ConstructorKind::None => {}
            ConstructorKind::Naked => {
                if !this.is_constructor() {
                    let constructor_name = function_node.borrow().ident.string().string().clone();
                    if constructor_name.is_null() || constructor_name.is_empty() {
                        this.emit_throw_type_error("Cannot call a constructor without |new|");
                    } else {
                        let error_message_str = crate::wtf::text::try_make_string_dyn(&[
                            &"Cannot call a constructor ",
                            &&constructor_name,
                            &" without |new|",
                        ]);
                        match error_message_str {
                            None => this.emit_throw_type_error("Cannot call a constructor without |new|"),
                            Some(message) => {
                                let identifier = Identifier::from_string(&this.vm, &message);
                                this.emit_throw_type_error_identifier(&identifier)
                            }
                        }
                    }
                    return this;
                }
            }
            ConstructorKind::Base | ConstructorKind::Extends => {
                if !this.is_constructor() {
                    let constructor_name = function_node.borrow().ident.string().string().clone();
                    if constructor_name.is_null() || constructor_name.is_empty() {
                        this.emit_throw_type_error("Cannot call a class constructor without |new|");
                    } else {
                        let error_message_str = crate::wtf::text::try_make_string_dyn(&[
                            &"Cannot call a class constructor ",
                            &&constructor_name,
                            &" without |new|",
                        ]);
                        match error_message_str {
                            None => this.emit_throw_type_error("Cannot call a class constructor without |new|"),
                            Some(message) => {
                                let identifier = Identifier::from_string(&this.vm, &message);
                                this.emit_throw_type_error_identifier(&identifier)
                            }
                        }
                    }
                    return this;
                }
            }
        }

        if function_name_is_in_scope(&function_node.borrow().ident, function_node.borrow().function_mode) {
            debug_assert!(parse_mode != SourceParseMode::GeneratorBodyMode);
            debug_assert!(!is_async_function_body_parse_mode(parse_mode));
            let is_dynamic_scope = function_name_scope_is_dynamic(this.uses_eval(), ecma_mode.is_strict());
            let is_function_name_captured = captures(&function_node.borrow().ident.impl_().unwrap());
            let mark_as_captured = is_dynamic_scope || is_function_name_captured;
            let callee_register = this.callee_register();
            this.emit_push_function_name_scope(&function_node.borrow().ident, Some(callee_register), mark_as_captured);
        }

        if should_capture_some_of_the_things {
            this.lexical_environment_register = Some(this.add_var());
        }

        if is_generator_or_async_function_body_parse_mode(parse_mode)
            || should_capture_some_of_the_things
            || this.should_emit_type_profiler_hooks()
        {
            symbol_table_constant_index = this
                .add_constant_value(
                    crate::runtime::js_value::JSValue::from_cell(function_symbol_table.borrow().cell_id()),
                    crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
                )
                .borrow()
                .index();
        }

        // We can allocate the "var" environment if we don't have default parameter expressions. If we have
        // default parameter expressions, we have to hold off on allocating the "var" environment because
        // the parent scope of the "var" environment is the parameter environment.
        if is_simple_parameter_list {
            this.initialize_var_lexical_environment(
                symbol_table_constant_index,
                &function_symbol_table,
                should_capture_some_of_the_things,
            );
        }

        // Need to know what our functions are called. Parameters have some goofy behaviors when it
        // comes to functions of the same name.
        for function in function_node.borrow().base.variable_environment.function_stack.iter() {
            this.functions.insert(function.ident.borrow().impl_().unwrap());
        }

        if this.needs_arguments {
            // Create the arguments object now. We may put the arguments object into the activation if
            // it is captured. Either way, we create two arguments object variables: one is our
            // private variable that is immutable, and another that is the user-visible variable. The
            // immutable one is only used here, or during formal parameter resolutions if we opt for
            // DirectArguments.

            let arguments_register = this.add_var();
            arguments_register.borrow_mut().ref_();
            this.arguments_register = Some(arguments_register);
        }

        if this.needs_arguments && !ecma_mode.is_strict() && is_simple_parameter_list {
            // If we captured any formal parameter by name, then we use ScopedArguments. Otherwise we
            // use DirectArguments. With ScopedArguments, we lift all of our arguments into the
            // activation.
            let mut captures_any_parameter_by_name = false;
            if function_node.borrow().base.var_declarations.has_captured_variables() {
                for i in 0..parameters.borrow().patterns.len() {
                    let pattern = parameters.borrow().patterns[i].0.clone();
                    debug_assert!(pattern.is_binding_node());
                    let ident = binding_bound_property(&pattern);
                    if captures(&ident.impl_().unwrap()) {
                        captures_any_parameter_by_name = true;
                        break;
                    }
                }
            }

            if captures_any_parameter_by_name {
                debug_assert!(this.lexical_environment_register.is_some());
                let success = function_symbol_table
                    .borrow_mut()
                    .try_set_arguments_length(&this.vm, parameters.borrow().patterns.len() as u32);
                if !success {
                    this.out_of_memory_during_construction = true;
                    return this;
                }

                // For each parameter, we have two possibilities:
                // Either it's a binding node with no function overlap, in which case it gets a name
                // in the symbol table - or it just gets space reserved in the symbol table. Either
                // way we lift the value into the scope.
                for i in 0..parameters.borrow().patterns.len() as u32 {
                    let offset = function_symbol_table.borrow_mut().take_next_scope_offset();
                    let success = function_symbol_table
                        .borrow_mut()
                        .try_set_argument_offset(&this.vm, i, offset);
                    if !success {
                        this.out_of_memory_during_construction = true;
                        return this;
                    }

                    let mut var_or_anonymous: u32 = u32::MAX;

                    let pattern = parameters.borrow().patterns[i as usize].0.clone();
                    if let Some(name) = this.visible_name_for_parameter(&pattern) {
                        let var_offset = VarOffset::from_scope_offset(offset);
                        let entry = SymbolTableEntry::from_var_offset(var_offset);
                        function_symbol_table.borrow_mut().set(name, entry);

                        let ident = binding_bound_property(&pattern);

                        var_or_anonymous = this.add_constant(&ident);
                    }

                    let lexical_environment_register = this.lexical_environment_register.clone();
                    OpPutToScope::emit(
                        &mut this,
                        lexical_environment_register.as_ref().unwrap(),
                        var_or_anonymous,
                        virtual_register_for_argument_including_this(1 + i as i32, 0),
                        GetPutInfo::new(
                            ResolveMode::ThrowIfNotFound,
                            ResolveType::ResolvedClosureVar,
                            InitializationMode::ScopedArgumentInitialization,
                            ecma_mode,
                        ),
                        SymbolTableOrScopeDepth::symbol_table(VirtualRegister::new(symbol_table_constant_index)),
                        offset.offset(),
                    );
                }

                // This creates a scoped arguments object and copies the overflow arguments into the
                // scope. It's the equivalent of calling ScopedArguments::createByCopying().
                let arguments_register = this.arguments_register.clone();
                let lexical_environment_register = this.lexical_environment_register.clone();
                OpCreateScopedArguments::emit(
                    &mut this,
                    arguments_register.as_ref().unwrap(),
                    lexical_environment_register.as_ref().unwrap(),
                );
            } else {
                // We're going to put all parameters into the DirectArguments object. First ensure
                // that the symbol table knows that this is happening.
                for i in 0..parameters.borrow().patterns.len() {
                    let pattern = parameters.borrow().patterns[i].0.clone();
                    if let Some(name) = this.visible_name_for_parameter(&pattern) {
                        function_symbol_table.borrow_mut().set(
                            name,
                            SymbolTableEntry::from_var_offset(VarOffset::from_direct_arguments_offset(DirectArgumentsOffset::new(
                                i as u32,
                            ))),
                        );
                    }
                }

                let arguments_register = this.arguments_register.clone();
                OpCreateDirectArguments::emit(&mut this, arguments_register.as_ref().unwrap());
            }
        } else if is_simple_parameter_list {
            // Create the formal parameters the normal way. Any of them could be captured, or not. If
            // captured, lift them into the scope. We cannot do this if we have default parameter expressions
            // because when default parameter expressions exist, they belong in their own lexical environment
            // separate from the "var" lexical environment.
            for i in 0..parameters.borrow().patterns.len() {
                let pattern = parameters.borrow().patterns[i].0.clone();
                let name = this.visible_name_for_parameter(&pattern);
                let name = match name {
                    Some(name) => name,
                    None => continue,
                };

                if !captures(&name) {
                    // This is the easy case - just tell the symbol table about the argument. It will
                    // be accessed directly.
                    function_symbol_table.borrow_mut().set(
                        name,
                        SymbolTableEntry::from_var_offset(VarOffset::from_virtual_register(
                            virtual_register_for_argument_including_this(1 + i as i32, 0),
                        )),
                    );
                    continue;
                }

                let offset = function_symbol_table.borrow_mut().take_next_scope_offset();
                function_symbol_table
                    .borrow_mut()
                    .set(name, SymbolTableEntry::from_var_offset(VarOffset::from_scope_offset(offset)));
                let ident = binding_bound_property(&pattern);

                let lexical_environment_register = this.lexical_environment_register.clone();
                let constant = this.add_constant(&ident);
                OpPutToScope::emit(
                    &mut this,
                    lexical_environment_register.as_ref().unwrap(),
                    constant,
                    virtual_register_for_argument_including_this(1 + i as i32, 0),
                    GetPutInfo::new(
                        ResolveMode::ThrowIfNotFound,
                        ResolveType::ResolvedClosureVar,
                        InitializationMode::NotInitialization,
                        ecma_mode,
                    ),
                    SymbolTableOrScopeDepth::symbol_table(VirtualRegister::new(symbol_table_constant_index)),
                    offset.offset(),
                );
            }
        }

        if this.needs_arguments && (ecma_mode.is_strict() || !is_simple_parameter_list) {
            // Allocate a cloned arguments object.
            let arguments_register = this.arguments_register.clone();
            OpCreateClonedArguments::emit(&mut this, arguments_register.as_ref().unwrap());
        }

        // There are some variables that need to be preinitialized to something other than Undefined:
        //
        // - "arguments": unless it's used as a function or parameter, this should refer to the
        //   arguments object.
        //
        // - functions: these always override everything else.
        //
        // The most logical way to do all of this is to initialize none of the variables until now,
        // and then initialize them in BytecodeGenerator::generate() in such an order that the rules
        // for how these things override each other end up holding. We would initialize "arguments" first,
        // then all arguments, then the functions.
        //
        // But some arguments are already initialized by default, since if they aren't captured and we
        // don't have "arguments" then we just point the symbol table at the stack slot of those
        // arguments. We end up initializing the rest of the arguments that have an uncomplicated
        // binding (i.e. don't involve destructuring) above when figuring out how to lay them out,
        // because that's just the simplest thing. This means that when we initialize them, we have to
        // watch out for the things that override arguments (namely, functions).

        // This is our final act of weirdness. "arguments" is overridden by everything except the
        // callee. We add it to the symbol table if it's not already there and it's not an argument.
        let mut should_create_arguments_variable_in_parameter_scope = false;
        if this.needs_arguments {
            // If "arguments" is overridden by a function or destructuring parameter name, then it's
            // OK for us to call createVariable() because it won't change anything. It's also OK for
            // us to them tell BytecodeGenerator::generate() to write to it because it will do so
            // before it initializes functions and destructuring parameters. But if "arguments" is
            // overridden by a "simple" function parameter, then we have to bail: createVariable()
            // would assert and BytecodeGenerator::generate() would write the "arguments" after the
            // argument value had already been properly initialized.

            let mut have_parameter_named_arguments = false;
            for i in 0..parameters.borrow().patterns.len() {
                let pattern = parameters.borrow().patterns[i].0.clone();
                let name = this.visible_name_for_parameter(&pattern);
                if name == arguments_impl {
                    have_parameter_named_arguments = true;
                    break;
                }
            }

            let should_create_argumen_variable = !have_parameter_named_arguments
                && !SourceParseModeSet::new(&[
                    SourceParseMode::ArrowFunctionMode,
                    SourceParseMode::AsyncArrowFunctionMode,
                    SourceParseMode::ClassFieldInitializerMode,
                ])
                .contains(this.code_block.parse_mode());
            should_create_arguments_variable_in_parameter_scope =
                should_create_argumen_variable && !is_simple_parameter_list;
            // Do not create arguments variable in case of Arrow function. Value will be loaded from parent scope
            if should_create_argumen_variable && !should_create_arguments_variable_in_parameter_scope {
                let arguments_identifier = this.property_names().arguments.clone();
                this.create_variable(
                    &arguments_identifier,
                    var_kind(&arguments_identifier.impl_().unwrap()),
                    &function_symbol_table,
                    ExistingVariableMode::VerifyExisting,
                );

                this.need_to_initialize_arguments = true;
            }
        }

        for function in function_node.borrow().base.variable_environment.function_stack.iter() {
            let ident = function.ident.borrow().clone();
            this.create_variable(
                &ident,
                var_kind(&ident.impl_().unwrap()),
                &function_symbol_table,
                ExistingVariableMode::VerifyExisting,
            );
            this.functions_to_initialize
                .push((function.clone(), FunctionVariableType::NormalFunctionVariable));
        }
        for entry in function_node.borrow().base.var_declarations.iter() {
            debug_assert!(!entry.1.is_let() && !entry.1.is_const());
            if !entry.1.is_var() {
                // This is either a parameter or callee.
                continue;
            }
            if should_create_arguments_variable_in_parameter_scope && Some(&entry.0) == arguments_impl.as_ref() {
                continue;
            }
            if let Some(names) = generator_or_async_wrapper_function_parameter_names {
                if names.iter().any(|name| name.impl_().as_ref() == Some(&entry.0)) {
                    continue;
                }
            }
            this.create_variable(
                &Identifier::from_uid(&this.vm, Some(&entry.0)),
                var_kind(&entry.0),
                &function_symbol_table,
                ExistingVariableMode::IgnoreExisting,
            );
        }

        if function_node.borrow().needs_new_target_register_for_this_scope()
            || this.is_new_target_used_in_inner_arrow_function()
            || this.uses_eval()
        {
            this.new_target_register = Some(this.add_var());
        }

        let should_emit_to_this = |this: &BytecodeGenerator| -> bool {
            if function_node.borrow().uses_this()
                || this.uses_eval()
                || this.scope_node.borrow().do_any_inner_arrow_functions_use_this()
                || this.scope_node.borrow().do_any_inner_arrow_functions_use_eval()
            {
                return true;
            }
            if (function_node.borrow().uses_super_property()
                || this.scope_node.borrow().do_any_inner_arrow_functions_use_super_property())
                && !ecma_mode.is_strict()
            {
                // We must emit to_this when we're not in strict mode because we
                // will convert |this| to an object, and that object may be passed
                // to a strict function as |this|. This is observable because that
                // strict function's to_this will just return the object.
                //
                // We don't need to emit this for strict-mode code because
                // strict-mode code may call another strict function, which will
                // to_this if it directly uses this; this is OK, because we defer
                // to_this until |this| is used directly. Strict-mode code might
                // also call a sloppy mode function, and that will to_this, which
                // will defer the conversion, again, until necessary.
                return true;
            }
            false
        };

        match parse_mode {
            SourceParseMode::GeneratorWrapperFunctionMode
            | SourceParseMode::GeneratorWrapperMethodMode
            | SourceParseMode::AsyncGeneratorWrapperMethodMode
            | SourceParseMode::AsyncGeneratorWrapperFunctionMode => {
                this.generator_register = Some(this.add_var());

                // FIXME: Emit to_this only when Generator uses it.
                // https://bugs.webkit.org/show_bug.cgi?id=151586
                this.emit_to_this_this_register();
            }

            SourceParseMode::AsyncArrowFunctionMode
            | SourceParseMode::AsyncMethodMode
            | SourceParseMode::AsyncFunctionMode => {
                debug_assert!(!this.is_constructor());
                debug_assert!(this.constructor_kind() == ConstructorKind::None);

                let is_async_function_without_await = this.scope_node.borrow().is_async_function_without_await();
                // Check if this async function body doesn't use await.
                // If so, we can skip generator creation entirely.
                if !is_async_function_without_await {
                    this.generator_register = Some(this.add_var());
                    this.promise_register = Some(this.add_var());
                }

                let mut will_emit_to_this = false;
                if parse_mode != SourceParseMode::AsyncArrowFunctionMode {
                    // FIXME: Emit to_this only when AsyncFunctionBody uses it.
                    // https://bugs.webkit.org/show_bug.cgi?id=151586
                    if is_async_function_without_await {
                        will_emit_to_this = should_emit_to_this(&this);
                    } else {
                        will_emit_to_this = true;
                    }
                }
                if will_emit_to_this {
                    this.emit_to_this_this_register();
                }

                if !is_async_function_without_await {
                    let promise_register = this.promise_register();
                    this.emit_new_promise(promise_register);
                    let generator_register = this.generator_register.clone();
                    this.emit_new_async_function_generator(generator_register);
                    let generator_register = this.generator_register();
                    let promise_register = this.promise_register();
                    this.emit_put_internal_field(
                        generator_register,
                        crate::runtime::js_async_function_generator::Field::Context as u32,
                        promise_register,
                    );
                }
            }

            SourceParseMode::AsyncGeneratorBodyMode
            | SourceParseMode::AsyncFunctionBodyMode
            | SourceParseMode::AsyncArrowFunctionBodyMode
            | SourceParseMode::GeneratorBodyMode => {
                // |this| is already filled correctly before here.
                if let Some(new_target_register) = this.new_target_register.clone() {
                    this.emit_load_js_value(Some(new_target_register), crate::runtime::js_value::JSValue::Undefined);
                }
            }

            SourceParseMode::ArrowFunctionMode => {}

            _ => {
                if this.is_constructor() {
                    let this_register = this.this_register();
                    if let Some(new_target_register) = this.new_target_register.clone() {
                        this.move_register(Some(&new_target_register), &this_register);
                    }
                    match this.constructor_kind() {
                        ConstructorKind::Naked => {
                            // Naked constructor not create |this| automatically.
                        }
                        ConstructorKind::None | ConstructorKind::Base => {
                            this.emit_create_this(Some(this_register.clone()));
                            if this.private_brand_requirement() == PrivateBrandRequirement::Needed {
                                this.emit_install_private_brand(Some(this_register.clone()));
                            }

                            let callee_register = this.callee_register();
                            let position = *this.scope_node.borrow().position();
                            this.emit_instance_field_initialization_if_needed(
                                Some(this_register.clone()),
                                Some(callee_register),
                                &position,
                                &position,
                                &position,
                            );
                        }
                        ConstructorKind::Extends => {
                            this.move_empty_value(Some(this_register));
                        }
                    }
                } else {
                    match this.constructor_kind() {
                        ConstructorKind::None => {
                            if should_emit_to_this(&this) {
                                this.emit_to_this_this_register();
                            }
                        }
                        ConstructorKind::Naked | ConstructorKind::Base | ConstructorKind::Extends => {
                            unreachable!("RELEASE_ASSERT_NOT_REACHED");
                        }
                    }
                }
            }
        }

        // We need load |super| & |this| for arrow function before initializeDefaultParameterValuesAndSetupFunctionScopeStack
        // if we have default parameter expression. Because |super| & |this| values can be used there
        if (SourceParseModeSet::new(&[SourceParseMode::ArrowFunctionMode, SourceParseMode::AsyncArrowFunctionMode])
            .contains(parse_mode)
            && !is_simple_parameter_list)
            || parse_mode == SourceParseMode::AsyncArrowFunctionBodyMode
        {
            if function_node.borrow().uses_this() || function_node.borrow().uses_super_property() {
                this.emit_load_this_from_arrow_function_lexical_environment();
            }

            if this.scope_node.borrow().needs_new_target_register_for_this_scope() {
                this.emit_load_new_target_from_arrow_function_lexical_environment();
            }
        }

        if this.needs_to_update_arrow_function_context() && !base.borrow().is_arrow_function() {
            let can_reuse_lexical_environment = is_simple_parameter_list;
            this.initialize_arrow_function_context_scope_if_needed(
                Some(&function_symbol_table),
                can_reuse_lexical_environment,
            );
            this.emit_put_this_to_arrow_function_context_scope();
            this.emit_put_new_target_to_arrow_function_context_scope();
            this.emit_put_derived_constructor_to_arrow_function_context_scope();
        }

        if is_async_function_wrapper_parse_mode(parse_mode) && !is_simple_parameter_list {
            let catch_start_label = this.new_label();
            let thrown_value = this.new_temporary();
            this.async_func_parameters_try_catch_info = Some(AsyncFuncParametersTryCatchInfo {
                catch_start_label: Some(catch_start_label),
                thrown_value: Some(thrown_value),
            });
        }

        this.async_func_parameters_try_catch_wrap(|generator| {
            // All "addVar()"s needs to happen before "initializeDefaultParameterValuesAndSetupFunctionScopeStack()" is called
            // because a function's default parameter ExpressionNodes will use temporary registers.
            generator.initialize_default_parameter_values_and_setup_function_scope_stack(
                &parameters.borrow(),
                is_simple_parameter_list,
                &function_node,
                &function_symbol_table,
                symbol_table_constant_index,
                &captures,
                should_create_arguments_variable_in_parameter_scope,
            );
        });

        // If we don't have  default parameter expression, then loading |this| inside an arrow function must be done
        // after initializeDefaultParameterValuesAndSetupFunctionScopeStack() because that function sets up the
        // SymbolTable stack and emitLoadThisFromArrowFunctionLexicalEnvironment() consults the SymbolTable stack
        if SourceParseModeSet::new(&[SourceParseMode::ArrowFunctionMode, SourceParseMode::AsyncArrowFunctionMode])
            .contains(parse_mode)
            && is_simple_parameter_list
        {
            if function_node.borrow().uses_this() || function_node.borrow().uses_super_property() {
                this.emit_load_this_from_arrow_function_lexical_environment();
            }

            if this.scope_node.borrow().needs_new_target_register_for_this_scope() {
                this.emit_load_new_target_from_arrow_function_lexical_environment();
            }
        }

        if is_generator_wrapper_parse_mode(parse_mode) {
            let generator_register = this.generator_register.clone();
            let callee_register = this.callee_register();
            this.emit_create_generator(generator_register, Some(callee_register));
        } else if is_async_generator_wrapper_parse_mode(parse_mode) {
            let generator_register = this.generator_register.clone();
            let callee_register = this.callee_register();
            this.emit_create_async_generator(generator_register, Some(callee_register));
        }

        // Set up the lexical environment scope as the generator frame. We store the saved and resumed generator registers into this scope with the symbol keys.
        // Since they are symbol keyed, these variables cannot be reached from the usual code.
        if is_generator_or_async_function_body_parse_mode(parse_mode) {
            this.generator_frame_symbol_table = Some(function_symbol_table.clone());
            this.generator_frame_symbol_table_index = symbol_table_constant_index;
            if let Some(lexical_environment_register) = this.lexical_environment_register.clone() {
                let generator_frame_register = this.generator_frame_register();
                this.move_register(Some(&generator_frame_register), &lexical_environment_register);
            } else {
                // It would be possible that generator does not need to suspend and resume any registers.
                // In this case, we would like to avoid creating a lexical environment as much as possible.
                // op_create_generator_frame_environment is a marker, which is similar to op_yield.
                // Generatorification inserts lexical environment creation if necessary. Otherwise, we convert it to op_mov frame, `undefined`.
                let generator_frame_register = this.generator_frame_register();
                let scope_register = this.scope_register();
                let undefined_constant = this.add_constant_value(
                    crate::runtime::js_value::JSValue::Undefined,
                    crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
                );
                OpCreateGeneratorFrameEnvironment::emit(
                    &mut this,
                    &generator_frame_register,
                    &scope_register,
                    VirtualRegister::new(symbol_table_constant_index),
                    undefined_constant,
                );
            }
            const _: () = assert!(
                crate::runtime::js_generator::Field::Frame as u32
                    == crate::runtime::js_async_generator::Field::Frame as u32
            );
            let generator_register = this.generator_register();
            let generator_frame_register = this.generator_frame_register();
            this.emit_put_internal_field(
                generator_register,
                crate::runtime::js_generator::Field::Frame as u32,
                Some(generator_frame_register),
            );
        }

        let should_initialize_block_scoped_functions = false; // We generate top-level function declarations in ::generate().
        let scope_node = this.scope_node.clone();
        this.push_lexical_scope(
            &scope_node.borrow().variable_environment,
            ScopeType::LetConstScope,
            TDZCheckOptimization::Optimize,
            NestedScopeType::IsNotNested,
            None,
            should_initialize_block_scoped_functions,
        );
        this
    }
}
