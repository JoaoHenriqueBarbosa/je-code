// NodesCodegen.cpp, linhas 1154 a 2227: `BaseDotNode::emitGetPropertyValue(dst, base)` até
// `FunctionCallBracketNode::emitBytecode` (incluído por include!, sem `use`).
//
// Convenções desta fatia: registrador é `Option<RegisterRef>` (o `RegisterID*` nulo do C++), o
// `RefPtr<RegisterID>` é o próprio `Option<RegisterRef>` (Rc), `.get()` vira `.clone()`.

type Cpp2Reg = Option<crate::bytecompiler::bytecode_generator::RegisterRef>;

/// Valor de um `StringNode` (`static_cast<StringNode*>(node->m_expr)->value()`).
fn cpp2_string_value(expr: &crate::parser::nodes::Expression) -> crate::runtime::identifier::Identifier {
    match expr {
        crate::parser::nodes::Expression::String(string_node) => string_node.borrow().value.clone(),
        _ => unreachable!("esperava um StringNode"),
    }
}

/// Valor de um `NumberNode` (`static_cast<NumberNode*>(...)->value()`, vale também para `IntegerNode`).
fn cpp2_number_value(expr: &crate::parser::nodes::Expression) -> f64 {
    match expr {
        crate::parser::nodes::Expression::Double(node) => node.borrow().base.value,
        crate::parser::nodes::Expression::Integer(node) => node.borrow().base.base.value,
        _ => unreachable!("esperava um NumberNode"),
    }
}

impl crate::parser::nodes::BaseDotNode {
    pub fn emit_get_property_value(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
        base: Cpp2Reg,
    ) -> Cpp2Reg {
        let mut this_value: Cpp2Reg = None;
        self.emit_get_property_value_with_this(generator, dst, base, &mut this_value)
    }

    pub fn emit_put_property(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        base: Cpp2Reg,
        value: Cpp2Reg,
    ) -> Cpp2Reg {
        let mut this_value: Cpp2Reg = None;
        self.emit_put_property_with_this(generator, base, value, &mut this_value)
    }

    pub fn emit_put_property_with_this(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        base: Cpp2Reg,
        value: Cpp2Reg,
        this_value: &mut Cpp2Reg,
    ) -> Cpp2Reg {
        if self.is_private_member() {
            let identifier_name = self.ident.clone();
            let private_traits = generator.get_private_traits(&identifier_name);
            if private_traits.is_setter() {
                let var = generator.variable(&identifier_name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                let scope = generator.emit_resolve_scope(None, &var);
                debug_assert!(scope.is_some()); // Private names are always captured.
                let temp = generator.new_temporary();
                let private_brand_symbol = generator.emit_get_private_brand(Some(temp), scope.clone(), private_traits.is_static());
                generator.emit_check_private_brand(base.clone(), private_brand_symbol.clone(), private_traits.is_static());

                let temp = generator.new_temporary();
                let getter_setter_obj = generator.emit_get_from_scope(
                    Some(temp),
                    scope.clone(),
                    &var,
                    crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
                );
                let temp = generator.new_temporary();
                let set_private_name = generator.property_names().builtin_names().set_dup_private_name();
                let setter_function = generator.emit_direct_get_by_id(Some(temp), getter_setter_obj, &set_private_name);
                let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 1);
                generator.move_register(args.this_register().as_ref(), base.as_ref().unwrap());
                generator.move_register(args.argument_register(0).as_ref(), value.as_ref().unwrap());
                let temp = generator.new_temporary();
                let position = self.base.base.position().clone();
                generator.emit_call_ignore_result(
                    Some(temp),
                    setter_function,
                    crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                    &mut args,
                    &position,
                    &position,
                    &position,
                    crate::bytecompiler::bytecode_generator::DebuggableCall::Yes,
                );

                return value;
            }

            if private_traits.is_getter() || private_traits.is_method() {
                let var = generator.variable(&identifier_name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                let scope = generator.emit_resolve_scope(None, &var);
                debug_assert!(scope.is_some()); // Private names are always captured.
                let temp = generator.new_temporary();
                let private_brand_symbol = generator.emit_get_private_brand(Some(temp), scope.clone(), private_traits.is_static());
                generator.emit_check_private_brand(base.clone(), private_brand_symbol.clone(), private_traits.is_static());

                generator.emit_throw_type_error("Trying to access an undefined private setter");
                return value;
            }

            debug_assert!(private_traits.is_field());
            let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            debug_assert!(var.local().is_none(), "Private Field names must be stored in captured variables");

            let scope = generator.emit_resolve_scope(None, &var);
            debug_assert!(scope.is_some()); // Private names are always captured.
            let private_name = Some(generator.new_temporary());
            generator.emit_get_from_scope(
                private_name.clone(),
                scope,
                &var,
                crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
            );
            return generator.emit_private_field_put(base, private_name, value);
        }

        if self.base_expr.is_super_node() {
            if this_value.is_none() {
                *this_value = Some(generator.ensure_this());
            }
            return generator.emit_put_by_id_with_this(base, this_value.clone(), &self.ident, value);
        }

        generator.emit_put_by_id(base, &self.ident, value)
    }
}

// ------------------------------ ArgumentListNode -----------------------------

impl crate::parser::nodes::ArgumentListNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        generator.emit_node_expression(dst, &self.expr)
    }
}

// ------------------------------ NewExprNode ----------------------------------

impl crate::parser::nodes::NewExprNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let expected_function = match &self.expr {
            crate::parser::nodes::Expression::Resolve(resolve) => {
                generator.expected_function_for_identifier(&resolve.borrow().ident)
            }
            _ => crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
        };

        let mut func: Cpp2Reg = None;
        if self.args.as_ref().is_some_and(|args| args.borrow().has_assignments) {
            func = Some(generator.new_temporary());
        }
        func = generator.emit_node_expression(func, &self.expr);
        let return_value = generator.final_destination(dst.as_ref(), func.as_ref());
        let mut call_arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, self.args.clone(), 0);
        generator.emit_construct(
            Some(return_value),
            func.clone(),
            func.clone(),
            expected_function,
            &mut call_arguments,
            &self.throwable.divot,
            &self.throwable.divot_start,
            &self.throwable.divot_end,
        )
    }
}

