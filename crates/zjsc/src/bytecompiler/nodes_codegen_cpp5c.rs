// NodesCodegen.cpp, linhas 4848 a 5191: `CaseBlockNode::tryTableSwitch` e `emitBytecodeForBlock`,
// `SwitchNode`, `LabelNode`, `ThrowNode`, `TryNode`, `ScopeNode::emitStatementsBytecode`,
// `emitProgramNodeBytecode`, `ProgramNode`, `ModuleProgramNode` e `EvalNode` (incluído por include!,
// sem `use`). Pára antes de `FunctionNode::emitBytecode` (linha 5193), que fica para a fatia seguinte.
//
// Convenções: as mesmas da cpp5b (`Cpp5bReg`, `Cpp5bSwitchKind`, `cpp5b_process_clause_list`).
// `Ref<Label>`/`RefPtr<Label>` é `LabelRef`; `std::optional<FinallyContext>` é `Option<Rc<RefCell<..>>>`
// porque o gerador guarda o contexto em `ControlFlowScope`.
//
// Dependências: todas já portadas (conferido por grep em 2026-10-08). `start_line`/`start_start_offset`
// em `parser/nodes_part2.rs`, `start_offset`/`line_start_offset`/`last_line`/`using_declaration_count`/
// `has_await_using_declaration` em `parser/nodes.rs`, `emit_statements_bytecode` neste arquivo.

impl crate::parser::nodes::CaseBlockNode {
    pub fn try_table_switch(
        &self,
        literal_vector: &mut Vec<crate::parser::nodes::Expression>,
        min_num: &mut i32,
        max_num: &mut i32,
    ) -> crate::parser::nodes::SwitchType {
        use crate::parser::nodes::SwitchType;

        let mut type_for_table = Cpp5bSwitchKind::SwitchUnset;
        let mut single_character_switch = true;

        cpp5b_process_clause_list(
            &self.list1,
            literal_vector,
            &mut type_for_table,
            &mut single_character_switch,
            min_num,
            max_num,
        );
        cpp5b_process_clause_list(
            &self.list2,
            literal_vector,
            &mut type_for_table,
            &mut single_character_switch,
            min_num,
            max_num,
        );

        if literal_vector.len() < crate::parser::nodes::CaseBlockNode::TABLE_SWITCH_MINIMUM {
            return SwitchType::None;
        }

        if type_for_table == Cpp5bSwitchKind::SwitchUnset || type_for_table == Cpp5bSwitchKind::SwitchNeither {
            return SwitchType::None;
        }

        if type_for_table == Cpp5bSwitchKind::SwitchNumber {
            // `int32_t range = max_num - min_num`: a subtração em int32_t do C++ (estouro é UB lá,
            // aqui embrulha como o x86_64 faz).
            let range = max_num.wrapping_sub(*min_num);
            if *min_num <= *max_num {
                if range <= 1000
                    && ((range / literal_vector.len() as i32) as u32)
                        < crate::runtime::options_list::Options::switch_jump_table_amount_threshold()
                {
                    return SwitchType::Immediate;
                }
                return SwitchType::ImmediateList;
            }
            return SwitchType::None;
        }

        debug_assert!(type_for_table == Cpp5bSwitchKind::SwitchString);

        if single_character_switch {
            let range = max_num.wrapping_sub(*min_num);
            if *min_num <= *max_num {
                if range <= 1000
                    && ((range / literal_vector.len() as i32) as u32)
                        < crate::runtime::options_list::Options::switch_jump_table_amount_threshold()
                {
                    return SwitchType::Character;
                }
                return SwitchType::CharacterList;
            }
        }

        SwitchType::String
    }

