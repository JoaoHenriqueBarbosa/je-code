// NodesCodegen.cpp, linhas 4431 a 4846: `ForInNode` (`tryGetBoundLocal`, `emitLoopHeader`,
// `emitBytecode`), `ForOfNode::emitBytecode`, `ContinueNode`, `BreakNode`, `ReturnNode`, `WithNode`,
// `CaseClauseNode::emitBytecode`, `SwitchKind` e `processClauseList` (incluído por include!, sem `use`).
// Pára antes de `CaseBlockNode::tryTableSwitch` (linha 4848), que fica para a fatia seguinte.
//
// Convenções desta fatia (as mesmas da cpp5): registrador é `Option<RegisterRef>` (o `RegisterID*`
// nulo do C++), o `RefPtr<RegisterID>` é o próprio `Option<RegisterRef>`, `.get()` vira `.clone()`.
// O `ForOfNode::emitBytecode` recebe `this` como `NodeRef<ForOfNode>` porque o `emitEnumeration`
// guarda o nó do laço.
//
// Dependências: todas já portadas (conferido por grep em 2026-10-08). `DestructuringPatternNode::bind_value`
// está na cpp7 (impl do enum) e `JSPropertyNameEnumerator::InitMode` em `runtime/js_property_name_enumerator.rs`.

type Cpp5bReg = Option<crate::bytecompiler::bytecode_generator::RegisterRef>;

/// `JSPropertyNameEnumerator::InitMode` (`runtime/JSPropertyNameEnumerator.h:45`).
const CPP5B_ENUMERATOR_INIT_MODE: u32 = crate::runtime::js_property_name_enumerator::INIT_MODE as u32;

