// Tradução de bytecompiler/NodesCodegen.cpp, linhas 1 a 540 (até o fim de ArrayNode::emitBytecode).
// Incluído por include!, sem `use` no topo: caminhos completos em tudo.
//
// Convenção: `dst: Option<RegisterRef>`; o `generator.ignoredResult()` do C++ é comparado por
// `Rc::ptr_eq` com `generator.ignored_result()`. O despacho por família (`Expression::emit_bytecode`)
// fica em outro arquivo; aqui só os corpos por struct concreta.

/// `RegisterID callee; callee.setIndex(CallFrameSlot::callee);`
fn nodes_codegen_callee_register() -> crate::bytecompiler::bytecode_generator::RegisterRef {
    let mut callee = crate::bytecompiler::register_id::RegisterID::default();
    callee.set_index(crate::bytecode::virtual_register::VirtualRegister::new(
        crate::interpreter::call_frame::CallFrameSlot::CALLEE,
    ));
    crate::bytecompiler::bytecode_generator::RegisterRef::new(&std::rc::Rc::new(std::cell::RefCell::new(callee)))
}

/// `ExpressionNode::emitBytecodeInConditionContext` (o padrão da família): `this` vem como
/// `Expression` porque `emitNode(this)` precisa do enum.
pub fn expression_node_emit_bytecode_in_condition_context(
    this: &crate::parser::nodes::Expression,
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    true_target: &crate::bytecompiler::label::LabelRef,
    false_target: &crate::bytecompiler::label::LabelRef,
    fall_through_mode: crate::parser::nodes::FallThroughMode,
) {
    let result = generator.emit_node_expression_no_dst(this);
    let result = result.expect("emitNode devolveu nulo sem dst");
    // `RegisterID* result = generator.emitNode(this)`: ponteiro cru, a fusão do salto pode desfazer a instrução.
    generator.with_raw_register(Some(result.get()), |generator| {
        if fall_through_mode == crate::parser::nodes::FallThroughMode::FallThroughMeansTrue {
            generator.emit_jump_if_false(&result, false_target);
        } else {
            generator.emit_jump_if_true(&result, true_target);
        }
    });
}

// ------------------------------ ThrowableExpressionData --------------------------------

impl crate::parser::nodes::ThrowableExpressionData {
    pub fn emit_throw_reference_error(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        message: &str,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        generator.emit_expression_info(&self.divot, &self.divot_start, &self.divot_end);
        generator.emit_throw_reference_error(message);
        if dst.is_some() {
            return dst;
        }
        Some(generator.new_temporary())
    }
}

// ------------------------------ ConstantNode ----------------------------------

/// `ConstantNode::emitBytecodeInConditionContext`. `this` é o nó como `Expression` (para o caso
/// indeterminado), `constant` é o resultado de `jsValue(generator)` (`None` quando o C++ devolve `JSValue()`).
pub fn constant_node_emit_bytecode_in_condition_context(
    this: &crate::parser::nodes::Expression,
    constant: Option<crate::runtime::js_value::JSValue>,
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    true_target: &crate::bytecompiler::label::LabelRef,
    false_target: &crate::bytecompiler::label::LabelRef,
    fall_through_mode: crate::parser::nodes::FallThroughMode,
) {
    let mut value = crate::parser::source_tainted_origin::TriState::Indeterminate;
    if let Some(constant) = constant {
        value = constant.pure_to_boolean();
    }

    if this.base().needs_debug_hook() && value != crate::parser::source_tainted_origin::TriState::Indeterminate {
        generator.emit_debug_hook_expression_data(this, None);
    }

    if value == crate::parser::source_tainted_origin::TriState::Indeterminate {
        expression_node_emit_bytecode_in_condition_context(this, generator, true_target, false_target, fall_through_mode);
    } else if value == crate::parser::source_tainted_origin::TriState::True
        && fall_through_mode == crate::parser::nodes::FallThroughMode::FallThroughMeansFalse
    {
        generator.emit_jump(true_target);
    } else if value == crate::parser::source_tainted_origin::TriState::False
        && fall_through_mode == crate::parser::nodes::FallThroughMode::FallThroughMeansTrue
    {
        generator.emit_jump(false_target);
    }

    // All other cases are unconditional fall-throughs, like "if (true)".
}

