// Parte 5 de bytecompiler/BytecodeGenerator.cpp (linhas 4036 a 5100, de emitConstructImpl até o fim de
// emitEnumeration). Juntada por include!. Convenções: `RegisterID*` anulável é `Option<RegisterRef>`,
// `Ref<Label>` é `LabelRef`, o `ScopedLambda` é `&mut dyn FnMut`, e a posse de `FinallyContext` (que no C++ vive
// na pilha e é apontado pelo `ControlFlowScope`) é `Rc<RefCell<FinallyContext>>`.

impl BytecodeGenerator {
    // BytecodeGenerator.cpp:4035 (template<typename ConstructOp>)
    pub fn emit_construct_impl(
        &mut self,
        construct_opcode: crate::bytecode::opcode::OpcodeID,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        lazy_this: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        mut expected_function: crate::bytecompiler::bytecode_generator::ExpectedFunction,
        call_arguments: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        is_default_derived_constructor_call: bool,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        debug_assert!(crate::wtf::ref_counted::RefCounted::ref_count(&*func.as_ref().unwrap().borrow()) != 0);

        // Generate code for arguments.
        let mut argument: usize = 0;
        if let Some(arguments_node) = call_arguments.arguments_node.clone() {
            let list_node = arguments_node.borrow().list_node.clone();
            if let Some(n) = list_node.clone() {
                if n.borrow().expr.is_spread_expression() {
                    assert!(n.borrow().next.is_none());
                    let expression = match &n.borrow().expr {
                        crate::parser::nodes::Expression::SpreadExpression(spread) => spread.borrow().expression.clone(),
                        _ => unreachable!("isSpreadExpression garante o SpreadExpressionNode"),
                    };
                    if expression.is_array_literal() {
                        let elements = match &expression {
                            crate::parser::nodes::Expression::Array(array) => array.borrow().element.clone(),
                            _ => unreachable!("isArrayLiteral garante o ArrayNode"),
                        };
                        if let Some(elements) = elements {
                            if elements.borrow().next.is_none() && elements.borrow().node.is_spread_expression() {
                                let element_node = elements.borrow().node.clone();
                                let spread = match element_node {
                                    crate::parser::nodes::Expression::SpreadExpression(spread) => spread,
                                    _ => unreachable!("isSpreadExpression garante o SpreadExpressionNode"),
                                };
                                let spread_expression = spread.borrow().expression.clone();
                                let argument_register_dst = call_arguments.argument_register(0);
                                let emitted = self.emit_node_expression(argument_register_dst, &spread_expression);
                                let argument_register = self.temp_destination(emitted.as_ref());

                                if !is_default_derived_constructor_call {
                                    self.emit_expression_info(
                                        &spread.borrow().throwable.divot,
                                        &spread.borrow().throwable.divot_start,
                                        &spread.borrow().throwable.divot_end,
                                    );
                                    crate::bytecode::bytecode_ops::OpSpread::emit(self, &argument_register, &argument_register);
                                }

                                let this_register = call_arguments.this_register();
                                self.move_register(this_register.as_ref(), lazy_this.as_ref().unwrap());
                                let first_free_register = self.new_temporary();
                                return self.emit_call_varargs_op(
                                    Self::varargs_opcode_for(construct_opcode),
                                    dst,
                                    func,
                                    this_register,
                                    Some(argument_register),
                                    Some(first_free_register),
                                    0,
                                    divot,
                                    divot_start,
                                    divot_end,
                                    crate::bytecompiler::bytecode_generator::DebuggableCall::No,
                                );
                            }
                        }
                    }
                    let argument_register_dst = call_arguments.argument_register(0);
                    let argument_register = expression.emit_bytecode(self, argument_register_dst);
                    let this_register = call_arguments.this_register();
                    self.move_register(this_register.as_ref(), lazy_this.as_ref().unwrap());
                    let first_free_register = self.new_temporary();
                    return self.emit_call_varargs_op(
                        Self::varargs_opcode_for(construct_opcode),
                        dst,
                        func,
                        this_register,
                        argument_register,
                        Some(first_free_register),
                        0,
                        divot,
                        divot_start,
                        divot_end,
                        crate::bytecompiler::bytecode_generator::DebuggableCall::No,
                    );
                }
            }

            let mut n = list_node;
            while let Some(node) = n {
                let argument_register = call_arguments.argument_register(argument);
                argument += 1;
                self.emit_node_expression(argument_register, &node.borrow().expr);
                n = node.borrow().next.clone();
            }
        }

        let this_register = call_arguments.this_register();
        self.move_register(this_register.as_ref(), lazy_this.as_ref().unwrap());

        // Reserve space for call frame.
        let mut call_frame: Vec<crate::bytecompiler::bytecode_generator::RegisterRef> = Vec::new();
        for _ in 0..crate::interpreter::call_frame::HEADER_SIZE_IN_REGISTERS {
            call_frame.push(self.new_temporary());
        }

        self.emit_expression_info(divot, divot_start, divot_end);

        let done = self.new_label();
        expected_function =
            self.emit_expected_function_snippet(dst.clone(), func.clone(), expected_function, call_arguments, &done);

        let value_profile_index = self.next_value_profile_index();
        if construct_opcode == crate::bytecode::opcode::OpcodeID::op_super_construct {
            crate::bytecode::bytecode_ops::OpSuperConstruct::emit(
                self,
                dst.as_ref().unwrap(),
                func.as_ref().unwrap(),
                call_arguments.argument_count_including_this(),
                call_arguments.stack_offset(),
                value_profile_index,
            );
        } else {
            crate::bytecode::bytecode_ops::OpConstruct::emit(
                self,
                dst.as_ref().unwrap(),
                func.as_ref().unwrap(),
                call_arguments.argument_count_including_this(),
                call_arguments.stack_offset(),
                value_profile_index,
            );
        }

        if expected_function != crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction {
            self.emit_label(&done);
        }

        // `callFrame` mantém os temporários referenciados até aqui, como o Vector<RefPtr<RegisterID>> do C++.
        drop(call_frame);
        dst
    }

    // BytecodeGenerator.cpp:4094
    pub fn emit_construct(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        lazy_this: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        expected_function: crate::bytecompiler::bytecode_generator::ExpectedFunction,
        call_arguments: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_construct_impl(
            crate::bytecode::opcode::OpcodeID::op_construct,
            dst, func, lazy_this, expected_function, call_arguments, divot, divot_start, divot_end, false,
        )
    }

    // BytecodeGenerator.cpp:4099
    pub fn emit_super_construct(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        lazy_this: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        expected_function: crate::bytecompiler::bytecode_generator::ExpectedFunction,
        call_arguments: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        is_default_derived_constructor_call: bool,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_construct_impl(
            crate::bytecode::opcode::OpcodeID::op_super_construct,
            dst,
            func,
            lazy_this,
            expected_function,
            call_arguments,
            divot,
            divot_start,
            divot_end,
            is_default_derived_constructor_call,
        )
    }

    // BytecodeGenerator.cpp:4104
    pub fn emit_strcat(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        count: i32,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpStrcat::emit(self, dst.as_ref().unwrap(), src.as_ref().unwrap(), count);
        dst
    }

    // BytecodeGenerator.cpp:4110
    pub fn emit_to_primitive(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        crate::bytecode::bytecode_ops::OpToPrimitive::emit(self, dst.as_ref().unwrap(), src.as_ref().unwrap());
    }

