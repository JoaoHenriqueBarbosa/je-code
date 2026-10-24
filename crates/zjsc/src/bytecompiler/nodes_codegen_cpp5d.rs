// NodesCodegen.cpp, linhas 5193 a 5532: `FunctionNode::emitBytecode` inteiro (incluído por include!,
// sem `use`). A função cruza a linha 5400 e por isso foi terminada aqui; `FuncDeclNode` e
// `FuncExprNode` (a partir da linha 5534) ficam para a fatia seguinte.
//
// Convenções: as mesmas da cpp5b/cpp5c (`Cpp5bReg` é o `RegisterID*` anulável). `emitLoad(dst,
// jsNumber(n))` é `emit_load_js_value(dst, js_number_i32(n))`. O `fallthrough` de
// `GeneratorBodyMode` para o corpo de `AsyncFunctionBodyMode` vira a função
// `cpp5d_emit_body_and_implicit_return`. A ordem de avaliação dos argumentos do C++ (clang,
// esquerda para direita) é preservada onde ela aloca registro ou emite instrução.
//
// Dependências conferidas por grep em 2026-10-08, todas existem:
// - `runtime/js_generator.rs`: `Field::{This, Next, State}`, `State::{Init, Executing}`,
//   `ResumeMode::{NormalMode, ThrowMode}`.
// - `bytecode/bytecode_intrinsics_table.rs`: `LinkTimeConstant::{NewResolvedPromise, NewRejectedPromise,
//   ResolvePromiseWithFirstResolvingFunctionCallCheck, RejectPromiseWithFirstResolvingFunctionCallCheck,
//   AsyncFunctionDrive}`.
// - `interpreter/interpreter.rs`: `DebugHookType::{DidEnterCallFrame, WillLeaveCallFrame}`.
// - `interpreter/call_frame.rs:531`: `argument_offset(i32) -> i32` (o tipo `i32` está confirmado).
// - `BytecodeGenerator::{emit_profile_type_flag_divots, emit_profile_control_flow, emit_debug_hook,
//   emit_will_leave_call_frame_debug_hook, emit_load_this_from_arrow_function_lexical_environment}`.
// - `ScopeNode::{start_line, start_start_offset, start_line_start_offset, using_declaration_count,
//   has_await_using_declaration, emit_statements_bytecode, single_statement, is_empty_body}`.
// - `StatementNode::last_line()` devolve `u32`; o cast para `i32` do `JSTextPosition::new` é meu.
// - `FunctionNode.parameters` como `Option<NodeRef<FunctionParameters>>` (o C++ nunca o tem nulo
//   aqui; `expect` é o `m_parameters->`).
// - `Statement::{ExprStatement, Block}` com `expr` e `last_statement()`, e `Expression: Clone`.

/// `emitBodyWithUsingIfNeeded(...) { emitStatementsBytecode(generator, generator.ignoredResult()); }`,
/// as quatro cópias do corpo em `FunctionNode::emitBytecode`.
fn cpp5d_emit_statements_with_using(
    function: &crate::parser::nodes::FunctionNode,
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
) {
    generator.emit_body_with_using_if_needed(
        function.base.variable_environment.using_declaration_count(),
        function.base.variable_environment.has_await_using_declaration(),
        &mut |generator| {
            let ignored_result = Some(generator.ignored_result());
            function.base.emit_statements_bytecode(generator, ignored_result);
        },
    );
}

/// `generator.move(dst, src)` com `src` anulável (o `argumentRegister(i)` do `CallArguments`).
fn cpp5d_move(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    dst: Cpp5bReg,
    src: &Cpp5bReg,
) {
    generator.move_register(dst.as_ref(), src.as_ref().expect("RegisterID"));
}

/// O `FuncExprNode` do único `ExprStatementNode` do corpo de uma função wrapper (generator ou async).
fn cpp5d_wrapper_function_expression(
    function: &crate::parser::nodes::FunctionNode,
) -> crate::parser::nodes::Expression {
    let single_statement = function.base.single_statement().expect("singleStatement");
    let crate::parser::nodes::Statement::ExprStatement(expr_statement) = single_statement else {
        unreachable!("ASSERT(singleStatement->isExprStatement())");
    };
    let expr = expr_statement.borrow().expr.clone();
    debug_assert!(expr.is_func_expr_node());
    expr
}

