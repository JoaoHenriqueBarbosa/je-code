// NodesCodegen.cpp, linhas 3633 a 4333: `emitReadModifyAssignment` até `IfElseNode::emitBytecode`
// (incluído por include!, sem `use`; usa o `Cpp4Reg` da fatia anterior,
// `cpp4`, e o `cpp2_string_value` da `cpp2`).
//
// Mesmas convenções da `cpp4`: registrador é `Option<RegisterRef>` (o `RegisterID*` nulo do C++),
// o `RefPtr<RegisterID>` é o próprio `Option<RegisterRef>`, `.get()` vira `.clone()`.

/// `generator.ecmaMode().isStrict() ? ThrowIfNotFound : DoNotThrowIfNotFound`, repetido em todo
/// `emitPutToScope` desta fatia.
fn cpp4b_put_resolve_mode(
    generator: &crate::bytecompiler::bytecode_generator::BytecodeGenerator,
) -> crate::runtime::get_put_info::ResolveMode {
    if generator.ecma_mode().is_strict() {
        crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound
    } else {
        crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound
    }
}

/// `OperandTypes(ResultType::unknownType(), m_right->resultDescriptor())`.
fn cpp4b_unknown_with(right: &crate::parser::nodes::Expression) -> crate::parser::result_type::OperandTypes {
    crate::parser::result_type::OperandTypes::new(crate::parser::result_type::ResultType::unknown_type(), right.result_descriptor())
}

// ------------------------------ ReadModifyResolveNode -----------------------------------

// FIXME do C++: isso deveria ser um método do BytecodeGenerator?
#[allow(clippy::too_many_arguments)]
fn cpp4b_emit_read_modify_assignment(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    dst: Cpp4Reg,
    src1: Cpp4Reg,
    right: &crate::parser::nodes::Expression,
    oper: crate::parser::nodes::Operator,
    types: crate::parser::result_type::OperandTypes,
    emit_expression_info_for_me: Option<&crate::parser::nodes::ReadModifyResolveNode>,
    emit_read_only_exception_if_needed_for_me: Option<&crate::bytecompiler::bytecode_generator::Variable>,
) -> Cpp4Reg {
    use crate::bytecode::opcode::OpcodeID;
    use crate::parser::nodes::Operator;

    let opcode_id = match oper {
        Operator::MultEq => OpcodeID::op_mul,
        Operator::DivEq => OpcodeID::op_div,
        Operator::PlusEq => {
            if right.is_add() && right.result_descriptor().definitely_is_string() {
                let result = match right {
                    crate::parser::nodes::Expression::Add(add) => {
                        add.borrow().base.emit_strcat(generator, dst, src1, emit_expression_info_for_me)
                    }
                    _ => unreachable!("isAdd() sem AddNode"),
                };
                if let Some(variable) = emit_read_only_exception_if_needed_for_me {
                    generator.emit_read_only_exception_if_needed(variable);
                }
                return result;
            }

            OpcodeID::op_add
        }
        Operator::MinusEq => OpcodeID::op_sub,
        Operator::LShift => OpcodeID::op_lshift,
        Operator::RShift => OpcodeID::op_rshift,
        Operator::URShift => OpcodeID::op_urshift,
        Operator::BitAndEq => OpcodeID::op_bitand,
        Operator::BitXOrEq => OpcodeID::op_bitxor,
        Operator::BitOrEq => OpcodeID::op_bitor,
        Operator::ModEq => OpcodeID::op_mod,
        Operator::PowEq => OpcodeID::op_pow,
        _ => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
    };

    let src2 = generator.emit_node_expression_no_dst(right);

    if let Some(variable) = emit_read_only_exception_if_needed_for_me {
        let threw_exception = generator.emit_read_only_exception_if_needed(variable);
        if threw_exception {
            return src2;
        }
    }

    // Certain read-modify nodes require expression info to be emitted *after* m_right has been generated.
    // If this is required the node is passed as 'emitExpressionInfoForMe'; do so now.
    if let Some(node) = emit_expression_info_for_me {
        generator.emit_expression_info(&node.throwable.divot, &node.throwable.divot_start, &node.throwable.divot_end);
    }

    let result = generator.emit_binary_op_dynamic(opcode_id, dst, src1, src2, types);
    if oper == Operator::URShift {
        return generator.emit_unary_op::<crate::bytecode::bytecode_ops::OpUnsigned>(result.clone(), result);
    }
    result
}