impl crate::bytecompiler::bytecode_generator::CallArguments {
    /// `CallArguments::CallArguments(BytecodeGenerator&, ArgumentsNode*, unsigned additionalArguments)`.
    pub fn new(
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        arguments_node: Option<crate::parser::nodes::NodeRef<crate::parser::nodes::ArgumentsNode>>,
        additional_arguments: u32,
    ) -> Self {
        let mut argument_count_including_this = 1 + additional_arguments as usize; // 'this' register.
        if let Some(arguments_node) = &arguments_node {
            let mut node = arguments_node.borrow().list_node.clone();
            while let Some(current) = node {
                argument_count_including_this += 1;
                node = current.borrow().next.clone();
            }
        }

        const STACK_ALIGNMENT_REGISTERS: usize = 2;
        let mut argv_size = argument_count_including_this;
        debug_assert!(argv_size >= 1);
        if (crate::interpreter::call_frame::HEADER_SIZE_IN_REGISTERS + argv_size as i32) % STACK_ALIGNMENT_REGISTERS as i32 != 0 {
            argv_size += 1;
        }
        argv_size += 1; // For stackOffset adjustment case.
        debug_assert!(argv_size >= 2);
        let mut allocated_registers: Vec<Option<crate::bytecompiler::bytecode_generator::RegisterRef>> = vec![None; argv_size];

        // Do not initialize index 0.
        let mut index = allocated_registers.len();
        generator.new_temporaries(allocated_registers.len() - 1, |slot| {
            index -= 1;
            allocated_registers[index] = Some(slot.clone());
        });

        // We initialize 0 based on offset. And adjust m_argv based on that.
        let second_index = allocated_registers[1].as_ref().unwrap().borrow().index() as isize;
        let argv = if (-second_index + crate::interpreter::call_frame::HEADER_SIZE_IN_REGISTERS as isize)
            % STACK_ALIGNMENT_REGISTERS as isize
            != 0
        {
            allocated_registers[0] = Some(generator.new_temporary());
            allocated_registers[..argument_count_including_this].to_vec()
        } else {
            allocated_registers[1..1 + argument_count_including_this].to_vec()
        };

        crate::bytecompiler::bytecode_generator::CallArguments { arguments_node, argv, allocated_registers }
    }
}

// ------------------------------ EvalFunctionCallNode ----------------------------------

impl crate::parser::nodes::EvalFunctionCallNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        // We need try to load 'this' before call eval in constructor, because 'this' can created by 'super' in some of the arrow function
        // var A = class A {
        //   constructor () { this.id = 'A'; }
        // }
        //
        // var B = class B extend A {
        //    constructor () {
        //       var arrow = () => super();
        //       arrow();
        //       eval("this.id = 'B'");
        //    }
        // }
        if generator.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends
            && generator.needs_to_update_arrow_function_context()
            && generator.is_this_used_in_inner_arrow_function()
        {
            generator.emit_load_this_from_arrow_function_lexical_environment();
        }

        let eval_name = generator.property_names().eval.clone();
        let var = generator.variable(&eval_name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let local = var.local();
        let func: Cpp2Reg;
        if let Some(local_register) = &local {
            generator.emit_tdz_check_if_necessary(&var, local.clone(), None);
            let temp_destination = generator.temp_destination(dst.as_ref());
            func = generator.move_register(Some(&temp_destination), local_register);
        } else {
            func = Some(generator.new_temporary());
        }
        let mut call_arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, Some(self.args.clone()), 0);

        if local.is_some() {
            generator.emit_load_js_value(call_arguments.this_register(), crate::runtime::js_value::js_undefined());
        } else {
            let divot_start = self.throwable.divot_start;
            let new_divot = divot_start + 4i32;
            generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
            let this_register = call_arguments.this_register();
            let scope = generator.emit_resolve_scope(this_register.clone(), &var);
            generator.move_register(this_register.as_ref(), scope.as_ref().unwrap());
            generator.emit_get_from_scope(
                func.clone(),
                this_register,
                &var,
                crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
            );
            generator.emit_tdz_check_if_necessary(&var, func.clone(), None);
        }

        let return_value = Some(generator.final_destination(dst.as_ref(), func.as_ref()));

        let first_is_spread = self
            .args
            .borrow()
            .list_node
            .as_ref()
            .is_some_and(|list| list.borrow().expr.is_spread_expression());
        if first_is_spread {
            let not_eval_function = generator.new_label();
            let done = generator.new_label();
            generator.emit_jump_if_not_eval_function(func.as_ref().unwrap(), &not_eval_function);

            {
                let first_expr = self.args.borrow().list_node.as_ref().unwrap().borrow().expr.clone();
                let spread = match &first_expr {
                    crate::parser::nodes::Expression::SpreadExpression(spread) => spread.clone(),
                    _ => unreachable!("esperava um SpreadExpressionNode"),
                };
                let spread = spread.borrow();
                let spread_register = generator.emit_node_expression(None, &spread.expression);
                generator.emit_expression_info(&spread.throwable.divot, &spread.throwable.divot_start, &spread.throwable.divot_end);

                let direct_eval_arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 1);
                generator.move_register(direct_eval_arguments.this_register().as_ref(), call_arguments.this_register().as_ref().unwrap());
                let zero = generator.emit_load_js_value(None, crate::runtime::js_value::js_number(0));
                generator.emit_get_by_val(direct_eval_arguments.argument_register(0), spread_register, zero);
                let mut direct_eval_arguments = direct_eval_arguments;
                generator.emit_call_direct_eval(
                    return_value.clone(),
                    func.clone(),
                    &mut direct_eval_arguments,
                    &self.throwable.divot,
                    &self.throwable.divot_start,
                    &self.throwable.divot_end,
                    crate::bytecompiler::bytecode_generator::DebuggableCall::No,
                );
                generator.emit_jump(&done);
            }

            generator.emit_label(&not_eval_function);
            generator.emit_call_in_tail_position(
                return_value.clone(),
                func.clone(),
                crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                &mut call_arguments,
                &self.throwable.divot,
                &self.throwable.divot_start,
                &self.throwable.divot_end,
                crate::bytecompiler::bytecode_generator::DebuggableCall::Yes,
            );
            generator.emit_label(&done);
        } else {
            generator.emit_call_direct_eval(
                return_value.clone(),
                func.clone(),
                &mut call_arguments,
                &self.throwable.divot,
                &self.throwable.divot_start,
                &self.throwable.divot_end,
                crate::bytecompiler::bytecode_generator::DebuggableCall::No,
            );
        }

        return_value
    }
}

// ------------------------------ FunctionCallValueNode ----------------------------------

