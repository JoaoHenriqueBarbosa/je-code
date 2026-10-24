// NodesCodegen.cpp, linhas 3375 a 3631: `EqualNode::emitBytecode` até
// `ConditionalNode::emitBytecodeInConditionContext` (incluído por include!, sem `use`).
//
// Convenções desta fatia (as mesmas da cpp2): registrador é `Option<RegisterRef>` (o `RegisterID*`
// nulo do C++), o `RefPtr<RegisterID>` é o próprio `Option<RegisterRef>`, `.get()` vira `.clone()`.
// Os `emitBytecodeInConditionContext` recebem `this` como `Expression` (para o `emitDebugHook`),
// como em `expression_node_emit_bytecode_in_condition_context`.

type Cpp4Reg = Option<crate::bytecompiler::bytecode_generator::RegisterRef>;

impl crate::parser::nodes::EqualNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let binary = &self.base;
        if binary.expr1.is_null() || binary.expr2.is_null() {
            let src = generator.emit_node_expression_no_dst(if binary.expr1.is_null() { &binary.expr2 } else { &binary.expr1 });
            let final_dst = Some(generator.final_destination(dst.as_ref(), src.as_ref()));
            return generator.emit_unary_op::<crate::bytecode::bytecode_ops::OpEqNull>(final_dst, src);
        }

        let mut left = binary.expr1.clone();
        let mut right = binary.expr2.clone();
        if left.is_string() {
            std::mem::swap(&mut left, &mut right);
        }

        let right_is_pure = binary.expr2.is_pure(generator);
        let src1 = generator.emit_node_for_left_hand_side(&left, binary.right_has_assignments, right_is_pure);
        let src2 = generator.emit_node_expression_no_dst(&right);
        let final_dst = Some(generator.final_destination(dst.as_ref(), src1.as_ref()));
        generator.emit_equality_op::<crate::bytecode::bytecode_ops::OpEq>(final_dst, src1, src2)
    }
}

impl crate::parser::nodes::StrictEqualNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let binary = &self.base;
        let mut left = binary.expr1.clone();
        let mut right = binary.expr2.clone();
        if left.is_string() {
            std::mem::swap(&mut left, &mut right);
        }

        let right_is_pure = binary.expr2.is_pure(generator);
        let src1 = generator.emit_node_for_left_hand_side(&left, binary.right_has_assignments, right_is_pure);
        let src2 = generator.emit_node_expression_no_dst(&right);
        let final_dst = Some(generator.final_destination(dst.as_ref(), src1.as_ref()));
        generator.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(final_dst, src1, src2)
    }
}

impl crate::parser::nodes::ThrowableBinaryOpNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let binary = &self.base;
        let right_is_pure = binary.expr2.is_pure(generator);
        let src1 = generator.emit_node_for_left_hand_side(&binary.expr1, binary.right_has_assignments, right_is_pure);
        let src2 = generator.emit_node_expression_no_dst(&binary.expr2);
        generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
        let final_dst = Some(generator.final_destination(dst.as_ref(), src1.as_ref()));
        generator.emit_binary_op_dynamic(
            binary.opcode_id,
            final_dst,
            src1,
            src2,
            crate::parser::result_type::OperandTypes::new(binary.expr1.result_descriptor(), binary.expr2.result_descriptor()),
        )
    }
}

impl crate::parser::nodes::InstanceOfNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let throwable = &self.base;
        let binary = &throwable.base;
        let right_is_pure = binary.expr2.is_pure(generator);
        let value = generator.emit_node_for_left_hand_side(&binary.expr1, binary.right_has_assignments, right_is_pure);
        let dst_reg = Some(generator.final_destination(dst.as_ref(), value.as_ref()));
        let constructor = generator.emit_node_expression_no_dst(&binary.expr2);
        let has_instance_or_prototype = Some(generator.new_temporary());
        generator.emit_expression_info(&throwable.throwable.divot, &throwable.throwable.divot_start, &throwable.throwable.divot_end);
        generator.emit_instanceof(dst_reg, value, constructor, has_instance_or_prototype)
    }
}

// ------------------------------ InNode ----------------------------