/// `ConstantNode::emitBytecode`; `constant` é `jsValue(generator)`.
pub fn constant_node_emit_bytecode(
    constant: Option<crate::runtime::js_value::JSValue>,
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
    if generator.is_ignored_dst(dst.as_ref()) {
        return None;
    }
    match constant {
        Some(constant) => generator.emit_load_js_value(dst, constant),
        // This can happen if we try to parse a string or BigInt so enormous that we OOM.
        None => generator.emit_throw_expression_too_deep_exception(),
    }
}

impl crate::parser::nodes::StringNode {
    pub fn js_value(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Option<crate::runtime::js_value::JSValue> {
        let string = generator.add_string_constant(&self.value);
        Some(crate::runtime::js_value::JSValue::from_cell(string.cell_id()))
    }
}

impl crate::parser::nodes::BigIntNode {
    pub fn js_value(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Option<crate::runtime::js_value::JSValue> {
        Some(generator.add_big_int_constant(&self.value, self.radix, self.sign))
    }
}

// ------------------------------ NumberNode ----------------------------------

impl crate::parser::nodes::NumberNode {
    pub fn js_value(
        &self,
        _generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Option<crate::runtime::js_value::JSValue> {
        Some(crate::runtime::js_value::js_number(self.value))
    }

    /// `is_integer_node` é o `isIntegerNode()` virtual (verdadeiro só para `IntegerNode`).
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        is_integer_node: bool,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if generator.is_ignored_dst(dst.as_ref()) {
            return None;
        }
        let constant = self.js_value(generator).expect("NumberNode::jsValue sempre tem valor");
        generator.emit_load_js_value_with_representation(
            dst,
            constant,
            if is_integer_node {
                crate::parser::parser_tokens::SourceCodeRepresentation::Integer
            } else {
                crate::parser::parser_tokens::SourceCodeRepresentation::Double
            },
        )
    }
}

// ------------------------------ RegExpNode -----------------------------------

impl crate::parser::nodes::RegExpNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if generator.is_ignored_dst(dst.as_ref()) {
            return None;
        }

        let flags = crate::yarr::yarr_flags::parse_flags(self.flags.string().span8());
        let flags = flags.expect("flags do literal de regexp já validadas pelo parser");
        let reg_exp = crate::runtime::reg_exp::RegExp::create(generator.vm(), self.pattern.string().string(), flags);
        if reg_exp.is_valid() {
            let final_dst = generator.final_destination(dst.as_ref(), None);
            return generator.emit_new_reg_exp(Some(final_dst), reg_exp);
        }

        let message = generator
            .parser_arena()
            .identifier_arena()
            .borrow_mut()
            .make_identifier(generator.vm(), reg_exp.error_message());
        generator.emit_throw_static_error(
            crate::runtime::error_type::ErrorTypeWithExtension::SyntaxError,
            &message,
        );
        let final_dst = generator.final_destination(dst.as_ref(), None);
        generator.emit_load_js_value(Some(final_dst), crate::runtime::js_value::js_undefined())
    }
}

// ------------------------------ ThisNode -------------------------------------

impl crate::parser::nodes::ThisNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        generator.ensure_this();
        if generator.is_ignored_dst(dst.as_ref()) {
            return None;
        }

        let this_register = generator.this_register();
        let result = generator.move_register(dst.as_ref(), &this_register);
        let this_length = "this".len() as u32;
        let position = *self.position();
        generator.emit_profile_type_divots(Some(this_register), &position, &(position + this_length));
        result
    }
}

// ------------------------------ SuperNode -------------------------------------

fn emit_home_object_for_callee(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
    if (generator.is_derived_class_context() || generator.is_derived_constructor_context())
        && generator.parse_mode() != crate::parser::parser_modes::SourceParseMode::ClassFieldInitializerMode
    {
        let derived_constructor = generator.emit_load_derived_constructor_from_arrow_function_lexical_environment();
        let dst = generator.new_temporary();
        let home_object_name = generator.property_names().builtin_names().home_object_private_name().clone();
        return generator.emit_get_by_id(Some(dst), derived_constructor, &home_object_name);
    }

    let callee = nodes_codegen_callee_register();
    let dst = generator.new_temporary();
    let home_object_name = generator.property_names().builtin_names().home_object_private_name().clone();
    generator.emit_get_by_id(Some(dst), Some(callee), &home_object_name)
}

