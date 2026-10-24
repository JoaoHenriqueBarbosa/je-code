// NodesCodegen.cpp, linhas 2410 a 3373: `areTrivialApplyArguments`, `ApplyFunctionCallDotNode`,
// `PostfixNode`, `DeleteResolveNode`/`DeleteBracketNode`/`DeleteDotNode`/`DeleteValueNode`, `VoidNode`,
// `TypeOfResolveNode`/`TypeOfValueNode`, `PrefixNode`, `UnaryOpNode`, `UnaryPlusNode`,
// `LogicalNotNode::emitBytecodeInConditionContext` e `BinaryOpNode` (emitStrcat, condição, tryFoldToBranch,
// emitBytecode). Incluído por include!, sem `use` (continua em `EqualNode`, linha 3375, na cpp4).
//
// Convenções desta fatia (as mesmas da cpp3): registrador é `Option<RegisterRef>` (o `RegisterID*` nulo
// do C++), o `RefPtr<RegisterID>` é o próprio `Option<RegisterRef>` (Rc), `.get()` vira `.clone()`.

type Cpp3bReg = Option<crate::bytecompiler::bytecode_generator::RegisterRef>;
type Cpp3bGen = crate::bytecompiler::bytecode_generator::BytecodeGenerator;

/// Igualdade de ponteiro entre dois `RegisterID*` (nulo só é igual a nulo).
fn cpp3b_same_register(a: &Cpp3bReg, b: &Cpp3bReg) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.is_same_register(b),
        (None, None) => true,
        _ => false,
    }
}

/// `static_cast<BinaryOpNode*>(node)`: despacha pelas variantes que herdam de `BinaryOpNode` (as mesmas
/// de `Expression::is_binary_op_node`) e avalia `$body` com `$node` ligado ao `NodeRef` da variante.
macro_rules! cpp3b_with_binary_op_node {
    ($expr:expr, $node:ident, $body:expr) => {
        match $expr {
            crate::parser::nodes::Expression::Pow($node) => $body,
            crate::parser::nodes::Expression::Mult($node) => $body,
            crate::parser::nodes::Expression::Div($node) => $body,
            crate::parser::nodes::Expression::Mod($node) => $body,
            crate::parser::nodes::Expression::Add($node) => $body,
            crate::parser::nodes::Expression::Sub($node) => $body,
            crate::parser::nodes::Expression::LeftShift($node) => $body,
            crate::parser::nodes::Expression::RightShift($node) => $body,
            crate::parser::nodes::Expression::UnsignedRightShift($node) => $body,
            crate::parser::nodes::Expression::Less($node) => $body,
            crate::parser::nodes::Expression::Greater($node) => $body,
            crate::parser::nodes::Expression::LessEq($node) => $body,
            crate::parser::nodes::Expression::GreaterEq($node) => $body,
            crate::parser::nodes::Expression::InstanceOf($node) => $body,
            crate::parser::nodes::Expression::In($node) => $body,
            crate::parser::nodes::Expression::Equal($node) => $body,
            crate::parser::nodes::Expression::NotEqual($node) => $body,
            crate::parser::nodes::Expression::StrictEqual($node) => $body,
            crate::parser::nodes::Expression::NotStrictEqual($node) => $body,
            crate::parser::nodes::Expression::BitAnd($node) => $body,
            crate::parser::nodes::Expression::BitOr($node) => $body,
            crate::parser::nodes::Expression::BitXOr($node) => $body,
            _ => unreachable!("esperava um BinaryOpNode"),
        }
    };
}

/// `static_cast<BinaryOpNode*>(node)->opcodeID()`.
fn cpp3b_binary_opcode_id(node: &crate::parser::nodes::Expression) -> crate::bytecode::opcode::OpcodeID {
    cpp3b_with_binary_op_node!(node, n, n.borrow().opcode_id)
}

/// `emitIncOrDec`.
fn cpp3b_emit_inc_or_dec(
    generator: &mut Cpp3bGen,
    src_dst: &crate::bytecompiler::bytecode_generator::RegisterRef,
    oper: crate::parser::nodes::Operator,
) -> Cpp3bReg {
    if oper == crate::parser::nodes::Operator::PlusPlus {
        generator.emit_inc(src_dst)
    } else {
        generator.emit_dec(src_dst)
    }
}

/// `emitPostIncOrDec`.
fn cpp3b_emit_post_inc_or_dec(
    generator: &mut Cpp3bGen,
    dst: Cpp3bReg,
    src_dst: &crate::bytecompiler::bytecode_generator::RegisterRef,
    oper: crate::parser::nodes::Operator,
) -> Cpp3bReg {
    if cpp3b_same_register(&dst, &Some(src_dst.clone())) {
        let final_dst = Some(generator.final_destination(dst.as_ref(), None));
        return generator.emit_to_numeric(final_dst, Some(src_dst.clone()));
    }
    // `RegisterID* dst` do C++ é ponteiro cru: um temporário recém-alocado por `finalDestination` não
    // segura referência, e o `newTemporary()` seguinte o reaproveita (mesmo índice). Aqui `dst` chega
    // contado; soltamos a contagem e reconstruímos o handle só no fim.
    let raw_dst = dst.as_ref().map(|d| d.get().clone());
    drop(dst);
    let temp = Some(generator.new_temporary());
    let tmp = generator.emit_to_numeric(temp, Some(src_dst.clone()));
    let result = Some(generator.temp_destination(Some(src_dst)));
    generator.move_register(result.as_ref(), tmp.as_ref().unwrap());
    cpp3b_emit_inc_or_dec(generator, result.as_ref().unwrap(), oper);
    generator.move_register(Some(src_dst), result.as_ref().unwrap());
    let dst = raw_dst.map(|r| crate::bytecompiler::bytecode_generator::RegisterRef::new(&r));
    generator.move_register(dst.as_ref(), tmp.as_ref().unwrap())
}

// ------------------------------ ApplyFunctionCallDotNode ----------------------------------

/// `areTrivialApplyArguments`.
fn cpp3b_are_trivial_apply_arguments(args: &crate::parser::nodes::NodeRef<crate::parser::nodes::ArgumentsNode>) -> bool {
    let args = args.borrow();
    let list_node = match args.list_node.as_ref() {
        Some(list_node) => list_node.borrow(),
        None => return true,
    };
    let next = match list_node.next.as_ref() {
        Some(next) => next.borrow(),
        None => return true,
    };
    next.next.is_none() && next.expr.is_simple_array()
}