impl crate::parser::nodes::FunctionCallValueNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction};
        let divot = self.throwable.divot;
        let divot_start = self.throwable.divot_start;
        let divot_end = self.throwable.divot_end;

        if self.expr.is_super_node() {
            let mut func = emit_get_super_function_for_construct(generator);
            let return_value = Some(generator.final_destination(dst.as_ref(), func.as_ref()));

            let is_default_derived_constructor_call = generator.is_builtin_default_class_constructor()
                && generator.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends;
            let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);

            debug_assert!(
                generator.is_constructor()
                    || generator.derived_context_type() == crate::bytecode::executable_info::DerivedContextType::DerivedConstructorContext
            );
            debug_assert!(
                generator.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends
                    || generator.derived_context_type() == crate::bytecode::executable_info::DerivedContextType::DerivedConstructorContext
            );
            let new_target = generator.new_target();
            let ret = generator.emit_super_construct(
                return_value.clone(),
                func.clone(),
                Some(new_target),
                ExpectedFunction::NoExpectedFunction,
                &mut call_arguments,
                &divot,
                &divot_start,
                &divot_end,
                is_default_derived_constructor_call,
            );

            let is_constructor_kind_derived = generator.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends;
            let do_we_use_arrow_function_in_constructor = is_constructor_kind_derived && generator.needs_to_update_arrow_function_context();

            if generator.is_derived_constructor_context()
                || (do_we_use_arrow_function_in_constructor && generator.is_super_call_used_in_inner_arrow_function())
            {
                generator.emit_load_this_from_arrow_function_lexical_environment();
            }

            let this_is_empty_label = generator.new_label();
            let temp = Some(generator.new_temporary());
            let this_register = generator.this_register();
            let is_empty = generator.emit_is_empty(temp, &this_register);
            generator.emit_jump_if_true_raw(is_empty.as_ref().unwrap(), &this_is_empty_label);
            generator.emit_throw_reference_error("'super()' can't be called more than once in a constructor.");
            generator.emit_label(&this_is_empty_label);

            generator.move_register(Some(&this_register), ret.as_ref().unwrap());

            if generator.is_derived_constructor_context() || do_we_use_arrow_function_in_constructor {
                generator.emit_put_this_to_arrow_function_context_scope();
            }

            // Initialize instance fields after super-call.
            if generator.private_brand_requirement() == crate::parser::parser_modes::PrivateBrandRequirement::Needed {
                generator.emit_install_private_brand(Some(this_register.clone()));
            }

            if generator.needs_class_field_initializer() == crate::bytecode::executable_info::NeedsClassFieldInitializer::Yes {
                debug_assert!(
                    generator.is_constructor()
                        || generator.derived_context_type()
                            == crate::bytecode::executable_info::DerivedContextType::DerivedConstructorContext
                );
                func = generator.emit_load_derived_constructor();
                generator.emit_instance_field_initialization_if_needed(
                    Some(this_register.clone()),
                    func,
                    &divot,
                    &divot_start,
                    &divot_end,
                );
            }
            return ret;
        }

        let mut func: Cpp2Reg = None;
        if self.args.borrow().has_assignments {
            func = Some(generator.new_temporary());
        }
        func = generator.emit_node_expression(func, &self.expr);
        let return_value = Some(generator.final_destination(dst.as_ref(), func.as_ref()));
        if self.is_optional_call {
            generator.emit_optional_check(func.as_ref().unwrap());
        }

        let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
        generator.emit_load_js_value(call_arguments.this_register(), crate::runtime::js_value::js_undefined());
        let ret = generator.emit_call_in_tail_position(
            return_value.clone(),
            func.clone(),
            ExpectedFunction::NoExpectedFunction,
            &mut call_arguments,
            &divot,
            &divot_start,
            &divot_end,
            DebuggableCall::Yes,
        );
        generator.emit_profile_type_divots(return_value.clone(), &divot_start, &divot_end);
        ret
    }
}

// ------------------------------ StaticBlockFunctionCallNode ----------------------------------

impl crate::parser::nodes::StaticBlockFunctionCallNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction};
        // There are two possible optimizations in this implementation.
        // https://bugs.webkit.org/show_bug.cgi?id=245925
        let home_object = emit_home_object_for_callee(generator);
        let function = generator.emit_node_expression(None, self.expr.as_ref().unwrap());
        emit_put_home_object(generator, function.as_ref().unwrap(), home_object.as_ref().unwrap());
        let return_value = Some(generator.final_destination(dst.as_ref(), function.as_ref()));

        let mut call_arguments = CallArguments::new(generator, None, 0);
        let this_register = generator.this_register();
        generator.move_register(call_arguments.this_register().as_ref(), &this_register);
        let result = generator.emit_call_in_tail_position(
            return_value.clone(),
            function.clone(),
            ExpectedFunction::NoExpectedFunction,
            &mut call_arguments,
            &self.throwable.divot,
            &self.throwable.divot_start,
            &self.throwable.divot_end,
            DebuggableCall::Yes,
        );

        generator.emit_profile_type_divots(return_value.clone(), &self.throwable.divot_start, &self.throwable.divot_end);
        result
    }
}

// ------------------------------ FunctionCallResolveNode ----------------------------------

impl crate::parser::nodes::FunctionCallResolveNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction, ResolveMode};
        if !crate::runtime::options::assert_enabled() {
            if self.ident == generator.vm().property_names.builtin_names().assert_private_name() {
                let undefined = generator.emit_load_js_value(None, crate::runtime::js_value::js_undefined());
                return generator.move_register(dst.as_ref(), undefined.as_ref().unwrap());
            }
        }

        let mut expected_function = generator.expected_function_for_identifier(&self.ident);

        let divot = self.throwable.divot;
        let divot_start = self.throwable.divot_start;
        let divot_end = self.throwable.divot_end;
        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let local = var.local();
        let func: Cpp2Reg;
        let new_divot = divot_start + self.ident.length();
        if let Some(local_register) = &local {
            generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
            generator.emit_tdz_check_if_necessary(&var, local.clone(), None);
            if self.args.borrow().has_assignments {
                let temp_destination = generator.temp_destination(dst.as_ref());
                func = generator.move_register(Some(&temp_destination), local_register);
            } else {
                func = local.clone();
            }
        } else {
            func = Some(generator.temp_destination(dst.as_ref()));
        }
        let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);

        if local.is_some() {
            generator.emit_load_js_value(call_arguments.this_register(), crate::runtime::js_value::js_undefined());
            // This passes NoExpectedFunction because we expect that if the function is in a
            // local variable, then it's not one of our built-in constructors.
            expected_function = ExpectedFunction::NoExpectedFunction;
        } else {
            generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
            let this_register = call_arguments.this_register();
            let scope = generator.emit_resolve_scope(this_register.clone(), &var);
            generator.move_register(this_register.as_ref(), scope.as_ref().unwrap());
            generator.emit_get_from_scope(func.clone(), this_register, &var, ResolveMode::ThrowIfNotFound);
            generator.emit_tdz_check_if_necessary(&var, func.clone(), None);
        }

        let return_value = Some(generator.final_destination(dst.as_ref(), func.as_ref()));
        if self.is_optional_call {
            generator.emit_optional_check(func.as_ref().unwrap());
        }

        let ret = generator.emit_call_in_tail_position(
            return_value.clone(),
            func.clone(),
            expected_function,
            &mut call_arguments,
            &divot,
            &divot_start,
            &divot_end,
            DebuggableCall::Yes,
        );
        generator.emit_profile_type_divots(return_value.clone(), &divot_start, &divot_end);
        ret
    }
}

// ------------------------------ BytecodeIntrinsicNode ----------------------------------