fn emit_super_base_for_callee(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
    let home_object = emit_home_object_for_callee(generator);
    let dst = generator.new_temporary();
    generator.emit_get_prototype_of(Some(dst), home_object)
}

fn emit_get_super_function_for_construct(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
    if generator.is_derived_constructor_context() {
        let derived_constructor = generator.emit_load_derived_constructor_from_arrow_function_lexical_environment();
        let dst = generator.new_temporary();
        return generator.emit_get_prototype_of(Some(dst), derived_constructor);
    }

    let callee = nodes_codegen_callee_register();
    let dst = generator.new_temporary();
    generator.emit_get_prototype_of(Some(dst), Some(callee))
}

impl crate::parser::nodes::SuperNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let result = emit_super_base_for_callee(generator).expect("base de super sempre tem registro");
        let final_dst = generator.final_destination(dst.as_ref(), None);
        generator.move_register(Some(&final_dst), &result)
    }
}

// ------------------------------ ImportNode -------------------------------------

impl crate::parser::nodes::ImportNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let import_module =
            generator.move_link_time_constant(None, crate::bytecode::link_time_constant::LinkTimeConstant::ImportModule);
        let argument_count = if self.deferred {
            3
        } else if self.option.is_some() {
            2
        } else {
            1
        };
        let mut arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, argument_count);
        generator.emit_load_js_value(arguments.this_register(), crate::runtime::js_value::js_undefined());
        generator.emit_node_expression(arguments.argument_register(0), &self.expr);
        if let Some(option) = &self.option {
            generator.emit_node_expression(arguments.argument_register(1), option);
        } else if self.deferred {
            generator.emit_load_js_value(arguments.argument_register(1), crate::runtime::js_value::js_undefined());
        }
        if self.deferred {
            generator.emit_load_js_value(arguments.argument_register(2), crate::runtime::js_value::js_boolean(true));
        }
        let import_module = import_module.expect("moveLinkTimeConstant devolve registro");
        let final_dst = generator.final_destination(dst.as_ref(), Some(&import_module));
        generator.emit_call(
            Some(final_dst),
            Some(import_module.clone()),
            crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
            &mut arguments,
            &self.throwable.divot,
            &self.throwable.divot_start,
            &self.throwable.divot_end,
            crate::bytecompiler::bytecode_generator::DebuggableCall::No,
        )
    }
}

// ------------------------------ NewTargetNode ----------------------------------

impl crate::parser::nodes::NewTargetNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if generator.is_ignored_dst(dst.as_ref()) {
            return None;
        }

        let new_target = generator.new_target();
        generator.move_register(dst.as_ref(), &new_target)
    }
}

// ------------------------------ ImportMetaNode ---------------------------------

impl crate::parser::nodes::ImportMetaNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        generator.emit_node_expression(dst, &self.expr)
    }
}

// ------------------------------ ResolveNode ----------------------------------

impl crate::parser::nodes::ResolveNode {
    pub fn is_pure(&self, generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator) -> bool {
        generator
            .variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local)
            .offset()
            .is_stack()
    }

    pub fn get_from_scope_can_throw(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> bool {
        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        if var.offset().is_stack() || var.offset().is_scope() {
            return !generator.needs_tdz_check(&var);
        }

        true
    }

    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let ident_length = self.ident.length() as u32;
        let divot = self.start + ident_length;
        if let Some(local) = var.local() {
            generator.emit_expression_info(&divot, &self.start, &divot);
            generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);
            if generator.is_ignored_dst(dst.as_ref()) {
                return None;
            }

            let position = *self.position();
            generator.emit_profile_type_variable(Some(local.clone()), &var, &position, &(position + ident_length));
            return generator.move_register(dst.as_ref(), &local);
        }

        generator.emit_expression_info(&divot, &self.start, &divot);
        let scope = generator.emit_resolve_scope(dst.clone(), &var);
        let final_dest = generator.final_destination(dst.as_ref(), None);
        if !generator.needs_tdz_check(&var) {
            generator.emit_get_from_scope(
                Some(final_dest.clone()),
                scope,
                &var,
                crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
            );
        } else {
            // O JSC do bun carrega o get_from_scope direto em finalDest e checa nele (sem temporário e sem mov).
            generator.emit_get_from_scope(
                Some(final_dest.clone()),
                scope,
                &var,
                crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
            );
            // `emitTDZCheck(finalDest, m_ident)`: o `Variable` implícito de `Identifier` leva o nome.
            let named = crate::bytecompiler::bytecode_generator::Variable::from_ident(&self.ident);
            generator.emit_tdz_check_variable(&final_dest, &named);
        }
        let position = *self.position();
        generator.emit_profile_type_variable(Some(final_dest.clone()), &var, &position, &(position + ident_length));
        Some(final_dest)
    }
}