impl crate::parser::nodes::InNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let throwable = &self.base;
        let binary = &throwable.base;
        let divot = &throwable.throwable.divot;
        let divot_start = &throwable.throwable.divot_start;
        let divot_end = &throwable.throwable.divot_end;

        if binary.expr1.is_private_identifier() {
            let base = generator.emit_node_expression_no_dst(&binary.expr2);

            let identifier = match &binary.expr1 {
                crate::parser::nodes::Expression::PrivateIdentifier(node) => node.borrow().ident.clone(),
                _ => unreachable!("esperava um PrivateIdentifierNode"),
            };
            let private_traits = generator.get_private_traits(&identifier);
            let var = generator.variable(&identifier, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            let scope = generator.emit_resolve_scope(None, &var);
            debug_assert!(scope.is_some()); // Private names are always captured.

            if private_traits.is_field() {
                let temp = generator.new_temporary();
                let private_name = generator.emit_get_from_scope(
                    Some(temp),
                    scope.clone(),
                    &var,
                    crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                );
                let final_dst = Some(generator.final_destination(dst.as_ref(), base.as_ref()));
                return generator.emit_has_private_name(final_dst, base, private_name);
            }

            debug_assert!(private_traits.is_private_method_or_accessor());
            let temp = generator.new_temporary();
            let private_brand = generator.emit_get_private_brand(Some(temp), scope.clone(), private_traits.is_static());
            let final_dst = Some(generator.final_destination(dst.as_ref(), base.as_ref()));
            return generator.emit_has_private_brand(final_dst, base, private_brand, private_traits.is_static());
        }

        if nodes_codegen_is_non_index_string_element(&binary.expr1) {
            let base = generator.emit_node_expression_no_dst(&binary.expr2);
            generator.emit_expression_info(divot, divot_start, divot_end);
            let final_dst = Some(generator.final_destination(dst.as_ref(), base.as_ref()));
            return generator.emit_in_by_id(final_dst, base, &cpp2_string_value(&binary.expr1));
        }

        let right_is_pure = binary.expr2.is_pure(generator);
        let key = generator.emit_node_for_left_hand_side(&binary.expr1, binary.right_has_assignments, right_is_pure);
        let base = generator.emit_node_expression_no_dst(&binary.expr2);
        generator.emit_expression_info(divot, divot_start, divot_end);
        let final_dst = Some(generator.final_destination(dst.as_ref(), key.as_ref()));
        generator.emit_in_by_val(final_dst, key, base)
    }
}

// ------------------------------ LogicalOpNode ----------------------------

impl crate::parser::nodes::LogicalOpNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        use crate::parser::nodes::FallThroughMode::{FallThroughMeansFalse, FallThroughMeansTrue};
        if generator.is_ignored_dst(dst.as_ref()) {
            let after_expr1 = generator.new_label();
            let after_expr2 = generator.new_label();
            if self.operator == crate::parser::nodes::LogicalOperator::And {
                generator.emit_node_in_condition_context(&self.expr1, &after_expr1, &after_expr2, FallThroughMeansTrue);
            } else {
                generator.emit_node_in_condition_context(&self.expr1, &after_expr2, &after_expr1, FallThroughMeansFalse);
            }
            generator.emit_label(&after_expr1);

            generator.emit_node_in_tail_position_expression(dst.clone(), &self.expr2);
            generator.emit_label(&after_expr2);
            return dst;
        }

        let temp = Some(generator.temp_destination(dst.as_ref()));
        let target = generator.new_label();

        generator.emit_node_expression(temp.clone(), &self.expr1);
        if self.operator == crate::parser::nodes::LogicalOperator::And {
            generator.emit_jump_if_false(temp.as_ref().unwrap(), &target);
        } else {
            generator.emit_jump_if_true(temp.as_ref().unwrap(), &target);
        }
        generator.emit_node_in_tail_position_expression(temp.clone(), &self.expr2);
        generator.emit_label(&target);

        generator.move_register(dst.as_ref(), temp.as_ref().unwrap())
    }

    pub fn emit_bytecode_in_condition_context(
        &self,
        this: &crate::parser::nodes::Expression,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        true_target: &crate::bytecompiler::label::LabelRef,
        false_target: &crate::bytecompiler::label::LabelRef,
        fall_through_mode: crate::parser::nodes::FallThroughMode,
    ) {
        use crate::parser::nodes::FallThroughMode::{FallThroughMeansFalse, FallThroughMeansTrue};
        if this.base().needs_debug_hook() {
            generator.emit_debug_hook_expression_data(this, None);
        }

        let after_expr1 = generator.new_label();
        if self.operator == crate::parser::nodes::LogicalOperator::And {
            generator.emit_node_in_condition_context(&self.expr1, &after_expr1, false_target, FallThroughMeansTrue);
        } else {
            generator.emit_node_in_condition_context(&self.expr1, true_target, &after_expr1, FallThroughMeansFalse);
        }
        generator.emit_label(&after_expr1);

        generator.emit_node_in_condition_context(&self.expr2, true_target, false_target, fall_through_mode);
    }
}

// ------------------------------ CoalesceNode ----------------------------