    // BytecodeGenerator.cpp:4115
    pub fn emit_to_property_key(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpToPropertyKey::emit(self, dst.as_ref().unwrap(), src.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:4121
    pub fn emit_to_property_key_or_number(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpToPropertyKeyOrNumber::emit(self, dst.as_ref().unwrap(), src.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:4127
    pub fn emit_push_with_scope(
        &mut self,
        object_scope: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.push_local_control_flow_scope();
        let new_scope = self.new_block_scope_variable();
        new_scope.borrow_mut().ref_();

        let scope_register = self.scope_register();
        crate::bytecode::bytecode_ops::OpPushWithScope::emit(
            self,
            &new_scope,
            scope_register.as_ref().unwrap(),
            object_scope.as_ref().unwrap(),
        );

        let scope_register = self.scope_register();
        self.move_register(scope_register.as_ref(), &new_scope);
        self.lexical_scope_stack.push(crate::bytecompiler::bytecode_generator::LexicalScopeStackEntry {
            symbol_table: None,
            scope: Some(new_scope.clone()),
            is_with_scope: true,
            symbol_table_constant_index: 0,
        });

        Some(new_scope)
    }

    // BytecodeGenerator.cpp:4141
    pub fn emit_get_parent_scope(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        scope: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpGetParentScope::emit(self, dst.as_ref().unwrap(), scope.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:4147
    pub fn emit_pop_with_scope(&mut self) {
        let scope_register = self.scope_register();
        self.emit_get_parent_scope(scope_register.clone(), scope_register);
        self.pop_local_control_flow_scope();
        let stack_entry = self.lexical_scope_stack.pop().expect("takeLast");
        stack_entry.scope.as_ref().unwrap().borrow_mut().deref();
        assert!(stack_entry.is_with_scope);
    }

    // BytecodeGenerator.cpp:4156
    pub fn emit_debug_hook(
        &mut self,
        debug_hook_type: crate::interpreter::interpreter::DebugHookType,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        data: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        if !self.should_emit_debug_hooks() {
            return;
        }

        if self.last_debug_hook.position == *divot && self.last_debug_hook.type_ == debug_hook_type {
            return;
        }

        self.last_debug_hook.position = divot.clone();
        self.last_debug_hook.type_ = debug_hook_type;

        self.emit_expression_info(divot, divot, divot);

        let data = match data {
            Some(data) => data,
            None => self.emit_load_js_value(None, crate::runtime::js_value::js_undefined()).unwrap(),
        };
        crate::bytecode::bytecode_ops::OpDebug::emit(self, debug_hook_type, &data);
    }

    // BytecodeGenerator.cpp:4174
    pub fn emit_debug_hook_statement_data(
        &mut self,
        statement: &crate::parser::nodes::Statement,
        data: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        // DebuggerStatementNode will output its own special debug hook.
        if statement.is_debugger_statement() {
            return;
        }

        self.emit_debug_hook(
            crate::interpreter::interpreter::DebugHookType::WillExecuteStatement,
            statement.base().position(),
            data,
        );
    }

    // BytecodeGenerator.cpp:4183
    pub fn emit_debug_hook_expression_data(
        &mut self,
        expr: &crate::parser::nodes::Expression,
        data: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        self.emit_debug_hook(
            crate::interpreter::interpreter::DebugHookType::WillExecuteStatement,
            expr.base().position(),
            data,
        );
    }

    // BytecodeGenerator.cpp:4188
    pub fn emit_will_leave_call_frame_debug_hook(&mut self) {
        let position = {
            let scope_node = self.scope_node.borrow();
            crate::parser::parser_tokens::JSTextPosition::new(
                scope_node.last_line() as i32,
                scope_node.start_offset() as i32,
                scope_node.line_start_offset() as i32,
            )
        };
        self.emit_debug_hook(crate::interpreter::interpreter::DebugHookType::WillLeaveCallFrame, &position, None);
    }

    // BytecodeGenerator.cpp:4193
    pub fn push_finally_control_flow_scope(
        &mut self,
        finally_context: &std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::FinallyContext>>,
    ) {
        let scope = crate::bytecompiler::bytecode_generator::ControlFlowScope::new(
            crate::bytecompiler::bytecode_generator::CONTROL_FLOW_SCOPE_FINALLY,
            self.current_lexical_scope_index(),
            Some(finally_context.clone()),
        );
        self.control_flow_scope_stack.push(scope);

        self.finally_depth += 1;
        self.current_finally_context = Some(finally_context.clone());
    }

    // BytecodeGenerator.cpp:4202
    pub fn pop_finally_control_flow_scope(&mut self) {
        debug_assert!(!self.control_flow_scope_stack.is_empty());
        debug_assert!(self.control_flow_scope_stack.last().unwrap().is_finally_scope());
        debug_assert!(self.finally_depth > 0);
        debug_assert!(self.current_finally_context.is_some());
        let outer = self.current_finally_context.as_ref().unwrap().borrow().outer_context();
        self.current_finally_context = outer;
        self.finally_depth -= 1;
        self.control_flow_scope_stack.pop();
    }

    // BytecodeGenerator.cpp:4213
    pub fn break_target(
        &mut self,
        name: &crate::runtime::identifier::Identifier,
    ) -> Option<std::rc::Rc<crate::bytecompiler::label_scope::LabelScope>> {
        crate::wtf::vector::shrink_to_fit(&mut self.label_scopes);

        if self.label_scopes.is_empty() {
            return None;
        }

        // We special-case the following, which is a syntax error in Firefox:
        // label:
        //     break;
        if name.is_empty() {
            for i in (0..self.label_scopes.len()).rev() {
                let scope = self.label_scopes[i].clone();
                if scope.type_() != crate::bytecompiler::label_scope::LabelScopeType::NamedLabel {
                    return Some(scope);
                }
            }
            return None;
        }

        for i in (0..self.label_scopes.len()).rev() {
            let scope = self.label_scopes[i].clone();
            if let Some(scope_name) = scope.name() {
                if scope_name == name {
                    return Some(scope.clone());
                }
            }
        }
        None
    }

    // BytecodeGenerator.cpp:4240
    pub fn continue_target(
        &mut self,
        name: &crate::runtime::identifier::Identifier,
    ) -> Option<std::rc::Rc<crate::bytecompiler::label_scope::LabelScope>> {
        crate::wtf::vector::shrink_to_fit(&mut self.label_scopes);

        if self.label_scopes.is_empty() {
            return None;
        }

        if name.is_empty() {
            for i in (0..self.label_scopes.len()).rev() {
                let scope = self.label_scopes[i].clone();
                if scope.type_() == crate::bytecompiler::label_scope::LabelScopeType::Loop {
                    debug_assert!(scope.continue_target().is_some());
                    return Some(scope);
                }
            }
            return None;
        }

        // Continue to the loop nested nearest to the label scope that matches
        // 'name'.
        let mut result = None;
        for i in (0..self.label_scopes.len()).rev() {
            let scope = self.label_scopes[i].clone();
            if scope.type_() == crate::bytecompiler::label_scope::LabelScopeType::Loop {
                debug_assert!(scope.continue_target().is_some());
                result = Some(scope.clone());
            }
            if let Some(scope_name) = scope.name() {
                if scope_name == name {
                    return result; // may be null.
                }
            }
        }
        None
    }

    // BytecodeGenerator.cpp:4273
    pub fn allocate_scope(&mut self) {
        let scope_register = self.add_var();
        scope_register.borrow_mut().ref_();
        let virtual_register = scope_register.borrow().virtual_register();
        self.scope_register = Some(scope_register);
        self.code_block.set_scope_register(virtual_register);
    }

    // BytecodeGenerator.cpp:4280
    pub fn push_try(
        &mut self,
        start: &crate::bytecompiler::label::LabelRef,
        handler_label: &crate::bytecompiler::label::LabelRef,
        handler_type: crate::bytecode::handler_info::HandlerType,
    ) -> std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::TryData>> {
        let result = std::rc::Rc::new(std::cell::RefCell::new(crate::bytecompiler::bytecode_generator::TryData {
            target: handler_label.clone(),
            handler_type,
        }));
        self.try_data.push(result.clone());

        self.try_context_stack.push(crate::bytecompiler::bytecode_generator::TryContext {
            start: start.clone(),
            try_data: result.clone(),
        });

        result
    }

    // BytecodeGenerator.cpp:4293
    pub fn pop_try(
        &mut self,
        try_data: &std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::TryData>>,
        end: &crate::bytecompiler::label::LabelRef,
    ) {
        self.uses_exceptions = true;

        let context = self.try_context_stack.pop().expect("m_tryContextStack.last()");
        debug_assert!(std::rc::Rc::ptr_eq(&context.try_data, try_data));

        self.try_ranges.push(crate::bytecompiler::bytecode_generator::TryRange {
            start: context.start.clone(),
            end: end.clone(),
            try_data: context.try_data.clone(),
        });
    }

    // BytecodeGenerator.cpp:4307
    pub fn emit_out_of_line_catch_handler(
        &mut self,
        thrown_value_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        completion_type_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        data: Option<&std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::TryData>>>,
    ) {
        let unused = self.new_temporary();
        self.emit_out_of_line_exception_handler(Some(unused), thrown_value_register, completion_type_register, data);
    }

    // BytecodeGenerator.cpp:4313
    pub fn emit_out_of_line_finally_handler(
        &mut self,
        exception_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        completion_type_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        data: Option<&std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::TryData>>>,
    ) {
        let unused = self.new_temporary();
        debug_assert!(completion_type_register.is_some());
        self.emit_out_of_line_exception_handler(exception_register, Some(unused), completion_type_register, data);
    }

    // BytecodeGenerator.cpp:4320
    pub fn emit_out_of_line_exception_handler(
        &mut self,
        exception_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        thrown_value_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        completion_type_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        data: Option<&std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::TryData>>>,
    ) {
        let completion_type_virtual_register = match &completion_type_register {
            Some(register) => register.borrow().virtual_register(),
            None => crate::bytecode::virtual_register::VirtualRegister::default(),
        };
        self.exception_handlers_to_emit.push(crate::bytecompiler::bytecode_generator::CatchEntry {
            try_data: data.unwrap().clone(),
            exception_register: exception_register.unwrap().borrow().virtual_register(),
            thrown_value_register: thrown_value_register.unwrap().borrow().virtual_register(),
            completion_type_register: completion_type_virtual_register,
        });
    }

    // BytecodeGenerator.cpp:4326
    pub fn restore_scope_register_at(&mut self, lexical_scope_index: i32) {
        if lexical_scope_index == crate::bytecompiler::bytecode_generator::BytecodeGenerator::CURRENT_LEXICAL_SCOPE_INDEX {
            return; // No change needed.
        }

        if lexical_scope_index != crate::bytecompiler::bytecode_generator::BytecodeGenerator::OUTERMOST_LEXICAL_SCOPE_INDEX {
            debug_assert!((lexical_scope_index as usize) < self.lexical_scope_stack.len());
            let end_index = lexical_scope_index as usize + 1;
            for i in (0..end_index).rev() {
                if let Some(scope) = self.lexical_scope_stack[i].scope.clone() {
                    let scope_register = self.scope_register();
                    self.move_register(scope_register.as_ref(), &scope);
                    return;
                }
            }
        }
        // Note that if we don't find a local scope in the current function/program,
        // we must grab the outer-most scope of this bytecode generation.
        if let Some(top_level_scope_register) = self.top_level_scope_register.clone() {
            let scope_register = self.scope_register();
            self.move_register(scope_register.as_ref(), &top_level_scope_register);
        } else {
            let scope_register = self.scope_register();
            crate::bytecode::bytecode_ops::OpGetScope::emit(self, scope_register.as_ref().unwrap());
        }
    }

    // BytecodeGenerator.cpp:4349
    pub fn restore_scope_register(&mut self) {
        let index = self.current_lexical_scope_index();
        self.restore_scope_register_at(index);
    }

    // BytecodeGenerator.cpp:4354
    pub fn label_scope_depth_to_lexical_scope_index(&mut self, target_label_scope_depth: i32) -> i32 {
        debug_assert!(self.label_scope_depth() - target_label_scope_depth >= 0);
        let scope_delta = (self.label_scope_depth() - target_label_scope_depth) as usize;
        debug_assert!(scope_delta <= self.control_flow_scope_stack.len());
        if scope_delta == 0 {
            return crate::bytecompiler::bytecode_generator::BytecodeGenerator::CURRENT_LEXICAL_SCOPE_INDEX;
        }

        let target_scope = &self.control_flow_scope_stack[target_label_scope_depth as usize];
        target_scope.lexical_scope_index
    }

    // BytecodeGenerator.cpp:4366
    pub fn emit_throw(&mut self, exc: Option<crate::bytecompiler::bytecode_generator::RegisterRef>) {
        self.uses_exceptions = true;
        crate::bytecode::bytecode_ops::OpThrow::emit(self, exc.as_ref().unwrap());
    }

    // BytecodeGenerator.cpp:4372
    pub fn emit_argument_count(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpArgumentCount::emit(self, dst.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:4378
    pub fn local_scope_depth(&self) -> u32 {
        self.local_scope_depth
    }

    // BytecodeGenerator.cpp:4383
    pub fn label_scope_depth(&self) -> i32 {
        let depth = self.local_scope_depth() + self.finally_depth;
        debug_assert!(depth as usize == self.control_flow_scope_stack.len());
        depth as i32
    }

    // BytecodeGenerator.cpp:4390
    pub fn emit_throw_static_error_register(
        &mut self,
        error_type: crate::runtime::error_type::ErrorTypeWithExtension,
        raw: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        let message = self.new_temporary();
        self.emit_to_string(Some(message.clone()), raw);
        crate::bytecode::bytecode_ops::OpThrowStaticError::emit(self, &message, error_type);
    }

    // BytecodeGenerator.cpp:4397
    pub fn emit_throw_static_error(
        &mut self,
        error_type: crate::runtime::error_type::ErrorTypeWithExtension,
        message: &crate::runtime::identifier::Identifier,
    ) {
        let string_constant = self.add_string_constant(message);
        let constant = self.add_constant_value(
            crate::runtime::js_value::JSValue::from_js_string(string_constant),
            crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
        );
        crate::bytecode::bytecode_ops::OpThrowStaticError::emit(self, &constant, error_type);
    }

    // BytecodeGenerator.cpp:4402
    pub fn emit_throw_reference_error(&mut self, message: &str) {
        let identifier = crate::runtime::identifier::Identifier::from_span(&self.vm, message.as_bytes());
        self.emit_throw_static_error(crate::runtime::error_type::ErrorTypeWithExtension::ReferenceError, &identifier);
    }

    // BytecodeGenerator.cpp:4407
    pub fn emit_throw_type_error(&mut self, message: &str) {
        let identifier = crate::runtime::identifier::Identifier::from_span(&self.vm, message.as_bytes());
        self.emit_throw_static_error(crate::runtime::error_type::ErrorTypeWithExtension::TypeError, &identifier);
    }

    // BytecodeGenerator.cpp:4412
    pub fn emit_throw_type_error_identifier(&mut self, message: &crate::runtime::identifier::Identifier) {
        self.emit_throw_static_error(crate::runtime::error_type::ErrorTypeWithExtension::TypeError, message);
    }

    // BytecodeGenerator.cpp:4417
    pub fn emit_throw_range_error(&mut self, message: &crate::runtime::identifier::Identifier) {
        self.emit_throw_static_error(crate::runtime::error_type::ErrorTypeWithExtension::RangeError, message);
    }

    // BytecodeGenerator.cpp:4422
    pub fn emit_throw_out_of_memory_error(&mut self) {
        let empty_identifier = self.vm.property_names.empty_identifier.clone();
        self.emit_throw_static_error(
            crate::runtime::error_type::ErrorTypeWithExtension::OutOfMemoryError,
            &empty_identifier,
        );
    }

    // BytecodeGenerator.cpp:4427
    pub fn emit_push_function_name_scope(
        &mut self,
        property: &crate::runtime::identifier::Identifier,
        callee: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        is_captured: bool,
    ) {
        // There is some nuance here:
        // If we're in strict mode code, the function name scope variable acts exactly like a "const" variable.
        // If we're not in strict mode code, we want to allow bogus assignments to the name scoped variable.
        // This means any assignment to the variable won't throw, but it won't actually assign a new value to it.
        // To accomplish this, we don't report that this scope is a lexical scope. This will prevent
        // any throws when trying to assign to the variable (while still ensuring it keeps its original
        // value). There is some ugliness and exploitation of a leaky abstraction here, but it's better than
        // having a completely new op code and a class to handle name scopes which are so close in functionality
        // to lexical environments.
        use crate::bytecompiler::bytecode_generator_base::GeneratorCodeBlock;
        let mut name_scope_environment = crate::parser::variable_environment::VariableEnvironment::default();
        {
            let entry = name_scope_environment.add_identifier(property);
            if is_captured {
                entry.value.set_is_captured();
            }
            entry.value.set_is_const(); // The function name scope name acts like a const variable.
        }
        let num_vars = self.code_block.num_vars();
        self.push_lexical_scope_internal(
            &mut name_scope_environment,
            TDZCheckOptimization::Optimize,
            NestedScopeType::IsNotNested,
            None,
            TDZRequirement::NotUnderTDZ,
            ScopeType::FunctionNameScope,
            ScopeRegisterType::Var,
        );
        debug_assert!(self.code_block.num_vars() == num_vars + 1); // Should have only created one new "var" for the function name scope.
        let should_treat_as_lexical_variable = self.ecma_mode().is_strict();
        let (symbol_table, symbol_table_constant_index, scope) = {
            let last = self.lexical_scope_stack.last().unwrap();
            (last.symbol_table.clone().unwrap(), last.symbol_table_constant_index, last.scope.clone())
        };
        let entry = symbol_table.borrow().get(&property.impl_().expect("identifier sem StringImpl"));
        let function_var =
            self.variable_for_local_entry(property, &entry, symbol_table_constant_index, should_treat_as_lexical_variable);
        self.emit_put_to_scope(
            scope,
            &function_var,
            callee,
            crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
            crate::runtime::get_put_info::InitializationMode::NotInitialization,
        );
    }

    // BytecodeGenerator.cpp:4451
    pub fn push_local_control_flow_scope(&mut self) {
        let scope = crate::bytecompiler::bytecode_generator::ControlFlowScope::new(
            crate::bytecompiler::bytecode_generator::CONTROL_FLOW_SCOPE_LABEL,
            self.current_lexical_scope_index(),
            None,
        );
        self.control_flow_scope_stack.push(scope);
        self.local_scope_depth += 1;
        self.local_scope_count += 1;
    }

    // BytecodeGenerator.cpp:4459
    pub fn pop_local_control_flow_scope(&mut self) {
        debug_assert!(!self.control_flow_scope_stack.is_empty());
        debug_assert!(!self.control_flow_scope_stack.last().unwrap().is_finally_scope());
        self.control_flow_scope_stack.pop();
        self.local_scope_depth -= 1;
    }

    // BytecodeGenerator.cpp:4467
    pub fn emit_push_catch_scope(
        &mut self,
        environment: &mut crate::parser::variable_environment::VariableEnvironment,
        scope_type: ScopeType,
    ) {
        self.push_lexical_scope_internal(
            environment,
            TDZCheckOptimization::Optimize,
            NestedScopeType::IsNotNested,
            None,
            TDZRequirement::UnderTDZ,
            scope_type,
            ScopeRegisterType::Block,
        );
    }

    // BytecodeGenerator.cpp:4477
    pub fn begin_switch(
        &mut self,
        scrutinee_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        switch_type: crate::parser::nodes::SwitchType,
    ) {
        use crate::parser::nodes::SwitchType;
        match switch_type {
            SwitchType::Immediate | SwitchType::ImmediateList => {
                let table_index = self.code_block.number_of_unlinked_switch_jump_tables();
                self.code_block.add_unlinked_switch_jump_table();
                crate::bytecode::bytecode_ops::OpSwitchImm::emit(self, table_index as u32, scrutinee_register.as_ref().unwrap());
            }
            SwitchType::Character | SwitchType::CharacterList => {
                let table_index = self.code_block.number_of_unlinked_switch_jump_tables();
                self.code_block.add_unlinked_switch_jump_table();
                crate::bytecode::bytecode_ops::OpSwitchChar::emit(self, table_index as u32, scrutinee_register.as_ref().unwrap());
            }
            SwitchType::String => {
                let table_index = self.code_block.number_of_unlinked_string_switch_jump_tables();
                self.code_block.add_unlinked_string_switch_jump_table();
                crate::bytecode::bytecode_ops::OpSwitchString::emit(self, table_index as u32, scrutinee_register.as_ref().unwrap());
            }
            SwitchType::None => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
        }

        let info = crate::parser::nodes::SwitchInfo {
            bytecode_offset: self.last_instruction.offset(),
            switch_type,
        };
        self.switch_context_stack.push(info);
    }

    // BytecodeGenerator.cpp:4508
    pub fn end_switch(
        &mut self,
        labels: &[crate::bytecompiler::label::LabelRef],
        nodes: &[crate::parser::nodes::Expression],
        default_label: &crate::bytecompiler::label::LabelRef,
        min: i32,
        max: i32,
    ) {
        use crate::parser::nodes::SwitchType;
        let switch_info = self.switch_context_stack.pop().expect("m_switchContextStack.last()");

        // Chave de um caso de `switch` numérico (Immediate/ImmediateList) ou de um caractere (Character/CharacterList).
        // Valor de um `NumberNode` (`static_cast<NumberNode*>(...)->value()`, vale também para `IntegerNode`).
        let number_value = |node: &crate::parser::nodes::Expression| -> f64 {
            match node {
                crate::parser::nodes::Expression::Double(number) => number.borrow().base.value,
                crate::parser::nodes::Expression::Integer(number) => number.borrow().base.base.value,
                _ => unreachable!("ASSERT(nodes[i]->isNumber())"),
            }
        };
        let number_key = |node: &crate::parser::nodes::Expression| -> i32 { number_value(node) as i32 };
        let character_key = |node: &crate::parser::nodes::Expression| -> i32 {
            match node {
                crate::parser::nodes::Expression::String(string) => {
                    let clause = string.borrow().value.clone();
                    let clause = clause.impl_().expect("identifier sem StringImpl");
                    debug_assert!(clause.0.length() == 1);
                    clause.0.char_at(0) as i32
                }
                _ => unreachable!("ASSERT(nodes[i]->isString())"),
            }
        };

        // `handleSwitch`: tabela densa (Immediate e Character).
        let handle_switch = |generator: &mut BytecodeGenerator, table_index: u32| {
            let size = (max - min + 1) as usize;
            let mut adds: Vec<(i32, i32)> = Vec::new();
            for i in 0..labels.len() {
                // We're emitting this after the clause labels should have been fixed, so
                // the labels should not be "forward" references
                debug_assert!(!labels[i].borrow().is_forward());
                let key = if switch_info.switch_type == SwitchType::Immediate {
                    let value = number_value(&nodes[i]);
                    let extracted = value as i32;
                    debug_assert!(extracted as f64 == value);
                    debug_assert!(extracted >= min);
                    debug_assert!(extracted <= max);
                    extracted - min
                } else {
                    let extracted = character_key(&nodes[i]);
                    debug_assert!(extracted >= min);
                    debug_assert!(extracted <= max);
                    extracted - min
                };
                let offset = labels[i].borrow_mut().bind_offset(switch_info.bytecode_offset);
                adds.push((key, offset.target(&*generator)));
            }
            debug_assert!(!default_label.borrow().is_forward());
            let default_offset = default_label.borrow_mut().bind_offset(switch_info.bytecode_offset).target(&*generator);

            let code_block = &mut generator.code_block;
            let jump_table = code_block.unlinked_switch_jump_table(table_index as usize);
            jump_table.min = min;
            jump_table.branch_offsets = vec![0i32; size];
            for (key, offset) in adds {
                jump_table.add(key, offset);
            }
            jump_table.default_offset = default_offset;
        };

        // `handleSwitchList`: tabela em lista de pares chave/deslocamento (ImmediateList e CharacterList).
        let handle_switch_list = |generator: &mut BytecodeGenerator, table_index: u32| {
            let mut branch_offsets: Vec<i32> = Vec::with_capacity(labels.len() * 2);
            let mut already_handled: std::collections::HashSet<i32> = std::collections::HashSet::new();

            for i in 0..labels.len() {
                // We're emitting this after the clause labels should have been fixed, so
                // the labels should not be "forward" references
                debug_assert!(!labels[i].borrow().is_forward());
                let key = if switch_info.switch_type == SwitchType::ImmediateList {
                    number_key(&nodes[i])
                } else {
                    character_key(&nodes[i])
                };

                // There is a chance that we may list up duplicate keys. In this case, the first one wins.
                if !already_handled.insert(key) {
                    continue;
                }

                branch_offsets.push(key);
                let offset = labels[i].borrow_mut().bind_offset(switch_info.bytecode_offset);
                branch_offsets.push(offset.target(&*generator));
            }

            debug_assert!(!default_label.borrow().is_forward());
            let default_offset = default_label.borrow_mut().bind_offset(switch_info.bytecode_offset).target(&*generator);
            let code_block = &mut generator.code_block;
            let jump_table = code_block.unlinked_switch_jump_table(table_index as usize);
            jump_table.min = i32::MAX;
            jump_table.is_list = 1;
            jump_table.branch_offsets = branch_offsets;
            jump_table.default_offset = default_offset;
        };

        let handle_string_switch = |generator: &mut BytecodeGenerator, table_index: u32| {
            let mut entries: Vec<(crate::wtf::text::string_impl::UniquedKey, i32)> = Vec::new();
            for i in 0..labels.len() {
                // We're emitting this after the clause labels should have been fixed, so
                // the labels should not be "forward" references
                debug_assert!(!labels[i].borrow().is_forward());

                let clause = match &nodes[i] {
                    crate::parser::nodes::Expression::String(string) => {
                        string.borrow().value.impl_().expect("identifier sem StringImpl")
                    }
                    _ => unreachable!("ASSERT(nodes[i]->isString())"),
                };
                debug_assert!(clause.0.is_atom());
                let offset = labels[i].borrow_mut().bind_offset(switch_info.bytecode_offset).target(&*generator);
                entries.push((clause, offset));
            }
            debug_assert!(!default_label.borrow().is_forward());
            let default_offset = default_label.borrow_mut().bind_offset(switch_info.bytecode_offset).target(&*generator);

            let code_block = &mut generator.code_block;
            let jump_table = code_block.unlinked_string_switch_jump_table(table_index as usize);
            for (clause, offset) in entries {
                let is_new_entry = jump_table.add_offset(
                    clause.0.clone(),
                    crate::bytecode::unlinked_code_block::OffsetLocation { branch_offset: offset, index_in_table: 0 },
                );
                if is_new_entry {
                    let size = jump_table.offset_table_size();
                    jump_table.set_index_in_table(&clause.0, (size - 1) as u32);
                    jump_table.min_length = std::cmp::min(jump_table.min_length, clause.0.length());
                    jump_table.max_length = std::cmp::max(jump_table.max_length, clause.0.length());
                }
            }
            jump_table.default_offset = default_offset;

            if jump_table.offset_table_is_empty() {
                jump_table.min_length = 0;
                jump_table.max_length = 0;
            }
        };

        let reference = self.writer.ref_at(switch_info.bytecode_offset);
        match switch_info.switch_type {
            SwitchType::Immediate => {
                let table_index = reference.as_op::<crate::bytecode::bytecode_ops::OpSwitchImm>().table_index;
                handle_switch(self, table_index);
            }
            SwitchType::ImmediateList => {
                let table_index = reference.as_op::<crate::bytecode::bytecode_ops::OpSwitchImm>().table_index;
                handle_switch_list(self, table_index);
            }
            SwitchType::Character => {
                let table_index = reference.as_op::<crate::bytecode::bytecode_ops::OpSwitchChar>().table_index;
                handle_switch(self, table_index);
            }
            SwitchType::CharacterList => {
                let table_index = reference.as_op::<crate::bytecode::bytecode_ops::OpSwitchChar>().table_index;
                handle_switch_list(self, table_index);
            }
            SwitchType::String => {
                let table_index = reference.as_op::<crate::bytecode::bytecode_ops::OpSwitchString>().table_index;
                handle_string_switch(self, table_index);
            }
            SwitchType::None => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
        }
    }

    // BytecodeGenerator.cpp:4641
    pub fn emit_throw_expression_too_deep_exception(&mut self) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // It would be nice to do an even better job of identifying exactly where the expression is.
        // And we could make the caller pass the node pointer in, if there was some way of getting
        // that from an arbitrary node. However, calling emitExpressionInfo without any useful data
        // is still good enough to get us an accurate line number.
        self.expression_too_deep = true;
        Some(self.new_temporary())
    }

    // BytecodeGenerator.cpp:4651
    pub fn is_argument_number(&mut self, ident: &crate::runtime::identifier::Identifier, argument_number: i32) -> bool {
        let register_id = self.variable(ident, ThisResolutionType::Local).local();
        match register_id {
            None => false,
            Some(register_id) => {
                register_id.borrow().index() == crate::interpreter::call_frame::argument_offset(argument_number)
            }
        }
    }

    // BytecodeGenerator.cpp:4659
    pub fn emit_read_only_exception_if_needed(&mut self, variable: &crate::bytecompiler::bytecode_generator::Variable) -> bool {
        // If we're in strict mode, we always throw.
        // If we're not in strict mode, we throw for "const" variables but not the function callee.
        if self.ecma_mode().is_strict() || variable.is_const() {
            // O V8 diz `Assignment to constant variable.` (medido no Chromium 154).
            let text = if crate::runtime::stack_frame::v8_stack_format() {
                "Assignment to constant variable."
            } else {
                crate::runtime::error_messages::READONLY_PROPERTY_WRITE_ERROR
            };
            let message = crate::runtime::identifier::Identifier::from_span(&self.vm, text.as_bytes());
            self.emit_throw_type_error_identifier(&message);
            return true;
        }
        false
    }

    // BytecodeGenerator.cpp:4670
    pub fn emit_try_with_finally_that_does_not_shadow_exception(
        &mut self,
        emit_try: &mut dyn FnMut(&mut BytecodeGenerator),
        emit_finally: &mut dyn FnMut(&mut BytecodeGenerator),
    ) {
        let finally_label = self.new_label();
        let finally_context = std::rc::Rc::new(std::cell::RefCell::new(
            crate::bytecompiler::bytecode_generator::FinallyContext::new(self, finally_label),
        ));
        self.push_finally_control_flow_scope(&finally_context);
        self.emit_try_with_finally_that_does_not_shadow_exception_with_context(&finally_context, emit_try, emit_finally);
        self.pop_finally_control_flow_scope();
    }

    // BytecodeGenerator.cpp:4679
    pub fn emit_try_with_finally_that_does_not_shadow_exception_with_context(
        &mut self,
        finally_context: &std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::FinallyContext>>,
        emit_try: &mut dyn FnMut(&mut BytecodeGenerator),
        emit_finally: &mut dyn FnMut(&mut BytecodeGenerator),
    ) {
        let try_start_label = self.new_emitted_label();
        let finally_label = finally_context.borrow().finally_label().unwrap();
        let try_data = self.push_try(
            &try_start_label,
            &finally_label,
            crate::bytecode::handler_info::HandlerType::SynthesizedFinally,
        );
        emit_try(self);
        let try_end_label = self.new_emitted_label();
        self.pop_try(&try_data, &try_end_label);

        {
            let done = self.new_label();

            self.emit_label(&finally_label);
            let completion_value_register = finally_context.borrow().completion_value_register();
            let completion_type_register = finally_context.borrow().completion_type_register();
            self.emit_out_of_line_finally_handler(completion_value_register, completion_type_register.clone(), Some(&try_data));

            let try_in_finally_start_label = self.new_emitted_label();
            let catch_in_finally_label = self.new_label();
            let try_in_finally_data = self.push_try(
                &try_in_finally_start_label,
                &catch_in_finally_label,
                crate::bytecode::handler_info::HandlerType::SynthesizedCatch,
            );
            emit_finally(self);
            let try_in_finally_end_label = self.new_emitted_label();
            self.pop_try(&try_in_finally_data, &try_in_finally_end_label);

            self.emit_finally_completion(&mut finally_context.borrow_mut(), &done);

            // Catch block for exceptions that may be thrown while executing the finally block.
            // The only reason we need this catch block is because if the above finally block
            // is entered due to a thrown exception, then we want to rethrow the original exception
            // on exiting the finally block. Otherwise, we would let any new exception pass through.
            {
                self.emit_label(&catch_in_finally_label);

                let exception_register = self.new_temporary();
                self.emit_out_of_line_catch_handler(Some(exception_register.clone()), None, Some(&try_in_finally_data));
                // Since this is a synthesized catch block and we are guaranteed to never need to
                // resolve any symbols from the scope, we can skip restoring the scope register here.

                let temporary = self.new_temporary();
                let throw_completion = self.emit_load_completion_type(None, CompletionType::THROW);
                let equals = self.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                    Some(temporary),
                    completion_type_register,
                    throw_completion,
                );
                self.emit_jump_if_true_raw(equals.as_ref().unwrap(), &try_in_finally_end_label);
                self.emit_throw(Some(exception_register));
            }

            self.emit_label(&done);
        }
    }

    // BytecodeGenerator.cpp:4722
    pub fn emit_prepare_disposable(
        &mut self,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        is_async: bool,
    ) {
        let (slot_value_register, slot_method_register, slot_reached_register) = {
            let using_scope = self.current_using_scope();
            debug_assert!((using_scope.next_slot as usize) < using_scope.slots.len());
            let index = using_scope.next_slot as usize;
            using_scope.next_slot += 1;
            let slot = &mut using_scope.slots[index];
            slot.is_async = is_async;
            (slot.value.clone(), slot.method.clone(), slot.reached.clone())
        };
        self.move_register(slot_value_register.as_ref(), value.as_ref().unwrap());

        let get_dispose_method_func = self.move_link_time_constant(
            None,
            if is_async {
                crate::bytecode::link_time_constant::LinkTimeConstant::GetAsyncDisposeMethod
            } else {
                crate::bytecode::link_time_constant::LinkTimeConstant::GetDisposeMethod
            },
        );
        let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 1);
        let this_register = args.this_register();
        self.emit_load_js_value(this_register, crate::runtime::js_value::js_undefined());
        let argument_register = args.argument_register(0);
        self.move_register(argument_register.as_ref(), value.as_ref().unwrap());
        self.emit_call(
            slot_method_register,
            get_dispose_method_func,
            crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
            &mut args,
            divot,
            divot,
            divot,
            crate::bytecompiler::bytecode_generator::DebuggableCall::No,
        );

        // Mark reached only after method lookup succeeds; if the above call threw, reached stays
        // false so the finally block skips this slot entirely (spec: no resource record is added).
        if is_async {
            debug_assert!(self.current_using_scope().has_await_using);
            debug_assert!(slot_reached_register.is_some());
            self.emit_load_js_value(slot_reached_register, crate::runtime::js_value::js_boolean(true));
        }
    }

    // BytecodeGenerator.cpp:4745
    pub fn emit_using_body_scope(
        &mut self,
        using_count: u32,
        has_await_using: bool,
        emit_body: &mut dyn FnMut(&mut BytecodeGenerator),
    ) {
        use crate::bytecompiler::bytecode_generator::{ExpectedFunction, UsingScope, UsingSlot};
        debug_assert!(
            !has_await_using
                || crate::parser::parser_modes::is_async_function_parse_mode(self.parse_mode())
                || crate::parser::parser_modes::is_module_parse_mode(self.parse_mode())
        );

        // Pre-allocate slots and initialize method registers to undefined BEFORE the try block.
        // This ensures that if an initializer throws, the method register has a known value (undefined)
        // so the finally block can safely check it.
        self.using_scope_stack.push(UsingScope::default());
        self.current_using_scope().has_await_using = has_await_using;
        for _ in 0..using_count {
            let value_copy = self.new_temporary();
            let method = self.new_temporary();
            let mut reached = None;
            self.emit_load_js_value(Some(value_copy.clone()), crate::runtime::js_value::js_undefined());
            self.emit_load_js_value(Some(method.clone()), crate::runtime::js_value::js_undefined());
            if has_await_using {
                let register = self.new_temporary();
                self.emit_load_js_value(Some(register.clone()), crate::runtime::js_value::js_boolean(false));
                reached = Some(register);
            }
            self.current_using_scope().slots.push(UsingSlot {
                value: Some(value_copy),
                method: Some(method),
                reached,
                is_async: false,
            });
        }

        let thrown_value = self.new_temporary();
        self.emit_load_js_value(Some(thrown_value.clone()), crate::runtime::js_value::js_undefined());

        let finally_label = self.new_label();
        let finally_context = std::rc::Rc::new(std::cell::RefCell::new(
            crate::bytecompiler::bytecode_generator::FinallyContext::new(self, finally_label),
        ));
        self.push_finally_control_flow_scope(&finally_context);

        let try_start_label = self.new_emitted_label();
        let finally_label = finally_context.borrow().finally_label().unwrap();
        let try_data = self.push_try(
            &try_start_label,
            &finally_label,
            crate::bytecode::handler_info::HandlerType::SynthesizedFinally,
        );
        emit_body(self);
        let try_end_label = self.new_emitted_label();
        self.pop_try(&try_data, &try_end_label);

        // Finally block: unrolled dispose calls in reverse order.
        {
            let done = self.new_label();

            self.emit_label(&finally_label);
            let completion_value_register = finally_context.borrow().completion_value_register();
            let completion_type_register = finally_context.borrow().completion_type_register();
            self.emit_out_of_line_exception_handler(
                completion_value_register,
                Some(thrown_value.clone()),
                completion_type_register.clone(),
                Some(&try_data),
            );

            let pending_error = self.new_temporary();
            let has_error = self.new_temporary();
            let dispose_threw = self.new_temporary();
            self.emit_load_js_value(Some(pending_error.clone()), crate::runtime::js_value::js_undefined());
            self.emit_load_js_value(Some(has_error.clone()), crate::runtime::js_value::js_boolean(false));
            self.emit_load_js_value(Some(dispose_threw.clone()), crate::runtime::js_value::js_boolean(false));

            // If body threw, seed pendingError with the thrown value.
            {
                let after_init = self.new_label();
                let temporary = self.new_temporary();
                let throw_completion = self.emit_load_completion_type(None, CompletionType::THROW);
                let equals = self.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                    Some(temporary),
                    completion_type_register.clone(),
                    throw_completion,
                );
                self.emit_jump_if_false_raw(equals.as_ref().unwrap(), &after_init);
                self.move_register(Some(&pending_error), &thrown_value);
                self.emit_load_js_value(Some(has_error.clone()), crate::runtime::js_value::js_boolean(true));
                self.emit_label(&after_init);
            }

            let divot = {
                let scope_node = self.scope_node.borrow();
                crate::parser::parser_tokens::JSTextPosition::new(
                    scope_node.first_line() as i32,
                    scope_node.start_offset() as i32,
                    scope_node.line_start_offset() as i32,
                )
            };

            // Async disposal state (per DisposeResources spec): needsAwait / hasAwaited.
            // Only allocated when this scope contains at least one await using declaration.
            let mut needs_await: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
            let mut has_awaited: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
            if has_await_using {
                let needs_await_register = self.new_temporary();
                let has_awaited_register = self.new_temporary();
                self.emit_load_js_value(Some(needs_await_register.clone()), crate::runtime::js_value::js_boolean(false));
                self.emit_load_js_value(Some(has_awaited_register.clone()), crate::runtime::js_value::js_boolean(false));
                needs_await = Some(needs_await_register);
                has_awaited = Some(has_awaited_register);
            }

            // Shared temporaries for the catch handler (one pair is enough across all slots).
            let caught_exception = self.new_temporary();
            let caught_value = self.new_temporary();
            let suppressed_error_ctor = self.new_temporary();

            // `emitSuppressedErrorCatch`: o lambda do C++ captura os registradores por referência.
            let emit_suppressed_error_catch = |generator: &mut BytecodeGenerator,
                                               try_slot_data: &std::rc::Rc<
                std::cell::RefCell<crate::bytecompiler::bytecode_generator::TryData>,
            >,
                                               catch_label: &crate::bytecompiler::label::LabelRef| {
                generator.emit_label(catch_label);
                generator.emit_out_of_line_exception_handler(
                    Some(caught_exception.clone()),
                    Some(caught_value.clone()),
                    None,
                    Some(try_slot_data),
                );

                let after_catch = generator.new_label();
                let first_error = generator.new_label();
                generator.emit_jump_if_false(&has_error, &first_error);

                generator.move_link_time_constant(
                    Some(suppressed_error_ctor.clone()),
                    crate::bytecode::link_time_constant::LinkTimeConstant::SuppressedError,
                );
                let mut se_args = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 2);
                let argument_register = se_args.argument_register(0);
                generator.move_register(argument_register.as_ref(), &caught_value);
                let argument_register = se_args.argument_register(1);
                generator.move_register(argument_register.as_ref(), &pending_error);
                generator.emit_construct(
                    Some(pending_error.clone()),
                    Some(suppressed_error_ctor.clone()),
                    Some(suppressed_error_ctor.clone()),
                    ExpectedFunction::NoExpectedFunction,
                    &mut se_args,
                    &divot,
                    &divot,
                    &divot,
                );
                generator.emit_jump(&after_catch);

                generator.emit_label(&first_error);
                generator.move_register(Some(&pending_error), &caught_value);
                generator.emit_load_js_value(Some(has_error.clone()), crate::runtime::js_value::js_boolean(true));

                generator.emit_label(&after_catch);
                generator.emit_load_js_value(Some(dispose_threw.clone()), crate::runtime::js_value::js_boolean(true));
            };

            let emit_await_undefined = |generator: &mut BytecodeGenerator| {
                let tmp = generator.new_temporary();
                generator.emit_load_js_value(Some(tmp.clone()), crate::runtime::js_value::js_undefined());
                generator.emit_await(Some(tmp.clone()), &tmp, &divot);
            };

            // Dispose each resource in reverse declaration order.
            // Track whether we've processed an async slot so far: needsAwait can only become
            // true after an async slot's null-value case, so Step 3.d before the first async
            // slot in disposal order is always dead and can be elided at compile time.
            let mut saw_async_slot_in_disposal_order = false;
            // Os registradores dos slots já estão todos povoados aqui (o corpo foi emitido).
            let slots: Vec<(
                Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
                Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
                Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
                bool,
            )> = self
                .current_using_scope()
                .slots
                .iter()
                .map(|slot| (slot.value.clone(), slot.method.clone(), slot.reached.clone(), slot.is_async))
                .collect();
            for (slot_value, slot_method, slot_reached, slot_is_async) in slots.iter().rev() {
                let skip_slot = self.new_label();

                if *slot_is_async {
                    saw_async_slot_in_disposal_order = true;
                    // Async disposal slot.
                    // If the declaration was never reached (reached == false), skip entirely
                    // without setting needsAwait; the spec only adds a resource record on evaluation.
                    self.emit_jump_if_false(slot_reached.as_ref().unwrap(), &skip_slot);

                    let method_defined = self.new_label();
                    let temporary = self.new_temporary();
                    let is_undefined = self.emit_is_undefined(Some(temporary), slot_method.as_ref().unwrap());
                    self.emit_jump_if_false_raw(is_undefined.as_ref().unwrap(), &method_defined);

                    // method is undefined (await using x = null/undefined).
                    // Per DisposeResources step 3.f: set needsAwait to true, no call.
                    self.emit_load_js_value(needs_await.clone(), crate::runtime::js_value::js_boolean(true));
                    self.emit_jump(&skip_slot);

                    // method is defined: call it and Await the result.
                    self.emit_label(&method_defined);
                    let catch_label = self.new_label();
                    let try_slot_start = self.new_emitted_label();
                    let try_slot_data = self.push_try(
                        &try_slot_start,
                        &catch_label,
                        crate::bytecode::handler_info::HandlerType::SynthesizedCatch,
                    );

                    let result = self.new_temporary();
                    let mut dispose_args = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 0);
                    let this_register = dispose_args.this_register();
                    self.move_register(this_register.as_ref(), slot_value.as_ref().unwrap());
                    self.emit_call(
                        Some(result.clone()),
                        slot_method.clone(),
                        ExpectedFunction::NoExpectedFunction,
                        &mut dispose_args,
                        &divot,
                        &divot,
                        &divot,
                        crate::bytecompiler::bytecode_generator::DebuggableCall::No,
                    );

                    // Set hasAwaited before Await: emitAwait throws on rejection, but a rejected
                    // Await still implies a microtask boundary has occurred.
                    self.emit_load_js_value(has_awaited.clone(), crate::runtime::js_value::js_boolean(true));
                    self.emit_await(Some(result.clone()), &result, &divot);

                    let try_slot_end = self.new_emitted_label();
                    self.pop_try(&try_slot_data, &try_slot_end);
                    self.emit_jump(&skip_slot);

                    emit_suppressed_error_catch(self, &try_slot_data, &catch_label);
                } else {
                    // Sync disposal slot.
                    // Per DisposeResources step 3.d: if needsAwait && !hasAwaited, Await(undefined) first.
                    // Only emit if an async slot was already processed (otherwise needsAwait is provably false).
                    if saw_async_slot_in_disposal_order {
                        let skip_await_check = self.new_label();
                        self.emit_jump_if_false(needs_await.as_ref().unwrap(), &skip_await_check);
                        self.emit_jump_if_true(has_awaited.as_ref().unwrap(), &skip_await_check);
                        emit_await_undefined(self);
                        self.emit_load_js_value(needs_await.clone(), crate::runtime::js_value::js_boolean(false));
                        self.emit_label(&skip_await_check);
                    }

                    let temporary = self.new_temporary();
                    let is_undefined = self.emit_is_undefined(Some(temporary), slot_method.as_ref().unwrap());
                    self.emit_jump_if_true_raw(is_undefined.as_ref().unwrap(), &skip_slot);

                    let catch_label = self.new_label();
                    let try_slot_start = self.new_emitted_label();
                    let try_slot_data = self.push_try(
                        &try_slot_start,
                        &catch_label,
                        crate::bytecode::handler_info::HandlerType::SynthesizedCatch,
                    );

                    let mut dispose_args = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 0);
                    let this_register = dispose_args.this_register();
                    self.move_register(this_register.as_ref(), slot_value.as_ref().unwrap());
                    let ignored = self.new_temporary();
                    self.emit_call_ignore_result(
                        Some(ignored),
                        slot_method.clone(),
                        ExpectedFunction::NoExpectedFunction,
                        &mut dispose_args,
                        &divot,
                        &divot,
                        &divot,
                        crate::bytecompiler::bytecode_generator::DebuggableCall::No,
                    );

                    let try_slot_end = self.new_emitted_label();
                    self.pop_try(&try_slot_data, &try_slot_end);
                    self.emit_jump(&skip_slot);

                    emit_suppressed_error_catch(self, &try_slot_data, &catch_label);
                }

                self.emit_label(&skip_slot);
            }

            // Per DisposeResources step 6: trailing Await(undefined) if needsAwait && !hasAwaited.
            // If the first-declared slot is sync, its Step 3.d (above) already consumed any pending
            // needsAwait, so this trailing check is provably dead and elided.
            if has_await_using && slots[0].3 {
                let skip_final_await = self.new_label();
                self.emit_jump_if_false(needs_await.as_ref().unwrap(), &skip_final_await);
                self.emit_jump_if_true(has_awaited.as_ref().unwrap(), &skip_final_await);
                emit_await_undefined(self);
                self.emit_label(&skip_final_await);
            }

            // If any dispose threw, throw the pending error (which may be a SuppressedError chain).
            // This takes priority over break/continue/return completions.
            {
                let after_dispose_check = self.new_label();
                self.emit_jump_if_false(&dispose_threw, &after_dispose_check);
                self.emit_throw(Some(pending_error.clone()));
                self.emit_label(&after_dispose_check);
            }

            self.emit_finally_completion(&mut finally_context.borrow_mut(), &done);
            self.emit_label(&done);
        }

        self.pop_finally_control_flow_scope();
        self.using_scope_stack.pop();
    }

    // BytecodeGenerator.cpp:4956
    pub fn emit_body_with_using_if_needed(
        &mut self,
        using_count: u32,
        has_await_using: bool,
        emit_body: &mut dyn FnMut(&mut BytecodeGenerator),
    ) {
        if using_count != 0 {
            self.emit_using_body_scope(using_count, has_await_using, emit_body);
        } else {
            emit_body(self);
        }
    }

    // BytecodeGenerator.cpp:4964
    pub fn emit_enumeration(
        &mut self,
        node: &crate::parser::nodes::ThrowableExpressionData,
        subject_node: &crate::parser::nodes::Expression,
        call_back: &mut dyn FnMut(&mut BytecodeGenerator, Option<crate::bytecompiler::bytecode_generator::RegisterRef>),
        for_loop_node: Option<&crate::parser::nodes::NodeRef<crate::parser::nodes::ForOfNode>>,
        for_loop_symbol_table: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        use crate::bytecompiler::bytecode_generator::EmitAwait;
        if let Some(for_loop_node) = for_loop_node.filter(|for_loop_node| for_loop_node.borrow().is_for_await) {
            debug_assert!(
                crate::parser::parser_modes::is_async_function_parse_mode(self.parse_mode())
                    || crate::parser::parser_modes::is_module_parse_mode(self.parse_mode())
            );

            let subject = self.new_temporary();
            self.emit_node_expression(Some(subject.clone()), subject_node);

            let iterator = self.new_temporary();
            let next_method = self.new_temporary();

            self.emit_get_generic_async_iterator(Some(iterator.clone()), Some(next_method.clone()), Some(subject), node);

            let loop_done = self.new_label();

            let finally_label = self.new_label();
            let finally_context = std::rc::Rc::new(std::cell::RefCell::new(
                crate::bytecompiler::bytecode_generator::FinallyContext::new(self, finally_label),
            ));
            self.push_finally_control_flow_scope(&finally_context);

            {
                let scope = self.new_label_scope(crate::bytecompiler::label_scope::LabelScopeType::Loop, None);
                let continue_target = scope.continue_target().unwrap().clone();
                let value = self.new_temporary();
                self.emit_load_js_value(Some(value.clone()), crate::runtime::js_value::js_undefined());

                self.emit_jump(&continue_target);

                let loop_start = self.new_label();
                self.emit_label(&loop_start);
                self.emit_loop_hint();

                {
                    let loop_start = loop_start.clone();
                    let continue_target = continue_target.clone();
                    let value = value.clone();
                    let iterator = iterator.clone();
                    self.emit_try_with_finally_that_does_not_shadow_exception_with_context(
                        &finally_context,
                        &mut |generator: &mut BytecodeGenerator| {
                            call_back(generator, Some(value.clone()));
                            generator.emit_jump(&continue_target);
                        },
                        &mut |generator: &mut BytecodeGenerator| {
                            generator.emit_iterator_generic_close(&iterator, node, EmitAwait::Yes);
                        },
                    );
                    let _ = loop_start;
                }

                self.emit_label(&continue_target);
                // ASSERT(forLoopNode->isForOfNode()): garantido pelo tipo `ForOfNode`.
                self.prepare_lexical_scope_for_next_for_loop_iteration(
                    &for_loop_node.borrow().variable_environment,
                    for_loop_symbol_table.clone(),
                );
                let lexpr = for_loop_node.borrow().lexpr.clone();
                self.emit_debug_hook_expression_data(&lexpr, None);

                {
                    self.emit_async_iterator_next(
                        Some(value.clone()),
                        Some(next_method.clone()),
                        Some(iterator.clone()),
                        None,
                        node,
                    );
                    let divot = node.divot.clone();
                    self.emit_await(Some(value.clone()), &value, &divot);

                    let type_is_object = self.new_label();
                    {
                        // O temporário do C++ morre no fim da instrução: o do `done` reaproveita o registrador.
                        let temporary = self.new_temporary();
                        let is_object = self.emit_is_object(Some(temporary), &value);
                        self.emit_jump_if_true_raw(is_object.as_ref().unwrap(), &type_is_object);
                    }
                    self.emit_throw_type_error("Iterator result interface is not an object.");
                    self.emit_label(&type_is_object);

                    {
                        let temporary = self.new_temporary();
                        let done_identifier = self.property_names().done.clone();
                        let done = self.emit_get_by_id(Some(temporary), Some(value.clone()), &done_identifier);
                        self.emit_jump_if_true_raw(done.as_ref().unwrap(), &loop_done);
                    }
                    let value_identifier = self.property_names().value.clone();
                    self.emit_get_by_id(Some(value.clone()), Some(value.clone()), &value_identifier);
                    self.emit_jump(&loop_start);
                }

                let break_label_is_bound = scope.break_target_may_be_bound();
                if break_label_is_bound {
                    let break_target = scope.break_target().clone();
                    self.emit_label(&break_target);
                }
                self.pop_finally_control_flow_scope();
                if break_label_is_bound {
                    // IteratorClose sequence for break-ed control flow.
                    self.emit_iterator_generic_close(&iterator, node, EmitAwait::Yes);
                }
            }
            self.emit_label(&loop_done);
            return;
        }

        let iterable = self.new_temporary();
        self.emit_node_expression(Some(iterable.clone()), subject_node);

        let next_or_index = self.new_temporary();
        let iterator = self.new_temporary();
        {
            self.emit_expression_info(&node.divot, &node.divot_start, &node.divot_end);
            let temporary = self.new_temporary();
            let iterator_symbol_identifier = self.property_names().iterator_symbol.clone();
            let iterator_symbol =
                self.emit_get_by_id(Some(temporary), Some(iterable.clone()), &iterator_symbol_identifier);
            let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 0);
            let this_register = args.this_register();
            self.move_register(this_register.as_ref(), &iterable);
            self.emit_iterator_open(
                &iterator,
                &next_or_index,
                iterator_symbol.as_ref().unwrap(),
                &mut args,
                node,
            );
        }

        let loop_done = self.new_label();

        // RefPtr<RegisterID> iterator's lifetime must be longer than IteratorCloseContext.
        let finally_label = self.new_label();
        let finally_context = std::rc::Rc::new(std::cell::RefCell::new(
            crate::bytecompiler::bytecode_generator::FinallyContext::new(self, finally_label),
        ));
        self.push_finally_control_flow_scope(&finally_context);

        {
            let scope = self.new_label_scope(crate::bytecompiler::label_scope::LabelScopeType::Loop, None);
            let continue_target = scope.continue_target().unwrap().clone();
            let value = self.new_temporary();
            self.emit_load_js_value(Some(value.clone()), crate::runtime::js_value::js_undefined());

            let loop_start = self.new_label();
            self.emit_label(&loop_start);
            self.emit_label(&continue_target);
            self.emit_loop_hint();

            if let Some(for_loop_node) = for_loop_node {
                // ASSERT(forLoopNode->isForOfNode()): garantido pelo tipo `ForOfNode`.
                self.prepare_lexical_scope_for_next_for_loop_iteration(
                    &for_loop_node.borrow().variable_environment,
                    for_loop_symbol_table.clone(),
                );
                let lexpr = for_loop_node.borrow().lexpr.clone();
                self.emit_debug_hook_expression_data(&lexpr, None);
            }

            {
                let done = self.new_temporary();
                let mut next_args = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 0);
                let this_register = next_args.this_register();
                self.move_register(this_register.as_ref(), &iterator);

                self.emit_iterator_next(
                    &done,
                    &value,
                    &iterable,
                    &next_or_index,
                    &mut next_args,
                    node,
                );
                self.emit_jump_if_true(&done, &loop_done);
            }

            {
                let loop_start = loop_start.clone();
                let value = value.clone();
                let iterator = iterator.clone();
                self.emit_try_with_finally_that_does_not_shadow_exception_with_context(
                    &finally_context,
                    &mut |generator: &mut BytecodeGenerator| {
                        call_back(generator, Some(value.clone()));
                        generator.emit_jump(&loop_start);
                    },
                    &mut |generator: &mut BytecodeGenerator| {
                        generator.emit_iterator_generic_close(&iterator, node, EmitAwait::No);
                    },
                );
            }

            let break_label_is_bound = scope.break_target_may_be_bound();
            if break_label_is_bound {
                let break_target = scope.break_target().clone();
                self.emit_label(&break_target);
            }
            self.pop_finally_control_flow_scope();
            if break_label_is_bound {
                // IteratorClose sequence for break-ed control flow.
                self.emit_iterator_generic_close(&iterator, node, EmitAwait::No);
            }
        }
        self.emit_label(&loop_done);
    }
}