impl crate::parser::nodes::ApplyFunctionCallDotNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction};
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;

        // A few simple cases can be trivially handled as ordinary function calls.
        // function.apply(), function.apply(arg) -> identical to function.call
        // function.apply(thisArg, [arg0, arg1, ...]) -> can be trivially coerced into function.call(thisArg, arg0, arg1, ...) and saves object allocation
        let may_be_call = cpp3b_are_trivial_apply_arguments(&self.args);

        let return_value = Some(generator.final_destination(dst.as_ref(), None));
        let base = generator.emit_node_expression(None, &self.base_expr);

        if self.base_expr.base().is_optional_chain_base {
            generator.emit_optional_check(base.as_ref().unwrap());
        }

        let base_is_super = self.base_expr.is_super_node();
        let is_optional_call = self.is_optional_call;
        let apply_name = generator.property_names().builtin_names().apply_public_name().clone();

        let emit_call_check = !generator.is_builtin_function();
        if self.distance_to_innermost_call_or_apply > MAX_DISTANCE_TO_INNERMOST_CALL_OR_APPLY && emit_call_check {
            let function = cpp3_make_call_or_apply_function(generator, base_is_super, &base, &dst, is_optional_call, &apply_name);
            let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
            generator.move_register(call_arguments.this_register().as_ref(), base.as_ref().unwrap());
            generator.emit_call_in_tail_position(
                return_value.clone(),
                function,
                ExpectedFunction::NoExpectedFunction,
                &mut call_arguments,
                &divot,
                &divot_start,
                &divot_end,
                DebuggableCall::Yes,
            );
            generator.move_register(dst.as_ref(), return_value.as_ref().unwrap());
            return return_value;
        }

        let real_call = generator.new_label();
        let end = generator.new_label();
        generator.emit_expression_info(
            &self.throwable.subexpression_divot(),
            &self.throwable.subexpression_start(),
            &self.throwable.subexpression_end(),
        );
        let mut function: Cpp3bReg = None;
        if emit_call_check {
            function = cpp3_make_call_or_apply_function(generator, base_is_super, &base, &dst, is_optional_call, &apply_name);
            generator.emit_jump_if_not_function_apply(function.as_ref().unwrap(), &real_call);
        }
        if may_be_call {
            let old_list = self.args.borrow().list_node.clone();
            if let Some(old_list) = old_list {
                let old_expr = old_list.borrow().expr.clone();
                let old_next = old_list.borrow().next.clone();
                if let crate::parser::nodes::Expression::SpreadExpression(spread) = &old_expr {
                    let spread = spread.clone();
                    let temp = Some(generator.new_temporary());
                    let real_function = generator.move_register(temp.as_ref(), base.as_ref().unwrap());
                    let temp = Some(generator.new_temporary());
                    let index = generator.emit_load_js_value(temp, crate::runtime::js_value::js_number(0));
                    let temp = Some(generator.new_temporary());
                    let this_register = generator.emit_load_js_value(temp, crate::runtime::js_value::js_undefined());
                    let temp = Some(generator.new_temporary());
                    let arguments_register = generator.emit_load_js_value(temp, crate::runtime::js_value::js_undefined());

                    let mut extractor = |generator: &mut Cpp3bGen, value: Cpp3bReg| {
                        let have_this = generator.new_label();
                        let end = generator.new_label();
                        let temp = Some(generator.new_temporary());
                        let zero = generator.emit_load_js_value(None, crate::runtime::js_value::js_number(0));
                        let is_zero = generator.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(temp, index.clone(), zero);
                        generator.emit_jump_if_false_raw(is_zero.as_ref().unwrap(), &have_this);
                        generator.move_register(this_register.as_ref(), value.as_ref().unwrap());
                        generator.emit_load_js_value(index.clone(), crate::runtime::js_value::js_number(1));
                        generator.emit_jump(&end);
                        generator.emit_label(&have_this);
                        let temp = Some(generator.new_temporary());
                        let one = generator.emit_load_js_value(None, crate::runtime::js_value::js_number(1));
                        let is_one = generator.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(temp, index.clone(), one);
                        generator.emit_jump_if_false_raw(is_one.as_ref().unwrap(), &end);
                        generator.move_register(arguments_register.as_ref(), value.as_ref().unwrap());
                        generator.emit_load_js_value(index.clone(), crate::runtime::js_value::js_number(2));
                        generator.emit_label(&end);
                    };
                    let subject = spread.borrow().expression.clone();
                    generator.emit_enumeration(&self.throwable.base, &subject, &mut extractor, None, None);
                    let first_free_register = Some(generator.new_temporary());
                    generator.emit_call_varargs_in_tail_position(
                        return_value.clone(),
                        real_function,
                        this_register,
                        arguments_register,
                        first_free_register,
                        0,
                        &divot,
                        &divot_start,
                        &divot_end,
                        DebuggableCall::Yes,
                    );
                } else if let Some(old_next) = old_next {
                    let array = match &old_next.borrow().expr {
                        crate::parser::nodes::Expression::Array(array) => array.clone(),
                        _ => unreachable!("ASSERT(m_args->m_listNode->m_next->m_expr->isSimpleArray())"),
                    };
                    debug_assert!(old_next.borrow().next.is_none());
                    let new_list = array.borrow().to_argument_list(0, 0);
                    self.args.borrow_mut().list_node = new_list;
                    let temp = Some(generator.temp_destination(dst.as_ref()));
                    let real_function = generator.move_register(temp.as_ref(), base.as_ref().unwrap());
                    let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
                    generator.emit_node_expression(call_arguments.this_register(), &old_expr);
                    generator.emit_call_in_tail_position(
                        return_value.clone(),
                        real_function,
                        ExpectedFunction::NoExpectedFunction,
                        &mut call_arguments,
                        &divot,
                        &divot_start,
                        &divot_end,
                        DebuggableCall::Yes,
                    );
                } else {
                    self.args.borrow_mut().list_node = old_next;
                    let temp = Some(generator.temp_destination(dst.as_ref()));
                    let real_function = generator.move_register(temp.as_ref(), base.as_ref().unwrap());
                    let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
                    generator.emit_node_expression(call_arguments.this_register(), &old_expr);
                    generator.emit_call_in_tail_position(
                        return_value.clone(),
                        real_function,
                        ExpectedFunction::NoExpectedFunction,
                        &mut call_arguments,
                        &divot,
                        &divot_start,
                        &divot_end,
                        DebuggableCall::Yes,
                    );
                }
                self.args.borrow_mut().list_node = Some(old_list);
            } else {
                let temp = Some(generator.temp_destination(dst.as_ref()));
                let real_function = generator.move_register(temp.as_ref(), base.as_ref().unwrap());
                let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
                generator.emit_load_js_value(call_arguments.this_register(), crate::runtime::js_value::js_undefined());
                generator.emit_call_in_tail_position(
                    return_value.clone(),
                    real_function,
                    ExpectedFunction::NoExpectedFunction,
                    &mut call_arguments,
                    &divot,
                    &divot_start,
                    &divot_end,
                    DebuggableCall::Yes,
                );
            }
        } else {
            let list_node = self.args.borrow().list_node.clone().expect("ASSERT(m_args->m_listNode && m_args->m_listNode->m_next)");
            let temp = Some(generator.temp_destination(dst.as_ref()));
            let real_function = generator.move_register(temp.as_ref(), base.as_ref().unwrap());
            let this_expr = list_node.borrow().expr.clone();
            let this_register = generator.emit_node_expression(None, &this_expr);
            let mut args = list_node.borrow().next.clone().expect("ASSERT(m_args->m_listNode->m_next)");
            let args_expr = args.borrow().expr.clone();
            let args_register = generator.emit_node_expression(None, &args_expr);

            // Function.prototype.apply ignores extra arguments, but we still
            // need to evaluate them for side effects.
            loop {
                let next = args.borrow().next.clone();
                match next {
                    Some(next) => {
                        args = next;
                        let expr = args.borrow().expr.clone();
                        generator.emit_node_expression(None, &expr);
                    }
                    None => break,
                }
            }

            let first_free_register = Some(generator.new_temporary());
            generator.emit_call_varargs_in_tail_position(
                return_value.clone(),
                real_function,
                this_register,
                args_register,
                first_free_register,
                0,
                &divot,
                &divot_start,
                &divot_end,
                DebuggableCall::Yes,
            );
        }
        if emit_call_check {
            generator.emit_jump(&end);
            generator.emit_label(&real_call);
            let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
            generator.move_register(call_arguments.this_register().as_ref(), base.as_ref().unwrap());
            generator.emit_call_in_tail_position(
                return_value.clone(),
                function,
                ExpectedFunction::NoExpectedFunction,
                &mut call_arguments,
                &divot,
                &divot_start,
                &divot_end,
                DebuggableCall::Yes,
            );
            generator.emit_label(&end);
        }
        generator.emit_profile_type_divots(return_value.clone(), &divot_start, &divot_end);
        return_value
    }
}

// ------------------------------ PostfixNode ----------------------------------