/// O ramo `AsyncFunctionBodyMode`/`AsyncArrowFunctionBodyMode` (e o corpo do generator, por
/// `[[fallthrough]]`): o corpo e o retorno implícito de `undefined`.
fn cpp5d_emit_body_and_implicit_return(
    function: &crate::parser::nodes::FunctionNode,
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
) {
    cpp5d_emit_statements_with_using(function, generator);

    let undefined = generator.emit_load_js_value(None, crate::runtime::js_value::js_undefined());
    generator.emit_return(undefined);
}

impl crate::parser::nodes::FunctionNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp5bReg,
    ) {
        use crate::bytecode::handler_info::HandlerType;
        use crate::interpreter::interpreter::DebugHookType;
        use crate::bytecompiler::bytecode_generator::{CallArguments, CompletionType, DebuggableCall, ExpectedFunction};
        use crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag;
        use crate::parser::parser_modes::SourceParseMode;
        use crate::parser::parser_tokens::JSTextPosition;
        use crate::runtime::js_value::{js_number_i32, js_undefined};

        if generator.should_emit_type_profiler_hooks() {
            // If the parameter list is non simple one, it is handled in bindValue's code.
            let parameters = self.parameters.as_ref().expect("m_parameters").borrow();
            if parameters.is_simple_parameter_list {
                for (i, parameter) in parameters.patterns.iter().enumerate() {
                    let crate::parser::nodes::DestructuringPatternNode::Binding(binding_node) = &parameter.0 else {
                        unreachable!("static_cast<BindingNode*> de lista de parâmetros simples");
                    };
                    let binding_node = binding_node.borrow();
                    let reg = std::rc::Rc::new(std::cell::RefCell::new(
                        crate::bytecompiler::register_id::RegisterID::from_index(
                            crate::interpreter::call_frame::argument_offset(i as i32),
                        ),
                    ));
                    generator.emit_profile_type_flag_divots(
                        Some(crate::bytecompiler::bytecode_generator::RegisterRef::new(&reg)),
                        ProfileTypeBytecodeFlag::ProfileTypeBytecodeFunctionArgument,
                        &binding_node.divot_start,
                        &binding_node.divot_end,
                    );
                }
            }
        }

        generator.emit_profile_control_flow(self.base.start_start_offset());
        generator.emit_debug_hook(
            DebugHookType::DidEnterCallFrame,
            &JSTextPosition::new(
                self.base.start_line(),
                self.base.start_start_offset(),
                self.base.start_line_start_offset(),
            ),
            None,
        );

        match generator.parse_mode() {
            SourceParseMode::GeneratorWrapperFunctionMode
            | SourceParseMode::GeneratorWrapperMethodMode
            | SourceParseMode::AsyncGeneratorWrapperMethodMode
            | SourceParseMode::AsyncGeneratorWrapperFunctionMode => {
                let func_expr = cpp5d_wrapper_function_expression(self);

                let next = Some(generator.new_temporary());
                generator.emit_node_expression(next.clone(), &func_expr);

                if generator.super_binding() == crate::parser::parser_modes::SuperBinding::Needed {
                    let home_object = emit_home_object_for_callee(generator);
                    emit_put_home_object(
                        generator,
                        next.as_ref().expect("next"),
                        home_object.as_ref().expect("homeObject"),
                    );
                }

                if crate::parser::parser_modes::is_generator_wrapper_parse_mode(generator.parse_mode()) {
                    generator.emit_put_generator_fields(next);
                } else {
                    debug_assert!(crate::parser::parser_modes::is_async_generator_wrapper_parse_mode(
                        generator.parse_mode()
                    ));
                    generator.emit_put_async_generator_fields(next);
                }

                generator.emit_debug_hook(
                    DebugHookType::WillLeaveCallFrame,
                    &JSTextPosition::new(
                        self.base.last_line() as i32,
                        self.base.start_offset(),
                        self.base.line_start_offset(),
                    ),
                    None,
                );
                let generator_register = generator.generator_register();
                generator.emit_return(generator_register);
            }

            SourceParseMode::AsyncFunctionMode
            | SourceParseMode::AsyncMethodMode
            | SourceParseMode::AsyncArrowFunctionMode => {
                let divot = JSTextPosition::new(
                    self.base.first_line(),
                    self.base.start_offset(),
                    self.base.line_start_offset(),
                );

                if generator.is_async_function_without_await() {
                    // async function without await. In this case, we fully inline entire body into wrapper function since there is no need to resume.
                    // This mode optimizes async function in several ways.
                    //
                    // 1. Do not allocate body function.
                    // 2. Due to (2), arguments are not specially captured.
                    // 3. Generator is not created because we do not need suspend and resume.
                    //
                    // We use try-catch-finally to handle the async function semantics:
                    // - try: Execute body statements
                    // - catch: Reject promise with the exception
                    // - finally: Resolve promise with completion value and return promise
                    //
                    // try {
                    //     body
                    //     transfer completion-value with completion-type = normal / return
                    // } catch (error) {
                    //     transfer error with completion-type = throw
                    // } finally {
                    //     get value with completion-type
                    //     if (completion-type === throw)
                    //         return @newRejectedPromise(completion-value);
                    //     return @newResolvedPromise(completion-value);
                    // }
                    if generator.parse_mode() == SourceParseMode::AsyncArrowFunctionMode
                        && generator.is_this_used_in_inner_arrow_function()
                    {
                        generator.emit_load_this_from_arrow_function_lexical_environment();
                    }

                    // If async function is just used for a signaling, not having a body, then just convert it to @newResolvedPromise(undefined).
                    //
                    //     Turn this: async function empty() { }
                    //     Into this: function empty() { return @newResolvedPromise(undefined); }
                    //
                    if self.base.is_empty_body() {
                        debug_assert!(self.base.variable_environment.using_declaration_count() == 0);
                        debug_assert!(!self.base.variable_environment.has_await_using_declaration());
                        let new_resolved_promise = generator.move_link_time_constant(
                            None,
                            crate::bytecode::link_time_constant::LinkTimeConstant::NewResolvedPromise,
                        );
                        let mut resolve_args = CallArguments::new(generator, None, 1);
                        generator.emit_load_js_value(resolve_args.this_register(), js_undefined());
                        generator.emit_load_js_value(resolve_args.argument_register(0), js_undefined());
                        let result = Some(generator.new_temporary());
                        generator.emit_call(
                            result.clone(),
                            new_resolved_promise,
                            ExpectedFunction::NoExpectedFunction,
                            &mut resolve_args,
                            &divot,
                            &divot,
                            &divot,
                            DebuggableCall::No,
                        );
                        generator.emit_will_leave_call_frame_debug_hook();
                        generator.emit_return(result);
                        return;
                    }

                    // Set up finally context to capture return values from the body.
                    // When a return statement is hit, it stores the value in completionValueRegister
                    // and jumps to the finally block.
                    let finally_label = generator.new_label();
                    let finally_context = std::rc::Rc::new(std::cell::RefCell::new(
                        crate::bytecompiler::bytecode_generator::FinallyContext::new(generator, finally_label.clone()),
                    ));
                    generator.push_finally_control_flow_scope(&finally_context);
                    let completion_value_register = finally_context.borrow().completion_value_register();
                    let completion_type_register = finally_context.borrow().completion_type_register();
                    generator.emit_load_js_value(completion_value_register.clone(), js_undefined());

                    // Try block. FinallyContext routes `return V` to finally with completionType=Return
                    // and completionValue=V. Normal fallthrough keeps completionType=Normal (its initial value).
                    let catch_label = generator.new_label();
                    let try_start_label = generator.new_emitted_label();
                    let try_data = generator.push_try(&try_start_label, &catch_label, HandlerType::Finally);
                    cpp5d_emit_statements_with_using(self, generator);
                    generator.emit_jump(&finally_label);
                    let try_end_label = generator.new_emitted_label();
                    generator.pop_try(&try_data, &try_end_label);

                    // Catch handler. Runtime populates completionValueRegister with the thrown value
                    // and completionTypeRegister with CompletionType::Throw.
                    generator.emit_label(&catch_label);
                    generator.emit_out_of_line_catch_handler(
                        completion_value_register.clone(),
                        completion_type_register.clone(),
                        Some(&try_data),
                    );

                    generator.pop_finally_control_flow_scope();

                    // Dispatch on completionType: Throw -> rejected, else -> resolved.
                    generator.emit_label(&finally_label);
                    {
                        let resolve_case = generator.new_label();
                        let throw_type = generator.emit_load_js_value(None, js_number_i32(CompletionType::THROW.0));
                        let condition_register = Some(generator.new_temporary());
                        let condition = generator.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                            condition_register,
                            completion_type_register.clone(),
                            throw_type,
                        );
                        generator.emit_jump_if_false_raw(condition.as_ref().expect("condition"), &resolve_case);

                        {
                            let new_rejected_promise = generator.move_link_time_constant(
                                None,
                                crate::bytecode::link_time_constant::LinkTimeConstant::NewRejectedPromise,
                            );
                            let mut reject_args = CallArguments::new(generator, None, 1);
                            generator.emit_load_js_value(reject_args.this_register(), js_undefined());
                            cpp5d_move(generator, reject_args.argument_register(0), &completion_value_register);
                            let result = Some(generator.new_temporary());
                            generator.emit_call(
                                result.clone(),
                                new_rejected_promise,
                                ExpectedFunction::NoExpectedFunction,
                                &mut reject_args,
                                &divot,
                                &divot,
                                &divot,
                                DebuggableCall::No,
                            );
                            generator.emit_will_leave_call_frame_debug_hook();
                            generator.emit_return(result);
                        }

                        generator.emit_label(&resolve_case);
                        {
                            let new_resolved_promise = generator.move_link_time_constant(
                                None,
                                crate::bytecode::link_time_constant::LinkTimeConstant::NewResolvedPromise,
                            );
                            let mut resolve_args = CallArguments::new(generator, None, 1);
                            generator.emit_load_js_value(resolve_args.this_register(), js_undefined());
                            cpp5d_move(generator, resolve_args.argument_register(0), &completion_value_register);
                            let result = Some(generator.new_temporary());
                            generator.emit_call(
                                result.clone(),
                                new_resolved_promise,
                                ExpectedFunction::NoExpectedFunction,
                                &mut resolve_args,
                                &divot,
                                &divot,
                                &divot,
                                DebuggableCall::No,
                            );
                            generator.emit_will_leave_call_frame_debug_hook();
                            generator.emit_return(result);
                        }
                    }
                    return;
                }

                // Full async function path with body function and generator infrastructure.
                let func_expr = cpp5d_wrapper_function_expression(self);

                let next = Some(generator.new_temporary());
                generator.emit_node_expression(next.clone(), &func_expr);

                if generator.super_binding() == crate::parser::parser_modes::SuperBinding::Needed
                    || (generator.parse_mode() == SourceParseMode::AsyncArrowFunctionMode
                        && generator.is_super_used_in_inner_arrow_function())
                {
                    let home_object = emit_home_object_for_callee(generator);
                    emit_put_home_object(
                        generator,
                        next.as_ref().expect("next"),
                        home_object.as_ref().expect("homeObject"),
                    );
                }

                if generator.parse_mode() == SourceParseMode::AsyncArrowFunctionMode
                    && generator.is_this_used_in_inner_arrow_function()
                {
                    generator.emit_load_this_from_arrow_function_lexical_environment();
                }

                // We do not store 'this' in arrow function within constructor,
                // because it might be not initialized, if super is called later.
                let generator_this: Cpp5bReg;
                if !(generator.is_derived_constructor_context()
                    && generator.parse_mode() == SourceParseMode::AsyncArrowFunctionMode)
                {
                    generator_this = Some(generator.this_register());
                    let generator_register = generator.generator_register();
                    generator.emit_put_internal_field(
                        generator_register,
                        crate::runtime::js_generator::Field::This as u32,
                        generator_this.clone(),
                    );
                } else {
                    generator_this = generator.emit_load_js_value(None, js_undefined());
                }

                let generator_register = generator.generator_register();
                generator.emit_put_internal_field(
                    generator_register.clone(),
                    crate::runtime::js_generator::Field::Next as u32,
                    next.clone(),
                );
                let executing_state = generator.emit_load_js_value(
                    None,
                    js_number_i32(crate::runtime::js_generator::State::Executing as i32),
                );
                generator.emit_put_internal_field(
                    generator_register.clone(),
                    crate::runtime::js_generator::Field::State as u32,
                    executing_state,
                );

                let try_start_label = generator.new_emitted_label();
                let catch_label = generator.new_label();
                let success_label = generator.new_label();
                let drive_label = generator.new_label();

                let try_data = generator.push_try(&try_start_label, &catch_label, HandlerType::SynthesizedCatch);

                let mut next_result = Some(generator.new_temporary());
                {
                    let mut next_args = CallArguments::new(generator, None, 5);
                    cpp5d_move(generator, next_args.this_register(), &generator_this);
                    cpp5d_move(generator, next_args.argument_register(0), &generator_register);
                    generator.emit_load_js_value(
                        next_args.argument_register(1),
                        js_number_i32(crate::runtime::js_generator::State::Init as i32),
                    );
                    generator.emit_load_js_value(next_args.argument_register(2), js_undefined());
                    generator.emit_load_js_value(
                        next_args.argument_register(3),
                        js_number_i32(crate::runtime::js_generator::ResumeMode::NormalMode as i32),
                    );
                    generator.emit_load_js_value(next_args.argument_register(4), js_undefined());

                    next_result = generator.emit_call(
                        next_result.clone(),
                        next.clone(),
                        ExpectedFunction::NoExpectedFunction,
                        &mut next_args,
                        &divot,
                        &divot,
                        &divot,
                        DebuggableCall::No,
                    );
                }

                {
                    let state_destination = Some(generator.new_temporary());
                    let current_state = generator.emit_get_internal_field(
                        state_destination,
                        generator_register.clone(),
                        crate::runtime::js_generator::Field::State as u32,
                    );
                    let executing_state = generator.emit_load_js_value(
                        None,
                        js_number_i32(crate::runtime::js_generator::State::Executing as i32),
                    );
                    let condition_register = Some(generator.new_temporary());
                    let condition = generator.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                        condition_register,
                        current_state,
                        executing_state,
                    );
                    generator.emit_jump_if_false_raw(condition.as_ref().expect("condition"), &drive_label);
                }

                {
                    let resolve_promise = generator.move_link_time_constant(
                        None,
                        crate::bytecode::link_time_constant::LinkTimeConstant::ResolvePromiseWithFirstResolvingFunctionCallCheck,
                    );
                    let mut resolve_args = CallArguments::new(generator, None, 2);
                    generator.emit_load_js_value(resolve_args.this_register(), js_undefined());
                    let promise_register = generator.promise_register();
                    cpp5d_move(generator, resolve_args.argument_register(0), &promise_register);
                    cpp5d_move(generator, resolve_args.argument_register(1), &next_result);
                    let ignored_destination = Some(generator.new_temporary());
                    generator.emit_call_ignore_result(
                        ignored_destination,
                        resolve_promise,
                        ExpectedFunction::NoExpectedFunction,
                        &mut resolve_args,
                        &divot,
                        &divot,
                        &divot,
                        DebuggableCall::No,
                    );
                    generator.emit_jump(&success_label);
                }

                {
                    generator.emit_label(&drive_label);
                    let async_function_drive = generator.move_link_time_constant(
                        None,
                        crate::bytecode::link_time_constant::LinkTimeConstant::AsyncFunctionDrive,
                    );
                    let mut drive_args = CallArguments::new(generator, None, 2);
                    generator.emit_load_js_value(drive_args.this_register(), js_undefined());
                    cpp5d_move(generator, drive_args.argument_register(0), &next_result);
                    cpp5d_move(generator, drive_args.argument_register(1), &generator_register);
                    let ignored_destination = Some(generator.new_temporary());
                    generator.emit_call_ignore_result(
                        ignored_destination,
                        async_function_drive,
                        ExpectedFunction::NoExpectedFunction,
                        &mut drive_args,
                        &divot,
                        &divot,
                        &divot,
                        DebuggableCall::No,
                    );
                    generator.emit_jump(&success_label);
                }

                let try_end_label = generator.new_emitted_label();
                generator.pop_try(&try_data, &try_end_label);

                {
                    generator.emit_label(&catch_label);
                    let thrown_value = Some(generator.new_temporary());
                    generator.emit_out_of_line_catch_handler(thrown_value.clone(), None, Some(&try_data));

                    let reject_promise = generator.move_link_time_constant(
                        None,
                        crate::bytecode::link_time_constant::LinkTimeConstant::RejectPromiseWithFirstResolvingFunctionCallCheck,
                    );
                    let mut reject_args = CallArguments::new(generator, None, 2);
                    generator.emit_load_js_value(reject_args.this_register(), js_undefined());
                    let promise_register = generator.promise_register();
                    cpp5d_move(generator, reject_args.argument_register(0), &promise_register);
                    cpp5d_move(generator, reject_args.argument_register(1), &thrown_value);
                    let ignored_destination = Some(generator.new_temporary());
                    generator.emit_call_ignore_result(
                        ignored_destination,
                        reject_promise,
                        ExpectedFunction::NoExpectedFunction,
                        &mut reject_args,
                        &divot,
                        &divot,
                        &divot,
                        DebuggableCall::No,
                    );
                }

                generator.emit_label(&success_label);
                generator.emit_debug_hook(
                    DebugHookType::WillLeaveCallFrame,
                    &JSTextPosition::new(
                        self.base.last_line() as i32,
                        self.base.start_offset(),
                        self.base.line_start_offset(),
                    ),
                    None,
                );
                let promise_register = generator.promise_register();
                generator.emit_return(promise_register);
            }

            SourceParseMode::AsyncGeneratorBodyMode | SourceParseMode::GeneratorBodyMode => {
                let generator_body_label = generator.new_label();
                {
                    // O temporário do C++ é ponteiro cru: não segura referência, e o segundo
                    // `newTemporary()` reaproveita o mesmo registrador. O auxiliar solta a referência
                    // ao fim de cada salto.
                    generator.emit_jump_if_resume_mode(
                        crate::runtime::js_generator::ResumeMode::NormalMode,
                        &generator_body_label,
                    );

                    let throw_label = generator.new_label();
                    generator.emit_jump_if_resume_mode(
                        crate::runtime::js_generator::ResumeMode::ThrowMode,
                        &throw_label,
                    );

                    let generator_value_register = Some(generator.generator_value_register());
                    generator.emit_return(generator_value_register.clone());

                    generator.emit_label(&throw_label);
                    generator.emit_throw(generator_value_register);
                }

                generator.emit_label(&generator_body_label);
                // [[fallthrough]]
                cpp5d_emit_body_and_implicit_return(self, generator);
            }

            SourceParseMode::AsyncArrowFunctionBodyMode | SourceParseMode::AsyncFunctionBodyMode => {
                cpp5d_emit_body_and_implicit_return(self, generator);
            }

            _ => {
                cpp5d_emit_statements_with_using(self, generator);

                let single_statement = self.base.single_statement();
                let mut has_return_node = false;

                // Check for a return statement at the end of a function composed of a single block,
                // or a function whose body is a single return statement (arrow function expression body).
                // With using declarations, emitUsingBodyScope ends with a `done` label that needs
                // a terminal, otherwise it collides with the op_catch stubs appended by generate().
                if let Some(single_statement) = &single_statement {
                    if self.base.variable_environment.using_declaration_count() == 0 {
                        if single_statement.is_return_node() {
                            has_return_node = true;
                        } else if let crate::parser::nodes::Statement::Block(block) = single_statement {
                            let last_statement_in_block = block.borrow().last_statement();
                            if let Some(last_statement_in_block) = last_statement_in_block {
                                if last_statement_in_block.is_return_node() {
                                    has_return_node = true;
                                }
                            }
                        }
                    }
                }

                // If there is no return we must automatically insert one.
                if !has_return_node {
                    let r0 = if generator.is_constructor()
                        && generator.constructor_kind() != crate::runtime::constructor_kind::ConstructorKind::Naked
                    {
                        Some(generator.ensure_this())
                    } else {
                        generator.emit_load_js_value(None, js_undefined())
                    };
                    // Do not emit expression info for this profile because it's not in the user's source code.
                    generator.emit_profile_type_flag(
                        r0.clone(),
                        ProfileTypeBytecodeFlag::ProfileTypeBytecodeFunctionReturnStatement,
                    );
                    generator.emit_will_leave_call_frame_debug_hook();
                    generator.emit_return(r0);
                }
            }
        }
    }
}