impl crate::parser::nodes::BytecodeIntrinsicNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        if self.entry.type_() == crate::bytecode::bytecode_intrinsic_registry::Type::Emitter {
            return self.emit_intrinsic_emitter(generator, dst, self.entry.emitter().unwrap());
        }
        if generator.is_ignored_dst(dst.as_ref()) {
            return None;
        }
        generator.move_link_time_constant(dst, self.entry.link_time_constant())
    }

    /// `(this->*m_entry.emitter())(generator, dst)`: o ponteiro para membro do C++ é o enum
    /// `BytecodeIntrinsicEmitter`; cada variante chama o método `emit_intrinsic_*` de mesmo nome.
    fn emit_intrinsic_emitter(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
        emitter: crate::bytecode::bytecode_intrinsics_table::BytecodeIntrinsicEmitter,
    ) -> Cpp2Reg {
        use crate::bytecode::bytecode_intrinsics_table::BytecodeIntrinsicEmitter as E;
        match emitter {
            E::Argument => self.emit_intrinsic_argument(generator, dst),
            E::ArgumentCount => self.emit_intrinsic_argument_count(generator, dst),
            E::ArrayPush => self.emit_intrinsic_array_push(generator, dst),
            E::GetByIdDirect => self.emit_intrinsic_get_by_id_direct(generator, dst),
            E::GetByIdDirectPrivate => self.emit_intrinsic_get_by_id_direct_private(generator, dst),
            E::GetByValWithThis => self.emit_intrinsic_get_by_val_with_this(generator, dst),
            E::GetPrototypeOf => self.emit_intrinsic_get_prototype_of(generator, dst),
            E::GetInternalField => self.emit_intrinsic_get_internal_field(generator, dst),
            E::GetGeneratorInternalField => self.emit_intrinsic_get_generator_internal_field(generator, dst),
            E::GetIteratorHelperInternalField => self.emit_intrinsic_get_iterator_helper_internal_field(generator, dst),
            E::GetAsyncDisposableStackInternalField => {
                self.emit_intrinsic_get_async_disposable_stack_internal_field(generator, dst)
            }
            E::GetArrayIteratorInternalField => self.emit_intrinsic_get_array_iterator_internal_field(generator, dst),
            E::GetProxyInternalField => self.emit_intrinsic_get_proxy_internal_field(generator, dst),
            E::GetWrapForValidIteratorInternalField => {
                self.emit_intrinsic_get_wrap_for_valid_iterator_internal_field(generator, dst)
            }
            E::GetDisposableStackInternalField => self.emit_intrinsic_get_disposable_stack_internal_field(generator, dst),
            E::IdWithProfile => self.emit_intrinsic_id_with_profile(generator, dst),
            E::IsAsyncDisposableStack => self.emit_intrinsic_is_async_disposable_stack(generator, dst),
            E::IsObject => self.emit_intrinsic_is_object(generator, dst),
            E::IsCallable => self.emit_intrinsic_is_callable(generator, dst),
            E::IsConstructor => self.emit_intrinsic_is_constructor(generator, dst),
            E::IsJSArray => self.emit_intrinsic_is_js_array(generator, dst),
            E::IsProxyObject => self.emit_intrinsic_is_proxy_object(generator, dst),
            E::IsDerivedArray => self.emit_intrinsic_is_derived_array(generator, dst),
            E::IsGenerator => self.emit_intrinsic_is_generator(generator, dst),
            E::IsIteratorHelper => self.emit_intrinsic_is_iterator_helper(generator, dst),
            E::IsPromise => self.emit_intrinsic_is_promise(generator, dst),
            E::IsRegExpObject => self.emit_intrinsic_is_reg_exp_object(generator, dst),
            E::IsMap => self.emit_intrinsic_is_map(generator, dst),
            E::IsSet => self.emit_intrinsic_is_set(generator, dst),
            E::IsShadowRealm => self.emit_intrinsic_is_shadow_realm(generator, dst),
            E::IsArrayIterator => self.emit_intrinsic_is_array_iterator(generator, dst),
            E::IsUndefinedOrNull => self.emit_intrinsic_is_undefined_or_null(generator, dst),
            E::IsWrapForValidIterator => self.emit_intrinsic_is_wrap_for_valid_iterator(generator, dst),
            E::IsDisposableStack => self.emit_intrinsic_is_disposable_stack(generator, dst),
            E::ThrowTypeError => self.emit_intrinsic_throw_type_error(generator, dst),
            E::ThrowRangeError => self.emit_intrinsic_throw_range_error(generator, dst),
            E::ThrowOutOfMemoryError => self.emit_intrinsic_throw_out_of_memory_error(generator, dst),
            E::PutByIdDirect => self.emit_intrinsic_put_by_id_direct(generator, dst),
            E::PutByIdDirectPrivate => self.emit_intrinsic_put_by_id_direct_private(generator, dst),
            E::PutByValDirect => self.emit_intrinsic_put_by_val_direct(generator, dst),
            E::PutByValWithThisSloppy => self.emit_intrinsic_put_by_val_with_this_sloppy(generator, dst),
            E::PutByValWithThisStrict => self.emit_intrinsic_put_by_val_with_this_strict(generator, dst),
            E::PutInternalField => self.emit_intrinsic_put_internal_field(generator, dst),
            E::PutGeneratorInternalField => self.emit_intrinsic_put_generator_internal_field(generator, dst),
            E::PutAsyncDisposableStackInternalField => {
                self.emit_intrinsic_put_async_disposable_stack_internal_field(generator, dst)
            }
            E::PutArrayIteratorInternalField => self.emit_intrinsic_put_array_iterator_internal_field(generator, dst),
            E::PutDisposableStackInternalField => self.emit_intrinsic_put_disposable_stack_internal_field(generator, dst),
            E::SuperSamplerBegin => self.emit_intrinsic_super_sampler_begin(generator, dst),
            E::SuperSamplerEnd => self.emit_intrinsic_super_sampler_end(generator, dst),
            E::ToNumber => self.emit_intrinsic_to_number(generator, dst),
            E::ToString => self.emit_intrinsic_to_string(generator, dst),
            E::ToPropertyKey => self.emit_intrinsic_to_property_key(generator, dst),
            E::ToObject => self.emit_intrinsic_to_object(generator, dst),
            E::ToThis => self.emit_intrinsic_to_this(generator, dst),
            E::MustValidateResultOfProxyGetAndSetTraps => {
                self.emit_intrinsic_must_validate_result_of_proxy_get_and_set_traps(generator, dst)
            }
            E::MustValidateResultOfProxyTrapsExceptGetAndSet => {
                self.emit_intrinsic_must_validate_result_of_proxy_traps_except_get_and_set(generator, dst)
            }
            E::NewArrayWithSize => self.emit_intrinsic_new_array_with_size(generator, dst),
            E::NewArrayWithSpecies => self.emit_intrinsic_new_array_with_species(generator, dst),
            E::NewPromise => self.emit_intrinsic_new_promise(generator, dst),
            E::IteratorGenericClose => self.emit_intrinsic_iterator_generic_close(generator, dst),
            E::IteratorGenericNext => self.emit_intrinsic_iterator_generic_next(generator, dst),
            E::IfAbruptCloseIterator => self.emit_intrinsic_if_abrupt_close_iterator(generator, dst),
            E::CreatePromise => self.emit_intrinsic_create_promise(generator, dst),
            // JSC_COMMON_BYTECODE_INTRINSIC_CONSTANTS_EACH_NAME: Undefined até OrderedHashTableSentinel.
            constant => self.emit_intrinsic_constant(generator, dst, constant),
        }
    }

    /// `JSC_DECLARE_BYTECODE_INTRINSIC_CONSTANT_GENERATORS(name)`: o corpo é o mesmo para as 44
    /// constantes, só o `name##Value(generator)` muda; ele vem do registro pela variante.
    fn emit_intrinsic_constant(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
        constant: crate::bytecode::bytecode_intrinsics_table::BytecodeIntrinsicEmitter,
    ) -> Cpp2Reg {
        debug_assert!(self.args.is_none());
        debug_assert!(self.type_ == crate::parser::nodes::BytecodeIntrinsicNodeType::Constant);
        if generator.is_ignored_dst(dst.as_ref()) {
            return None;
        }
        let value = generator.vm().bytecode_intrinsic_registry().constant_value(constant, generator);
        generator.emit_load_js_value(dst, value)
    }

    /// `m_args->m_listNode`.
    fn intrinsic_list_node(&self) -> Option<crate::parser::nodes::NodeRef<crate::parser::nodes::ArgumentListNode>> {
        self.args.as_ref().unwrap().borrow().list_node.clone()
    }

    pub fn emit_intrinsic_get_by_id_direct(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        debug_assert!(node.borrow().expr.is_string());
        let ident = cpp2_string_value(&node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());
        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_direct_get_by_id(final_destination, base, &ident)
    }

    pub fn emit_intrinsic_get_by_id_direct_private(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        debug_assert!(node.borrow().expr.is_string());
        let symbol = generator
            .vm()
            .property_names
            .builtin_names()
            .look_up_private_name_identifier(&cpp2_string_value(&node.borrow().expr));
        debug_assert!(symbol.is_some());
        debug_assert!(node.borrow().next.is_none());
        let identifier = generator.parser_arena().identifier_arena().borrow_mut().make_symbol_identifier(&symbol.unwrap());
        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_direct_get_by_id(final_destination, base, &identifier)
    }

    pub fn emit_intrinsic_get_by_val_with_this(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let this_value = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let property = generator.emit_node_for_property(&node.borrow().expr);

        debug_assert!(node.borrow().next.is_none());

        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_get_by_val_with_this(final_destination, base, this_value, property)
    }

    pub fn emit_intrinsic_put_by_val_with_this_sloppy(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        self.emit_intrinsic_put_by_val_with_this(generator, crate::runtime::ecma_mode::ECMAMode::sloppy());
        dst
    }

    pub fn emit_intrinsic_put_by_val_with_this_strict(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        self.emit_intrinsic_put_by_val_with_this(generator, crate::runtime::ecma_mode::ECMAMode::strict());
        dst
    }

    /// `static ALWAYS_INLINE void emitIntrinsicPutByValWithThis(generator, node, ecmaMode)`.
    fn emit_intrinsic_put_by_val_with_this(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        ecma_mode: crate::runtime::ecma_mode::ECMAMode,
    ) {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let this_value = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let property = generator.emit_node_for_property(&node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let value = generator.emit_node_for_property(&node.borrow().expr);

        debug_assert!(node.borrow().next.is_none());

        generator.emit_put_by_val_with_ecma_mode(base, this_value, property, value, ecma_mode);
    }

    pub fn emit_intrinsic_get_prototype_of(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let value = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());
        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_get_prototype_of(final_destination, value)
    }

    pub fn emit_intrinsic_get_internal_field(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        assert!(node.borrow().expr.is_number());
        let index = cpp2_number_value(&node.borrow().expr) as u32;
        debug_assert!(node.borrow().next.is_none());

        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_get_internal_field(final_destination, base, index)
    }

    pub fn emit_intrinsic_argument(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        debug_assert!(node.borrow().expr.is_number());
        let value = cpp2_number_value(&node.borrow().expr);
        let index = value as i32;
        debug_assert!(value == index as f64);
        debug_assert!(index >= 0);
        debug_assert!(node.borrow().next.is_none());

        // The body functions of generator and async have different mechanism for arguments.
        debug_assert!(generator.parse_mode() != crate::parser::parser_modes::SourceParseMode::GeneratorBodyMode);
        debug_assert!(!crate::parser::parser_modes::is_async_function_body_parse_mode(generator.parse_mode()));

        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_get_argument(final_destination, index)
    }

    pub fn emit_intrinsic_argument_count(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        debug_assert!(self.intrinsic_list_node().is_none());

        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_argument_count(final_destination)
    }

    pub fn emit_intrinsic_array_push(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let value = generator.emit_node_expression(None, &node.borrow().expr);

        debug_assert!(node.borrow().next.is_none());

        let temp = Some(generator.new_temporary());
        let length = generator.emit_get_length(temp, base.clone());
        let put = generator.emit_direct_put_by_val(base, length, value);
        generator.move_register(dst.as_ref(), put.as_ref().unwrap())
    }

    pub fn emit_intrinsic_put_by_id_direct(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        debug_assert!(node.borrow().expr.is_string());
        let ident = cpp2_string_value(&node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let value = generator.emit_node_expression(None, &node.borrow().expr);

        debug_assert!(node.borrow().next.is_none());

        let put = generator.emit_direct_put_by_id(base, &ident, value);
        generator.move_register(dst.as_ref(), put.as_ref().unwrap())
    }

    pub fn emit_intrinsic_put_by_id_direct_private(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        debug_assert!(node.borrow().expr.is_string());
        let symbol = generator
            .vm()
            .property_names
            .builtin_names()
            .look_up_private_name_identifier(&cpp2_string_value(&node.borrow().expr));
        debug_assert!(symbol.is_some());
        let node = node.borrow().next.clone().unwrap();
        let value = generator.emit_node_expression(None, &node.borrow().expr);

        debug_assert!(node.borrow().next.is_none());

        let identifier = generator.parser_arena().identifier_arena().borrow_mut().make_symbol_identifier(&symbol.unwrap());
        let put = generator.emit_direct_put_by_id(base, &identifier, value);
        generator.move_register(dst.as_ref(), put.as_ref().unwrap())
    }

    pub fn emit_intrinsic_put_by_val_direct(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let index = generator.emit_node_for_property(&node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let value = generator.emit_node_expression(None, &node.borrow().expr);

        debug_assert!(node.borrow().next.is_none());

        let put = generator.emit_direct_put_by_val(base, index, value);
        generator.move_register(dst.as_ref(), put.as_ref().unwrap())
    }

    pub fn emit_intrinsic_put_internal_field(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let base = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        assert!(node.borrow().expr.is_number());
        let index = cpp2_number_value(&node.borrow().expr) as u32;
        let node = node.borrow().next.clone().unwrap();
        let value = generator.emit_node_expression(None, &node.borrow().expr);

        debug_assert!(node.borrow().next.is_none());

        let put = generator.emit_put_internal_field(base, index, value);
        generator.move_register(dst.as_ref(), put.as_ref().unwrap())
    }

    pub fn emit_intrinsic_super_sampler_begin(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        debug_assert!(self.intrinsic_list_node().is_none());
        generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::js_undefined());
        crate::bytecode::bytecode_ops::OpSuperSamplerBegin::emit(generator);

        dst
    }

    pub fn emit_intrinsic_super_sampler_end(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        debug_assert!(self.intrinsic_list_node().is_none());
        crate::bytecode::bytecode_ops::OpSuperSamplerEnd::emit(generator);
        generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::js_undefined());

        dst
    }

    pub fn emit_intrinsic_throw_type_error(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        debug_assert!(node.borrow().next.is_none());
        if node.borrow().expr.is_string() {
            let ident = cpp2_string_value(&node.borrow().expr);
            generator.emit_throw_type_error_identifier(&ident);
        } else {
            let message = generator.emit_node_expression(None, &node.borrow().expr);
            generator.emit_throw_static_error_register(
                crate::runtime::error_type::ErrorTypeWithExtension::TypeError,
                message,
            );
        }
        dst
    }

    pub fn emit_intrinsic_throw_range_error(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        debug_assert!(node.borrow().next.is_none());
        if node.borrow().expr.is_string() {
            let ident = cpp2_string_value(&node.borrow().expr);
            generator.emit_throw_range_error(&ident);
        } else {
            let message = generator.emit_node_expression(None, &node.borrow().expr);
            generator.emit_throw_static_error_register(
                crate::runtime::error_type::ErrorTypeWithExtension::RangeError,
                message,
            );
        }

        dst
    }

    pub fn emit_intrinsic_throw_out_of_memory_error(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        debug_assert!(self.intrinsic_list_node().is_none());

        generator.emit_throw_out_of_memory_error();
        dst
    }

    pub fn emit_intrinsic_to_number(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let src = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let temp = Some(generator.temp_destination(dst.as_ref()));
        let converted = generator.emit_to_number(temp, src);
        generator.move_register(dst.as_ref(), converted.as_ref().unwrap())
    }

    pub fn emit_intrinsic_to_string(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let src = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let temp = Some(generator.temp_destination(dst.as_ref()));
        let converted = generator.emit_to_string(temp, src);
        generator.move_register(dst.as_ref(), converted.as_ref().unwrap())
    }

    pub fn emit_intrinsic_to_property_key(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let src = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let temp = Some(generator.temp_destination(dst.as_ref()));
        let converted = generator.emit_to_property_key(temp, src);
        generator.move_register(dst.as_ref(), converted.as_ref().unwrap())
    }

    pub fn emit_intrinsic_to_object(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let src = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone();

        let temp = Some(generator.temp_destination(dst.as_ref()));
        if let Some(node) = node {
            debug_assert!(node.borrow().expr.is_string());
            let message = cpp2_string_value(&node.borrow().expr);
            debug_assert!(node.borrow().next.is_none());
            let converted = generator.emit_to_object(temp, src, &message);
            return generator.move_register(dst.as_ref(), converted.as_ref().unwrap());
        }
        let empty_identifier = generator.vm().property_names.empty_identifier.clone();
        let converted = generator.emit_to_object(temp, src, &empty_identifier);
        generator.move_register(dst.as_ref(), converted.as_ref().unwrap())
    }

    pub fn emit_intrinsic_to_this(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let src = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let converted = generator.emit_to_this(src.as_ref().unwrap());
        generator.move_register(dst.as_ref(), converted.as_ref().unwrap())
    }

    pub fn emit_intrinsic_id_with_profile(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let mut node = self.intrinsic_list_node().unwrap();
        let id_value = Some(generator.new_temporary());
        generator.emit_node_expression(id_value.clone(), &node.borrow().expr);
        let mut speculation = crate::bytecode::speculated_type::SPEC_NONE;
        loop {
            let next = node.borrow().next.clone();
            let Some(next) = next else { break };
            node = next;
            debug_assert!(node.borrow().expr.is_string());
            let ident = cpp2_string_value(&node.borrow().expr);
            speculation |= crate::bytecode::speculated_type::speculation_from_string(&String::from_utf8_lossy(&ident.utf8()));
        }

        let profiled = generator.emit_id_with_profile(id_value, speculation);
        generator.move_register(dst.as_ref(), profiled.as_ref().unwrap())
    }

    pub fn emit_intrinsic_must_validate_result_of_proxy_get_and_set_traps(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let src = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let temp = Some(generator.temp_destination(dst.as_ref()));
        let checked = generator.emit_has_structure_with_flags(
            temp,
            src,
            crate::runtime::structure::Structure::HAS_NON_CONFIGURABLE_READ_ONLY_OR_GETTER_SETTER_PROPERTIES_BITS,
        );
        generator.move_register(dst.as_ref(), checked.as_ref().unwrap())
    }

    pub fn emit_intrinsic_must_validate_result_of_proxy_traps_except_get_and_set(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let src = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let temp = Some(generator.temp_destination(dst.as_ref()));
        let checked = generator.emit_has_structure_with_flags(
            temp,
            src,
            crate::runtime::structure::Structure::HAS_NON_CONFIGURABLE_PROPERTIES_BITS
                | crate::runtime::structure::Structure::DID_PREVENT_EXTENSIONS_BITS,
        );
        generator.move_register(dst.as_ref(), checked.as_ref().unwrap())
    }

    pub fn emit_intrinsic_new_array_with_size(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let size = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_new_array_with_size(final_destination.clone(), size);
        final_destination
    }

    pub fn emit_intrinsic_new_array_with_species(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let size = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let array = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_new_array_with_species(final_destination.clone(), size, array);
        final_destination
    }

    pub fn emit_intrinsic_create_promise(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let new_target = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_create_promise(final_destination, new_target)
    }

    pub fn emit_intrinsic_new_promise(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        debug_assert!(self.intrinsic_list_node().is_none());
        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_new_promise(final_destination.clone());
        final_destination
    }

    pub fn emit_intrinsic_iterator_generic_close(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let iterator = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        generator.emit_iterator_generic_close(
            iterator.as_ref().unwrap(),
            &self.throwable,
            crate::bytecompiler::bytecode_generator::EmitAwait::No,
        );
        dst
    }

    pub fn emit_intrinsic_iterator_generic_next(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let next_method = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        let iterator = generator.emit_node_expression(None, &node.borrow().expr);
        debug_assert!(node.borrow().next.is_none());

        let final_destination = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_iterator_generic_next(
            final_destination,
            next_method.as_ref().unwrap(),
            iterator.as_ref().unwrap(),
            &self.throwable,
            crate::bytecompiler::bytecode_generator::EmitAwait::No,
        )
    }

    pub fn emit_intrinsic_if_abrupt_close_iterator(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        let node = self.intrinsic_list_node().unwrap();
        let iterator = generator.emit_node_expression(None, &node.borrow().expr);
        let node = node.borrow().next.clone().unwrap();
        debug_assert!(node.borrow().next.is_none());

        let end = generator.new_label();
        let mut emit_second_argument_node = |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator| {
            generator.emit_node_expression(None, &node.borrow().expr);
            generator.emit_jump(&end);
        };

        let mut emit_iterator_close = |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator| {
            generator.emit_iterator_generic_close(
                iterator.as_ref().unwrap(),
                &self.throwable,
                crate::bytecompiler::bytecode_generator::EmitAwait::No,
            );
        };

        generator.emit_try_with_finally_that_does_not_shadow_exception(&mut emit_second_argument_node, &mut emit_iterator_close);
        generator.emit_label(&end);

        dst
    }
}