impl crate::parser::nodes::ReadModifyResolveNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let divot_start = self.throwable.divot_start;
        let divot_end = self.throwable.divot_end;
        let new_divot = divot_start + self.ident.length();
        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        if let Some(local) = var.local() {
            generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);
            if var.is_read_only() {
                let final_dst = Some(generator.final_destination(dst.as_ref(), None));
                let result = cpp4b_emit_read_modify_assignment(
                    generator,
                    final_dst,
                    Some(local),
                    &self.right,
                    self.operator,
                    cpp4b_unknown_with(&self.right),
                    None,
                    Some(&var),
                );
                generator.emit_profile_type_divots(result.clone(), &divot_start, &divot_end);
                return result;
            }

            let right_is_pure = self.right.is_pure(generator);
            if generator.left_hand_side_needs_copy(self.right_has_assignments, right_is_pure) {
                let result = Some(generator.new_temporary());
                generator.move_register(result.as_ref(), &local);
                cpp4b_emit_read_modify_assignment(
                    generator,
                    result.clone(),
                    result.clone(),
                    &self.right,
                    self.operator,
                    cpp4b_unknown_with(&self.right),
                    None,
                    None,
                );
                generator.move_register(Some(&local), result.as_ref().unwrap());
                generator.emit_profile_type_divots(Some(local), &divot_start, &divot_end);
                return generator.move_register(dst.as_ref(), result.as_ref().unwrap());
            }

            let result = cpp4b_emit_read_modify_assignment(
                generator,
                Some(local.clone()),
                Some(local),
                &self.right,
                self.operator,
                cpp4b_unknown_with(&self.right),
                None,
                None,
            );
            generator.emit_profile_type_divots(result.clone(), &divot_start, &divot_end);
            return generator.move_register(dst.as_ref(), result.as_ref().unwrap());
        }

        generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
        let scope = generator.emit_resolve_scope(None, &var);
        let temporary = Some(generator.new_temporary());
        let value = generator.emit_get_from_scope(temporary, scope.clone(), &var, crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound);
        generator.emit_tdz_check_if_necessary(&var, value.clone(), None);
        let final_dst = Some(generator.final_destination(dst.as_ref(), value.as_ref()));
        let result = cpp4b_emit_read_modify_assignment(
            generator,
            final_dst,
            value,
            &self.right,
            self.operator,
            cpp4b_unknown_with(&self.right),
            Some(self),
            if var.is_read_only() { Some(&var) } else { None },
        );
        let mut return_result = result.clone();
        if !var.is_read_only() {
            let resolve_mode = cpp4b_put_resolve_mode(generator);
            return_result = generator.emit_put_to_scope(
                scope,
                &var,
                result.clone(),
                resolve_mode,
                crate::runtime::get_put_info::InitializationMode::NotInitialization,
            );
            generator.emit_profile_type_variable(result, &var, &divot_start, &divot_end);
        }
        return_result
    }
}

// ------------------------------ ShortCircuitReadModifyResolveNode -----------------------------------

fn cpp4b_emit_short_circuit_assignment(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    value: &crate::bytecompiler::bytecode_generator::RegisterRef,
    oper: crate::parser::nodes::Operator,
    after_assignment: &crate::bytecompiler::label::LabelRef,
) {
    match oper {
        crate::parser::nodes::Operator::CoalesceEq => {
            let temporary = Some(generator.new_temporary());
            let is_nullish = generator.emit_is_undefined_or_null(temporary, value);
            generator.emit_jump_if_false_raw(is_nullish.as_ref().unwrap(), after_assignment);
        }
        crate::parser::nodes::Operator::OrEq => generator.emit_jump_if_true(value, after_assignment),
        crate::parser::nodes::Operator::AndEq => generator.emit_jump_if_false(value, after_assignment),
        _ => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
    }
}