    pub fn emit_bytecode_for_block(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        switch_expression: Cpp5bReg,
        dst: Cpp5bReg,
    ) {
        use crate::parser::nodes::SwitchType;

        let mut label_vector: Vec<crate::bytecompiler::label::LabelRef> = Vec::new();
        let mut literal_vector: Vec<crate::parser::nodes::Expression> = Vec::new();
        let mut min_num: i32 = i32::MAX;
        let mut max_num: i32 = i32::MIN;
        let switch_type = self.try_table_switch(&mut literal_vector, &mut min_num, &mut max_num);

        let default_label = generator.new_label();
        if switch_type != SwitchType::None {
            // Prepare the various labels
            for _ in 0..literal_vector.len() {
                label_vector.push(generator.new_label());
            }
            generator.begin_switch(switch_expression.clone(), switch_type);
        } else {
            // Setup jumps
            for list in [&self.list1, &self.list2] {
                let mut list = list.clone();
                while let Some(node) = list {
                    let clause_expr = node.borrow().clause.borrow().expr.clone().expect("o case tem expressão");
                    let clause_val = generator.emit_node_expression_no_dst(&clause_expr);
                    let clause_label = generator.new_label();
                    label_vector.push(clause_label.clone());
                    let temporary = generator.new_temporary();
                    let comparison = generator
                        .emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                            Some(temporary),
                            clause_val,
                            switch_expression.clone(),
                        )
                        .expect("emitEqualityOp devolve o dst");
                    generator.emit_jump_if_true_raw(&comparison, &clause_label);
                    list = node.borrow().next.clone();
                }
            }
            generator.emit_jump(&default_label);
        }

        let mut i: usize = 0;
        let mut list = self.list1.clone();
        while let Some(node) = list {
            generator.emit_label(&label_vector[i]);
            i += 1;
            node.borrow().clause.borrow().emit_bytecode(generator, dst.clone());
            list = node.borrow().next.clone();
        }

        if let Some(default_clause) = &self.default_clause {
            generator.emit_label(&default_label);
            default_clause.borrow().emit_bytecode(generator, dst.clone());
        }

        let mut list = self.list2.clone();
        while let Some(node) = list {
            generator.emit_label(&label_vector[i]);
            i += 1;
            node.borrow().clause.borrow().emit_bytecode(generator, dst.clone());
            list = node.borrow().next.clone();
        }
        if self.default_clause.is_none() {
            generator.emit_label(&default_label);
        }

        debug_assert!(i == label_vector.len());
        if switch_type != SwitchType::None {
            debug_assert!(label_vector.len() == literal_vector.len());
            generator.end_switch(&label_vector, &literal_vector, &default_label, min_num, max_num);
        }
    }
}

// ------------------------------ SwitchNode -----------------------------------

impl crate::parser::nodes::SwitchNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5bReg,
    ) {
        if generator.should_be_concerned_with_completion_value() {
            generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
        }

        let scope = generator.new_label_scope(crate::bytecompiler::label_scope::LabelScopeType::Switch, None);

        let r0 = generator.emit_node_expression_no_dst(&self.expr);

        generator.push_lexical_scope(
            &self.variable_environment,
            crate::bytecompiler::bytecode_generator::ScopeType::LetConstScope,
            crate::bytecompiler::bytecode_generator::TDZCheckOptimization::DoNotOptimize,
            crate::bytecompiler::bytecode_generator::NestedScopeType::IsNested,
            None,
            true,
        );

        generator.emit_body_with_using_if_needed(
            self.variable_environment.using_declaration_count(),
            self.variable_environment.has_await_using_declaration(),
            &mut |generator| {
                self.block.borrow().emit_bytecode_for_block(generator, r0.clone(), dst.clone());
            },
        );

        generator.pop_lexical_scope(&self.variable_environment);

        generator.emit_label(scope.break_target());
        generator.emit_profile_control_flow(self.end_offset());
    }
}

// ------------------------------ LabelNode ------------------------------------

impl crate::parser::nodes::LabelNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5bReg,
    ) {
        debug_assert!(generator.break_target(&self.name).is_none());

        let scope = generator.new_label_scope(crate::bytecompiler::label_scope::LabelScopeType::NamedLabel, Some(&self.name));
        generator.emit_node_in_tail_position_statement(dst, &self.statement);

        generator.emit_label(scope.break_target());
    }
}