/// `static JSGenerator::Field generatorInternalFieldIndex(BytecodeIntrinsicNode*)` e as cinco irmãs
/// (iteratorHelper, arrayIterator, proxy, wrapForValidIterator, disposableStack,
/// asyncDisposableStack): o emitter do nó dá a variante do campo; devolve o índice numérico.
macro_rules! intrinsic_internal_field_index {
    ($name:ident, $field:ty, { $($emitter:ident => $variant:ident),+ $(,)? }) => {
        fn $name(node: &crate::parser::nodes::BytecodeIntrinsicNode) -> u32 {
            debug_assert!(node.entry.type_() == crate::bytecode::bytecode_intrinsic_registry::Type::Emitter);
            match node.entry.emitter() {
                $(Some(crate::bytecode::bytecode_intrinsics_table::BytecodeIntrinsicEmitter::$emitter) => <$field>::$variant as u32,)+
                _ => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
            }
        }
    };
}

intrinsic_internal_field_index!(generator_internal_field_index, crate::runtime::js_generator::Field, {
    GeneratorFieldState => State,
    GeneratorFieldNext => Next,
    GeneratorFieldThis => This,
    GeneratorFieldFrame => Frame,
});
intrinsic_internal_field_index!(iterator_helper_internal_field_index, crate::runtime::js_iterator_helper::Field, {
    IteratorHelperFieldGenerator => Generator,
    IteratorHelperFieldUnderlyingIterator => UnderlyingIterator,
});
intrinsic_internal_field_index!(array_iterator_internal_field_index, crate::runtime::js_array_iterator::Field, {
    ArrayIteratorFieldIndex => Index,
    ArrayIteratorFieldIteratedObject => IteratedObject,
    ArrayIteratorFieldKind => Kind,
});
intrinsic_internal_field_index!(proxy_internal_field_index, crate::runtime::proxy_object::Field, {
    ProxyFieldTarget => Target,
    ProxyFieldHandler => Handler,
});
intrinsic_internal_field_index!(wrap_for_valid_iterator_internal_field_index, crate::runtime::js_wrap_for_valid_iterator::Field, {
    WrapForValidIteratorFieldIteratedIterator => IteratedIterator,
    WrapForValidIteratorFieldIteratedNextMethod => IteratedNextMethod,
});
intrinsic_internal_field_index!(disposable_stack_internal_field_index, crate::runtime::js_disposable_stack::Field, {
    DisposableStackFieldState => State,
    DisposableStackFieldCapability => Capability,
});
intrinsic_internal_field_index!(async_disposable_stack_internal_field_index, crate::runtime::js_async_disposable_stack::Field, {
    AsyncDisposableStackFieldState => State,
    AsyncDisposableStackFieldCapability => Capability,
});