impl crate::parser::nodes::PostfixNode {
    pub fn emit_resolve(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        use crate::bytecompiler::bytecode_generator::ThisResolutionType;
        use crate::runtime::get_put_info::{InitializationMode, ResolveMode};
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;
        if generator.is_ignored_dst(dst.as_ref()) {
            return self.base.emit_resolve(generator, dst);
        }

        let resolve = match &self.expr {
            crate::parser::nodes::Expression::Resolve(resolve) => resolve.clone(),
            _ => unreachable!("ASSERT(m_expr->isResolveNode())"),
        };
        let ident = resolve.borrow().ident.clone();

        let var = generator.variable(&ident, ThisResolutionType::Local);
        let new_divot = divot_start + ident.length();
        if let Some(local) = var.local() {
            generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
            generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);
            let mut local_reg = Some(local.clone());
            if var.is_read_only() {
                generator.emit_read_only_exception_if_needed(&var);
                let temp = Some(generator.temp_destination(dst.as_ref()));
                local_reg = generator.move_register(temp.as_ref(), &local);
            }
            let final_dst = Some(generator.final_destination(dst.as_ref(), None));
            let old_value = cpp3b_emit_post_inc_or_dec(generator, final_dst, local_reg.as_ref().unwrap(), self.operator);
            generator.emit_profile_type_variable(local_reg, &var, &divot_start, &divot_end);
            return old_value;
        }

        generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
        let scope = generator.emit_resolve_scope(None, &var);
        let temp = Some(generator.new_temporary());
        let value = generator.emit_get_from_scope(temp, scope.clone(), &var, ResolveMode::ThrowIfNotFound);
        generator.emit_tdz_check_if_necessary(&var, value.clone(), None);
        if var.is_read_only() {
            let threw_exception = generator.emit_read_only_exception_if_needed(&var);
            if threw_exception {
                return value;
            }
        }
        let final_dst = Some(generator.final_destination(dst.as_ref(), None));
        let old_value = cpp3b_emit_post_inc_or_dec(generator, final_dst, value.as_ref().unwrap(), self.operator);
        if !var.is_read_only() {
            let resolve_mode = if generator.ecma_mode().is_strict() { ResolveMode::ThrowIfNotFound } else { ResolveMode::DoNotThrowIfNotFound };
            generator.emit_put_to_scope(scope, &var, value.clone(), resolve_mode, InitializationMode::NotInitialization);
            generator.emit_profile_type_variable(value, &var, &divot_start, &divot_end);
        }

        old_value
    }

    pub fn emit_bracket(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;
        if generator.is_ignored_dst(dst.as_ref()) {
            return self.base.emit_bracket(generator, dst);
        }

        let bracket_accessor = match &self.expr {
            crate::parser::nodes::Expression::BracketAccessor(bracket_accessor) => bracket_accessor.clone(),
            _ => unreachable!("ASSERT(m_expr->isBracketAccessorNode())"),
        };
        let bracket_accessor = bracket_accessor.borrow();
        let base_node = bracket_accessor.base_expr.clone();
        let subscript = bracket_accessor.subscript.clone();

        let subscript_is_pure = subscript.is_pure(generator);
        let base = generator.emit_node_for_left_hand_side(&base_node, bracket_accessor.subscript_has_assignments, subscript_is_pure);
        let mut property = generator.emit_node_for_property(&subscript);
        if !subscript.is_number() && !subscript.is_string() {
            // Never double-evaluate the subscript expression;
            // don't even evaluate it once if the base isn't subscriptable.
            generator.emit_require_object_coercible(base.as_ref().unwrap(), "Cannot access property of undefined or null");
            let temp = Some(generator.new_temporary());
            property = generator.emit_to_property_key_or_number(temp, property);
        }

        generator.emit_expression_info(&bracket_accessor.throwable.divot, &bracket_accessor.throwable.divot_start, &bracket_accessor.throwable.divot_end);
        let value;
        let mut this_value: Cpp3bReg = None;
        if base_node.is_super_node() {
            this_value = Some(generator.ensure_this());
            let temp = Some(generator.new_temporary());
            value = generator.emit_get_by_val_with_this(temp, base.clone(), this_value.clone(), property.clone());
        } else {
            let temp = Some(generator.new_temporary());
            value = generator.emit_get_by_val(temp, base.clone(), property.clone());
        }
        let temp_dst = Some(generator.temp_destination(dst.as_ref()));
        let old_value = cpp3b_emit_post_inc_or_dec(generator, temp_dst, value.as_ref().unwrap(), self.operator);
        generator.emit_expression_info(&divot, &divot_start, &divot_end);
        if base_node.is_super_node() {
            generator.emit_put_by_val_with_this(base, this_value, property, value.clone());
        } else {
            generator.emit_put_by_val(base, property, value.clone());
        }
        generator.emit_profile_type_divots(value, &divot_start, &divot_end);
        generator.move_register(dst.as_ref(), old_value.as_ref().unwrap())
    }

    pub fn emit_dot(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction, ThisResolutionType};
        use crate::runtime::get_put_info::ResolveMode;
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;
        let position = *self.position();
        if generator.is_ignored_dst(dst.as_ref()) {
            return self.base.emit_dot(generator, dst);
        }

        let dot_accessor = match &self.expr {
            crate::parser::nodes::Expression::DotAccessor(dot_accessor) => dot_accessor.clone(),
            _ => unreachable!("ASSERT(m_expr->isDotAccessorNode())"),
        };
        let dot_accessor = dot_accessor.borrow();
        let base_node = dot_accessor.base_expr.clone();
        let base_is_super = base_node.is_super_node();
        let ident = dot_accessor.ident.clone();

        let base = generator.emit_node_expression(None, &base_node);

        generator.emit_expression_info(&dot_accessor.throwable.divot, &dot_accessor.throwable.divot_start, &dot_accessor.throwable.divot_end);

        if dot_accessor.is_private_member() {
            debug_assert!(!base_is_super);
            let private_traits = generator.get_private_traits(&ident);

            if private_traits.is_field() {
                let var = generator.variable(&ident, ThisResolutionType::Local);
                let scope = generator.emit_resolve_scope(None, &var);
                debug_assert!(scope.is_some()); // Private names are always captured.
                let private_name = Some(generator.new_temporary());
                generator.emit_get_from_scope(private_name.clone(), scope, &var, ResolveMode::DoNotThrowIfNotFound);

                let temp = Some(generator.new_temporary());
                let value = generator.emit_get_private_name(temp, base.clone(), private_name.clone());
                let temp_dst = Some(generator.temp_destination(dst.as_ref()));
                let old_value = cpp3b_emit_post_inc_or_dec(generator, temp_dst, value.as_ref().unwrap(), self.operator);
                generator.emit_expression_info(&divot, &divot_start, &divot_end);
                generator.emit_private_field_put(base, private_name, value.clone());
                generator.emit_profile_type_divots(value, &divot_start, &divot_end);
                return generator.move_register(dst.as_ref(), old_value.as_ref().unwrap());
            }

            if private_traits.is_method() {
                let var = generator.variable(&ident, ThisResolutionType::Local);
                let scope = generator.emit_resolve_scope(None, &var);
                debug_assert!(scope.is_some()); // Private names are always captured.
                let temp = Some(generator.new_temporary());
                let private_brand_symbol = generator.emit_get_private_brand(temp, scope, private_traits.is_static());
                generator.emit_check_private_brand(base, private_brand_symbol, private_traits.is_static());

                generator.emit_expression_info(&divot, &divot_start, &divot_end);
                generator.emit_throw_type_error("Trying to access an undefined private setter");
                return Some(generator.temp_destination(dst.as_ref()));
            }

            let var = generator.variable(&ident, ThisResolutionType::Local);
            let scope = generator.emit_resolve_scope(None, &var);
            debug_assert!(scope.is_some()); // Private names are always captured.
            let temp = Some(generator.new_temporary());
            let private_brand_symbol = generator.emit_get_private_brand(temp, scope.clone(), private_traits.is_static());
            generator.emit_check_private_brand(base.clone(), private_brand_symbol, private_traits.is_static());

            let value;
            if private_traits.is_getter() {
                let temp = Some(generator.new_temporary());
                let getter_setter_obj = generator.emit_get_from_scope(temp, scope.clone(), &var, ResolveMode::ThrowIfNotFound);
                let temp = Some(generator.new_temporary());
                let get_private_name = generator.property_names().builtin_names().get_private_name().clone();
                let getter_function = generator.emit_direct_get_by_id(temp, getter_setter_obj, &get_private_name);
                let mut args = CallArguments::new(generator, None, 0);
                generator.move_register(args.this_register().as_ref(), base.as_ref().unwrap());
                let temp = Some(generator.new_temporary());
                value = generator.emit_call(temp, getter_function, ExpectedFunction::NoExpectedFunction, &mut args, &position, &position, &position, DebuggableCall::Yes);
            } else {
                generator.emit_throw_type_error("Trying to access an undefined private getter");
                return Some(generator.temp_destination(dst.as_ref()));
            }

            let temp_dst = Some(generator.temp_destination(dst.as_ref()));
            let old_value = cpp3b_emit_post_inc_or_dec(generator, temp_dst, value.as_ref().unwrap(), self.operator);
            generator.emit_expression_info(&divot, &divot_start, &divot_end);

            if private_traits.is_setter() {
                let temp = Some(generator.new_temporary());
                let getter_setter_obj = generator.emit_get_from_scope(temp, scope, &var, ResolveMode::ThrowIfNotFound);
                let temp = Some(generator.new_temporary());
                let set_private_name = generator.property_names().builtin_names().set_dup_private_name().clone();
                let setter_function = generator.emit_direct_get_by_id(temp, getter_setter_obj, &set_private_name);
                let mut args = CallArguments::new(generator, None, 1);
                generator.move_register(args.this_register().as_ref(), base.as_ref().unwrap());
                generator.move_register(args.argument_register(0).as_ref(), value.as_ref().unwrap());
                let temp = Some(generator.new_temporary());
                generator.emit_call_ignore_result(temp, setter_function, ExpectedFunction::NoExpectedFunction, &mut args, &position, &position, &position, DebuggableCall::Yes);
                generator.emit_profile_type_divots(value, &divot_start, &divot_end);
                return generator.move_register(dst.as_ref(), old_value.as_ref().unwrap());
            }

            generator.emit_throw_type_error("Trying to access an undefined private getter");
            return generator.move_register(dst.as_ref(), old_value.as_ref().unwrap());
        }

        let value;
        let mut this_value: Cpp3bReg = None;
        if base_is_super {
            this_value = Some(generator.ensure_this());
            let temp = Some(generator.new_temporary());
            value = generator.emit_get_by_id_with_this(temp, base.clone(), this_value.clone(), &ident);
        } else {
            let temp = Some(generator.new_temporary());
            value = generator.emit_get_by_id(temp, base.clone(), &ident);
        }
        let temp_dst = Some(generator.temp_destination(dst.as_ref()));
        let old_value = cpp3b_emit_post_inc_or_dec(generator, temp_dst, value.as_ref().unwrap(), self.operator);
        generator.emit_expression_info(&divot, &divot_start, &divot_end);
        if base_is_super {
            generator.emit_put_by_id_with_this(base, this_value, &ident, value.clone());
        } else {
            generator.emit_put_by_id(base, &ident, value.clone());
        }
        generator.emit_profile_type_divots(value, &divot_start, &divot_end);
        generator.move_register(dst.as_ref(), old_value.as_ref().unwrap())
    }

    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        if self.expr.is_resolve_node() {
            return self.emit_resolve(generator, dst);
        }

        if self.expr.is_bracket_accessor_node() {
            return self.emit_bracket(generator, dst);
        }

        if self.expr.is_dot_accessor_node() {
            return self.emit_dot(generator, dst);
        }

        debug_assert!(self.expr.is_function_call());
        generator.emit_node_expression(None, &self.expr);
        self.throwable.base.emit_throw_reference_error(
            generator,
            if self.operator == crate::parser::nodes::Operator::PlusPlus {
                "Postfix ++ operator applied to value that is not a reference."
            } else {
                "Postfix -- operator applied to value that is not a reference."
            },
            dst,
        )
    }
}