// ------------------------------ TemplateStringNode -----------------------------------

impl crate::parser::nodes::TemplateStringNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if generator.is_ignored_dst(dst.as_ref()) {
            return None;
        }
        let cooked = self.cooked.as_ref().expect("TemplateStringNode::emitBytecode exige cooked");
        let constant = generator.add_string_constant(cooked);
        generator.emit_load_js_value(dst, crate::runtime::js_value::JSValue::from_cell(constant.cell_id()))
    }
}

// ------------------------------ TemplateLiteralNode -----------------------------------

impl crate::parser::nodes::TemplateLiteralNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let template_strings = self.template_strings.clone().expect("template sempre tem ao menos uma string");
        if self.template_expressions.is_none() {
            // Only one template element exists because there's no expression in a given template literal.
            debug_assert!(template_strings.borrow().next.is_none());
            let template_string = crate::parser::nodes::Expression::TemplateString(template_strings.borrow().node.clone());
            return generator.emit_node_expression(dst, &template_string);
        }

        let mut temporary_registers: Vec<crate::bytecompiler::bytecode_generator::RegisterRef> = Vec::new();

        let mut template_string = Some(template_strings);
        let mut template_expression = self.template_expressions.clone();
        while let Some(expression_list) = template_expression {
            let string_list = template_string.clone().expect("uma string para cada expressão");
            // Evaluate TemplateString.
            let string_node = string_list.borrow().node.clone();
            let is_empty = string_node.borrow().cooked.as_ref().expect("cooked").is_empty();
            if !is_empty {
                let temporary = generator.new_temporary();
                temporary_registers.push(temporary.clone());
                generator.emit_node_expression(Some(temporary), &crate::parser::nodes::Expression::TemplateString(string_node));
            }

            // Evaluate Expression.
            let temporary = generator.new_temporary();
            temporary_registers.push(temporary.clone());
            generator.emit_node_expression(Some(temporary.clone()), &expression_list.borrow().node);
            generator.emit_to_string(Some(temporary.clone()), Some(temporary));

            template_expression = expression_list.borrow().next.clone();
            template_string = string_list.borrow().next.clone();
        }

        // Evaluate tail TemplateString.
        let tail = template_string.expect("string final do template");
        let string_node = tail.borrow().node.clone();
        let is_empty = string_node.borrow().cooked.as_ref().expect("cooked").is_empty();
        if !is_empty {
            let temporary = generator.new_temporary();
            temporary_registers.push(temporary.clone());
            generator.emit_node_expression(Some(temporary), &crate::parser::nodes::Expression::TemplateString(string_node));
        }

        if temporary_registers.len() == 1 {
            let final_dst = generator.final_destination(dst.as_ref(), Some(&temporary_registers[0]));
            return generator.move_register(Some(&final_dst), &temporary_registers[0]);
        }

        let final_dst = generator.final_destination(dst.as_ref(), Some(&temporary_registers[0]));
        generator.emit_strcat(Some(final_dst), Some(temporary_registers[0].clone()), temporary_registers.len() as i32)
    }
}

// ------------------------------ TaggedTemplateNode -----------------------------------

