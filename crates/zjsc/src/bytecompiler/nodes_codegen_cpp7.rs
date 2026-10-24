// NodesCodegen.cpp, linhas 5982 a 6473: `ArrayPatternNode::toString`/`collectBoundIdentifiers`,
// `ObjectPatternNode` (`toString`, `bindValue`, `collectBoundIdentifiers`), `BindingNode`,
// `AssignmentElementNode`, `RestParameterNode`, `SpreadExpressionNode` e `ObjectSpreadExpressionNode`
// (incluído por include!, sem `use`).
//
// Convenções desta fatia (as mesmas da cpp5b): registrador é `Option<RegisterRef>` (o `RegisterID*`
// nulo do C++) e o `RefPtr<RegisterID>` é o próprio `Option<RegisterRef>`, com `.get()` virando `.clone()`.
//
// Despacho virtual: `DestructuringPatternNode` é um `enum` por classe concreta, então os métodos
// virtuais da base (`bindValue`, `toString`, `collectBoundIdentifiers`, `bindValueCanThrow`,
// `writableDirectBindingIfPossible`, `finishDirectBindingAssignment`) ficam no `impl` do enum, no fim
// do arquivo, com os padrões de `parser/Nodes.h:2517-2519`. `ArrayPatternNode::bind_value` é da fatia
// anterior (`bindValue` do `ArrayPatternNode`, antes da linha 5900).
//
// Dependências: todas já portadas (conferido por grep em 2026-10-08):
// `StringBuilder::append_quoted_json_string` em `wtf/text/string_builder.rs` e
// `ArrayPatternNode::bind_value` na cpp6.

type Cpp7Reg = Option<crate::bytecompiler::bytecode_generator::RegisterRef>;

/// `AssignmentElementNode::BaseAndPropertyName` (`std::pair<RefPtr<RegisterID>, RefPtr<RegisterID>>`).
pub type Cpp7BaseAndPropertyName = (Cpp7Reg, Cpp7Reg);

// ------------------------------ ArrayPatternNode -----------------------------------

impl crate::parser::nodes::ArrayPatternNode {
    pub fn to_string(&self, builder: &mut crate::wtf::text::string_builder::StringBuilder) {
        use crate::parser::nodes::ArrayPatternBindingType;

        builder.append_character(u16::from(b'['));
        for i in 0..self.target_patterns.len() {
            let target = &self.target_patterns[i];

            match target.binding_type {
                ArrayPatternBindingType::Elision => {
                    builder.append_character(u16::from(b','));
                }

                ArrayPatternBindingType::Element => {
                    target.pattern.as_ref().unwrap().to_string(builder);
                    if i < self.target_patterns.len() - 1 {
                        builder.append_character(u16::from(b','));
                    }
                }

                ArrayPatternBindingType::RestElement => {
                    builder.append_ascii_literal("...");
                    target.pattern.as_ref().unwrap().to_string(builder);
                }
            }
        }
        builder.append_character(u16::from(b']'));
    }

    pub fn collect_bound_identifiers(&self, identifiers: &mut Vec<crate::runtime::identifier::Identifier>) {
        for i in 0..self.target_patterns.len() {
            if let Some(node) = &self.target_patterns[i].pattern {
                node.collect_bound_identifiers(identifiers);
            }
        }
    }
}

// ------------------------------ ObjectPatternNode -----------------------------------

impl crate::parser::nodes::ObjectPatternNode {
    pub fn to_string(&self, builder: &mut crate::wtf::text::string_builder::StringBuilder) {
        builder.append_character(u16::from(b'{'));
        for i in 0..self.target_patterns.len() {
            if self.target_patterns[i].was_string {
                builder.append_quoted_json_string(self.target_patterns[i].property_name.string().string());
            } else {
                builder.append_atom_string(self.target_patterns[i].property_name.string());
            }
            builder.append_character(u16::from(b':'));
            self.target_patterns[i].pattern.to_string(builder);
            if i < self.target_patterns.len() - 1 {
                builder.append_character(u16::from(b','));
            }
        }
        builder.append_character(u16::from(b'}'));
    }

