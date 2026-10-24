// NodesCodegen.cpp, linhas 2228 a 2414: `FunctionCallDotNode::emitBytecode`,
// `maxDistanceToInnermostCallOrApply`, `CallFunctionCallDotNode::emitBytecode` e
// `HasOwnPropertyFunctionCallDotNode::emitBytecode` (incluído por include!, sem `use`).
// Pára antes de `areTrivialApplyArguments` (linha 2411), que abre o `ApplyFunctionCallDotNode`.
//
// Convenções desta fatia: registrador é `Option<RegisterRef>` (o `RegisterID*` nulo do C++), o
// `RefPtr<RegisterID>` é o próprio `Option<RegisterRef>` (Rc), `.get()` vira `.clone()`.

type Cpp3Reg = Option<crate::bytecompiler::bytecode_generator::RegisterRef>;

/// `CallFunctionCallDotNode`/`ApplyFunctionCallDotNode`: `makeFunction` (o lambda do C++), que
/// carrega `base.call` ou `base.apply` conforme `name`.
fn cpp3_make_call_or_apply_function(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    base_is_super: bool,
    base: &Cpp3Reg,
    dst: &Cpp3Reg,
    is_optional_call: bool,
    name: &crate::runtime::identifier::Identifier,
) -> Cpp3Reg {
    let temp = Some(generator.temp_destination(dst.as_ref()));
    let function = if base_is_super {
        let this_value = Some(generator.ensure_this());
        generator.emit_get_by_id_with_this(temp, base.clone(), this_value, name)
    } else {
        generator.emit_get_by_id(temp, base.clone(), name)
    };

    if is_optional_call {
        generator.emit_optional_check(function.as_ref().unwrap());
    }
    function
}

// ------------------------------ FunctionCallDotNode ----------------------------------

impl crate::parser::nodes::FunctionCallDotNode {
    pub fn emit_bytecode(
        &self,
        this_node: &crate::parser::nodes::NodeRef<crate::parser::nodes::FunctionCallDotNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp3Reg,
    ) -> Cpp3Reg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction};
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;

        let function = Some(generator.temp_destination(dst.as_ref()));
        let return_value = Some(generator.final_destination(dst.as_ref(), function.as_ref()));
        let call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
        let mut call_arguments = call_arguments;
        let base_is_super = self.base_expr.is_super_node();
        let should_get_arguments_dot_length_fast =
            generator.should_get_arguments_dot_length_fast(&crate::parser::nodes::Expression::FunctionCallDot(this_node.clone()));
        if base_is_super {
            let this_value = generator.ensure_this();
            generator.move_register(call_arguments.this_register().as_ref(), &this_value);
        } else if should_get_arguments_dot_length_fast {
            generator.emit_load_js_value(call_arguments.this_register(), crate::runtime::js_value::js_undefined());
        } else {
            generator.emit_node_expression(call_arguments.this_register(), &self.base_expr);
            if self.base_expr.base().is_optional_chain_base {
                generator.emit_optional_check(call_arguments.this_register().as_ref().unwrap());
            }
        }
        generator.emit_expression_info(
            &self.throwable.subexpression_divot(),
            &self.throwable.subexpression_start(),
            &self.throwable.subexpression_end(),
        );

        if should_get_arguments_dot_length_fast {
            generator.emit_argument_count(function.clone());
        } else {
            let base = if base_is_super {
                emit_super_base_for_callee(generator)
            } else {
                call_arguments.this_register()
            };
            self.emit_get_property_value(generator, function.clone(), base);
        }

        if self.is_optional_call {
            generator.emit_optional_check(function.as_ref().unwrap());
        }

        let ret = generator.emit_call_in_tail_position(
            return_value.clone(),
            function.clone(),
            ExpectedFunction::NoExpectedFunction,
            &mut call_arguments,
            &divot,
            &divot_start,
            &divot_end,
            DebuggableCall::Yes,
        );
        generator.emit_profile_type_divots(return_value, &divot_start, &divot_end);
        ret
    }
}