/// `emit_intrinsic_get<X>InternalField`: base, o nó do campo (outro intrinsic) e o `emitGetInternalField`.
macro_rules! intrinsic_get_internal_field {
    ($method:ident, $index_fn:ident, $count:path) => {
        impl crate::parser::nodes::BytecodeIntrinsicNode {
            pub fn $method(
                &self,
                generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
                dst: Cpp2Reg,
            ) -> Cpp2Reg {
                let node = self.intrinsic_list_node().unwrap();
                let base = generator.emit_node_expression(None, &node.borrow().expr);
                let node = node.borrow().next.clone().unwrap();
                let index = match &node.borrow().expr {
                    crate::parser::nodes::Expression::BytecodeIntrinsic(field) => $index_fn(&field.borrow()),
                    _ => panic!("RELEASE_ASSERT(node->m_expr->isBytecodeIntrinsicNode())"),
                };
                debug_assert!(index < $count);
                debug_assert!(node.borrow().next.is_none());

                let final_destination = Some(generator.final_destination(dst.as_ref(), None));
                generator.emit_get_internal_field(final_destination, base, index)
            }
        }
    };
}

intrinsic_get_internal_field!(
    emit_intrinsic_get_generator_internal_field,
    generator_internal_field_index,
    crate::runtime::js_generator::NUMBER_OF_INTERNAL_FIELDS
);
intrinsic_get_internal_field!(
    emit_intrinsic_get_iterator_helper_internal_field,
    iterator_helper_internal_field_index,
    crate::runtime::js_iterator_helper::NUMBER_OF_INTERNAL_FIELDS
);
intrinsic_get_internal_field!(
    emit_intrinsic_get_proxy_internal_field,
    proxy_internal_field_index,
    crate::runtime::proxy_object::NUMBER_OF_INTERNAL_FIELDS
);
intrinsic_get_internal_field!(
    emit_intrinsic_get_array_iterator_internal_field,
    array_iterator_internal_field_index,
    crate::runtime::js_array_iterator::NUMBER_OF_INTERNAL_FIELDS
);
intrinsic_get_internal_field!(
    emit_intrinsic_get_wrap_for_valid_iterator_internal_field,
    wrap_for_valid_iterator_internal_field_index,
    crate::runtime::js_wrap_for_valid_iterator::NUMBER_OF_INTERNAL_FIELDS
);
intrinsic_get_internal_field!(
    emit_intrinsic_get_disposable_stack_internal_field,
    disposable_stack_internal_field_index,
    crate::runtime::js_disposable_stack::NUMBER_OF_INTERNAL_FIELDS
);
intrinsic_get_internal_field!(
    emit_intrinsic_get_async_disposable_stack_internal_field,
    async_disposable_stack_internal_field_index,
    crate::runtime::js_async_disposable_stack::NUMBER_OF_INTERNAL_FIELDS
);