// ------------------------------ ThrowNode ------------------------------------

impl crate::parser::nodes::ThrowNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp5bReg,
    ) {
        // `if (dst == generator.ignoredResult()) dst = nullptr;` : o dst não é usado depois.
        let expr = generator.emit_node_expression_no_dst(&self.expr);
        generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
        generator.emit_throw(expr);

        generator.emit_profile_control_flow(self.end_offset());
    }
}

// ------------------------------ TryNode --------------------------------------

impl crate::parser::nodes::TryNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5bReg,
    ) {
        use crate::bytecode::handler_info::HandlerType;
        use crate::bytecompiler::bytecode_generator::CompletionType;

        // NOTE: The catch and finally blocks must be labeled explicitly, so the
        // optimizer knows they may be jumped to from anywhere.
        debug_assert!(self.catch_block.is_some() || self.finally_block.is_some());

        // `registerOrNull == ignoredResult()` direto no gerador (sem closure, que prenderia o `&mut`).

        let mut try_catch_dst = dst.clone();
        if generator.should_be_concerned_with_completion_value() {
            if self.finally_block.is_some() {
                try_catch_dst = Some(generator.new_temporary());
            }

            if self.finally_block.is_some() || self.try_block.has_early_break_or_continue() {
                generator.emit_load_js_value(try_catch_dst.clone(), crate::runtime::js_value::JSValue::Undefined);
            }
        }

        let mut catch_label: Option<crate::bytecompiler::label::LabelRef> = None;
        let mut catch_end_label: Option<crate::bytecompiler::label::LabelRef> = None;
        let mut finally_label: Option<crate::bytecompiler::label::LabelRef> = None;
        let mut finally_end_label: Option<crate::bytecompiler::label::LabelRef> = None;
        let mut finally_context: Option<
            std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::FinallyContext>>,
        > = None;

        if self.finally_block.is_some() {
            let label = generator.new_label();
            finally_label = Some(label.clone());
            finally_end_label = Some(generator.new_label());

            let context = std::rc::Rc::new(std::cell::RefCell::new(
                crate::bytecompiler::bytecode_generator::FinallyContext::new(generator, label),
            ));
            generator.push_finally_control_flow_scope(&context);
            finally_context = Some(context);
        }
        if self.catch_block.is_some() {
            catch_label = Some(generator.new_label());
            catch_end_label = Some(generator.new_label());
        }

        let try_label = generator.new_emitted_label();
        let try_handler_label = if self.catch_block.is_some() {
            catch_label.clone().expect("catchLabel")
        } else {
            finally_label.clone().expect("finallyLabel")
        };
        let try_handler_type = if self.catch_block.is_some() { HandlerType::Catch } else { HandlerType::Finally };
        let try_data = generator.push_try(&try_label, &try_handler_label, try_handler_type);
        let mut finally_try_data = None;
        if self.catch_block.is_none() && self.finally_block.is_some() {
            finally_try_data = Some(try_data.clone());
        }

        let local_scope_count_before_try_block = generator.local_scope_count();

        if generator.is_ignored_dst(try_catch_dst.as_ref()) {
            generator.emit_node_in_ignore_result_position_statement(&self.try_block);
        } else {
            generator.emit_node(try_catch_dst.as_ref(), &self.try_block);
        }

        if self.catch_block.is_some() {
            if self.finally_block.is_some() {
                generator.emit_jump(&finally_label.clone().expect("finallyLabel"));
            } else {
                generator.emit_jump(&catch_end_label.clone().expect("catchEndLabel"));
            }
        }

        let try_end_label = generator.new_emitted_label();
        generator.pop_try(&try_data, &try_end_label);

        if let Some(catch_block) = &self.catch_block {
            let catch_label = catch_label.clone().expect("catchLabel");
            // Uncaught exception path: the catch block.
            generator.emit_label(&catch_label);
            let thrown_value_register = Some(generator.new_temporary());
            let completion_type_register = match &finally_context {
                Some(context) => context.borrow().completion_type_register(),
                None => None,
            };
            generator.emit_out_of_line_catch_handler(
                thrown_value_register.clone(),
                completion_type_register,
                Some(&try_data),
            );
            if generator.local_scope_count() > local_scope_count_before_try_block {
                generator.restore_scope_register();
            }

            if self.finally_block.is_some() {
                // If the catch block throws an exception and we have a finally block, then the finally
                // block should "catch" that exception.
                finally_try_data = Some(generator.push_try(
                    &catch_label,
                    &finally_label.clone().expect("finallyLabel"),
                    HandlerType::Finally,
                ));
            }

            if let Some(catch_pattern) = &self.catch_pattern {
                let scope_type = if catch_pattern.is_binding_node() {
                    crate::bytecompiler::bytecode_generator::ScopeType::CatchScopeWithSimpleParameter
                } else {
                    crate::bytecompiler::bytecode_generator::ScopeType::CatchScope
                };
                // `lexicalVariables()` do C++ é mutável por um `this` const; aqui o nó é `&self`, então a
                // cópia vale pelo par push/pop (ver errs-nodes-cpp6-5c.md).
                let mut catch_environment = self.variable_environment.lexical_variables.clone();
                generator.emit_push_catch_scope(&mut catch_environment, scope_type);
                catch_pattern.bind_value(generator, thrown_value_register.clone());
            }

            generator.emit_profile_control_flow(self.try_block.base().end_offset() + 1);

            if generator.should_be_concerned_with_completion_value() {
                generator.emit_load_js_value(try_catch_dst.clone(), crate::runtime::js_value::JSValue::Undefined);
            }

            if self.finally_block.is_some() {
                if generator.is_ignored_dst(try_catch_dst.as_ref()) {
                    generator.emit_node_in_ignore_result_position_statement(catch_block);
                } else {
                    generator.emit_node(try_catch_dst.as_ref(), catch_block);
                }
            } else {
                generator.emit_node_in_tail_position_statement(try_catch_dst.clone(), catch_block);
            }

            if self.catch_pattern.is_some() {
                let mut catch_environment = self.variable_environment.lexical_variables.clone();
                generator.pop_lexical_scope_internal(&mut catch_environment);
            }

            if self.finally_block.is_some() {
                let completion_type_register =
                    finally_context.as_ref().expect("finallyContext").borrow().completion_type_register();
                generator.emit_load_completion_type(completion_type_register, CompletionType::NORMAL);
                generator.pop_try(
                    finally_try_data.as_ref().expect("finallyTryData"),
                    &finally_label.clone().expect("finallyLabel"),
                );
            }

            generator.emit_label(&catch_end_label.clone().expect("catchEndLabel"));
            generator.emit_profile_control_flow(catch_block.base().end_offset() + 1);
        }

        if let Some(finally_block) = &self.finally_block {
            let finally_context = finally_context.clone().expect("finallyContext");
            let finally_label = finally_label.clone().expect("finallyLabel");
            let finally_end_label = finally_end_label.clone().expect("finallyEndLabel");

            generator.pop_finally_control_flow_scope();

            // Entry to the finally block for CompletionType::Throw to be generated later.
            let completion_value_register = finally_context.borrow().completion_value_register();
            let completion_type_register = finally_context.borrow().completion_type_register();
            generator.emit_out_of_line_finally_handler(
                completion_value_register,
                completion_type_register,
                finally_try_data.as_ref(),
            );

            // Entry to the finally block for CompletionTypes other than Throw.
            generator.emit_label(&finally_label);
            if generator.local_scope_count() > local_scope_count_before_try_block {
                generator.restore_scope_register();
            }

            let finally_start_offset = match &self.catch_block {
                Some(catch_block) => catch_block.base().end_offset() + 1,
                None => self.try_block.base().end_offset() + 1,
            };

            // The completion value of a finally block is ignored *just* when it is a normal completion.
            if generator.should_be_concerned_with_completion_value() {
                debug_assert!(!match (&dst, &try_catch_dst) {
                    (Some(a), Some(b)) => a.is_same_register(b),
                    (None, None) => true,
                    _ => false,
                });
                if finally_block.has_early_break_or_continue() {
                    generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
                }

                generator.emit_profile_control_flow(finally_start_offset);
                generator.emit_node_in_tail_position_statement(dst.clone(), finally_block);

                generator.move_register(dst.as_ref(), try_catch_dst.as_ref().expect("tryCatchDst"));
            } else {
                generator.emit_profile_control_flow(finally_start_offset);
                generator.emit_node_in_tail_position_statement_no_dst(finally_block);
            }

            generator.emit_finally_completion(&mut finally_context.borrow_mut(), &finally_end_label);
            generator.emit_label(&finally_end_label);
            generator.emit_profile_control_flow(finally_block.base().end_offset() + 1);
        }
    }
}