    pub fn bind_value(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        rhs: Cpp7Reg,
    ) {
        use crate::parser::nodes::ObjectPatternBindingType;

        if !generator.vm().is_safe_to_recurse() {
            generator.emit_throw_expression_too_deep_exception();
            return;
        }

        let mut first_property_name: Option<&crate::runtime::identifier::Identifier> = None;
        if !self.target_patterns.is_empty() {
            let first_target = &self.target_patterns[0];
            if first_target.property_expression.is_none()
                && !first_target.property_name.is_null()
                && !first_target.property_name.is_private_name()
            {
                first_property_name = Some(&first_target.property_name);
            }
        }
        generator.emit_require_object_coercible_for_destructuring(rhs.as_ref().unwrap(), first_property_name);

        let mut preserved_tdz_stack = crate::bytecompiler::bytecode_generator::PreservedTDZStack::default();
        generator.preserve_tdz_stack(&mut preserved_tdz_stack);

        {
            let mut rest_element_base: Cpp7Reg = None;
            let mut rest_element_property_name: Cpp7Reg = None;
            let mut new_object: Cpp7Reg = None;
            let mut excluded_set = crate::parser::parser::IdentifierSet::default();
            let mut args: Option<crate::bytecompiler::bytecode_generator::CallArguments> = None;
            let mut number_of_computed_properties: u32 = 0;
            let mut index_in_arguments: u32 = 2;
            if self.contains_rest_element {
                if self.contains_computed_property {
                    for target in self.target_patterns.iter() {
                        if target.binding_type == ObjectPatternBindingType::Element && target.property_expression.is_some() {
                            number_of_computed_properties += 1;
                        }
                    }
                }
                rest_element_base = Some(generator.new_temporary());
                rest_element_property_name = Some(generator.new_temporary());
                new_object = Some(generator.new_temporary());
                args = Some(crate::bytecompiler::bytecode_generator::CallArguments::new(
                    generator,
                    None,
                    (index_in_arguments + number_of_computed_properties) as _,
                ));
            }

            for i in 0..self.target_patterns.len() {
                let target = &self.target_patterns[i];
                if target.binding_type == ObjectPatternBindingType::Element {
                    // If the destructuring becomes get_by_id and mov, then we should store results directly to the local's binding.
                    // From
                    //     get_by_id          dst:loc10, base:loc9, property:0
                    //     mov                dst:loc6, src:loc10
                    // To
                    //     get_by_id          dst:loc6, base:loc9, property:0
                    let writable_direct_binding_if_possible =
                        |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator| -> Cpp7Reg {
                            // The following pattern is possible. In that case, after setting |data| local variable, we need to store property name into the set.
                            // So, old property name |data| result must be kept before setting it into |data|.
                            //     ({ [data]: data, ...obj } = object);
                            if self.contains_rest_element
                                && self.contains_computed_property
                                && target.property_expression.is_some()
                            {
                                return None;
                            }
                            // default value can include a reference to local variable. So filling value to a local variable can differ result.
                            // We give up fast path if default value includes non constant.
                            // For example,
                            //     ({ data = data } = object);
                            if let Some(default_value) = &target.default_value {
                                if !default_value.is_constant() {
                                    return None;
                                }
                            }
                            target.pattern.writable_direct_binding_if_possible(generator)
                        };

                    let finish_direct_binding_assignment =
                        |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator| {
                            debug_assert!(writable_direct_binding_if_possible(generator).is_some());
                            target.pattern.finish_direct_binding_assignment(generator);
                        };

                    let direct_binding = writable_direct_binding_if_possible(generator);
                    let temp: Cpp7Reg = if direct_binding.is_some() {
                        direct_binding.clone()
                    } else {
                        Some(generator.new_temporary())
                    };

                    let mut target_base_and_property_name: Option<Cpp7BaseAndPropertyName> = None;
                    match &target.property_expression {
                        None => {
                            if let crate::parser::nodes::DestructuringPatternNode::AssignmentElement(element) = &target.pattern {
                                target_base_and_property_name =
                                    element.borrow().emit_nodes_for_destructuring(generator, None, None);
                            }
                            let optional_index = crate::runtime::identifier::parse_index_identifier(&target.property_name);
                            match optional_index {
                                None => {
                                    generator.emit_get_by_id(temp.clone(), rhs.clone(), &target.property_name);
                                }
                                Some(index) => {
                                    let property_index = generator
                                        .emit_load_js_value(None, crate::runtime::js_value::JSValue::from_u32(index));
                                    generator.emit_get_by_val(temp.clone(), rhs.clone(), property_index);
                                }
                            }
                            if self.contains_rest_element {
                                excluded_set.add(&target.property_name.impl_().unwrap(), ());
                            }
                        }
                        Some(property_expression) => {
                            let mut property_name: Cpp7Reg;
                            if self.contains_rest_element {
                                property_name = generator.emit_node_for_property_dst(
                                    args.as_ref().unwrap().argument_register(index_in_arguments as usize),
                                    property_expression,
                                );
                            } else {
                                property_name = generator.emit_node_for_property(property_expression);
                            }
                            if !property_expression.is_number() && !property_expression.is_string() {
                                // ToPropertyKey(Number | String) does not have side-effect.
                                // And for Number case, passing it to GetByVal is better for performance.
                                let dst = if self.contains_rest_element {
                                    args.as_ref().unwrap().argument_register(index_in_arguments as usize)
                                } else {
                                    Some(generator.new_temporary())
                                };
                                property_name = generator.emit_to_property_key_or_number(dst, property_name);
                            }
                            if self.contains_rest_element {
                                index_in_arguments += 1;
                            }
                            if let crate::parser::nodes::DestructuringPatternNode::AssignmentElement(element) = &target.pattern {
                                target_base_and_property_name =
                                    element.borrow().emit_nodes_for_destructuring(generator, None, None);
                            }
                            generator.emit_get_by_val(temp.clone(), rhs.clone(), property_name);
                        }
                    }

                    if let Some(default_value) = &target.default_value {
                        cpp6_assign_default_value_if_undefined(generator, &temp, default_value);
                    }

                    if direct_binding.is_some() {
                        debug_assert!(target_base_and_property_name.is_none());
                        finish_direct_binding_assignment(generator);
                    } else if let Some(base_and_property_name) = target_base_and_property_name {
                        let crate::parser::nodes::DestructuringPatternNode::AssignmentElement(element) = &target.pattern else {
                            unreachable!("RELEASE_ASSERT_NOT_REACHED");
                        };
                        element
                            .borrow()
                            .bind_value_with_emitted_nodes(generator, base_and_property_name, temp.clone());
                    } else {
                        target.pattern.bind_value(generator, temp.clone());
                    }
                } else {
                    debug_assert!(target.binding_type == ObjectPatternBindingType::RestElement);
                    debug_assert!(i == self.target_patterns.len() - 1);

                    let mut target_base_and_property_name: Option<Cpp7BaseAndPropertyName> = None;
                    if let crate::parser::nodes::DestructuringPatternNode::AssignmentElement(element) = &target.pattern {
                        target_base_and_property_name = element.borrow().emit_nodes_for_destructuring(
                            generator,
                            rest_element_base.clone(),
                            rest_element_property_name.clone(),
                        );
                    }

                    generator.emit_new_object(new_object.clone());

                    // load and call @copyDataProperties
                    let copy_data_properties = generator.move_link_time_constant(
                        None,
                        crate::bytecode::link_time_constant::LinkTimeConstant::CopyDataProperties,
                    );

                    // This must be non-tail-call because @copyDataProperties accesses caller-frame.
                    let args = args.as_mut().unwrap();
                    generator.move_register(args.this_register().as_ref(), new_object.as_ref().unwrap());
                    generator.move_register(args.argument_register(0).as_ref(), rhs.as_ref().unwrap());
                    generator.emit_load_excluded_list(args.argument_register(1), std::mem::take(&mut excluded_set));
                    let ignored_result = Some(generator.new_temporary());
                    generator.emit_call_ignore_result(
                        ignored_result,
                        copy_data_properties,
                        crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                        args,
                        &self.throwable.divot,
                        &self.throwable.divot_start,
                        &self.throwable.divot_end,
                        crate::bytecompiler::bytecode_generator::DebuggableCall::No,
                    );

                    if let Some(base_and_property_name) = target_base_and_property_name {
                        let crate::parser::nodes::DestructuringPatternNode::AssignmentElement(element) = &target.pattern else {
                            unreachable!("RELEASE_ASSERT_NOT_REACHED");
                        };
                        element
                            .borrow()
                            .bind_value_with_emitted_nodes(generator, base_and_property_name, new_object.clone());
                    } else {
                        target.pattern.bind_value(generator, new_object.clone());
                    }
                }
            }
        }

        generator.restore_tdz_stack(&preserved_tdz_stack);
    }