impl crate::parser::nodes::TaggedTemplateNode {
    /// `this_expression` é este nó como `Expression` (o C++ passa `this` a `emitGetTemplateObject`).
    pub fn emit_bytecode(
        &self,
        this_node: &crate::parser::nodes::NodeRef<crate::parser::nodes::TaggedTemplateNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let mut expected_function = crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction;
        let mut tag: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
        let mut base: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
        let divot_start = self.throwable.divot_start;
        if !self.tag.is_location() {
            let temporary = generator.new_temporary();
            tag = generator.emit_node_expression(Some(temporary), &self.tag);
        } else if let crate::parser::nodes::Expression::Resolve(resolve) = &self.tag {
            let identifier = resolve.borrow().ident.clone();
            expected_function = generator.expected_function_for_identifier(&identifier);

            let var = generator.variable(&identifier, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            if let Some(local) = var.local() {
                generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);
                let temporary = generator.new_temporary();
                tag = generator.move_register(Some(&temporary), &local);
            } else {
                let tag_register = generator.new_temporary();
                let base_register = generator.new_temporary();
                tag = Some(tag_register.clone());
                base = Some(base_register.clone());

                let new_divot = divot_start + (identifier.length() as u32);
                generator.emit_expression_info(&new_divot, &divot_start, &new_divot);
                let scope = generator.emit_resolve_scope(Some(base_register.clone()), &var).expect("emitResolveScope");
                generator.move_register(Some(&base_register), &scope);
                generator.emit_get_from_scope(
                    Some(tag_register.clone()),
                    Some(base_register),
                    &var,
                    crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
                );
                generator.emit_tdz_check_if_necessary(&var, Some(tag_register), None);
            }
        } else if let crate::parser::nodes::Expression::BracketAccessor(bracket) = &self.tag {
            let base_register = generator.new_temporary();
            let bracket = bracket.borrow();
            base = generator.emit_node_expression(Some(base_register), &bracket.base_expr);
            let property = generator.emit_node_for_property(&bracket.subscript);
            let temporary = generator.new_temporary();
            if bracket.base_expr.is_super_node() {
                let this_value = generator.ensure_this();
                tag = generator.emit_get_by_val_with_this(Some(temporary), base.clone(), Some(this_value), property);
            } else {
                tag = generator.emit_get_by_val(Some(temporary), base.clone(), property);
            }
        } else {
            let dot = match &self.tag {
                crate::parser::nodes::Expression::DotAccessor(dot) => dot,
                _ => unreachable!("isDotAccessorNode"),
            };
            let tag_register = generator.new_temporary();
            let base_register = generator.new_temporary();
            let dot = dot.borrow();
            base = generator.emit_node_expression(Some(base_register), &dot.base_expr);
            tag = dot.emit_get_property_value(generator, Some(tag_register), base.clone());
        }

        let template_object = generator.emit_get_template_object(None, this_node);

        let mut expressions_count: u32 = 0;
        let mut template_expression = self.template_literal.borrow().template_expressions.clone();
        while let Some(expression_list) = template_expression {
            expressions_count += 1;
            template_expression = expression_list.borrow().next.clone();
        }

        let mut call_arguments =
            crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 1 + expressions_count);
        if let Some(base) = &base {
            generator.move_register(call_arguments.this_register().as_ref(), base);
        } else {
            generator.emit_load_js_value(call_arguments.this_register(), crate::runtime::js_value::js_undefined());
        }

        let mut argument_index = 0;
        let template_object = template_object.expect("emitGetTemplateObject");
        generator.move_register(call_arguments.argument_register(argument_index).as_ref(), &template_object);
        argument_index += 1;
        let mut template_expression = self.template_literal.borrow().template_expressions.clone();
        while let Some(expression_list) = template_expression {
            generator.emit_node_expression(call_arguments.argument_register(argument_index), &expression_list.borrow().node);
            argument_index += 1;
            template_expression = expression_list.borrow().next.clone();
        }

        let tag = tag.expect("tag avaliada");
        let final_dst = generator.final_destination(dst.as_ref(), Some(&tag));
        generator.emit_call_in_tail_position(
            Some(final_dst),
            Some(tag.clone()),
            expected_function,
            &mut call_arguments,
            &self.throwable.divot,
            &self.throwable.divot_start,
            &self.throwable.divot_end,
            crate::bytecompiler::bytecode_generator::DebuggableCall::Yes,
        )
    }
}

// ------------------------------ ArrayNode ------------------------------------