/// `static constexpr size_t maxDistanceToInnermostCallOrApply = 2;`
const MAX_DISTANCE_TO_INNERMOST_CALL_OR_APPLY: usize = 2;

// ------------------------------ CallFunctionCallDotNode ----------------------------------

impl crate::parser::nodes::CallFunctionCallDotNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp3Reg,
    ) -> Cpp3Reg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction};
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;

        let return_value = Some(generator.final_destination(dst.as_ref(), None));
        let base = generator.emit_node_expression(None, &self.base_expr);

        if self.base_expr.base().is_optional_chain_base {
            generator.emit_optional_check(base.as_ref().unwrap());
        }

        generator.emit_expression_info(
            &self.throwable.subexpression_divot(),
            &self.throwable.subexpression_start(),
            &self.throwable.subexpression_end(),
        );

        let base_is_super = self.base_expr.is_super_node();
        let is_optional_call = self.is_optional_call;
        let call_name = generator.property_names().builtin_names().call_public_name().clone();

        let emit_call_check = !generator.is_builtin_function();
        if self.distance_to_innermost_call_or_apply > MAX_DISTANCE_TO_INNERMOST_CALL_OR_APPLY && emit_call_check {
            let function = cpp3_make_call_or_apply_function(generator, base_is_super, &base, &dst, is_optional_call, &call_name);
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

        let mut function: Cpp3Reg = None;
        if emit_call_check {
            function = cpp3_make_call_or_apply_function(generator, base_is_super, &base, &dst, is_optional_call, &call_name);
            generator.emit_jump_if_not_function_call(function.as_ref().unwrap(), &real_call);
        }
        {
            let first_expr = self
                .args
                .borrow()
                .list_node
                .as_ref()
                .map(|list| list.borrow().expr.clone());
            if first_expr.as_ref().is_some_and(|expr| expr.is_spread_expression()) {
                let spread = match first_expr.as_ref() {
                    Some(crate::parser::nodes::Expression::SpreadExpression(spread)) => spread.clone(),
                    _ => unreachable!("esperava um SpreadExpressionNode"),
                };
                let spread = spread.borrow();
                let subject = &spread.expression;
                let arguments_register = generator.emit_node_expression(None, subject);
                generator.emit_expression_info(&spread.throwable.divot, &spread.throwable.divot_start, &spread.throwable.divot_end);
                let zero = generator.emit_load_js_value(None, crate::runtime::js_value::js_number(0));
                let temp = Some(generator.new_temporary());
                let this_register = generator.emit_get_by_val(temp, arguments_register.clone(), zero);
                let first_free_register = Some(generator.new_temporary());
                generator.emit_call_varargs_in_tail_position(
                    return_value.clone(),
                    base.clone(),
                    this_register,
                    arguments_register,
                    first_free_register,
                    1,
                    &divot,
                    &divot_start,
                    &divot_end,
                    DebuggableCall::Yes,
                );
            } else if let Some(first_expr) = first_expr {
                let old_list = self.args.borrow().list_node.clone();
                let next = old_list.as_ref().and_then(|list| list.borrow().next.clone());
                self.args.borrow_mut().list_node = next;

                let temp = Some(generator.temp_destination(dst.as_ref()));
                let real_function = generator.move_register(temp.as_ref(), base.as_ref().unwrap());
                let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
                generator.emit_node_expression(call_arguments.this_register(), &first_expr);
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
                self.args.borrow_mut().list_node = old_list;
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
        }
        if emit_call_check {
            generator.emit_jump(&end);
            generator.emit_label(&real_call);
            {
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
            }
            generator.emit_label(&end);
        }
        generator.emit_profile_type_divots(return_value.clone(), &divot_start, &divot_end);
        return_value
    }
}

// ------------------------------ HasOwnPropertyFunctionCallDotNode ----------------------------------