// ------------------------------ DeleteResolveNode -----------------------------------

impl crate::parser::nodes::DeleteResolveNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        if var.local().is_some() {
            let final_dst = Some(generator.final_destination(dst.as_ref(), None));
            return generator.emit_load_js_value(final_dst, crate::runtime::js_value::js_boolean(false));
        }

        generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
        let base = generator.emit_resolve_scope(dst.clone(), &var);
        let final_dst = Some(generator.final_destination(dst.as_ref(), base.as_ref()));
        generator.emit_delete_by_id(final_dst, base, &self.ident)
    }
}

// ------------------------------ DeleteBracketNode -----------------------------------

impl crate::parser::nodes::DeleteBracketNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        let final_dest = Some(generator.final_destination(dst.as_ref(), None));
        let r0 = generator.emit_node_expression(None, &self.base_expr);

        if self.base_expr.base().is_optional_chain_base {
            generator.emit_optional_check(r0.as_ref().unwrap());
        }

        let r1 = generator.emit_node_expression(None, &self.subscript);
        generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
        if self.base_expr.is_super_node() {
            return self.throwable.emit_throw_reference_error(generator, "Cannot delete a super property", dst);
        }
        generator.emit_delete_by_val(final_dest, r0, r1)
    }
}

// ------------------------------ DeleteDotNode -----------------------------------

impl crate::parser::nodes::DeleteDotNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        let final_dest = Some(generator.final_destination(dst.as_ref(), None));
        let r0 = generator.emit_node_expression(None, &self.base_expr);

        if self.base_expr.base().is_optional_chain_base {
            generator.emit_optional_check(r0.as_ref().unwrap());
        }

        generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
        if self.base_expr.is_super_node() {
            return self.throwable.emit_throw_reference_error(generator, "Cannot delete a super property", dst);
        }
        generator.emit_delete_by_id(final_dest, r0, &self.ident)
    }
}

// ------------------------------ DeleteValueNode -----------------------------------

impl crate::parser::nodes::DeleteValueNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        generator.emit_node_in_ignore_result_position_expression(&self.expr);

        // delete on a non-location expression ignores the value and returns true
        let final_dst = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_load_js_value(final_dst, crate::runtime::js_value::js_boolean(true))
    }
}

// ------------------------------ VoidNode -------------------------------------

impl crate::parser::nodes::VoidNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        if generator.is_ignored_dst(dst.as_ref()) {
            generator.emit_node_in_ignore_result_position_expression(&self.expr);
            return None;
        }
        generator.emit_node_in_ignore_result_position_expression(&self.expr);
        generator.emit_load_js_value(dst, crate::runtime::js_value::js_undefined())
    }
}

// ------------------------------ TypeOfResolveNode -----------------------------------