impl crate::parser::nodes::ShortCircuitReadModifyResolveNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let divot = self.throwable.divot;
        let divot_start = self.throwable.divot_start;
        let divot_end = self.throwable.divot_end;
        let new_divot = divot_start + self.ident.length();

        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let is_read_only = var.is_read_only();

        if let Some(local) = var.local() {
            generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);

            if is_read_only {
                let result = Some(generator.temp_destination(dst.as_ref()));
                generator.move_register(result.as_ref(), &local);

                let after_assignment = generator.new_label();
                cpp4b_emit_short_circuit_assignment(generator, result.as_ref().unwrap(), self.operator, &after_assignment);

                generator.emit_node_expression(result.clone(), &self.right); // Execute side effects first.
                let threw_exception = generator.emit_read_only_exception_if_needed(&var);

                if !threw_exception {
                    generator.emit_profile_type_divots(result.clone(), &divot_start, &divot_end);
                }

                generator.emit_label(&after_assignment);
                return generator.move_register(dst.as_ref(), result.as_ref().unwrap());
            }

            let right_is_pure = self.right.is_pure(generator);
            if generator.left_hand_side_needs_copy(self.right_has_assignments, right_is_pure) {
                let result = Some(generator.temp_destination(dst.as_ref()));
                generator.move_register(result.as_ref(), &local);

                let after_assignment = generator.new_label();
                cpp4b_emit_short_circuit_assignment(generator, result.as_ref().unwrap(), self.operator, &after_assignment);

                generator.emit_node_expression(result.clone(), &self.right);
                generator.move_register(Some(&local), result.as_ref().unwrap());
                generator.emit_profile_type_variable(result.clone(), &var, &divot_start, &divot_end);

                generator.emit_label(&after_assignment);
                return generator.move_register(dst.as_ref(), result.as_ref().unwrap());
            }

            let result = Some(local);

            let after_assignment = generator.new_label();
            cpp4b_emit_short_circuit_assignment(generator, result.as_ref().unwrap(), self.operator, &after_assignment);

            generator.emit_node_expression(result.clone(), &self.right);
            generator.emit_profile_type_variable(result.clone(), &var, &divot_start, &divot_end);

            generator.emit_label(&after_assignment);
            return generator.move_register(dst.as_ref(), result.as_ref().unwrap());
        }

        generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
        let scope = generator.emit_resolve_scope(None, &var);

        let unchecked_result = Some(generator.new_temporary());

        generator.emit_get_from_scope(unchecked_result.clone(), scope.clone(), &var, crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound);
        generator.emit_tdz_check_if_necessary(&var, unchecked_result.clone(), None);

        let after_assignment = generator.new_label();
        cpp4b_emit_short_circuit_assignment(generator, unchecked_result.as_ref().unwrap(), self.operator, &after_assignment);

        generator.emit_node_expression(unchecked_result.clone(), &self.right); // Execute side effects first.

        let threw_exception = is_read_only && generator.emit_read_only_exception_if_needed(&var);

        if !threw_exception {
            generator.emit_expression_info(&divot, &divot_start, &divot_end);
        }

        if !is_read_only {
            let resolve_mode = cpp4b_put_resolve_mode(generator);
            generator.emit_put_to_scope(
                scope,
                &var,
                unchecked_result.clone(),
                resolve_mode,
                crate::runtime::get_put_info::InitializationMode::NotInitialization,
            );
            generator.emit_profile_type_variable(unchecked_result.clone(), &var, &divot_start, &divot_end);
        }

        generator.emit_label(&after_assignment);
        let final_dst = Some(generator.final_destination(dst.as_ref(), unchecked_result.as_ref()));
        generator.move_register(final_dst.as_ref(), unchecked_result.as_ref().unwrap())
    }
}

// ------------------------------ AssignResolveNode -----------------------------------

fn cpp4b_initialization_mode_for_assignment_context(
    assignment_context: crate::parser::nodes::AssignmentContext,
) -> crate::runtime::get_put_info::InitializationMode {
    use crate::parser::nodes::AssignmentContext;
    match assignment_context {
        AssignmentContext::DeclarationStatement => crate::runtime::get_put_info::InitializationMode::Initialization,
        AssignmentContext::ConstDeclarationStatement
        | AssignmentContext::UsingDeclarationStatement
        | AssignmentContext::AwaitUsingDeclarationStatement => crate::runtime::get_put_info::InitializationMode::ConstInitialization,
        AssignmentContext::AssignmentExpression => crate::runtime::get_put_info::InitializationMode::NotInitialization,
    }
}

fn cpp4b_is_using_or_await_using_assignment_context(context: crate::parser::nodes::AssignmentContext) -> bool {
    context == crate::parser::nodes::AssignmentContext::UsingDeclarationStatement
        || context == crate::parser::nodes::AssignmentContext::AwaitUsingDeclarationStatement
}