impl crate::parser::nodes::ForInNode {
    pub fn try_get_bound_local(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Cpp5bReg {
        if self.lexpr.is_resolve_node() {
            let ident = self.lexpr.as_resolve_node().unwrap().borrow().ident.clone();
            return generator
                .variable(&ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local)
                .local();
        }

        if self.lexpr.is_destructuring_node() {
            let assign_node = self.lexpr.as_destructuring_node().unwrap();
            let binding = assign_node.borrow().bindings.clone();
            let crate::parser::nodes::DestructuringPatternNode::Binding(simple_binding) = &binding else {
                return None;
            };

            let ident = simple_binding.borrow().bound_property.clone();
            let var = generator.variable(&ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            if var.is_special() {
                return None;
            }
            return var.local();
        }

        None
    }

    /// O lambda `lambdaEmitResolveVariable` do `emitLoopHeader`.
    fn emit_resolve_variable_for_loop_header(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        property_name: &Cpp5bReg,
        ident: &crate::runtime::identifier::Identifier,
    ) {
        let var = generator.variable(ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        if let Some(local) = var.local() {
            if var.is_read_only() {
                generator.emit_read_only_exception_if_needed(&var);
            }
            generator.move_register(Some(&local), property_name.as_ref().unwrap());
        } else {
            if generator.ecma_mode().is_strict() {
                generator.emit_expression_info(
                    &self.throwable.divot,
                    &self.throwable.divot_start,
                    &self.throwable.divot_end,
                );
            }
            if var.is_read_only() {
                generator.emit_read_only_exception_if_needed(&var);
            }
            let scope = generator.emit_resolve_scope(None, &var);
            generator.emit_expression_info(
                &self.throwable.divot,
                &self.throwable.divot_start,
                &self.throwable.divot_end,
            );
            generator.emit_put_to_scope(
                scope,
                &var,
                property_name.clone(),
                if generator.ecma_mode().is_strict() {
                    crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound
                } else {
                    crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound
                },
                crate::runtime::get_put_info::InitializationMode::NotInitialization,
            );
        }
        let start = self.lexpr.base().position().clone();
        let end = start.clone() + ident.length();
        generator.emit_profile_type_variable(property_name.clone(), &var, &start, &end);
    }

    pub fn emit_loop_header(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        property_name: Cpp5bReg,
    ) {
        if self.lexpr.is_resolve_node() {
            let ident = self.lexpr.as_resolve_node().unwrap().borrow().ident.clone();
            self.emit_resolve_variable_for_loop_header(generator, &property_name, &ident);
            return;
        }

        if self.lexpr.is_assign_resolve_node() {
            let ident = self.lexpr.as_assign_resolve_node().unwrap().borrow().ident.clone();
            self.emit_resolve_variable_for_loop_header(generator, &property_name, &ident);
            return;
        }

        if self.lexpr.is_dot_accessor_node() {
            let assign_node = self.lexpr.as_dot_accessor_node().unwrap();
            let assign_node = assign_node.borrow();
            let base = generator.emit_node_expression_no_dst(&assign_node.base.base_expr);
            generator.emit_expression_info(
                &assign_node.throwable.divot,
                &assign_node.throwable.divot_start,
                &assign_node.throwable.divot_end,
            );
            assign_node.emit_put_property(generator, base, property_name.clone());
            generator.emit_profile_type_divots(
                property_name,
                &assign_node.throwable.divot_start,
                &assign_node.throwable.divot_end,
            );
            return;
        }

        if self.lexpr.is_bracket_accessor_node() {
            let assign_node = self.lexpr.as_bracket_accessor_node().unwrap();
            let assign_node = assign_node.borrow();
            let base = generator.emit_node_expression_no_dst(&assign_node.base_expr);
            let subscript = generator.emit_node_for_property(&assign_node.subscript);
            generator.emit_expression_info(
                &assign_node.throwable.divot,
                &assign_node.throwable.divot_start,
                &assign_node.throwable.divot_end,
            );
            if assign_node.base_expr.is_super_node() {
                let this_value = Some(generator.ensure_this());
                generator.emit_put_by_val_with_this(base, this_value, subscript, property_name.clone());
            } else {
                generator.emit_put_by_val(base, subscript, property_name.clone());
            }
            generator.emit_profile_type_divots(
                property_name,
                &assign_node.throwable.divot_start,
                &assign_node.throwable.divot_end,
            );
            return;
        }

        if self.lexpr.is_destructuring_node() {
            let assign_node = self.lexpr.as_destructuring_node().unwrap();
            let binding = assign_node.borrow().bindings.clone();
            let crate::parser::nodes::DestructuringPatternNode::Binding(simple_binding) = &binding else {
                binding.bind_value(generator, property_name);
                return;
            };

            let simple_binding = simple_binding.borrow();
            let ident = simple_binding.bound_property.clone();
            let var = generator.variable(&ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            let Some(local) = var.local().filter(|_| !var.is_special()) else {
                binding.bind_value(generator, property_name);
                return;
            };
            generator.move_register(Some(&local), property_name.as_ref().unwrap());
            generator.emit_profile_type_variable(
                property_name,
                &var,
                &simple_binding.divot_start,
                &simple_binding.divot_end,
            );
            return;
        }

        unreachable!("RELEASE_ASSERT_NOT_REACHED");
    }

    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5bReg,
    ) {
        if !self.lexpr.is_assign_resolve_node() && !self.lexpr.is_assignment_location() {
            debug_assert!(self.lexpr.is_function_call());
            generator.emit_node_expression_no_dst(&self.lexpr);
            self.throwable.emit_throw_reference_error(generator, "Left side of for-in statement is not a reference.", None);
            return;
        }

        if generator.should_be_concerned_with_completion_value() && self.statement.has_early_break_or_continue() {
            generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
        }

        let mut for_loop_symbol_table: Cpp5bReg = None;
        generator.push_lexical_scope(
            &self.variable_environment,
            crate::bytecompiler::bytecode_generator::ScopeType::LetConstScope,
            crate::bytecompiler::bytecode_generator::TDZCheckOptimization::Optimize,
            crate::bytecompiler::bytecode_generator::NestedScopeType::IsNested,
            Some(&mut for_loop_symbol_table),
            true,
        );

        if self.lexpr.is_assign_resolve_node() {
            generator.emit_node_in_ignore_result_position_expression(&self.lexpr);
        }

        let base = Some(generator.new_temporary());

        generator.emit_node_expression(base.clone(), &self.expr);
        let local = self.try_get_bound_local(generator);

        let base_variable = generator.try_resolve_variable(&self.expr);

        let profiler_start_offset = self.statement.base().start_offset();
        let profiler_end_offset = self.statement.base().end_offset() + if self.statement.is_block() { 1 } else { 0 };

        {
            let enumerator = Some(generator.new_temporary());
            let mode_temp = Some(generator.new_temporary());
            let mode = generator.emit_load_js_value(
                mode_temp,
                crate::runtime::js_value::JSValue::from_u32(CPP5B_ENUMERATOR_INIT_MODE),
            );
            let index_temp = Some(generator.new_temporary());
            let index = generator.emit_load_js_value(index_temp, crate::runtime::js_value::js_number_i32(0));
            let property_name = Some(generator.new_temporary());
            let scope = generator.new_label_scope(crate::bytecompiler::label_scope::LabelScopeType::Loop, None);

            let enumerator_dst = Some(generator.new_temporary());
            let enumerator = generator.emit_get_property_enumerator(enumerator_dst, base.as_ref().unwrap()).or(enumerator);
            generator.emit_jump_if_empty_property_name_enumerator(
                enumerator.as_ref().unwrap(),
                scope.break_target(),
            );

            generator.emit_label(scope.continue_target().expect("o laço sempre tem continueTarget"));
            generator.emit_loop_hint();
            generator.prepare_lexical_scope_for_next_for_loop_iteration(
                &self.variable_environment,
                for_loop_symbol_table.clone(),
            );
            // Pause at the assignment expression for each for..in iteration.
            generator.emit_debug_hook_expression_data(&self.lexpr, None);

            // FIXME: We should have a way to see if anyone is actually using the propertyName for something other than a get_by_val. If not, we could eliminate the toString in this opcode.
            generator.emit_enumerator_next(
                property_name.as_ref().unwrap(),
                mode.as_ref().unwrap(),
                index.as_ref().unwrap(),
                base.as_ref().unwrap(),
                enumerator.as_ref().unwrap(),
            );
            generator.emit_jump_if_sentinel_string(property_name.as_ref().unwrap(), scope.break_target());

            self.emit_loop_header(generator, property_name.clone());

            generator.emit_profile_control_flow(profiler_start_offset);

            generator.push_for_in_scope(
                local.clone(),
                property_name.clone(),
                index.clone(),
                enumerator.clone(),
                mode.clone(),
                base_variable,
            );
            // Os `RefPtr<RegisterID>` do C++ vivem até o fim do bloco; mover para `push_for_in_scope` (que
            // retorna cedo sem `local`) soltaria `mode`, `index` e `enumerator` e o corpo reusaria os registros.
            generator.emit_node(dst.as_ref(), &self.statement);
            generator.pop_for_in_scope(local);

            generator.emit_profile_control_flow(profiler_end_offset);
            generator.emit_jump(scope.continue_target().expect("o laço sempre tem continueTarget"));

            generator.emit_label(scope.break_target());
        }

        generator.pop_lexical_scope(&self.variable_environment);
        generator.emit_profile_control_flow(profiler_end_offset);
    }
}

impl crate::parser::nodes::ForOfNode {
    pub fn emit_bytecode(
        this: &crate::parser::nodes::NodeRef<crate::parser::nodes::ForOfNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5bReg,
    ) {
        let node = this.borrow();
        if !node.lexpr.is_assignment_location() {
            debug_assert!(node.lexpr.is_function_call());
            generator.emit_node_expression_no_dst(&node.lexpr);
            node.throwable.emit_throw_reference_error(generator, "Left side of for-of statement is not a reference.", None);
            return;
        }

        if generator.should_be_concerned_with_completion_value() && node.statement.has_early_break_or_continue() {
            generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
        }

        let mut for_loop_symbol_table: Cpp5bReg = None;
        generator.push_lexical_scope(
            &node.variable_environment,
            crate::bytecompiler::bytecode_generator::ScopeType::LetConstScope,
            crate::bytecompiler::bytecode_generator::TDZCheckOptimization::Optimize,
            crate::bytecompiler::bytecode_generator::NestedScopeType::IsNested,
            Some(&mut for_loop_symbol_table),
            true,
        );
        let is_using_declaration = node.variable_environment.has_using_declaration();
        let is_await_using_declaration = node.variable_environment.has_await_using_declaration();
        let mut extractor = |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator, value: Cpp5bReg| {
            let mut emit_body = |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator| {
                if node.lexpr.is_resolve_node() {
                    let ident = node.lexpr.as_resolve_node().unwrap().borrow().ident.clone();
                    let var = generator
                        .variable(&ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                    if let Some(local) = var.local() {
                        if var.is_read_only() && !is_using_declaration {
                            generator.emit_read_only_exception_if_needed(&var);
                        }
                        generator.move_register(Some(&local), value.as_ref().unwrap());
                    } else {
                        if generator.ecma_mode().is_strict() {
                            generator.emit_expression_info(
                                &node.throwable.divot,
                                &node.throwable.divot_start,
                                &node.throwable.divot_end,
                            );
                        }
                        if var.is_read_only() && !is_using_declaration {
                            generator.emit_read_only_exception_if_needed(&var);
                        }
                        let scope = generator.emit_resolve_scope(None, &var);
                        generator.emit_expression_info(
                            &node.throwable.divot,
                            &node.throwable.divot_start,
                            &node.throwable.divot_end,
                        );
                        generator.emit_put_to_scope(
                            scope,
                            &var,
                            value.clone(),
                            if generator.ecma_mode().is_strict() {
                                crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound
                            } else {
                                crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound
                            },
                            if is_using_declaration {
                                crate::runtime::get_put_info::InitializationMode::ConstInitialization
                            } else {
                                crate::runtime::get_put_info::InitializationMode::NotInitialization
                            },
                        );
                    }
                    let start = node.lexpr.base().position().clone();
                    let end = start.clone() + ident.length();
                    generator.emit_profile_type_variable(value.clone(), &var, &start, &end);
                    if is_using_declaration {
                        generator.emit_prepare_disposable(value.clone(), &node.throwable.divot_start, is_await_using_declaration);
                    }
                } else if node.lexpr.is_dot_accessor_node() {
                    let assign_node = node.lexpr.as_dot_accessor_node().unwrap();
                    let assign_node = assign_node.borrow();
                    let base = generator.emit_node_expression_no_dst(&assign_node.base.base_expr);
                    generator.emit_expression_info(
                        &assign_node.throwable.divot,
                        &assign_node.throwable.divot_start,
                        &assign_node.throwable.divot_end,
                    );
                    assign_node.emit_put_property(generator, base, value.clone());
                    generator.emit_profile_type_divots(
                        value.clone(),
                        &assign_node.throwable.divot_start,
                        &assign_node.throwable.divot_end,
                    );
                } else if node.lexpr.is_bracket_accessor_node() {
                    let assign_node = node.lexpr.as_bracket_accessor_node().unwrap();
                    let assign_node = assign_node.borrow();
                    let base = generator.emit_node_expression_no_dst(&assign_node.base_expr);
                    let subscript = generator.emit_node_for_property(&assign_node.subscript);

                    generator.emit_expression_info(
                        &assign_node.throwable.divot,
                        &assign_node.throwable.divot_start,
                        &assign_node.throwable.divot_end,
                    );
                    if assign_node.base_expr.is_super_node() {
                        let this_value = Some(generator.ensure_this());
                        generator.emit_put_by_val_with_this(base, this_value, subscript, value.clone());
                    } else {
                        generator.emit_put_by_val(base, subscript, value.clone());
                    }
                    generator.emit_profile_type_divots(
                        value.clone(),
                        &assign_node.throwable.divot_start,
                        &assign_node.throwable.divot_end,
                    );
                } else {
                    debug_assert!(node.lexpr.is_destructuring_node());
                    let assign_node = node.lexpr.as_destructuring_node().unwrap();
                    let bindings = assign_node.borrow().bindings.clone();
                    bindings.bind_value(generator, value.clone());
                }
                generator.emit_profile_control_flow(node.statement.base().start_offset());
                generator.emit_node(dst.as_ref(), &node.statement);
            };

            generator.emit_body_with_using_if_needed(
                if is_using_declaration { 1 } else { 0 },
                is_await_using_declaration,
                &mut |generator| {
                    emit_body(generator);
                },
            );
        };
        generator.emit_enumeration(&node.throwable, &node.expr, &mut extractor, Some(this), for_loop_symbol_table);
        generator.pop_lexical_scope(&node.variable_environment);
        generator.emit_profile_control_flow(node.statement.base().end_offset() + if node.statement.is_block() { 1 } else { 0 });
    }
}

// ------------------------------ ContinueNode ---------------------------------

impl crate::parser::nodes::ContinueNode {
    pub fn trivial_target(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Option<crate::bytecompiler::label::LabelRef> {
        if generator.should_emit_debug_hooks() {
            return None;
        }

        let scope = generator.continue_target(&self.ident);
        debug_assert!(scope.is_some());
        let scope = scope.unwrap();

        if generator.label_scope_depth() != scope.scope_depth() {
            return None;
        }

        scope.continue_target().cloned()
    }

    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp5bReg,
    ) {
        let scope = generator.continue_target(&self.ident);
        debug_assert!(scope.is_some());
        let scope = scope.unwrap();
        let continue_target = scope.continue_target().expect("o laço sempre tem continueTarget").clone();

        let has_finally = generator.emit_jump_via_finally_if_needed(scope.scope_depth(), &continue_target);
        if !has_finally {
            let lexical_scope_index = generator.label_scope_depth_to_lexical_scope_index(scope.scope_depth());
            generator.restore_scope_register_at(lexical_scope_index);
            generator.emit_jump(&continue_target);
        }

        generator.emit_profile_control_flow(self.end_offset());
    }
}

// ------------------------------ BreakNode ------------------------------------

impl crate::parser::nodes::BreakNode {
    pub fn trivial_target(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Option<crate::bytecompiler::label::LabelRef> {
        if generator.should_emit_debug_hooks() {
            return None;
        }

        let scope = generator.break_target(&self.ident);
        debug_assert!(scope.is_some());
        let scope = scope.unwrap();

        if generator.label_scope_depth() != scope.scope_depth() {
            return None;
        }

        Some(scope.break_target().clone())
    }

    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp5bReg,
    ) {
        let scope = generator.break_target(&self.ident);
        debug_assert!(scope.is_some());
        let scope = scope.unwrap();
        let break_target = scope.break_target().clone();

        let has_finally = generator.emit_jump_via_finally_if_needed(scope.scope_depth(), &break_target);
        if !has_finally {
            let lexical_scope_index = generator.label_scope_depth_to_lexical_scope_index(scope.scope_depth());
            generator.restore_scope_register_at(lexical_scope_index);
            generator.emit_jump(&break_target);
        }

        generator.emit_profile_control_flow(self.end_offset());
    }
}

// ------------------------------ ReturnNode -----------------------------------

impl crate::parser::nodes::ReturnNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        mut dst: Cpp5bReg,
    ) {
        debug_assert!(generator.code_type() == crate::bytecode::code_type::CodeType::FunctionCode);

        if let Some(register) = &dst {
            if generator.is_ignored_result(register) {
                dst = None;
            }
        }

        let mut return_register: Cpp5bReg;
        if let Some(value) = &self.value {
            // When in a finally scope, we must not use tail call optimization because
            // the finally block must execute before actually returning.
            if generator.has_finally_scopes() {
                return_register = generator.emit_node_expression(dst, value);
            } else {
                return_register = generator.emit_node_in_tail_position_from_return_node(dst, value);
            }
            if generator.parse_mode() == crate::parser::parser_modes::SourceParseMode::AsyncGeneratorBodyMode {
                let temp = generator.new_temporary();
                // `newTemporary()` cru no C++ (contagem 0): o `jstricteq` do await reaproveita o slot.
                let raw_temp = temp.get().clone();
                let position = self.position();
                let src = return_register.as_ref().unwrap();
                return_register = generator.with_raw_register(Some(&raw_temp), |generator| generator.emit_await(Some(temp), src, position));
            }
        } else {
            return_register = generator.emit_load_js_value(dst, crate::runtime::js_value::JSValue::Undefined);
        }

        generator.emit_profile_type_flag_divots(
            return_register.clone(),
            crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag::ProfileTypeBytecodeFunctionReturnStatement,
            &self.throwable.divot_start,
            &self.throwable.divot_end,
        );

        let has_finally = generator.emit_return_via_finally_if_needed(return_register.as_ref().unwrap());
        if !has_finally {
            generator.emit_will_leave_call_frame_debug_hook();
            generator.emit_return(return_register);
        }

        generator.emit_profile_control_flow(self.end_offset());
        // Emitting an unreachable return here is needed in case this op_profile_control_flow is the
        // last opcode in a CodeBlock because a CodeBlock's instructions must end with a terminal opcode.
        if generator.should_emit_control_flow_profiler_hooks() {
            let undefined = generator.emit_load_js_value(None, crate::runtime::js_value::JSValue::Undefined);
            generator.emit_return(undefined);
        }
    }
}

// ------------------------------ WithNode -------------------------------------

impl crate::parser::nodes::WithNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5bReg,
    ) {
        let scope = generator.emit_node_expression_no_dst(&self.expr);
        let divot_start = self.divot.clone() - self.expression_length;
        generator.emit_expression_info(&self.divot, &divot_start, &self.divot);
        generator.emit_push_with_scope(scope.clone());
        if generator.should_be_concerned_with_completion_value() && self.statement.has_early_break_or_continue() {
            generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
        }
        generator.emit_node_in_tail_position_statement(dst, &self.statement);
        generator.emit_pop_with_scope();
    }
}