impl crate::parser::nodes::TypeOfResolveNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        use crate::runtime::get_put_info::ResolveMode;
        let divot_end = self.throwable.divot_end;
        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let new_divot = divot_end - self.ident.length();
        if let Some(local) = var.local() {
            generator.emit_expression_info(&new_divot, &new_divot, &divot_end);
            generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);
            if generator.is_ignored_dst(dst.as_ref()) {
                return None;
            }
            let final_dst = Some(generator.final_destination(dst.as_ref(), None));
            return generator.emit_type_of(final_dst, Some(local));
        }

        let scope = generator.emit_resolve_scope(dst.clone(), &var);
        let temp = Some(generator.new_temporary());
        let value = generator.emit_get_from_scope(temp, scope.clone(), &var, ResolveMode::DoNotThrowIfNotFound);
        generator.emit_expression_info(&new_divot, &new_divot, &divot_end);
        generator.emit_tdz_check_if_necessary(&var, value.clone(), None);
        if generator.is_ignored_dst(dst.as_ref()) {
            return None;
        }
        let final_dst = Some(generator.final_destination(dst.as_ref(), scope.as_ref()));
        generator.emit_type_of(final_dst, value)
    }
}

// ------------------------------ TypeOfValueNode -----------------------------------

impl crate::parser::nodes::TypeOfValueNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        if generator.is_ignored_dst(dst.as_ref()) {
            generator.emit_node_in_ignore_result_position_expression(&self.expr);
            return None;
        }
        let src = generator.emit_node_expression(None, &self.expr);
        let final_dst = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_type_of(final_dst, src)
    }
}

// ------------------------------ PrefixNode ----------------------------------

impl crate::parser::nodes::PrefixNode {
    pub fn emit_resolve(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        use crate::bytecompiler::bytecode_generator::ThisResolutionType;
        use crate::runtime::get_put_info::{InitializationMode, ResolveMode};
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;
        let resolve = match &self.expr {
            crate::parser::nodes::Expression::Resolve(resolve) => resolve.clone(),
            _ => unreachable!("ASSERT(m_expr->isResolveNode())"),
        };
        let ident = resolve.borrow().ident.clone();

        let var = generator.variable(&ident, ThisResolutionType::Local);
        if let Some(local) = var.local() {
            generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);
            let mut local_reg = Some(local.clone());
            if var.is_read_only() {
                generator.emit_read_only_exception_if_needed(&var);
                let temp = Some(generator.temp_destination(dst.as_ref()));
                local_reg = generator.move_register(temp.as_ref(), local_reg.as_ref().unwrap());
            } else if generator.should_emit_type_profiler_hooks() {
                let temp_dst = Some(generator.temp_destination(dst.as_ref()));
                generator.move_register(temp_dst.as_ref(), local_reg.as_ref().unwrap());
                cpp3b_emit_inc_or_dec(generator, temp_dst.as_ref().unwrap(), self.operator);
                generator.move_register(local_reg.as_ref(), temp_dst.as_ref().unwrap());
                generator.emit_profile_type_variable(local_reg, &var, &divot_start, &divot_end);
                return generator.move_register(dst.as_ref(), temp_dst.as_ref().unwrap());
            }
            cpp3b_emit_inc_or_dec(generator, local_reg.as_ref().unwrap(), self.operator);
            return generator.move_register(dst.as_ref(), local_reg.as_ref().unwrap());
        }

        generator.emit_expression_info(&divot, &divot_start, &divot_end);
        let scope = generator.emit_resolve_scope(dst.clone(), &var);
        let temp = Some(generator.new_temporary());
        let value = generator.emit_get_from_scope(temp, scope.clone(), &var, ResolveMode::ThrowIfNotFound);
        generator.emit_tdz_check_if_necessary(&var, value.clone(), None);
        if var.is_read_only() {
            let threw_exception = generator.emit_read_only_exception_if_needed(&var);
            if threw_exception {
                return value;
            }
        }