impl crate::parser::nodes::AssignResolveNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        use crate::parser::nodes::AssignmentContext;

        let mut dst = dst;
        let divot_start = self.throwable.divot_start;
        let divot_end = self.throwable.divot_end;
        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let is_read_only = var.is_read_only()
            && self.assignment_context != AssignmentContext::ConstDeclarationStatement
            && !cpp4b_is_using_or_await_using_assignment_context(self.assignment_context);
        let new_divot = divot_start + self.ident.length();
        if let Some(local) = var.local() {
            let result: Cpp4Reg;

            if is_read_only {
                result = generator.emit_node_expression(dst.clone(), &self.right); // Execute side effects first.

                if self.assignment_context == AssignmentContext::AssignmentExpression {
                    generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
                    generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);
                }

                generator.emit_read_only_exception_if_needed(&var);
                generator.emit_profile_type_variable(result.clone(), &var, &divot_start, &divot_end);
            } else if (self.assignment_context == AssignmentContext::AssignmentExpression && generator.needs_tdz_check(&var)) || var.is_special() {
                let temp_dst = Some(generator.temp_destination(dst.as_ref()));
                generator.emit_node_expression(temp_dst.clone(), &self.right); // Execute side effects first.

                if self.assignment_context == AssignmentContext::AssignmentExpression {
                    generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
                    generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);
                }

                generator.move_register(Some(&local), temp_dst.as_ref().unwrap());
                generator.emit_profile_type_variable(Some(local.clone()), &var, &divot_start, &divot_end);
                result = generator.move_register(dst.as_ref(), temp_dst.as_ref().unwrap());
            } else {
                let right = generator.emit_node_expression(Some(local.clone()), &self.right);
                generator.emit_profile_type_variable(right.clone(), &var, &divot_start, &divot_end);
                result = generator.move_register(dst.as_ref(), right.as_ref().unwrap());
            }

            if cpp4b_is_using_or_await_using_assignment_context(self.assignment_context) {
                generator.emit_prepare_disposable(
                    Some(local.clone()),
                    &divot_start,
                    self.assignment_context == AssignmentContext::AwaitUsingDeclarationStatement,
                );
            }

            if self.assignment_context != AssignmentContext::AssignmentExpression {
                generator.lift_tdz_check_if_possible(&var);
            }
            return result;
        }

        if generator.ecma_mode().is_strict() {
            generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
        }
        let scope = generator.emit_resolve_scope(None, &var);
        if self.assignment_context == AssignmentContext::AssignmentExpression {
            generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
            generator.emit_tdz_check_if_necessary(&var, None, scope.clone());
        }
        if generator.is_ignored_dst(dst.as_ref()) {
            dst = None;
        }
        let result = generator.emit_node_expression(dst, &self.right); // Execute side effects first.
        generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
        if is_read_only {
            let threw_exception = generator.emit_read_only_exception_if_needed(&var);
            if threw_exception {
                return result;
            }
        }
        let mut return_result = result.clone();
        if !is_read_only {
            let resolve_mode = cpp4b_put_resolve_mode(generator);
            return_result = generator.emit_put_to_scope(
                scope,
                &var,
                result.clone(),
                resolve_mode,
                cpp4b_initialization_mode_for_assignment_context(self.assignment_context),
            );
            generator.emit_profile_type_variable(result.clone(), &var, &divot_start, &divot_end);
        }

        if cpp4b_is_using_or_await_using_assignment_context(self.assignment_context) {
            generator.emit_prepare_disposable(
                result.clone(),
                &divot_start,
                self.assignment_context == AssignmentContext::AwaitUsingDeclarationStatement,
            );
        }

        if self.assignment_context != AssignmentContext::AssignmentExpression {
            generator.lift_tdz_check_if_possible(&var);
        }
        return_result
    }
}

// ------------------------------ AssignDotNode -----------------------------------

impl crate::parser::nodes::AssignDotNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let is_pure = self.right.is_pure(generator);
        let base = generator.emit_node_for_left_hand_side(&self.base.base_expr, self.right_has_assignments, is_pure);
        let value = generator.destination_for_assign_result(dst.as_ref());
        let result = generator.emit_node_expression(value, &self.right);
        generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
        let forward_result = if generator.is_ignored_dst(dst.as_ref()) {
            result
        } else {
            let temporary = Some(generator.temp_destination(result.as_ref()));
            generator.move_register(temporary.as_ref(), result.as_ref().unwrap())
        };
        self.base.emit_put_property(generator, base, forward_result.clone());
        generator.emit_profile_type_divots(forward_result.clone(), &self.throwable.divot_start, &self.throwable.divot_end);
        generator.move_register(dst.as_ref(), forward_result.as_ref().unwrap())
    }
}

// ------------------------------ ReadModifyDotNode -----------------------------------