impl crate::parser::nodes::CoalesceNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let temp = Some(generator.temp_destination(dst.as_ref()));
        let end_label = generator.new_label();

        if self.has_absorbed_optional_chain {
            generator.push_optional_chain_target();
        }
        generator.emit_node_expression(temp.clone(), &self.expr1);
        let scratch = Some(generator.new_temporary());
        let is_nullish = generator.emit_is_undefined_or_null(scratch, temp.as_ref().unwrap());
        generator.emit_jump_if_false_raw(is_nullish.as_ref().unwrap(), &end_label);

        if self.has_absorbed_optional_chain {
            generator.pop_optional_chain_target();
        }
        generator.emit_node_in_tail_position_expression(temp.clone(), &self.expr2);

        generator.emit_label(&end_label);
        generator.move_register(dst.as_ref(), temp.as_ref().unwrap())
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

        let nullish_target = generator.new_label();

        if self.has_absorbed_optional_chain {
            generator.push_optional_chain_target_existing(&nullish_target);
        }
        let value = generator.emit_node_expression_no_dst(&self.expr1);
        let scratch = Some(generator.new_temporary());
        let is_nullish = generator.emit_is_undefined_or_null(scratch, value.as_ref().unwrap());
        generator.emit_jump_if_true_raw(is_nullish.as_ref().unwrap(), &nullish_target);
        if self.has_absorbed_optional_chain {
            generator.discard_optional_chain_target();
        }

        if fall_through_mode == crate::parser::nodes::FallThroughMode::FallThroughMeansTrue {
            generator.emit_jump_if_false(value.as_ref().unwrap(), false_target);
            generator.emit_jump(true_target);
        } else {
            generator.emit_jump_if_true(value.as_ref().unwrap(), true_target);
            generator.emit_jump(false_target);
        }

        generator.emit_label(&nullish_target);
        generator.emit_node_in_condition_context(&self.expr2, true_target, false_target, fall_through_mode);
    }
}

// ------------------------------ OptionalChainNode ----------------------------

impl crate::parser::nodes::OptionalChainNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let final_dest = Some(generator.final_destination(dst.as_ref(), None));

        if self.is_outermost {
            generator.push_optional_chain_target();
        }
        generator.emit_node_in_tail_position_expression(final_dest.clone(), &self.expr);
        if self.is_outermost {
            generator.pop_optional_chain_target_dst(final_dest.clone(), self.expr.is_delete_node());
        }

        final_dest
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

        if self.expr.is_delete_node() {
            expression_node_emit_bytecode_in_condition_context(this, generator, true_target, false_target, fall_through_mode);
            return;
        }

        // Short-circuiting produces undefined, which is falsy. Route the optional
        // chain bail-out straight to falseTarget instead of materializing undefined.
        if self.is_outermost {
            generator.push_optional_chain_target_existing(false_target);
        }
        generator.emit_node_in_condition_context(&self.expr, true_target, false_target, fall_through_mode);
        if self.is_outermost {
            generator.discard_optional_chain_target();
        }
    }
}

// ------------------------------ ConditionalNode ------------------------------

impl crate::parser::nodes::ConditionalNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp4Reg,
    ) -> Cpp4Reg {
        let new_dst = Some(generator.final_destination(dst.as_ref(), None));
        let before_else = generator.new_label();
        let after_else = generator.new_label();

        let before_then = generator.new_label();
        generator.emit_node_in_condition_context(
            &self.logical,
            &before_then,
            &before_else,
            crate::parser::nodes::FallThroughMode::FallThroughMeansTrue,
        );
        generator.emit_label(&before_then);

        generator.emit_profile_control_flow(self.expr1.base().start_offset() as i32);
        generator.emit_node_in_tail_position_expression(new_dst.clone(), &self.expr1);
        generator.emit_jump(&after_else);

        generator.emit_label(&before_else);
        generator.emit_profile_control_flow(self.expr1.base().end_offset() as i32 + 1);
        generator.emit_node_in_tail_position_expression(new_dst.clone(), &self.expr2);

        generator.emit_label(&after_else);

        generator.emit_profile_control_flow(self.expr2.base().end_offset() as i32 + 1);

        new_dst
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

        let before_then = generator.new_label();
        let before_else = generator.new_label();
        let end = generator.new_label();

        generator.emit_node_in_condition_context(
            &self.logical,
            &before_then,
            &before_else,
            crate::parser::nodes::FallThroughMode::FallThroughMeansTrue,
        );
        generator.emit_label(&before_then);

        generator.emit_node_in_condition_context(&self.expr1, true_target, false_target, fall_through_mode);
        generator.emit_jump(&end);

        generator.emit_label(&before_else);
        generator.emit_node_in_condition_context(&self.expr2, true_target, false_target, fall_through_mode);

        generator.emit_label(&end);
    }
}