        cpp3b_emit_inc_or_dec(generator, value.as_ref().unwrap(), self.operator);
        if !var.is_read_only() {
            let resolve_mode = if generator.ecma_mode().is_strict() { ResolveMode::ThrowIfNotFound } else { ResolveMode::DoNotThrowIfNotFound };
            generator.emit_put_to_scope(scope, &var, value.clone(), resolve_mode, InitializationMode::NotInitialization);
            generator.emit_profile_type_variable(value.clone(), &var, &divot_start, &divot_end);
        }
        generator.move_register(dst.as_ref(), value.as_ref().unwrap())
    }

    pub fn emit_bracket(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;
        let bracket_accessor = match &self.expr {
            crate::parser::nodes::Expression::BracketAccessor(bracket_accessor) => bracket_accessor.clone(),
            _ => unreachable!("ASSERT(m_expr->isBracketAccessorNode())"),
        };
        let bracket_accessor = bracket_accessor.borrow();
        let base_node = bracket_accessor.base_expr.clone();
        let subscript = bracket_accessor.subscript.clone();

        let subscript_is_pure = subscript.is_pure(generator);
        let base = generator.emit_node_for_left_hand_side(&base_node, bracket_accessor.subscript_has_assignments, subscript_is_pure);
        let mut property = generator.emit_node_for_property(&subscript);
        if !subscript.is_number() && !subscript.is_string() {
            // Never double-evaluate the subscript expression;
            // don't even evaluate it once if the base isn't subscriptable.
            generator.emit_require_object_coercible(base.as_ref().unwrap(), "Cannot access property of undefined or null");
            let temp = Some(generator.new_temporary());
            property = generator.emit_to_property_key_or_number(temp, property);
        }
        let prop_dst = Some(generator.temp_destination(dst.as_ref()));

        generator.emit_expression_info(&bracket_accessor.throwable.divot, &bracket_accessor.throwable.divot_start, &bracket_accessor.throwable.divot_end);
        let value;
        let mut this_value: Cpp3bReg = None;
        if base_node.is_super_node() {
            this_value = Some(generator.ensure_this());
            value = generator.emit_get_by_val_with_this(prop_dst.clone(), base.clone(), this_value.clone(), property.clone());
        } else {
            value = generator.emit_get_by_val(prop_dst.clone(), base.clone(), property.clone());
        }
        cpp3b_emit_inc_or_dec(generator, value.as_ref().unwrap(), self.operator);
        generator.emit_expression_info(&divot, &divot_start, &divot_end);
        if base_node.is_super_node() {
            generator.emit_put_by_val_with_this(base, this_value, property, value.clone());
        } else {
            generator.emit_put_by_val(base, property, value.clone());
        }
        generator.emit_profile_type_divots(value, &divot_start, &divot_end);
        generator.move_register(dst.as_ref(), prop_dst.as_ref().unwrap())
    }

    pub fn emit_dot(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction, ThisResolutionType};
        use crate::runtime::get_put_info::ResolveMode;
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;
        let position = *self.position();
        let dot_accessor = match &self.expr {
            crate::parser::nodes::Expression::DotAccessor(dot_accessor) => dot_accessor.clone(),
            _ => unreachable!("ASSERT(m_expr->isDotAccessorNode())"),
        };
        let dot_accessor = dot_accessor.borrow();
        let base_node = dot_accessor.base_expr.clone();
        let ident = dot_accessor.ident.clone();

        let base = generator.emit_node_expression(None, &base_node);
        let prop_dst = Some(generator.temp_destination(dst.as_ref()));

        generator.emit_expression_info(&dot_accessor.throwable.divot, &dot_accessor.throwable.divot_start, &dot_accessor.throwable.divot_end);
        let value;
        if dot_accessor.is_private_member() {
            let private_traits = generator.get_private_traits(&ident);
            if private_traits.is_field() {
                debug_assert!(!base_node.is_super_node());
                let var = generator.variable(&ident, ThisResolutionType::Local);
                let scope = generator.emit_resolve_scope(None, &var);
                let private_name = Some(generator.new_temporary());
                generator.emit_get_from_scope(private_name.clone(), scope, &var, ResolveMode::DoNotThrowIfNotFound);

                let value = generator.emit_get_private_name(prop_dst.clone(), base.clone(), private_name.clone());
                cpp3b_emit_inc_or_dec(generator, value.as_ref().unwrap(), self.operator);
                generator.emit_expression_info(&divot, &divot_start, &divot_end);
                generator.emit_private_field_put(base, private_name, value.clone());
                generator.emit_profile_type_divots(value, &divot_start, &divot_end);
                return generator.move_register(dst.as_ref(), prop_dst.as_ref().unwrap());
            }

            if private_traits.is_method() {
                let var = generator.variable(&ident, ThisResolutionType::Local);
                let scope = generator.emit_resolve_scope(None, &var);
                debug_assert!(scope.is_some()); // Private names are always captured.
                let temp = Some(generator.new_temporary());
                let private_brand_symbol = generator.emit_get_private_brand(temp, scope, private_traits.is_static());
                generator.emit_check_private_brand(base, private_brand_symbol, private_traits.is_static());

                generator.emit_expression_info(&divot, &divot_start, &divot_end);
                generator.emit_throw_type_error("Trying to access an undefined private setter");
                return generator.move_register(dst.as_ref(), prop_dst.as_ref().unwrap());
            }

            let var = generator.variable(&ident, ThisResolutionType::Local);
            let scope = generator.emit_resolve_scope(None, &var);
            debug_assert!(scope.is_some()); // Private names are always captured.
            let temp = Some(generator.new_temporary());
            let private_brand_symbol = generator.emit_get_private_brand(temp, scope.clone(), private_traits.is_static());
            generator.emit_check_private_brand(base.clone(), private_brand_symbol, private_traits.is_static());

            let private_value;
            if private_traits.is_getter() {
                let temp = Some(generator.new_temporary());
                let getter_setter_obj = generator.emit_get_from_scope(temp, scope.clone(), &var, ResolveMode::ThrowIfNotFound);
                let temp = Some(generator.new_temporary());
                let get_private_name = generator.property_names().builtin_names().get_private_name().clone();
                let getter_function = generator.emit_direct_get_by_id(temp, getter_setter_obj, &get_private_name);
                let mut args = CallArguments::new(generator, None, 0);
                generator.move_register(args.this_register().as_ref(), base.as_ref().unwrap());
                private_value = generator.emit_call(prop_dst.clone(), getter_function, ExpectedFunction::NoExpectedFunction, &mut args, &position, &position, &position, DebuggableCall::Yes);
            } else {
                generator.emit_throw_type_error("Trying to access an undefined private getter");
                return generator.move_register(dst.as_ref(), prop_dst.as_ref().unwrap());
            }

            cpp3b_emit_inc_or_dec(generator, private_value.as_ref().unwrap(), self.operator);
            generator.emit_expression_info(&divot, &divot_start, &divot_end);

            if private_traits.is_setter() {
                let temp = Some(generator.new_temporary());
                let getter_setter_obj = generator.emit_get_from_scope(temp, scope, &var, ResolveMode::ThrowIfNotFound);
                let temp = Some(generator.new_temporary());
                let set_private_name = generator.property_names().builtin_names().set_dup_private_name().clone();
                let setter_function = generator.emit_direct_get_by_id(temp, getter_setter_obj, &set_private_name);
                let mut args = CallArguments::new(generator, None, 1);
                generator.move_register(args.this_register().as_ref(), base.as_ref().unwrap());
                generator.move_register(args.argument_register(0).as_ref(), private_value.as_ref().unwrap());
                let temp = Some(generator.new_temporary());
                generator.emit_call_ignore_result(temp, setter_function, ExpectedFunction::NoExpectedFunction, &mut args, &position, &position, &position, DebuggableCall::Yes);
                generator.emit_profile_type_divots(private_value, &divot_start, &divot_end);
                return generator.move_register(dst.as_ref(), prop_dst.as_ref().unwrap());
            }

            generator.emit_throw_type_error("Trying to access an undefined private getter");
            return generator.move_register(dst.as_ref(), prop_dst.as_ref().unwrap());
        }

        let mut this_value: Cpp3bReg = None;
        if base_node.is_super_node() {
            this_value = Some(generator.ensure_this());
            value = generator.emit_get_by_id_with_this(prop_dst.clone(), base.clone(), this_value.clone(), &ident);
        } else {
            value = generator.emit_get_by_id(prop_dst.clone(), base.clone(), &ident);
        }
        cpp3b_emit_inc_or_dec(generator, value.as_ref().unwrap(), self.operator);
        generator.emit_expression_info(&divot, &divot_start, &divot_end);
        if base_node.is_super_node() {
            generator.emit_put_by_id_with_this(base, this_value, &ident, value.clone());
        } else {
            generator.emit_put_by_id(base, &ident, value.clone());
        }
        generator.emit_profile_type_divots(value, &divot_start, &divot_end);
        generator.move_register(dst.as_ref(), prop_dst.as_ref().unwrap())
    }

    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        if self.expr.is_resolve_node() {
            return self.emit_resolve(generator, dst);
        }

        if self.expr.is_bracket_accessor_node() {
            return self.emit_bracket(generator, dst);
        }

        if self.expr.is_dot_accessor_node() {
            return self.emit_dot(generator, dst);
        }

        debug_assert!(self.expr.is_function_call());
        generator.emit_node_expression(None, &self.expr);
        self.throwable.base.emit_throw_reference_error(
            generator,
            if self.operator == crate::parser::nodes::Operator::PlusPlus {
                "Prefix ++ operator applied to value that is not a reference."
            } else {
                "Prefix -- operator applied to value that is not a reference."
            },
            dst,
        )
    }
}

// ------------------------------ Unary Operation Nodes -----------------------------------

impl crate::parser::nodes::UnaryOpNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        if generator.is_ignored_dst(dst.as_ref()) {
            // op_not is not user-observable. We can skip it completely if the result is not used.
            // This is used in the wild, for example,
            // ```
            //     !(function (a) {
            //          ...
            //     })(a);
            // ```
            if self.opcode_id == crate::bytecode::opcode::OpcodeID::op_not {
                generator.emit_node_in_ignore_result_position_expression(&self.expr);
                return None;
            }
        }
        let src = generator.emit_node_expression(None, &self.expr);
        let position = *self.position();
        generator.emit_expression_info(&position, &position, &position);
        let final_dst = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_unary_op_dynamic(self.opcode_id, final_dst, src, self.expr.result_descriptor())
    }
}

// ------------------------------ UnaryPlusNode -----------------------------------

impl crate::parser::nodes::UnaryPlusNode {
    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        debug_assert!(self.opcode_id == crate::bytecode::opcode::OpcodeID::op_to_number);
        let src = generator.emit_node_expression(None, &self.expr);
        let position = *self.position();
        generator.emit_expression_info(&position, &position, &position);
        let final_dst = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_to_number(final_dst, src)
    }
}

// ------------------------------ LogicalNotNode -----------------------------------

impl crate::parser::nodes::LogicalNotNode {
    pub fn emit_bytecode_in_condition_context(
        &self,
        this: &crate::parser::nodes::Expression,
        generator: &mut Cpp3bGen,
        true_target: &crate::bytecompiler::label::LabelRef,
        false_target: &crate::bytecompiler::label::LabelRef,
        fall_through_mode: crate::parser::nodes::FallThroughMode,
    ) {
        if this.base().needs_debug_hook() {
            generator.emit_debug_hook_expression_data(this, None);
        }

        // Reverse the true and false targets.
        generator.emit_node_in_condition_context(&self.expr, false_target, true_target, crate::parser::nodes::invert(fall_through_mode));
    }
}

// ------------------------------ Binary Operation Nodes -----------------------------------

/// `UInt32Result` do lambda `isUInt32` de `BinaryOpNode::emitBytecode`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cpp3bUInt32Result {
    UInt32,
    Constant,
}