impl crate::parser::nodes::ReadModifyDotNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let is_pure = self.right.is_pure(generator);
        let base = generator.emit_node_for_left_hand_side(&self.base.base_expr, self.right_has_assignments, is_pure);

        generator.emit_expression_info(&self.throwable.subexpression_divot(), &self.throwable.subexpression_start(), &self.throwable.subexpression_end());
        let mut this_value: Cpp4Reg = None;
        let temporary = Some(generator.temp_destination(dst.as_ref()));
        let value = self.base.emit_get_property_value_with_this(generator, temporary, base.clone(), &mut this_value);

        let final_dst = Some(generator.final_destination(dst.as_ref(), value.as_ref()));
        let updated_value = cpp4b_emit_read_modify_assignment(
            generator,
            final_dst,
            value,
            &self.right,
            self.operator,
            cpp4b_unknown_with(&self.right),
            None,
            None,
        );

        let throwable = &self.throwable.base;
        generator.emit_expression_info(&throwable.divot, &throwable.divot_start, &throwable.divot_end);
        let ret = self.base.emit_put_property_with_this(generator, base, updated_value.clone(), &mut this_value);
        generator.emit_profile_type_divots(updated_value, &throwable.divot_start, &throwable.divot_end);
        ret
    }
}

// ------------------------------ ShortCircuitReadModifyDotNode -----------------------------------

impl crate::parser::nodes::ShortCircuitReadModifyDotNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let is_pure = self.right.is_pure(generator);
        let base = generator.emit_node_for_left_hand_side(&self.base.base_expr, self.right_has_assignments, is_pure);
        let mut this_value: Cpp4Reg = None;

        let result = Some(generator.temp_destination(dst.as_ref()));

        generator.emit_expression_info(&self.throwable.subexpression_divot(), &self.throwable.subexpression_start(), &self.throwable.subexpression_end());
        self.base.emit_get_property_value_with_this(generator, result.clone(), base.clone(), &mut this_value);
        let after_assignment = generator.new_label();
        cpp4b_emit_short_circuit_assignment(generator, result.as_ref().unwrap(), self.operator, &after_assignment);

        generator.emit_node_expression(result.clone(), &self.right);
        let throwable = &self.throwable.base;
        generator.emit_expression_info(&throwable.divot, &throwable.divot_start, &throwable.divot_end);
        self.base.emit_put_property_with_this(generator, base, result.clone(), &mut this_value);
        generator.emit_profile_type_divots(result.clone(), &throwable.divot_start, &throwable.divot_end);

        generator.emit_label(&after_assignment);
        generator.move_register(dst.as_ref(), result.as_ref().unwrap())
    }
}

// ------------------------------ AssignErrorNode -----------------------------------

impl crate::parser::nodes::AssignErrorNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        debug_assert!(self.left.is_function_call());
        generator.emit_node_expression_no_dst(&self.left);
        self.throwable.emit_throw_reference_error(generator, "Left side of assignment is not a reference.", dst)
    }
}

// ------------------------------ AssignBracketNode -----------------------------------

impl crate::parser::nodes::AssignBracketNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let mut context = None;
        if let crate::parser::nodes::Expression::Resolve(resolve) = &self.subscript {
            let argument_variable = generator.variable(&resolve.borrow().ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            if argument_variable.is_local() {
                let property = argument_variable.local();
                context = generator.find_for_in_context(property.as_ref().unwrap());
            }
        }

        let subscript_is_pure = self.subscript.is_pure(generator);
        let right_is_pure = self.right.is_pure(generator);
        let base = generator.emit_node_for_left_hand_side(
            &self.base_expr,
            self.subscript_has_assignments || self.right_has_assignments,
            subscript_is_pure && right_is_pure,
        );
        let property = generator.emit_node_for_left_hand_side_for_property(&self.subscript, self.right_has_assignments, right_is_pure);
        let value = generator.destination_for_assign_result(dst.as_ref());
        let result = generator.emit_node_expression(value, &self.right);

        generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
        let forward_result = if generator.is_ignored_dst(dst.as_ref()) {
            result
        } else {
            let temporary = Some(generator.temp_destination(result.as_ref()));
            generator.move_register(temporary.as_ref(), result.as_ref().unwrap())
        };

        if nodes_codegen_is_non_index_string_element(&self.subscript) {
            let name = cpp2_string_value(&self.subscript);
            if self.base_expr.is_super_node() {
                let this_value = Some(generator.ensure_this());
                generator.emit_put_by_id_with_this(base, this_value, &name, forward_result.clone());
            } else {
                generator.emit_put_by_id(base, &name, forward_result.clone());
            }
        } else if self.base_expr.is_super_node() {
            let this_value = Some(generator.ensure_this());
            generator.emit_put_by_val_with_this(base, this_value, property, forward_result.clone());
        } else if let Some(context) = context {
            generator.emit_enumerator_put_by_val(&mut context.borrow_mut(), base, property, forward_result.clone());
        } else {
            generator.emit_put_by_val(base, property, forward_result.clone());
        }

        generator.emit_profile_type_divots(forward_result.clone(), &self.throwable.divot_start, &self.throwable.divot_end);
        generator.move_register(dst.as_ref(), forward_result.as_ref().unwrap())
    }
}