// ------------------------------ ScopeNode -----------------------------

fn cpp5c_emit_program_node_bytecode(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    scope_node: &crate::parser::nodes::ScopeNode,
) {
    use crate::interpreter::interpreter::DebugHookType;

    generator.emit_debug_hook(
        DebugHookType::WillExecuteProgram,
        &crate::parser::parser_tokens::JSTextPosition::new(
            scope_node.start_line(),
            scope_node.start_start_offset(),
            scope_node.start_line_start_offset(),
        ),
        None,
    );

    let dst_register = Some(generator.new_temporary());
    generator.emit_load_js_value(dst_register.clone(), crate::runtime::js_value::JSValue::Undefined);
    generator.emit_profile_control_flow(scope_node.start_start_offset());

    generator.emit_body_with_using_if_needed(
        scope_node.variable_environment.using_declaration_count(),
        scope_node.variable_environment.has_await_using_declaration(),
        &mut |generator| {
            scope_node.emit_statements_bytecode(generator, dst_register.clone());
        },
    );

    generator.emit_debug_hook(
        DebugHookType::DidExecuteProgram,
        &crate::parser::parser_tokens::JSTextPosition::new(
            scope_node.last_line() as i32,
            scope_node.start_offset(),
            scope_node.line_start_offset(),
        ),
        None,
    );
    generator.emit_return(dst_register);
}