impl crate::parser::nodes::HasOwnPropertyFunctionCallDotNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp3Reg,
    ) -> Cpp3Reg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction, ThisResolutionType};
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;

        let return_value = Some(generator.final_destination(dst.as_ref(), None));
        let base = generator.emit_node_expression(None, &self.base_expr);

        if self.base_expr.base().is_optional_chain_base {
            generator.emit_optional_check(base.as_ref().unwrap());
        }

        generator.emit_expression_info(
            &self.throwable.subexpression_divot(),
            &self.throwable.subexpression_start(),
            &self.throwable.subexpression_end(),
        );

        let has_own_property = generator.property_names().has_own_property.clone();
        let temp = Some(generator.new_temporary());
        let function = generator.emit_get_by_id(temp, base.clone(), &has_own_property);
        if self.is_optional_call {
            generator.emit_optional_check(function.as_ref().unwrap());
        }

        // RELEASE_ASSERT(m_args->m_listNode && m_args->m_listNode->m_expr && !m_args->m_listNode->m_next);
        let argument = {
            let args = self.args.borrow();
            let list_node = args.list_node.as_ref().expect("RELEASE_ASSERT: lista de argumentos vazia");
            let list_node = list_node.borrow();
            assert!(list_node.next.is_none());
            list_node.expr.clone()
        };
        let argument_ident = match &argument {
            crate::parser::nodes::Expression::Resolve(resolve) => resolve.borrow().ident.clone(),
            _ => unreachable!("RELEASE_ASSERT(argument->isResolveNode())"),
        };
        let mut context: Option<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::ForInContext>>> = None;
        let argument_variable = generator.variable(&argument_ident, ThisResolutionType::Scoped);
        if argument_variable.is_local() {
            let property = argument_variable.local().unwrap();
            context = generator.find_for_in_context(&property);
        }

        let can_use_fast_has_own_property = |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator| -> bool {
            let context = match &context {
                Some(context) => context.borrow(),
                None => return false,
            };
            let base_variable = match context.base_variable() {
                Some(base_variable) => base_variable.clone(),
                None => return false,
            };
            match &self.base_expr {
                crate::parser::nodes::Expression::Resolve(resolve) => {
                    let ident = resolve.borrow().ident.clone();
                    generator.variable(&ident, ThisResolutionType::Scoped) == base_variable
                }
                crate::parser::nodes::Expression::This(_) => {
                    // After generator.ensureThis (which must be invoked in |base|'s materialization), we can ensure that |this| is in local this-register.
                    debug_assert!(base.is_some());
                    let this_private_name = generator.property_names().builtin_names().this_private_name().clone();
                    generator.variable(&this_private_name, ThisResolutionType::Local) == base_variable
                }
                _ => false,
            }
        };

        if can_use_fast_has_own_property(generator) {
            let context = context.clone().unwrap();
            // It is possible that base register is variable and each for-in body replaces JS object in the base register with a different one.
            // Even though, this is OK since HasOwnStructureProperty will reject the replaced JS object.
            let real_call = generator.new_label();
            let end = generator.new_label();

            let branch_insn_offset = generator.emit_wide_jump_if_not_function_has_own_property(function.as_ref().unwrap(), &real_call);
            let argument_register = generator.emit_node_expression(None, &argument);
            {
                let context = context.borrow();
                generator.emit_enumerator_has_own_property(
                    return_value.clone(),
                    base.as_ref().unwrap(),
                    context.mode().as_ref().unwrap(),
                    argument_register.as_ref().unwrap(),
                    context.property_offset().as_ref().unwrap(),
                    context.enumerator().as_ref().unwrap(),
                );
            }
            generator.emit_jump(&end);

            generator.emit_label(&real_call);
            {
                let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
                generator.move_register(call_arguments.this_register().as_ref(), base.as_ref().unwrap());
                generator.emit_call_in_tail_position(
                    return_value.clone(),
                    function.clone(),
                    ExpectedFunction::NoExpectedFunction,
                    &mut call_arguments,
                    &divot,
                    &divot_start,
                    &divot_end,
                    DebuggableCall::Yes,
                );
            }

            generator.emit_label(&end);

            generator.record_has_own_property_in_for_in_loop(&mut context.borrow_mut(), branch_insn_offset, &real_call);
        } else {
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
        }

        generator.emit_profile_type_divots(return_value.clone(), &divot_start, &divot_end);
        return_value
    }
}