// ------------------------------ ReadModifyBracketNode -----------------------------------

impl crate::parser::nodes::ReadModifyBracketNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let subscript_is_pure = self.subscript.is_pure(generator);
        let right_is_pure = self.right.is_pure(generator);
        let base = generator.emit_node_for_left_hand_side(
            &self.base_expr,
            self.subscript_has_assignments || self.right_has_assignments,
            subscript_is_pure && right_is_pure,
        );
        let mut property = generator.emit_node_for_left_hand_side_for_property(&self.subscript, self.right_has_assignments, right_is_pure);
        if !self.subscript.is_number() && !self.subscript.is_string() {
            // Never double-evaluate the subscript expression;
            // don't even evaluate it once if the base isn't subscriptable.
            generator.emit_require_object_coercible(base.as_ref().unwrap(), "Cannot access property of undefined or null");
            let temporary = Some(generator.new_temporary());
            property = generator.emit_to_property_key_or_number(temporary, property);
        }

        generator.emit_expression_info(&self.throwable.subexpression_divot(), &self.throwable.subexpression_start(), &self.throwable.subexpression_end());
        let value;
        let mut this_value: Cpp4Reg = None;
        let temporary = Some(generator.temp_destination(dst.as_ref()));
        if self.base_expr.is_super_node() {
            this_value = Some(generator.ensure_this());
            value = generator.emit_get_by_val_with_this(temporary, base.clone(), this_value.clone(), property.clone());
        } else {
            value = generator.emit_get_by_val(temporary, base.clone(), property.clone());
        }
        let final_dst = Some(generator.final_destination(dst.as_ref(), value.as_ref()));
        let updated_value = cpp4b_emit_read_modify_assignment(
            generator,
            final_dst,
            value,
            &self.right,
            self.operator,
            cpp4b_unknown_with(&self.right),
            None,
            None,
        );

        let throwable = &self.throwable.base;
        generator.emit_expression_info(&throwable.divot, &throwable.divot_start, &throwable.divot_end);
        if self.base_expr.is_super_node() {
            generator.emit_put_by_val_with_this(base, this_value, property, updated_value.clone());
        } else {
            generator.emit_put_by_val(base, property, updated_value.clone());
        }
        generator.emit_profile_type_divots(updated_value.clone(), &throwable.divot_start, &throwable.divot_end);

        updated_value
    }
}

// ------------------------------ ShortCircuitReadModifyBracketNode -----------------------------------

impl crate::parser::nodes::ShortCircuitReadModifyBracketNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let subscript_is_pure = self.subscript.is_pure(generator);
        let right_is_pure = self.right.is_pure(generator);
        let base = generator.emit_node_for_left_hand_side(
            &self.base_expr,
            self.subscript_has_assignments || self.right_has_assignments,
            subscript_is_pure && right_is_pure,
        );
        let mut property = generator.emit_node_for_left_hand_side_for_property(&self.subscript, self.right_has_assignments, right_is_pure);
        if !self.subscript.is_number() && !self.subscript.is_string() {
            // Never double-evaluate the subscript expression;
            // don't even evaluate it once if the base isn't subscriptable.
            generator.emit_require_object_coercible(base.as_ref().unwrap(), "Cannot access property of undefined or null");
            let temporary = Some(generator.new_temporary());
            property = generator.emit_to_property_key_or_number(temporary, property);
        }

        let mut this_value: Cpp4Reg = None;
        let result = Some(generator.temp_destination(dst.as_ref()));

        generator.emit_expression_info(&self.throwable.subexpression_divot(), &self.throwable.subexpression_start(), &self.throwable.subexpression_end());
        if self.base_expr.is_super_node() {
            this_value = Some(generator.ensure_this());
            generator.emit_get_by_val_with_this(result.clone(), base.clone(), this_value.clone(), property.clone());
        } else {
            generator.emit_get_by_val(result.clone(), base.clone(), property.clone());
        }

        let after_assignment = generator.new_label();
        cpp4b_emit_short_circuit_assignment(generator, result.as_ref().unwrap(), self.operator, &after_assignment);

        generator.emit_node_expression(result.clone(), &self.right);
        let throwable = &self.throwable.base;
        generator.emit_expression_info(&throwable.divot, &throwable.divot_start, &throwable.divot_end);
        if self.base_expr.is_super_node() {
            generator.emit_put_by_val_with_this(base, this_value, property, result.clone());
        } else {
            generator.emit_put_by_val(base, property, result.clone());
        }
        generator.emit_profile_type_divots(result.clone(), &throwable.divot_start, &throwable.divot_end);

        generator.emit_label(&after_assignment);
        generator.move_register(dst.as_ref(), result.as_ref().unwrap())
    }
}