/// O lambda `isUInt32` de `BinaryOpNode::emitBytecode`.
fn cpp3b_is_uint32(node: &crate::parser::nodes::Expression) -> Option<Cpp3bUInt32Result> {
    if node.is_binary_op_node() && cpp3b_binary_opcode_id(node) == crate::bytecode::opcode::OpcodeID::op_urshift {
        return Some(Cpp3bUInt32Result::UInt32);
    }
    if let crate::parser::nodes::Expression::Integer(integer) = node {
        let value = crate::runtime::js_value::js_number(integer.borrow().value);
        if value.is_int32() && value.as_int32() >= 0 {
            return Some(Cpp3bUInt32Result::Constant);
        }
    }
    None
}

/// `canFoldToBranch`.
fn cpp3b_can_fold_to_branch(
    opcode_id: crate::bytecode::opcode::OpcodeID,
    branch_expression: &crate::parser::nodes::Expression,
    constant: crate::runtime::js_value::JSValue,
) -> bool {
    use crate::bytecode::opcode::OpcodeID;
    let expression_type = branch_expression.result_descriptor();

    if expression_type.definitely_is_boolean() && constant.is_boolean() {
        true
    } else if expression_type.definitely_is_boolean() && constant.is_int32() && (constant.as_int32() == 0 || constant.as_int32() == 1) {
        opcode_id == OpcodeID::op_eq || opcode_id == OpcodeID::op_neq // Strict equality is false in the case of type mismatch.
    } else {
        expression_type.is_int32() && constant.is_int32() && constant.as_int32() == 0
    }
}

impl crate::parser::nodes::BinaryOpNode {
    // BinaryOpNode::emitStrcat:
    //
    // This node generates an op_strcat operation.  This opcode can handle concatenation of three or
    // more values, where we can determine a set of separate op_add operations would be operating on
    // string values.
    //
    // This function expects to be operating on a graph of AST nodes looking something like this:
    //
    //     (a)...     (b)
    //          \   /
    //           (+)     (c)
    //              \   /
    //      [d]     ((+))
    //         \    /
    //          [+=]
    //
    // The assignment operation is optional, if it exists the register holding the value on the
    // lefthand side of the assignment should be passing as the optional 'lhs' argument.
    //
    // The method should be called on the node at the root of the tree of regular binary add
    // operations (marked in the diagram with a double set of parentheses).  This node must
    // be performing a string concatenation (determined by statically detecting that at least
    // one child must be a string).
    //
    // Since the minimum number of values being concatenated together is expected to be 3, if
    // a lhs to a concatenating assignment is not provided then the  root add should have at
    // least one left child that is also an add that can be determined to be operating on strings.
    pub fn emit_strcat(
        &self,
        generator: &mut Cpp3bGen,
        dst: Cpp3bReg,
        lhs: Cpp3bReg,
        emit_expression_info_for_me: Option<&crate::parser::nodes::ReadModifyResolveNode>,
    ) -> Cpp3bReg {
        debug_assert!(self.result_descriptor().definitely_is_string());

        // Create a list of expressions for all the adds in the tree of nodes we can convert into
        // a string concatenation.  The rightmost node (c) is added first.  The rightmost node is
        // added first, and the leftmost child is never added, so the vector produced for the
        // example above will be [ c, b ].
        let mut reverse_expression_list: Vec<crate::parser::nodes::Expression> = vec![self.expr2.clone()];

        // Examine the left child of the add.  So long as this is a string add, add its right-child
        // to the list, and keep processing along the left fork.
        let mut left_most_add_child = self.expr1.clone();
        while left_most_add_child.is_add() && left_most_add_child.result_descriptor().definitely_is_string() {
            let (expr1, expr2) = match &left_most_add_child {
                crate::parser::nodes::Expression::Add(add) => (add.borrow().expr1.clone(), add.borrow().expr2.clone()),
                _ => unreachable!("esperava um AddNode"),
            };
            reverse_expression_list.push(expr2);
            left_most_add_child = expr1;
        }

        let mut temporary_registers: Vec<crate::bytecompiler::bytecode_generator::RegisterRef> = Vec::new();

        // If there is an assignment, allocate a temporary to hold the lhs after conversion.
        // We could possibly avoid this (the lhs is converted last anyway, we could let the
        // op_strcat node handle its conversion if required).
        if lhs.is_some() {
            temporary_registers.push(generator.new_temporary());
        }

        // Emit code for the leftmost node ((a) in the example).
        temporary_registers.push(generator.new_temporary());
        let mut left_most_add_child_temp_register = Some(temporary_registers.last().unwrap().clone());
        generator.emit_node_expression(left_most_add_child_temp_register.clone(), &left_most_add_child);

        // Note on ordering of conversions:
        //
        // We maintain the same ordering of conversions as we would see if the concatenations
        // was performed as a sequence of adds (otherwise this optimization could change
        // behaviour should an object have been provided a valueOf or toString method).
        //
        // Considering the above example, the sequnce of execution is:
        //     * evaluate operand (a)
        //     * evaluate operand (b)
        //     * convert (a) to primitive   <-  (this would be triggered by the first add)
        //     * convert (b) to primitive   <-  (ditto)
        //     * evaluate operand (c)
        //     * convert (c) to primitive   <-  (this would be triggered by the second add)
        // And optionally, if there is an assignment:
        //     * convert (d) to primitive   <-  (this would be triggered by the assigning addition)
        //
        // As such we do not plant an op to convert the leftmost child now.  Instead, use
        // 'leftMostAddChildTempRegister' as a flag to trigger generation of the conversion
        // once the second node has been generated.  However, if the leftmost child is an
        // immediate we can trivially determine that no conversion will be required.
        // If this is the case
        if left_most_add_child.is_string() {
            left_most_add_child_temp_register = None;
        }

        while let Some(node) = reverse_expression_list.pop() {
            // Emit the code for the current node.
            temporary_registers.push(generator.new_temporary());
            let last = Some(temporary_registers.last().unwrap().clone());
            generator.emit_node_expression(last.clone(), &node);

            // On the first iteration of this loop, when we first reach this point we have just
            // generated the second node, which means it is time to convert the leftmost operand.
            if left_most_add_child_temp_register.is_some() {
                generator.emit_to_primitive(left_most_add_child_temp_register.clone(), left_most_add_child_temp_register.clone());
                left_most_add_child_temp_register = None; // Only do this once.
            }
            // Plant a conversion for this node, if necessary.
            if !node.is_string() {
                generator.emit_to_primitive(last.clone(), last);
            }
        }
        debug_assert!(temporary_registers.len() >= 3);

        // Certain read-modify nodes require expression info to be emitted *after* m_right has been generated.
        // If this is required the node is passed as 'emitExpressionInfoForMe'; do so now.
        if let Some(read_modify) = emit_expression_info_for_me {
            generator.emit_expression_info(&read_modify.throwable.divot, &read_modify.throwable.divot_start, &read_modify.throwable.divot_end);
        }
        // If there is an assignment convert the lhs now.  This will also copy lhs to
        // the temporary register we allocated for it.
        if lhs.is_some() {
            generator.emit_to_primitive(Some(temporary_registers[0].clone()), lhs);
        }

        let first = Some(temporary_registers[0].clone());
        let final_dst = Some(generator.final_destination(dst.as_ref(), first.as_ref()));
        generator.emit_strcat(final_dst, first, temporary_registers.len() as i32)
    }