/// `emit_intrinsic_put<X>InternalField`: base, campo, valor e `emitPutInternalField`.
macro_rules! intrinsic_put_internal_field {
    ($method:ident, $index_fn:ident, $count:path) => {
        impl crate::parser::nodes::BytecodeIntrinsicNode {
            pub fn $method(
                &self,
                generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
                dst: Cpp2Reg,
            ) -> Cpp2Reg {
                let node = self.intrinsic_list_node().unwrap();
                let base = generator.emit_node_expression(None, &node.borrow().expr);
                let node = node.borrow().next.clone().unwrap();
                let index = match &node.borrow().expr {
                    crate::parser::nodes::Expression::BytecodeIntrinsic(field) => $index_fn(&field.borrow()),
                    _ => panic!("RELEASE_ASSERT(node->m_expr->isBytecodeIntrinsicNode())"),
                };
                debug_assert!(index < $count);
                let node = node.borrow().next.clone().unwrap();
                let value = generator.emit_node_expression(None, &node.borrow().expr);

                debug_assert!(node.borrow().next.is_none());

                let put = generator.emit_put_internal_field(base, index, value);
                generator.move_register(dst.as_ref(), put.as_ref().unwrap())
            }
        }
    };
}

intrinsic_put_internal_field!(
    emit_intrinsic_put_generator_internal_field,
    generator_internal_field_index,
    crate::runtime::js_generator::NUMBER_OF_INTERNAL_FIELDS
);
intrinsic_put_internal_field!(
    emit_intrinsic_put_array_iterator_internal_field,
    array_iterator_internal_field_index,
    crate::runtime::js_array_iterator::NUMBER_OF_INTERNAL_FIELDS
);
intrinsic_put_internal_field!(
    emit_intrinsic_put_disposable_stack_internal_field,
    disposable_stack_internal_field_index,
    crate::runtime::js_disposable_stack::NUMBER_OF_INTERNAL_FIELDS
);
intrinsic_put_internal_field!(
    emit_intrinsic_put_async_disposable_stack_internal_field,
    async_disposable_stack_internal_field_index,
    crate::runtime::js_async_disposable_stack::NUMBER_OF_INTERNAL_FIELDS
);