// ------------------------------ CommaNode ------------------------------------

impl crate::parser::nodes::CommaNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let mut expr = self.expr.clone();
        let mut next = self.next.clone();
        while let Some(node) = next {
            generator.emit_node_in_ignore_result_position_expression(&expr);
            let borrowed = node.borrow();
            expr = borrowed.expr.clone();
            next = borrowed.next.clone();
        }
        generator.emit_node_in_tail_position_expression(dst, &expr)
    }

    pub fn emit_bytecode_in_condition_context(
        &self,
        this: &crate::parser::nodes::Expression,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        true_target: &crate::bytecompiler::label::LabelRef,
        false_target: &crate::bytecompiler::label::LabelRef,
        fall_through_mode: crate::parser::nodes::FallThroughMode,
    ) {
        if this.base().needs_debug_hook() {
            generator.emit_debug_hook_expression_data(this, None);
        }

        let mut expr = self.expr.clone();
        let mut next = self.next.clone();
        while let Some(node) = next {
            generator.emit_node_in_ignore_result_position_expression(&expr);
            let borrowed = node.borrow();
            expr = borrowed.expr.clone();
            next = borrowed.next.clone();
        }
        generator.emit_node_in_condition_context(&expr, true_target, false_target, fall_through_mode);
    }
}

// ------------------------------ SourceElements -------------------------------

impl crate::parser::nodes::SourceElements {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) {
        let mut last_statement_with_completion_value = None;
        if generator.should_be_concerned_with_completion_value() {
            let mut statement = self.first_statement();
            while let Some(current) = statement {
                if current.has_completion_value() {
                    last_statement_with_completion_value = Some(current.clone());
                }
                statement = current.base().next();
            }
        }

        let mut statement = self.first_statement();
        while let Some(current) = statement {
            if generator.should_be_concerned_with_completion_value() {
                if last_statement_with_completion_value.as_ref() == Some(&current) {
                    generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
                }
                generator.emit_node_in_tail_position_statement(dst.clone(), &current);
            } else {
                let ignored = Some(generator.ignored_result());
                generator.emit_node_in_tail_position_statement(ignored, &current);
            }
            statement = current.base().next();
        }
    }
}

// ------------------------------ BlockNode ------------------------------------

impl crate::parser::nodes::BlockNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) {
        let Some(statements) = &self.statements else {
            return;
        };
        generator.push_lexical_scope(
            &self.variable_environment,
            crate::bytecompiler::bytecode_generator::ScopeType::LetConstScope,
            crate::bytecompiler::bytecode_generator::TDZCheckOptimization::Optimize,
            crate::bytecompiler::bytecode_generator::NestedScopeType::IsNested,
            None,
            true,
        );

        let using_count = self.variable_environment.using_declaration_count();
        let has_await_using = self.variable_environment.has_await_using_declaration();
        generator.emit_body_with_using_if_needed(using_count, has_await_using, &mut |generator| {
            statements.borrow().emit_bytecode(generator, dst.clone());
        });

        generator.pop_lexical_scope(&self.variable_environment);
    }
}

// ------------------------------ EmptyStatementNode ---------------------------

impl crate::parser::nodes::EmptyStatementNode {
    pub fn emit_bytecode(
        &self,
        _generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp4Reg,
    ) {
    }
}

// ------------------------------ DebuggerStatementNode ---------------------------

impl crate::parser::nodes::DebuggerStatementNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp4Reg,
    ) {
        let position = *self.base.position();
        generator.emit_debug_hook(crate::interpreter::interpreter::DebugHookType::DidReachDebuggerStatement, &position, None);
    }
}

// ------------------------------ ExprStatementNode ----------------------------

impl crate::parser::nodes::ExprStatementNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) {
        generator.emit_node_in_tail_position_from_expr_statement_node(dst, &self.expr);
    }
}

// ------------------------------ DeclarationStatement ----------------------------

impl crate::parser::nodes::DeclarationStatement {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp4Reg,
    ) {
        generator.emit_node_expression_no_dst(&self.expr);
    }
}

// ------------------------------ EmptyVarExpression ----------------------------