    pub fn emit_bytecode_in_condition_context(
        &self,
        this: &crate::parser::nodes::Expression,
        generator: &mut Cpp3bGen,
        true_target: &crate::bytecompiler::label::LabelRef,
        false_target: &crate::bytecompiler::label::LabelRef,
        fall_through_mode: crate::parser::nodes::FallThroughMode,
    ) {
        use crate::parser::source_tainted_origin::TriState;
        let (branch_condition, branch_expression) = self.try_fold_to_branch(generator);

        if this.base().needs_debug_hook() && branch_condition != TriState::Indeterminate {
            generator.emit_debug_hook_expression_data(this, None);
        }

        if branch_condition == TriState::Indeterminate {
            expression_node_emit_bytecode_in_condition_context(this, generator, true_target, false_target, fall_through_mode);
        } else if branch_condition == TriState::True {
            generator.emit_node_in_condition_context(branch_expression.as_ref().unwrap(), true_target, false_target, fall_through_mode);
        } else {
            generator.emit_node_in_condition_context(
                branch_expression.as_ref().unwrap(),
                false_target,
                true_target,
                crate::parser::nodes::invert(fall_through_mode),
            );
        }
    }

    /// `tryFoldToBranch`: devolve `(branchCondition, branchExpression)` (os parâmetros de saída do C++).
    pub fn try_fold_to_branch(
        &self,
        generator: &mut Cpp3bGen,
    ) -> (crate::parser::source_tainted_origin::TriState, Option<crate::parser::nodes::Expression>) {
        use crate::bytecode::opcode::OpcodeID;
        use crate::parser::source_tainted_origin::TriState;
        let branch_condition = TriState::Indeterminate;

        let constant_expression;
        let branch_expression;
        if self.expr1.is_constant() {
            constant_expression = self.expr1.clone();
            branch_expression = self.expr2.clone();
        } else if self.expr2.is_constant() {
            constant_expression = self.expr2.clone();
            branch_expression = self.expr1.clone();
        } else {
            return (branch_condition, None);
        }

        let opcode_id = self.opcode_id;
        let value = match nodes_codegen_constant_js_value(generator, &constant_expression) {
            Some(value) => value,
            None => return (branch_condition, Some(branch_expression)),
        };
        if !cpp3b_can_fold_to_branch(opcode_id, &branch_expression, value) {
            return (branch_condition, Some(branch_expression));
        }

        let mut branch_condition = branch_condition;
        if opcode_id == OpcodeID::op_eq || opcode_id == OpcodeID::op_stricteq {
            branch_condition = if value.pure_to_boolean() != TriState::False { TriState::True } else { TriState::False };
        } else if opcode_id == OpcodeID::op_neq || opcode_id == OpcodeID::op_nstricteq {
            branch_condition = if value.pure_to_boolean() == TriState::False { TriState::True } else { TriState::False };
        }
        (branch_condition, Some(branch_expression))
    }

    pub fn emit_bytecode(&self, generator: &mut Cpp3bGen, dst: Cpp3bReg) -> Cpp3bReg {
        use crate::bytecode::opcode::OpcodeID;
        use crate::parser::result_type::OperandTypes;
        let opcode_id = self.opcode_id;
        let position = *self.position();

        if opcode_id == OpcodeID::op_less || opcode_id == OpcodeID::op_lesseq || opcode_id == OpcodeID::op_greater || opcode_id == OpcodeID::op_greatereq {
            let left_result = cpp3b_is_uint32(&self.expr1);
            let right_result = cpp3b_is_uint32(&self.expr2);
            if left_result.is_some()
                && right_result.is_some()
                && (left_result == Some(Cpp3bUInt32Result::UInt32) || right_result == Some(Cpp3bUInt32Result::UInt32))
            {
                let left = self.expr1.clone();
                let right = self.expr2.clone();
                if left.is_binary_op_node() {
                    debug_assert!(cpp3b_binary_opcode_id(&left) == OpcodeID::op_urshift);
                    cpp3b_with_binary_op_node!(&left, n, n.borrow_mut().should_to_unsigned_result = false);
                }
                if right.is_binary_op_node() {
                    debug_assert!(cpp3b_binary_opcode_id(&right) == OpcodeID::op_urshift);
                    cpp3b_with_binary_op_node!(&right, n, n.borrow_mut().should_to_unsigned_result = false);
                }
                let right_is_pure = right.is_pure(generator);
                let mut src1 = generator.emit_node_for_left_hand_side(&left, self.right_has_assignments, right_is_pure);
                let mut src2 = generator.emit_node_expression(None, &right);
                generator.emit_expression_info(&position, &position, &position);

                // Since the both sides only accept Int32, replacing operands is not observable to users.
                let mut replace_operands = false;
                let result_op = match opcode_id {
                    OpcodeID::op_less => OpcodeID::op_below,
                    OpcodeID::op_lesseq => OpcodeID::op_beloweq,
                    OpcodeID::op_greater => {
                        replace_operands = true;
                        OpcodeID::op_below
                    }
                    OpcodeID::op_greatereq => {
                        replace_operands = true;
                        OpcodeID::op_beloweq
                    }
                    _ => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
                };
                let mut operand_types = OperandTypes::new(left.result_descriptor(), right.result_descriptor());
                if replace_operands {
                    std::mem::swap(&mut src1, &mut src2);
                    operand_types = OperandTypes::new(right.result_descriptor(), left.result_descriptor());
                }
                let final_dst = Some(generator.final_destination(dst.as_ref(), src1.as_ref()));
                return generator.emit_binary_op_dynamic(result_op, final_dst, src1, src2, operand_types);
            }
        }

        if opcode_id == OpcodeID::op_add && self.expr1.is_add() && self.expr1.result_descriptor().definitely_is_string() {
            generator.emit_expression_info(&position, &position, &position);
            return self.emit_strcat(generator, dst, None, None);
        }

        if opcode_id == OpcodeID::op_neq && (self.expr1.is_null() || self.expr2.is_null()) {
            let src = generator.emit_node_expression(None, if self.expr1.is_null() { &self.expr2 } else { &self.expr1 });
            let final_dst = Some(generator.final_destination(dst.as_ref(), src.as_ref()));
            return generator.emit_unary_op::<crate::bytecode::bytecode_ops::OpNeqNull>(final_dst, src);
        }

        let mut left = self.expr1.clone();
        let mut right = self.expr2.clone();
        if (opcode_id == OpcodeID::op_neq || opcode_id == OpcodeID::op_nstricteq) && left.is_string() {
            std::mem::swap(&mut left, &mut right);
        }

        let right_is_pure = right.is_pure(generator);
        let src1 = generator.emit_node_for_left_hand_side(&left, self.right_has_assignments, right_is_pure);
        let was_typeof = generator.last_opcode_id() == OpcodeID::op_typeof;
        let src2 = generator.emit_node_expression(None, &right);
        generator.emit_expression_info(&position, &position, &position);
        if was_typeof && (opcode_id == OpcodeID::op_neq || opcode_id == OpcodeID::op_nstricteq) {
            let tmp = Some(generator.temp_destination(dst.as_ref()));
            let equality_dst = Some(generator.final_destination(tmp.as_ref(), src1.as_ref()));
            if opcode_id == OpcodeID::op_neq {
                generator.emit_equality_op::<crate::bytecode::bytecode_ops::OpEq>(equality_dst, src1, src2);
            } else {
                generator.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(equality_dst, src1, src2);
            }
            let final_dst = Some(generator.final_destination(dst.as_ref(), tmp.as_ref()));
            return generator.emit_unary_op::<crate::bytecode::bytecode_ops::OpNot>(final_dst, tmp);
        }
        let final_dst = Some(generator.final_destination(dst.as_ref(), src1.as_ref()));
        let result = generator.emit_binary_op_dynamic(
            opcode_id,
            final_dst,
            src1,
            src2,
            OperandTypes::new(left.result_descriptor(), right.result_descriptor()),
        );
        if self.should_to_unsigned_result && opcode_id == OpcodeID::op_urshift && !generator.is_ignored_dst(dst.as_ref()) {
            return generator.emit_unary_op::<crate::bytecode::bytecode_ops::OpUnsigned>(result.clone(), result);
        }
        result
    }
}