impl crate::parser::nodes::ScopeNode {
    pub fn emit_statements_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5bReg,
    ) {
        let Some(statements) = &self.statements else {
            return;
        };
        statements.borrow().emit_bytecode(generator, dst);
    }
}

// ------------------------------ ProgramNode -----------------------------

impl crate::parser::nodes::ProgramNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp5bReg,
    ) {
        cpp5c_emit_program_node_bytecode(generator, &self.base);
    }
}

// ------------------------------ ModuleProgramNode --------------------

impl crate::parser::nodes::ModuleProgramNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp5bReg,
    ) {
        cpp5c_emit_program_node_bytecode(generator, &self.base);
    }
}

// ------------------------------ EvalNode -----------------------------

impl crate::parser::nodes::EvalNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp5bReg,
    ) {
        use crate::interpreter::interpreter::DebugHookType;

        generator.emit_debug_hook(
            DebugHookType::WillExecuteProgram,
            &crate::parser::parser_tokens::JSTextPosition::new(
                self.base.start_line(),
                self.base.start_start_offset(),
                self.base.start_line_start_offset(),
            ),
            None,
        );

        let dst_register = Some(generator.new_temporary());
        generator.emit_load_js_value(dst_register.clone(), crate::runtime::js_value::JSValue::Undefined);

        generator.emit_body_with_using_if_needed(
            self.base.variable_environment.using_declaration_count(),
            self.base.variable_environment.has_await_using_declaration(),
            &mut |generator| {
                self.base.emit_statements_bytecode(generator, dst_register.clone());
            },
        );

        generator.emit_debug_hook(
            DebugHookType::DidExecuteProgram,
            &crate::parser::parser_tokens::JSTextPosition::new(
                self.base.last_line() as i32,
                self.base.start_offset(),
                self.base.line_start_offset(),
            ),
            None,
        );
        generator.emit_return(dst_register);
    }
}