impl crate::parser::nodes::EmptyVarExpression {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp4Reg,
    ) -> Cpp4Reg {
        // It's safe to return null here because this node will always be a child node of DeclarationStatement which ignores our return value.
        if !generator.should_emit_type_profiler_hooks() {
            return None;
        }

        let position = *self.base.position();
        let end = position + self.ident.length();
        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        if let Some(local) = var.local() {
            generator.emit_profile_type_variable(Some(local), &var, &position, &end);
        } else {
            let scope = generator.emit_resolve_scope(None, &var);
            let temporary = Some(generator.new_temporary());
            let value = generator.emit_get_from_scope(temporary, scope, &var, crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound);
            generator.emit_profile_type_variable(value, &var, &position, &end);
        }

        None
    }
}

// ------------------------------ EmptyLetExpression ----------------------------

impl crate::parser::nodes::EmptyLetExpression {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp4Reg,
    ) -> Cpp4Reg {
        // Lexical declarations like 'let' must move undefined into their variables so we don't
        // get TDZ errors for situations like this: `let x; x;`
        let position = *self.base.position();
        let end = position + self.ident.length();
        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        if let Some(local) = var.local() {
            generator.emit_load_js_value(Some(local.clone()), crate::runtime::js_value::JSValue::Undefined);
            generator.emit_profile_type_variable(Some(local), &var, &position, &end);
        } else {
            let scope = generator.emit_resolve_scope(None, &var);
            let value = generator.emit_load_js_value(None, crate::runtime::js_value::JSValue::Undefined);
            let resolve_mode = cpp4b_put_resolve_mode(generator);
            generator.emit_put_to_scope(
                scope,
                &var,
                value.clone(),
                resolve_mode,
                crate::runtime::get_put_info::InitializationMode::Initialization,
            );
            generator.emit_profile_type_variable(value, &var, &position, &end);
        }

        generator.lift_tdz_check_if_possible(&var);

        // It's safe to return null here because this node will always be a child node of DeclarationStatement which ignores our return value.
        None
    }
}

// ------------------------------ IfElseNode ---------------------------------------

/// `singleStatement(StatementNode*)`.
fn cpp4b_single_statement(statement_node: &crate::parser::nodes::Statement) -> Option<crate::parser::nodes::Statement> {
    if let crate::parser::nodes::Statement::Block(block) = statement_node {
        return block.borrow().single_statement();
    }
    Some(statement_node.clone())
}

impl crate::parser::nodes::IfElseNode {
    pub fn try_fold_break_and_continue(
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        if_block: &crate::parser::nodes::Statement,
        true_target: &mut crate::bytecompiler::label::LabelRef,
        fall_through_mode: &mut crate::parser::nodes::FallThroughMode,
    ) -> bool {
        let Some(single_statement) = cpp4b_single_statement(if_block) else {
            return false;
        };

        let target = match &single_statement {
            crate::parser::nodes::Statement::Break(break_node) => break_node.borrow().trivial_target(generator),
            crate::parser::nodes::Statement::Continue(continue_node) => continue_node.borrow().trivial_target(generator),
            _ => return false,
        };
        let Some(target) = target else {
            return false;
        };
        *true_target = target;
        *fall_through_mode = crate::parser::nodes::FallThroughMode::FallThroughMeansFalse;
        true
    }

    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) {
        if generator.should_be_concerned_with_completion_value()
            && (self.if_block.has_early_break_or_continue() || self.else_block.as_ref().is_some_and(|else_block| else_block.has_early_break_or_continue()))
        {
            generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
        }

        let before_then = generator.new_label();
        let before_else = generator.new_label();
        let after_else = generator.new_label();

        let mut true_target = before_then.clone();
        let mut fall_through_mode = crate::parser::nodes::FallThroughMode::FallThroughMeansTrue;
        let did_fold_if_block = Self::try_fold_break_and_continue(generator, &self.if_block, &mut true_target, &mut fall_through_mode);

        generator.emit_node_in_condition_context(&self.condition, &true_target, &before_else, fall_through_mode);
        generator.emit_label(&before_then);
        generator.emit_profile_control_flow(self.if_block.base().start_offset());

        if !did_fold_if_block {
            generator.emit_node_in_tail_position_statement(dst.clone(), &self.if_block);
            if self.else_block.is_some() {
                generator.emit_jump(&after_else);
            }
        }

        generator.emit_label(&before_else);

        if let Some(else_block) = &self.else_block {
            generator.emit_profile_control_flow(self.if_block.base().end_offset() + if self.if_block.is_block() { 1 } else { 0 });
            generator.emit_node_in_tail_position_statement(dst, else_block);
        }

        generator.emit_label(&after_else);
        let ending_block = self.else_block.as_ref().unwrap_or(&self.if_block);
        generator.emit_profile_control_flow(ending_block.base().end_offset() + if ending_block.is_block() { 1 } else { 0 });
    }
}