/// `CREATE_INTRINSIC_FOR_BRAND_CHECK(lowerName, upperName)`.
macro_rules! intrinsic_for_brand_check {
    ($($method:ident => $emit:ident),+ $(,)?) => {
        impl crate::parser::nodes::BytecodeIntrinsicNode {
            $(
                pub fn $method(
                    &self,
                    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
                    dst: Cpp2Reg,
                ) -> Cpp2Reg {
                    let node = self.intrinsic_list_node().unwrap();
                    let src = generator.emit_node_expression(None, &node.borrow().expr);
                    debug_assert!(node.borrow().next.is_none());
                    let temp = Some(generator.temp_destination(dst.as_ref()));
                    let checked = generator.$emit(temp, src.as_ref().unwrap());
                    generator.move_register(dst.as_ref(), checked.as_ref().unwrap())
                }
            )+
        }
    };
}

intrinsic_for_brand_check! {
    emit_intrinsic_is_object => emit_is_object,
    emit_intrinsic_is_callable => emit_is_callable,
    emit_intrinsic_is_constructor => emit_is_constructor,
    emit_intrinsic_is_js_array => emit_is_js_array,
    emit_intrinsic_is_proxy_object => emit_is_proxy_object,
    emit_intrinsic_is_derived_array => emit_is_derived_array,
    emit_intrinsic_is_generator => emit_is_generator,
    emit_intrinsic_is_iterator_helper => emit_is_iterator_helper,
    emit_intrinsic_is_promise => emit_is_promise,
    emit_intrinsic_is_reg_exp_object => emit_is_reg_exp_object,
    emit_intrinsic_is_map => emit_is_map,
    emit_intrinsic_is_set => emit_is_set,
    emit_intrinsic_is_shadow_realm => emit_is_shadow_realm,
    emit_intrinsic_is_array_iterator => emit_is_array_iterator,
    emit_intrinsic_is_undefined_or_null => emit_is_undefined_or_null,
    emit_intrinsic_is_wrap_for_valid_iterator => emit_is_wrap_for_valid_iterator,
    emit_intrinsic_is_disposable_stack => emit_is_disposable_stack,
    emit_intrinsic_is_async_disposable_stack => emit_is_async_disposable_stack,
}

// ------------------------------ FunctionCallBracketNode ----------------------------------

impl crate::parser::nodes::FunctionCallBracketNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp2Reg,
    ) -> Cpp2Reg {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, ExpectedFunction};
        let divot = self.throwable.base.divot;
        let divot_start = self.throwable.base.divot_start;
        let divot_end = self.throwable.base.divot_end;

        let function = Some(generator.temp_destination(dst.as_ref()));
        let return_value = Some(generator.final_destination(dst.as_ref(), function.as_ref()));
        let base_is_super = self.base_expr.is_super_node();
        let subscript_is_non_index_string = nodes_codegen_is_non_index_string_element(&self.subscript);

        let base: Cpp2Reg;
        if base_is_super {
            base = emit_super_base_for_callee(generator);
        } else {
            if subscript_is_non_index_string {
                base = generator.emit_node_expression(None, &self.base_expr);
            } else {
                let subscript_is_pure = self.subscript.is_pure(generator);
                base = generator.emit_node_for_left_hand_side(
                    &self.base_expr,
                    self.subscript_has_assignments,
                    subscript_is_pure,
                );
            }

            if self.base_expr.base().is_optional_chain_base {
                generator.emit_optional_check(base.as_ref().unwrap());
            }
        }

        let mut this_register: Cpp2Reg = None;
        if base_is_super {
            // Note that we only need to do this once because we either have a non-TDZ this or we throw. Once we have a non-TDZ this, we can't change its value back to TDZ.
            this_register = Some(generator.ensure_this());
        }
        if subscript_is_non_index_string {
            generator.emit_expression_info(&self.throwable.subexpression_divot(), &self.throwable.subexpression_start(), &self.throwable.subexpression_end());
            let value = cpp2_string_value(&self.subscript);
            if base_is_super {
                generator.emit_get_by_id_with_this(function.clone(), base.clone(), this_register.clone(), &value);
            } else {
                generator.emit_get_by_id(function.clone(), base.clone(), &value);
            }
        } else {
            let property = generator.emit_node_for_property(&self.subscript);
            generator.emit_expression_info(&self.throwable.subexpression_divot(), &self.throwable.subexpression_start(), &self.throwable.subexpression_end());
            if base_is_super {
                generator.emit_get_by_val_with_this(function.clone(), base.clone(), this_register.clone(), property);
            } else {
                generator.emit_get_by_val(function.clone(), base.clone(), property);
            }
        }
        if self.is_optional_call {
            generator.emit_optional_check(function.as_ref().unwrap());
        }

        let mut call_arguments = CallArguments::new(generator, Some(self.args.clone()), 0);
        if base_is_super {
            let this_register_of_generator = generator.this_register();
            generator.emit_tdz_check(&this_register_of_generator);
            generator.move_register(call_arguments.this_register().as_ref(), this_register.as_ref().unwrap());
        } else {
            generator.move_register(call_arguments.this_register().as_ref(), base.as_ref().unwrap());
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
        generator.emit_profile_type_divots(return_value.clone(), &divot_start, &divot_end);
        ret
    }
}