// ------------------------------ CaseClauseNode --------------------------------

impl crate::parser::nodes::CaseClauseNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5bReg,
    ) {
        generator.emit_profile_control_flow(self.start_offset);
        let Some(statements) = &self.statements else {
            return;
        };
        statements.borrow().emit_bytecode(generator, dst);
    }
}

// ------------------------------ CaseBlockNode --------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cpp5bSwitchKind {
    SwitchUnset = 0,
    SwitchNumber = 1,
    SwitchString = 2,
    SwitchNeither = 3,
}

fn cpp5b_process_clause_list(
    list: &Option<crate::parser::nodes::NodeRef<crate::parser::nodes::ClauseListNode>>,
    literal_vector: &mut Vec<crate::parser::nodes::Expression>,
    type_for_table: &mut Cpp5bSwitchKind,
    single_character_switch: &mut bool,
    min_num: &mut i32,
    max_num: &mut i32,
) {
    let mut list = list.clone();
    while let Some(node) = list {
        let clause_expression = node.borrow().clause.borrow().expr.clone().expect("o case tem expressão");
        literal_vector.push(clause_expression.clone());
        if clause_expression.is_number() {
            let value = clause_expression.as_number_node().unwrap().value;
            let int_val = crate::wtf::math_extras::truncate_double_to_int32(value);
            // `(typeForTable & ~SwitchNumber)`: qualquer bit fora de SwitchNumber.
            if ((*type_for_table as i32) & !(Cpp5bSwitchKind::SwitchNumber as i32)) != 0 || (int_val as f64) != value {
                *type_for_table = Cpp5bSwitchKind::SwitchNeither;
                break;
            }
            if int_val < *min_num {
                *min_num = int_val;
            }
            if int_val > *max_num {
                *max_num = int_val;
            }
            *type_for_table = Cpp5bSwitchKind::SwitchNumber;
            list = node.borrow().next.clone();
            continue;
        }
        if clause_expression.is_string() {
            if ((*type_for_table as i32) & !(Cpp5bSwitchKind::SwitchString as i32)) != 0 {
                *type_for_table = Cpp5bSwitchKind::SwitchNeither;
                break;
            }
            let string_node = clause_expression.as_string_node().unwrap();
            let value = string_node.borrow().value.clone();
            let value = value.string();
            *single_character_switch &= value.length() == 1;
            if *single_character_switch {
                let int_val = value.char_at(0) as i32;
                if int_val < *min_num {
                    *min_num = int_val;
                }
                if int_val > *max_num {
                    *max_num = int_val;
                }
            }
            *type_for_table = Cpp5bSwitchKind::SwitchString;
            list = node.borrow().next.clone();
            continue;
        }
        *type_for_table = Cpp5bSwitchKind::SwitchNeither;
        break;
    }
}