    pub fn collect_bound_identifiers(&self, identifiers: &mut Vec<crate::runtime::identifier::Identifier>) {
        for i in 0..self.target_patterns.len() {
            self.target_patterns[i].pattern.collect_bound_identifiers(identifiers);
        }
    }
}

// ------------------------------ BindingNode -----------------------------------

impl crate::parser::nodes::BindingNode {
    pub fn bind_value_can_throw(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> bool {
        use crate::parser::nodes::AssignmentContext;

        let var = generator.variable(&self.bound_property, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        if var.offset().is_stack() || var.offset().is_scope() {
            if self.binding_context != AssignmentContext::ConstDeclarationStatement && var.is_read_only() {
                return true;
            }
            if self.binding_context == AssignmentContext::AssignmentExpression && generator.needs_tdz_check(&var) {
                return true;
            }
            return false;
        }

        true
    }

    pub fn writable_direct_binding_if_possible(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Cpp7Reg {
        use crate::parser::nodes::AssignmentContext;

        let var = generator.variable(&self.bound_property, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let is_read_only = var.is_read_only() && self.binding_context != AssignmentContext::ConstDeclarationStatement;
        if let Some(local) = var.local() {
            if self.binding_context == AssignmentContext::AssignmentExpression && generator.needs_tdz_check(&var) {
                return None;
            }
            if is_read_only {
                return None;
            }
            return Some(local);
        }
        None
    }

    pub fn finish_direct_binding_assignment(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) {
        use crate::parser::nodes::AssignmentContext;

        debug_assert!(self.writable_direct_binding_if_possible(generator).is_some());
        let var = generator.variable(&self.bound_property, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let local = var.local();
        generator.emit_profile_type_variable(local, &var, &self.divot_start, &self.divot_end);
        if self.binding_context == AssignmentContext::DeclarationStatement
            || self.binding_context == AssignmentContext::ConstDeclarationStatement
        {
            generator.lift_tdz_check_if_possible(&var);
        }
    }

    pub fn bind_value(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        value: Cpp7Reg,
    ) {
        use crate::parser::nodes::AssignmentContext;

        let var = generator.variable(&self.bound_property, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let is_read_only = var.is_read_only()
            && self.binding_context != AssignmentContext::ConstDeclarationStatement
            && !cpp4b_is_using_or_await_using_assignment_context(self.binding_context);
        if let Some(local) = var.local() {
            if self.binding_context == AssignmentContext::AssignmentExpression {
                generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);
            }
            if is_read_only {
                generator.emit_read_only_exception_if_needed(&var);
                return;
            }
            generator.move_register(Some(&local), value.as_ref().unwrap());
            generator.emit_profile_type_variable(Some(local.clone()), &var, &self.divot_start, &self.divot_end);
            if cpp4b_is_using_or_await_using_assignment_context(self.binding_context) {
                generator.emit_prepare_disposable(
                    Some(local.clone()),
                    &self.divot_start,
                    self.binding_context == AssignmentContext::AwaitUsingDeclarationStatement,
                );
            }
            if self.binding_context != AssignmentContext::AssignmentExpression {
                generator.lift_tdz_check_if_possible(&var);
            }
            return;
        }
        if generator.ecma_mode().is_strict() {
            generator.emit_expression_info(&self.divot_end, &self.divot_start, &self.divot_end);
        }
        let scope = generator.emit_resolve_scope(None, &var);
        generator.emit_expression_info(&self.divot_end, &self.divot_start, &self.divot_end);
        if self.binding_context == AssignmentContext::AssignmentExpression {
            generator.emit_tdz_check_if_necessary(&var, None, scope.clone());
        }
        if is_read_only {
            generator.emit_read_only_exception_if_needed(&var);
            return;
        }
        let resolve_mode = cpp4b_put_resolve_mode(generator);
        generator.emit_put_to_scope(
            scope,
            &var,
            value.clone(),
            resolve_mode,
            cpp4b_initialization_mode_for_assignment_context(self.binding_context),
        );
        generator.emit_profile_type_variable(value.clone(), &var, &self.divot_start, &self.divot_end);
        if cpp4b_is_using_or_await_using_assignment_context(self.binding_context) {
            generator.emit_prepare_disposable(
                value.clone(),
                &self.divot_start,
                self.binding_context == AssignmentContext::AwaitUsingDeclarationStatement,
            );
        }
        if self.binding_context != AssignmentContext::AssignmentExpression {
            generator.lift_tdz_check_if_possible(&var);
        }
    }

    pub fn to_string(&self, builder: &mut crate::wtf::text::string_builder::StringBuilder) {
        builder.append_atom_string(self.bound_property.string());
    }

    pub fn collect_bound_identifiers(&self, identifiers: &mut Vec<crate::runtime::identifier::Identifier>) {
        identifiers.push(self.bound_property.clone());
    }
}

// ------------------------------ AssignmentElementNode -----------------------------------

impl crate::parser::nodes::AssignmentElementNode {
    /// Os dois argumentos opcionais do C++ (`base` e `propertyName`, padrão nulo) são explícitos.
    pub fn emit_nodes_for_destructuring(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        base: Cpp7Reg,
        property_name: Cpp7Reg,
    ) -> Option<Cpp7BaseAndPropertyName> {
        let mut base = base;
        let mut property_name = property_name;

        if self.assignment_target.is_dot_accessor_node() {
            if base.is_none() {
                base = Some(generator.new_temporary());
            }

            let node = self.assignment_target.as_dot_accessor_node().unwrap();
            generator.emit_node_expression(base.clone(), &node.borrow().base.base_expr);
            generator.emit_expression_info(&self.divot_end, &self.divot_start, &self.divot_end);

            return Some((base, None));
        }

        if self.assignment_target.is_bracket_accessor_node() {
            if base.is_none() {
                base = Some(generator.new_temporary());
            }
            if property_name.is_none() {
                property_name = Some(generator.new_temporary());
            }

            let node = self.assignment_target.as_bracket_accessor_node().unwrap();
            generator.emit_node_expression(base.clone(), &node.borrow().base_expr);
            generator.emit_node_for_property_dst(property_name.clone(), &node.borrow().subscript);
            generator.emit_expression_info(&self.divot_end, &self.divot_start, &self.divot_end);

            return Some((base, property_name));
        }

        None
    }

    pub fn bind_value_with_emitted_nodes(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        pair: Cpp7BaseAndPropertyName,
        value: Cpp7Reg,
    ) {
        if self.assignment_target.is_dot_accessor_node() {
            let node = self.assignment_target.as_dot_accessor_node().unwrap();
            node.borrow().emit_put_property(generator, pair.0.clone(), value.clone());
            generator.emit_profile_type_divots(value, &self.divot_start, &self.divot_end);
        } else if self.assignment_target.is_bracket_accessor_node() {
            let node = self.assignment_target.as_bracket_accessor_node().unwrap();
            if node.borrow().base_expr.is_super_node() {
                let this_value = Some(generator.ensure_this());
                generator.emit_put_by_val_with_this(pair.0.clone(), this_value, pair.1.clone(), value.clone());
            } else {
                generator.emit_put_by_val(pair.0.clone(), pair.1.clone(), value.clone());
            }
            generator.emit_profile_type_divots(value, &self.divot_start, &self.divot_end);
        }
    }

    pub fn bind_value_can_throw(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> bool {
        if self.assignment_target.is_resolve_node() {
            let lhs = self.assignment_target.as_resolve_node().unwrap();
            let ident = lhs.borrow().ident.clone();
            let var = generator.variable(&ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            if var.offset().is_stack() || var.offset().is_scope() {
                return var.is_read_only() || generator.needs_tdz_check(&var);
            }
        }

        true
    }

    pub fn writable_direct_binding_if_possible(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Cpp7Reg {
        if !self.assignment_target.is_resolve_node() {
            return None;
        }
        let lhs = self.assignment_target.as_resolve_node().unwrap();
        let ident = lhs.borrow().ident.clone();
        let var = generator.variable(&ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let is_read_only = var.is_read_only();
        if let Some(local) = var.local() {
            if generator.needs_tdz_check(&var) {
                return None;
            }
            if is_read_only {
                return None;
            }
            return Some(local);
        }
        None
    }

    pub fn finish_direct_binding_assignment(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) {
        debug_assert!(self.writable_direct_binding_if_possible(generator).is_some());
        let lhs = self.assignment_target.as_resolve_node().unwrap();
        let ident = lhs.borrow().ident.clone();
        let var = generator.variable(&ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        let local = var.local();
        generator.emit_profile_type_divots(local, &self.divot_start, &self.divot_end);
    }

    pub fn collect_bound_identifiers(&self, _identifiers: &mut Vec<crate::runtime::identifier::Identifier>) {}

    pub fn bind_value(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        value: Cpp7Reg,
    ) {
        if self.assignment_target.is_resolve_node() {
            let lhs = self.assignment_target.as_resolve_node().unwrap();
            let ident = lhs.borrow().ident.clone();
            let var = generator.variable(&ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            let is_read_only = var.is_read_only();
            if let Some(local) = var.local() {
                generator.emit_tdz_check_if_necessary(&var, Some(local.clone()), None);

                if is_read_only {
                    generator.emit_read_only_exception_if_needed(&var);
                } else {
                    generator.move_register(Some(&local), value.as_ref().unwrap());
                    generator.emit_profile_type_divots(Some(local.clone()), &self.divot_start, &self.divot_end);
                }
                return;
            }
            if generator.ecma_mode().is_strict() {
                generator.emit_expression_info(&self.divot_end, &self.divot_start, &self.divot_end);
            }
            let scope = generator.emit_resolve_scope(None, &var);
            generator.emit_tdz_check_if_necessary(&var, None, scope.clone());
            if is_read_only {
                let threw_exception = generator.emit_read_only_exception_if_needed(&var);
                if threw_exception {
                    return;
                }
            }
            generator.emit_expression_info(&self.divot_end, &self.divot_start, &self.divot_end);
            if !is_read_only {
                let resolve_mode = cpp4b_put_resolve_mode(generator);
                generator.emit_put_to_scope(
                    scope,
                    &var,
                    value.clone(),
                    resolve_mode,
                    crate::runtime::get_put_info::InitializationMode::NotInitialization,
                );
                generator.emit_profile_type_variable(value.clone(), &var, &self.divot_start, &self.divot_end);
            }
        } else if self.assignment_target.is_dot_accessor_node() {
            let lhs = self.assignment_target.as_dot_accessor_node().unwrap();
            let lhs = lhs.borrow();
            let base = generator.emit_node_for_left_hand_side(&lhs.base.base_expr, true, false);
            generator.emit_expression_info(&self.divot_end, &self.divot_start, &self.divot_end);
            lhs.emit_put_property(generator, base, value.clone());
            generator.emit_profile_type_divots(value, &self.divot_start, &self.divot_end);
        } else if self.assignment_target.is_bracket_accessor_node() {
            let lhs = self.assignment_target.as_bracket_accessor_node().unwrap();
            let lhs = lhs.borrow();
            let base = generator.emit_node_for_left_hand_side(&lhs.base_expr, true, false);
            let property = generator.emit_node_for_left_hand_side_for_property(&lhs.subscript, true, false);
            generator.emit_expression_info(&self.divot_end, &self.divot_start, &self.divot_end);
            if lhs.base_expr.is_super_node() {
                let this_value = Some(generator.ensure_this());
                generator.emit_put_by_val_with_this(base, this_value, property, value.clone());
            } else {
                generator.emit_put_by_val(base, property, value.clone());
            }
            generator.emit_profile_type_divots(value, &self.divot_start, &self.divot_end);
        }
    }

    pub fn to_string(&self, builder: &mut crate::wtf::text::string_builder::StringBuilder) {
        if self.assignment_target.is_resolve_node() {
            let target = self.assignment_target.as_resolve_node().unwrap();
            builder.append_atom_string(target.borrow().ident.string());
        }
    }
}

// ------------------------------ RestParameterNode -----------------------------------

impl crate::parser::nodes::RestParameterNode {
    pub fn collect_bound_identifiers(&self, identifiers: &mut Vec<crate::runtime::identifier::Identifier>) {
        self.pattern.collect_bound_identifiers(identifiers);
    }

    pub fn to_string(&self, builder: &mut crate::wtf::text::string_builder::StringBuilder) {
        builder.append_ascii_literal("...");
        self.pattern.to_string(builder);
    }

    pub fn bind_value(
        &self,
        _generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _value: Cpp7Reg,
    ) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED");
    }

    pub fn emit(&self, generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator) {
        let direct_binding = self.pattern.writable_direct_binding_if_possible(generator);
        if direct_binding.is_some() {
            generator.emit_rest_parameter(direct_binding, self.num_parameters_to_skip);
            self.pattern.finish_direct_binding_assignment(generator);
            return;
        }
        let temp = Some(generator.new_temporary());
        generator.emit_rest_parameter(temp.clone(), self.num_parameters_to_skip);
        self.pattern.bind_value(generator, temp);
    }
}

// ------------------------------ SpreadExpressionNode, ObjectSpreadExpressionNode -----------------------------------

impl crate::parser::nodes::SpreadExpressionNode {
    pub fn emit_bytecode(
        &self,
        _generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp7Reg,
    ) -> Cpp7Reg {
        unreachable!("RELEASE_ASSERT_NOT_REACHED");
    }
}

impl crate::parser::nodes::ObjectSpreadExpressionNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp7Reg,
    ) -> Cpp7Reg {
        let src = Some(generator.new_temporary());
        generator.emit_node_expression(src.clone(), &self.expression);

        let copy_data_properties = generator.move_link_time_constant(
            None,
            crate::bytecode::link_time_constant::LinkTimeConstant::CopyDataProperties,
        );

        let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 1);
        generator.move_register(args.this_register().as_ref(), dst.as_ref().unwrap());
        generator.move_register(args.argument_register(0).as_ref(), src.as_ref().unwrap());

        // This must be non-tail-call because @copyDataProperties accesses caller-frame.
        let ignored_result = Some(generator.new_temporary());
        generator.emit_call_ignore_result(
            ignored_result,
            copy_data_properties,
            crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
            &mut args,
            &self.throwable.divot,
            &self.throwable.divot_start,
            &self.throwable.divot_end,
            crate::bytecompiler::bytecode_generator::DebuggableCall::No,
        );

        dst
    }
}

// ------------------------------ DestructuringPatternNode (despacho virtual) -----------------------------------

impl crate::parser::nodes::DestructuringPatternNode {
    pub fn collect_bound_identifiers(&self, identifiers: &mut Vec<crate::runtime::identifier::Identifier>) {
        use crate::parser::nodes::DestructuringPatternNode as Pattern;
        match self {
            Pattern::ArrayPattern(node) => node.borrow().collect_bound_identifiers(identifiers),
            Pattern::ObjectPattern(node) => node.borrow().collect_bound_identifiers(identifiers),
            Pattern::Binding(node) => node.borrow().collect_bound_identifiers(identifiers),
            Pattern::RestParameter(node) => node.borrow().collect_bound_identifiers(identifiers),
            Pattern::AssignmentElement(node) => node.borrow().collect_bound_identifiers(identifiers),
        }
    }

    pub fn to_string(&self, builder: &mut crate::wtf::text::string_builder::StringBuilder) {
        use crate::parser::nodes::DestructuringPatternNode as Pattern;
        match self {
            Pattern::ArrayPattern(node) => node.borrow().to_string(builder),
            Pattern::ObjectPattern(node) => node.borrow().to_string(builder),
            Pattern::Binding(node) => node.borrow().to_string(builder),
            Pattern::RestParameter(node) => node.borrow().to_string(builder),
            Pattern::AssignmentElement(node) => node.borrow().to_string(builder),
        }
    }

    pub fn bind_value(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        value: Cpp7Reg,
    ) {
        use crate::parser::nodes::DestructuringPatternNode as Pattern;
        match self {
            Pattern::ArrayPattern(node) => node.borrow().bind_value(generator, value),
            Pattern::ObjectPattern(node) => node.borrow().bind_value(generator, value),
            Pattern::Binding(node) => node.borrow().bind_value(generator, value),
            Pattern::RestParameter(node) => node.borrow().bind_value(generator, value),
            Pattern::AssignmentElement(node) => node.borrow().bind_value(generator, value),
        }
    }

    /// `virtual bool bindValueCanThrow(BytecodeGenerator&) const { return true; }` (`Nodes.h:2517`).
    pub fn bind_value_can_throw(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> bool {
        use crate::parser::nodes::DestructuringPatternNode as Pattern;
        match self {
            Pattern::Binding(node) => node.borrow().bind_value_can_throw(generator),
            Pattern::AssignmentElement(node) => node.borrow().bind_value_can_throw(generator),
            Pattern::ArrayPattern(_) | Pattern::ObjectPattern(_) | Pattern::RestParameter(_) => true,
        }
    }

    /// `virtual RegisterID* writableDirectBindingIfPossible(BytecodeGenerator&) const { return nullptr; }`
    /// (`Nodes.h:2518`).
    pub fn writable_direct_binding_if_possible(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Cpp7Reg {
        use crate::parser::nodes::DestructuringPatternNode as Pattern;
        match self {
            Pattern::Binding(node) => node.borrow().writable_direct_binding_if_possible(generator),
            Pattern::AssignmentElement(node) => node.borrow().writable_direct_binding_if_possible(generator),
            Pattern::ArrayPattern(_) | Pattern::ObjectPattern(_) | Pattern::RestParameter(_) => None,
        }
    }

    /// `virtual void finishDirectBindingAssignment(BytecodeGenerator&) const { }` (`Nodes.h:2519`).
    pub fn finish_direct_binding_assignment(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) {
        use crate::parser::nodes::DestructuringPatternNode as Pattern;
        match self {
            Pattern::Binding(node) => node.borrow().finish_direct_binding_assignment(generator),
            Pattern::AssignmentElement(node) => node.borrow().finish_direct_binding_assignment(generator),
            Pattern::ArrayPattern(_) | Pattern::ObjectPattern(_) | Pattern::RestParameter(_) => {}
        }
    }
}