impl crate::parser::nodes::ArrayNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let mut had_variable_expression = false;
        let mut all_dense_strings = true;
        let mut length: u32 = 0;

        let mut recommended_indexing_type = crate::runtime::indexing_type::ARRAY_WITH_UNDECIDED;
        let mut first_put_element = self.element.clone();
        while let Some(element) = first_put_element.clone() {
            {
                let element_ref = element.borrow();
                if element_ref.elision != 0 || element_ref.node.is_spread_expression() {
                    break;
                }
                if !element_ref.node.is_constant() {
                    had_variable_expression = true;
                } else {
                    let constant = nodes_codegen_constant_js_value(generator, &element_ref.node);
                    match constant {
                        None => had_variable_expression = true,
                        Some(constant) => {
                            recommended_indexing_type = crate::runtime::indexing_type::least_upper_bound_of_indexing_type_and_value(
                                recommended_indexing_type,
                                constant,
                            );
                            if !constant.is_string() {
                                all_dense_strings = false;
                            } else {
                                let is_atom = match constant.as_js_string().try_get_value_impl() {
                                    Some(value_impl) => value_impl.is_atom(),
                                    None => false,
                                };
                                if !is_atom {
                                    all_dense_strings = false;
                                }
                            }
                        }
                    }
                }
                length += 1;
            }
            let next = element.borrow().next.clone();
            first_put_element = next;
        }
        if had_variable_expression {
            all_dense_strings = false;
        }

        // `newArray` do C++ (lambda): captura por referência `hadVariableExpression`,
        // `recommendedIndexingType` e `allDenseStrings`.
        let new_array = |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
                         dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
                         elements: Option<crate::parser::nodes::NodeRef<crate::parser::nodes::ElementNode>>,
                         length: u32,
                         recommended_indexing_type: &mut crate::runtime::indexing_type::IndexingType,
                         all_dense_strings: bool| {
            if length != 0 && !had_variable_expression {
                *recommended_indexing_type |= crate::runtime::indexing_type::COPY_ON_WRITE;
                // We run bytecode generator under a DeferGC.
                // (O C++ tem aqui só `ASSERT(vm.heap.isDeferred())`, checagem de depuração; o `Heap` não existe.)

                let cell_butterfly_structure = if all_dense_strings {
                    generator.vm().cell_butterfly_only_atom_strings_structure()
                } else {
                    generator.vm().cell_butterfly_structure(*recommended_indexing_type)
                };
                let array = crate::runtime::js_cell_butterfly::JSCellButterfly::try_create(
                    generator.vm(),
                    cell_butterfly_structure,
                    length,
                )
                .expect("JSCellButterfly::tryCreate");

                let mut index: u32 = 0;
                let mut element = elements;
                while index < length {
                    let current = element.expect("elemento constante");
                    let mut constant = {
                        let current_ref = current.borrow();
                        debug_assert!(current_ref.node.is_constant());
                        nodes_codegen_constant_js_value(generator, &current_ref.node).expect("constante")
                    };
                    if all_dense_strings {
                        let string = constant.as_js_string();
                        let string_impl = string.get_value_impl();
                        constant = generator
                            .vm()
                            .atom_string_to_js_string_map()
                            .ensure_value(&string_impl, || string.clone());
                    }
                    array.set_index(generator.vm(), index, constant);
                    index += 1;
                    element = current.borrow().next.clone();
                }
                return generator.emit_new_array_buffer(dst, array, *recommended_indexing_type);
            }
            // `finalDestination(dst)`/`tempDestination(dst)` chegam crus ao `emitNewArray` (sem a referência do clone).
            let raw_dst = dst.as_ref().map(|register| register.get().clone());
            generator.with_raw_register(raw_dst.as_ref(), |generator| generator.emit_new_array(dst, elements, length, *recommended_indexing_type))
        };

        if first_put_element.is_none() && self.elision == 0 {
            let final_dst = generator.final_destination(dst.as_ref(), None);
            return new_array(generator, Some(final_dst), self.element.clone(), length, &mut recommended_indexing_type, all_dense_strings);
        }

        all_dense_strings = false;

        if let Some(first) = &first_put_element {
            if first.borrow().node.is_spread_expression() {
                let mut has_elision = self.elision != 0;
                if !has_elision {
                    let mut node = first_put_element.clone();
                    while let Some(current) = node {
                        if current.borrow().elision != 0 {
                            has_elision = true;
                            break;
                        }
                        node = current.borrow().next.clone();
                    }
                }

                if !has_elision {
                    let final_dst = generator.final_destination(dst.as_ref(), None);
                    let raw_dst = final_dst.get().clone();
                    return generator.with_raw_register(Some(&raw_dst), |generator| generator.emit_new_array_with_spread(Some(final_dst), self.element.clone()));
                }
            }
        }

        let temp_dst = generator.temp_destination(dst.as_ref());
        let array = new_array(generator, Some(temp_dst), self.element.clone(), length, &mut recommended_indexing_type, all_dense_strings)
            .expect("newArray devolve registro");
        let mut n = first_put_element;
        let mut handle_spread = false;
        while let Some(current) = n.clone() {
            if current.borrow().node.is_spread_expression() {
                handle_spread = true;
                break;
            }
            let value = generator.emit_node_expression_no_dst(&current.borrow().node);
            length += current.borrow().elision as u32;

            let index = generator.emit_load_js_value(None, crate::runtime::js_value::js_number(length as f64));
            length += 1;
            generator.emit_direct_put_by_val(Some(array.clone()), index, value);
            n = current.borrow().next.clone();
        }

        if !handle_spread {
            if self.elision != 0 {
                let value =
                    generator.emit_load_js_value(None, crate::runtime::js_value::js_number((self.elision as u32 + length) as f64));
                let length_name = generator.property_names().length.clone();
                generator.emit_put_by_id(Some(array.clone()), &length_name, value);
            }

            return generator.move_register(dst.as_ref(), &array);
        }

        // handleSpread:
        let index_register = generator.new_temporary();
        generator.emit_load_js_value(Some(index_register.clone()), crate::runtime::js_value::js_number(length as f64));
        let spreader_array = array.clone();
        let spreader_index = index_register.clone();
        let mut spreader = move |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
                             value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>| {
            generator.emit_direct_put_by_val(Some(spreader_array.clone()), Some(spreader_index.clone()), value);
            generator.emit_inc(&spreader_index);
        };
        while let Some(current) = n.clone() {
            let elision = current.borrow().elision;
            if elision != 0 {
                let addend = generator.emit_load_js_value(None, crate::runtime::js_value::js_number(elision as f64));
                generator.emit_binary_op::<crate::bytecode::bytecode_ops::OpAdd>(
                    Some(index_register.clone()),
                    Some(index_register.clone()),
                    addend,
                    crate::parser::result_type::OperandTypes::new(
                        crate::parser::result_type::ResultType::number_type_is_int32(),
                        crate::parser::result_type::ResultType::number_type_is_int32(),
                    ),
                );
            }
            let value_expression = current.borrow().node.clone();
            if let crate::parser::nodes::Expression::SpreadExpression(spread) = &value_expression {
                let spread_expression = spread.borrow().expression.clone();
                let throwable = spread.borrow().throwable.clone();
                generator.emit_enumeration(&throwable, &spread_expression, &mut spreader, None, None);
            } else {
                let value = generator.emit_node_expression_no_dst(&value_expression);
                generator.emit_direct_put_by_val(Some(array.clone()), Some(index_register.clone()), value);
                generator.emit_inc(&index_register);
            }
            n = current.borrow().next.clone();
        }

        if self.elision != 0 {
            let addend = generator.emit_load_js_value(None, crate::runtime::js_value::js_number(self.elision as f64));
            generator.emit_binary_op::<crate::bytecode::bytecode_ops::OpAdd>(
                Some(index_register.clone()),
                Some(index_register.clone()),
                addend,
                crate::parser::result_type::OperandTypes::new(
                    crate::parser::result_type::ResultType::number_type_is_int32(),
                    crate::parser::result_type::ResultType::number_type_is_int32(),
                ),
            );
            let length_name = generator.property_names().length.clone();
            generator.emit_put_by_id(Some(array.clone()), &length_name, Some(index_register));
        }
        generator.move_register(dst.as_ref(), &array)
    }
}

/// `static_cast<ConstantNode*>(expr)->jsValue(generator)`: despacho de `ConstantNode::jsValue` pelas
/// variantes constantes (`Null`, `Boolean`, `Double`, `Integer`, `String`, `BigInt`).
fn nodes_codegen_constant_js_value(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    expression: &crate::parser::nodes::Expression,
) -> Option<crate::runtime::js_value::JSValue> {
    match expression {
        crate::parser::nodes::Expression::Null(_) => Some(crate::runtime::js_value::js_null()),
        crate::parser::nodes::Expression::Boolean(node) => Some(crate::runtime::js_value::js_boolean(node.borrow().value)),
        crate::parser::nodes::Expression::Double(node) => node.borrow().js_value(generator),
        crate::parser::nodes::Expression::Integer(node) => node.borrow().js_value(generator),
        crate::parser::nodes::Expression::String(node) => node.borrow().js_value(generator),
        crate::parser::nodes::Expression::BigInt(node) => node.borrow().js_value(generator),
        _ => unreachable!("isConstant"),
    }
}
