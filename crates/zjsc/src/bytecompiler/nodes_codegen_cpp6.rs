// NodesCodegen.cpp, linhas 5535 a 5980: `FuncDeclNode`, `FuncExprNode`, `ArrowFuncExprNode`,
// `MethodDefinitionNode`, `YieldExprNode`, `AwaitExprNode`, `DefineFieldNode`, `ClassDeclNode`,
// `ClassExprNode`, as declarações de import/export, `DestructuringAssignmentNode::emitBytecode`,
// `assignDefaultValueIfUndefined` e `ArrayPatternNode::bindValue` (incluído por include!, sem `use`).
// Pára antes de `ArrayPatternNode::toString` (linha 5982).
//
// Convenções desta fatia (as mesmas da cpp5b): registrador é `Option<RegisterRef>` (o `RegisterID*`
// nulo do C++), o `RefPtr<RegisterID>` é o próprio `Option<RegisterRef>`, `.get()` vira `.clone()`.
// Os nós que o gerador recebe por ponteiro (`emitNewFunctionExpression(dst, this)`, `pushLexicalScope(this,
// ...)`) recebem `this` como `NodeRef<T>`. O `StrictModeScope` guarda o gerador e o devolve por
// `generator()`; o `Drop` restaura o modo ao sair do bloco.
//
// Dependências: todas já portadas na cpp7 (conferido por grep em 2026-10-08):
// `DestructuringPatternNode::bind_value`/`bind_value_can_throw` e
// `AssignmentElementNode::emit_nodes_for_destructuring`/`bind_value_with_emitted_nodes`.

type Cpp6Reg = Option<crate::bytecompiler::bytecode_generator::RegisterRef>;

// ------------------------------ FuncDeclNode ---------------------------------

impl crate::parser::nodes::FuncDeclNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp6Reg,
    ) {
        generator.hoist_sloppy_mode_function_if_necessary(&self.metadata);
    }
}

// ------------------------------ FuncExprNode ---------------------------------

impl crate::parser::nodes::FuncExprNode {
    pub fn emit_bytecode(
        this: &crate::parser::nodes::NodeRef<crate::parser::nodes::FuncExprNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp6Reg,
    ) -> Cpp6Reg {
        let final_dst = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_new_function_expression(final_dst, this)
    }
}

// ------------------------------ ArrowFuncExprNode ---------------------------------

impl crate::parser::nodes::ArrowFuncExprNode {
    pub fn emit_bytecode(
        this: &crate::parser::nodes::NodeRef<crate::parser::nodes::ArrowFuncExprNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp6Reg,
    ) -> Cpp6Reg {
        let final_dst = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_new_arrow_function_expression(final_dst, this)
    }
}

// ------------------------------ MethodDefinitionNode ---------------------------------

impl crate::parser::nodes::MethodDefinitionNode {
    pub fn emit_bytecode(
        this: &crate::parser::nodes::NodeRef<crate::parser::nodes::MethodDefinitionNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp6Reg,
    ) -> Cpp6Reg {
        let final_dst = Some(generator.final_destination(dst.as_ref(), None));
        generator.emit_new_method_definition(final_dst, this)
    }
}

// ------------------------------ YieldExprNode --------------------------------

impl crate::parser::nodes::YieldExprNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp6Reg,
    ) -> Cpp6Reg {
        if !self.delegate {
            let arg = match &self.argument {
                Some(argument) => generator.emit_node_expression(None, argument),
                None => generator.emit_load_js_value(None, crate::runtime::js_value::JSValue::Undefined),
            };
            let value = generator.emit_yield(arg.as_ref().unwrap());
            if generator.is_ignored_dst(dst.as_ref()) {
                return None;
            }
            let final_dst = generator.final_destination(dst.as_ref(), None);
            return generator.move_register(Some(&final_dst), value.as_ref().unwrap());
        }
        let argument = self.argument.as_ref().expect("yield* sempre tem argumento");
        let arg = generator.emit_node_expression(None, argument);
        let value = generator.emit_delegate_yield(arg.as_ref().unwrap(), &self.throwable);
        if generator.is_ignored_dst(dst.as_ref()) {
            return None;
        }
        let final_dst = generator.final_destination(dst.as_ref(), None);
        generator.move_register(Some(&final_dst), value.as_ref().unwrap())
    }
}

// ------------------------------ AwaitExprNode --------------------------------

impl crate::parser::nodes::AwaitExprNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp6Reg,
    ) -> Cpp6Reg {
        let arg = generator.emit_node_expression(None, &self.argument);
        let target = match dst {
            Some(register) => Some(register),
            None => Some(generator.new_temporary()),
        };
        // `dst ? dst : newTemporary()` chega cru ao `emitAwait` (contagem 0 no temporário novo).
        let raw_target = target.as_ref().map(|register| register.get().clone());
        let position = self.position();
        generator.with_raw_register(raw_target.as_ref(), |generator| generator.emit_await(target, arg.as_ref().unwrap(), position))
    }
}

// ------------------------------ DefineFieldNode ---------------------------------

impl crate::parser::nodes::DefineFieldNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp6Reg,
    ) {
        let value = Some(generator.new_temporary());
        let mut should_set_function_name = false;

        match &self.assign {
            None => {
                generator.emit_load_js_value(value.clone(), crate::runtime::js_value::JSValue::Undefined);
            }
            Some(assign) => {
                generator.emit_node_expression(value.clone(), assign);
                should_set_function_name = generator.should_set_function_name(assign);
                if should_set_function_name && self.type_ != crate::parser::nodes::DefineFieldType::ComputedName {
                    generator.emit_set_function_name_identifier(value.clone(), &self.ident);
                }
            }
        }

        match self.type_ {
            crate::parser::nodes::DefineFieldType::Name => {
                let mut strict_mode_scope = crate::bytecompiler::bytecode_generator::StrictModeScope::new(generator);
                let generator = strict_mode_scope.generator();
                if let Some(index) = crate::runtime::identifier::parse_index_identifier(&self.ident) {
                    let property_name =
                        generator.emit_load_js_value(None, crate::runtime::js_value::JSValue::from_u32(index));
                    let this_register = generator.this_register();
                    generator.emit_direct_put_by_val(Some(this_register), property_name, value);
                } else {
                    let this_register = generator.this_register();
                    generator.emit_direct_put_by_id(Some(this_register), &self.ident, value);
                }
            }
            crate::parser::nodes::DefineFieldType::PrivateName => {
                let var = generator
                    .variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                assert!(var.local().is_none(), "Private Field names must be stored in captured variables");

                let end = self.position().clone() + self.ident.length();
                generator.emit_expression_info(self.position(), self.position(), &end);
                let scope = generator.emit_resolve_scope(None, &var);
                let private_name = Some(generator.new_temporary());
                generator.emit_get_from_scope(
                    private_name.clone(),
                    scope,
                    &var,
                    crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                );
                let this_register = generator.this_register();
                generator.emit_define_private_field(Some(this_register), private_name, value);
            }
            crate::parser::nodes::DefineFieldType::ComputedName => {
                // For ComputedNames, the expression has already been evaluated earlier during evaluation of a ClassExprNode.
                // Here, `m_ident` refers to private symbol ID in a class lexical scope, containing the value already converted to an Expression.
                let var = generator
                    .variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                assert!(var.local().is_none(), "Computed names must be stored in captured variables");

                let end = self.position().clone() + 1;
                generator.emit_expression_info(self.position(), self.position(), &end);
                let scope = generator.emit_resolve_scope(None, &var);
                let private_name = Some(generator.new_temporary());
                generator.emit_get_from_scope(
                    private_name.clone(),
                    scope,
                    &var,
                    crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
                );
                if should_set_function_name {
                    generator.emit_set_function_name(value.clone(), private_name.clone());
                }
                let profile_end = self.position().clone() + self.ident.length();
                generator.emit_profile_type_variable(private_name.clone(), &var, self.position(), &profile_end);
                {
                    let mut strict_mode_scope =
                        crate::bytecompiler::bytecode_generator::StrictModeScope::new(generator);
                    let generator = strict_mode_scope.generator();
                    let this_register = generator.this_register();
                    generator.emit_direct_put_by_val(Some(this_register), private_name, value);
                }
            }
        }
    }
}

// ------------------------------ ClassDeclNode ---------------------------------

impl crate::parser::nodes::ClassDeclNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp6Reg,
    ) {
        generator.emit_node_expression(None, &self.class_declaration);
    }
}

// ------------------------------ ClassExprNode ---------------------------------

impl crate::parser::nodes::ClassExprNode {
    pub fn emit_bytecode(
        this: &crate::parser::nodes::NodeRef<crate::parser::nodes::ClassExprNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp6Reg,
    ) -> Cpp6Reg {
        let mut strict_mode_scope = crate::bytecompiler::bytecode_generator::StrictModeScope::new(generator);
        let generator = strict_mode_scope.generator();

        let has_name = !this.borrow().name.is_null();
        if has_name {
            generator.push_class_head_lexical_scope(&mut this.borrow_mut().class_head_environment);
        }

        let node = this.borrow();

        // Class heritage must be evaluated outside of private fields access.
        let mut superclass: Cpp6Reg = None;
        if let Some(class_heritage) = &node.class_heritage {
            superclass = Some(generator.new_temporary());
            generator.emit_node_expression(superclass.clone(), class_heritage);
        }

        if node.needs_lexical_scope {
            generator.push_lexical_scope(
                &node.variable_environment,
                crate::bytecompiler::bytecode_generator::ScopeType::ClassScope,
                crate::bytecompiler::bytecode_generator::TDZCheckOptimization::Optimize,
                crate::bytecompiler::bytecode_generator::NestedScopeType::IsNested,
                None,
                true,
            );
        }

        let lexical_variables = &node.variable_environment.lexical_variables;
        let has_private_names = lexical_variables.private_names_size() != 0;
        let should_emit_private_brand = lexical_variables.has_instance_private_method_or_accessor();
        let should_install_brand_on_constructor = lexical_variables.has_static_private_method_or_accessor();
        if has_private_names {
            generator.push_private_access_names(lexical_variables.private_name_environment());
        }
        if should_emit_private_brand {
            generator.emit_create_private_brand(node.position(), node.position(), node.position());
        }

        let mut constructor: Cpp6Reg = Some(generator.temp_destination(dst.as_ref()));
        let mut needs_home_object = false;

        let needs_class_field_initializer = if node.has_instance_fields() {
            crate::bytecode::executable_info::NeedsClassFieldInitializer::Yes
        } else {
            crate::bytecode::executable_info::NeedsClassFieldInitializer::No
        };
        let private_brand_requirement = if should_emit_private_brand {
            crate::parser::parser_modes::PrivateBrandRequirement::Needed
        } else {
            crate::parser::parser_modes::PrivateBrandRequirement::None
        };
        if let Some(constructor_expression) = &node.constructor_expression {
            debug_assert!(constructor_expression.is_func_expr_node());
            let metadata = constructor_expression.as_func_expr_node().unwrap().metadata.clone();
            metadata.set_ecma_name(node.ecma_name());
            metadata.set_class_source(&node.class_source);
            // `needsClassFieldInitializer` e `privateBrandRequirement` são `Cell` no `FunctionMetadataNode`.
            metadata
                .needs_class_field_initializer
                .set(needs_class_field_initializer == crate::bytecode::executable_info::NeedsClassFieldInitializer::Yes);
            metadata.private_brand_requirement.set(private_brand_requirement);
            constructor = generator.emit_node_expression(constructor.clone(), constructor_expression);
            needs_home_object = node.class_heritage.is_some()
                || metadata.super_binding == crate::parser::parser_modes::SuperBinding::Needed;
        } else {
            constructor = generator.emit_new_default_constructor(
                constructor.clone(),
                if node.class_heritage.is_some() {
                    crate::runtime::constructor_kind::ConstructorKind::Extends
                } else {
                    crate::runtime::constructor_kind::ConstructorKind::Base
                },
                &node.name,
                node.ecma_name(),
                &node.class_source,
                needs_class_field_initializer,
                private_brand_requirement,
            );
        }

        let property_names_constructor = generator.property_names().constructor.clone();
        let property_names_prototype = generator.property_names().prototype.clone();
        let new_object_dst = Some(generator.new_temporary());
        let prototype = generator.emit_new_object(new_object_dst);

        if let Some(superclass) = &superclass {
            let proto_parent = Some(generator.new_temporary());
            generator.emit_load_js_value(proto_parent.clone(), crate::runtime::js_value::JSValue::Null);

            let superclass_is_null_label = generator.new_label();
            // Temporários do C++ sem `RefPtr`: soltos logo após o salto, para o próximo `newTemporary` reaproveitar.
            {
                let is_null_dst = Some(generator.new_temporary());
                let is_null = generator.emit_is_null(is_null_dst, superclass);
                generator.emit_jump_if_true_raw(is_null.as_ref().unwrap(), &superclass_is_null_label);
            }

            let superclass_is_constructor_label = generator.new_label();
            {
                let is_constructor_dst = Some(generator.new_temporary());
                let is_constructor = generator.emit_is_constructor(is_constructor_dst, superclass);
                generator.emit_jump_if_true_raw(is_constructor.as_ref().unwrap(), &superclass_is_constructor_label);
            }
            generator.emit_expression_info(
                &node.throwable.divot,
                &node.throwable.divot_start,
                &node.throwable.divot_end,
            );
            generator.emit_throw_type_error("The superclass is not a constructor.");
            generator.emit_label(&superclass_is_constructor_label);
            generator.emit_get_by_id(proto_parent.clone(), Some(superclass.clone()), &property_names_prototype);

            generator.emit_direct_set_prototype_of(
                crate::bytecompiler::bytecode_generator::InvalidPrototypeMode::Throw,
                constructor.clone(),
                Some(superclass.clone()),
                node.position(),
                node.position(),
                node.position(),
            ); // never actually throws
            generator.emit_label(&superclass_is_null_label);
            generator.emit_direct_set_prototype_of(
                crate::bytecompiler::bytecode_generator::InvalidPrototypeMode::Throw,
                prototype.clone(),
                proto_parent,
                &node.throwable.divot,
                &node.throwable.divot_start,
                &node.throwable.divot_end,
            );
        }

        if needs_home_object {
            emit_put_home_object(generator, constructor.as_ref().unwrap(), prototype.as_ref().unwrap());
        }

        let constructor_name_register = generator.emit_load_identifier(None, &property_names_constructor);
        generator.emit_call_define_property(
            prototype.clone(),
            constructor_name_register,
            constructor.clone(),
            None,
            None,
            crate::bytecompiler::bytecode_generator::PROPERTY_CONFIGURABLE
                | crate::bytecompiler::bytecode_generator::PROPERTY_WRITABLE,
            node.position(),
        );

        let prototype_name_register = generator.emit_load_identifier(None, &property_names_prototype);
        generator.emit_call_define_property(
            constructor.clone(),
            prototype_name_register,
            prototype.clone(),
            None,
            None,
            0,
            node.position(),
        );

        let mut static_element_definitions: Vec<crate::bytecode::unlinked_function_executable::ClassElementDefinition> =
            Vec::new();
        if let Some(class_elements) = &node.class_elements {
            let scope_register = generator.scope_register();
            crate::parser::nodes::PropertyListNode::emit_declare_private_field_names(
                class_elements,
                generator,
                scope_register.as_ref().unwrap(),
            );

            let mut instance_element_definitions: Vec<
                crate::bytecode::unlinked_function_executable::ClassElementDefinition,
            > = Vec::new();
            generator.emit_define_class_elements(
                class_elements,
                constructor.as_ref().unwrap(),
                prototype.as_ref().unwrap(),
                &mut instance_element_definitions,
                &mut static_element_definitions,
            );
            if !instance_element_definitions.is_empty() {
                let initializer_dst = Some(generator.new_temporary());
                let instance_field_initializer = generator.emit_new_class_field_initializer_function(
                    initializer_dst,
                    instance_element_definitions,
                    node.class_heritage.is_some(),
                );

                // FIXME: Skip this if the initializer function isn't going to need a home object (no eval or super properties)
                // https://bugs.webkit.org/show_bug.cgi?id=196867
                emit_put_home_object(
                    generator,
                    instance_field_initializer.as_ref().unwrap(),
                    prototype.as_ref().unwrap(),
                );

                let initializer_name = generator
                    .property_names()
                    .builtin_names()
                    .instance_field_initializer_private_name()
                    .clone();
                generator.emit_direct_put_by_id(constructor.clone(), &initializer_name, instance_field_initializer);
            }
        }

        if has_name {
            let class_name_var =
                generator.variable(&node.name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            assert!(class_name_var.is_resolved());
            let scope = generator.emit_resolve_scope(None, &class_name_var);
            generator.emit_put_to_scope(
                scope,
                &class_name_var,
                constructor.clone(),
                crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
                crate::runtime::get_put_info::InitializationMode::Initialization,
            );
        }

        if should_install_brand_on_constructor {
            generator.emit_install_private_class_brand(constructor.clone());
        }

        if !static_element_definitions.is_empty() {
            let initializer_dst = Some(generator.new_temporary());
            let static_field_initializer = generator.emit_new_class_field_initializer_function(
                initializer_dst,
                static_element_definitions,
                node.class_heritage.is_some(),
            );
            // FIXME: Skip this if the initializer function isn't going to need a home object (no eval or super properties)
            // https://bugs.webkit.org/show_bug.cgi?id=196867
            emit_put_home_object(
                generator,
                static_field_initializer.as_ref().unwrap(),
                constructor.as_ref().unwrap(),
            );

            let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 0);
            generator.move_register(args.this_register().as_ref(), constructor.as_ref().unwrap());
            let ignored_dst = Some(generator.new_temporary());
            generator.emit_call_ignore_result(
                ignored_dst,
                static_field_initializer,
                crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                &mut args,
                node.position(),
                node.position(),
                node.position(),
                crate::bytecompiler::bytecode_generator::DebuggableCall::No,
            );
        }

        if has_private_names {
            generator.pop_private_access_names();
        }

        if node.needs_lexical_scope {
            generator.pop_lexical_scope(&node.variable_environment);
        }

        drop(node);
        if has_name {
            generator.pop_lexical_scope_internal(&mut this.borrow_mut().class_head_environment);
        }

        let final_dst = generator.final_destination(dst.as_ref(), constructor.as_ref());
        generator.move_register(Some(&final_dst), constructor.as_ref().unwrap())
    }
}

// ------------------------------ ImportDeclarationNode -----------------------

impl crate::parser::nodes::ImportDeclarationNode {
    pub fn emit_bytecode(
        &self,
        _generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp6Reg,
    ) {
        // Do nothing at runtime.
    }
}

// ------------------------------ ExportAllDeclarationNode --------------------

impl crate::parser::nodes::ExportAllDeclarationNode {
    pub fn emit_bytecode(
        &self,
        _generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp6Reg,
    ) {
        // Do nothing at runtime.
    }
}

// ------------------------------ ExportDefaultDeclarationNode ----------------

impl crate::parser::nodes::ExportDefaultDeclarationNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp6Reg,
    ) {
        generator.emit_node(dst.as_ref(), &self.declaration);
    }
}

// ------------------------------ ExportLocalDeclarationNode ------------------

impl crate::parser::nodes::ExportLocalDeclarationNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp6Reg,
    ) {
        generator.emit_node(dst.as_ref(), &self.declaration);
    }
}

// ------------------------------ ExportNamedDeclarationNode ------------------

impl crate::parser::nodes::ExportNamedDeclarationNode {
    pub fn emit_bytecode(
        &self,
        _generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        _dst: Cpp6Reg,
    ) {
        // Do nothing at runtime.
    }
}

// ------------------------------ DestructuringAssignmentNode -----------------

impl crate::parser::nodes::DestructuringAssignmentNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp6Reg,
    ) -> Cpp6Reg {
        let initializer = Some(generator.temp_destination(dst.as_ref()));
        let initializer_expression = self.initializer.as_ref().expect("a atribuição tem inicializador");
        generator.emit_node_expression(initializer.clone(), initializer_expression);
        self.bindings.bind_value(generator, initializer.clone());
        generator.move_register(dst.as_ref(), initializer.as_ref().unwrap())
    }
}

fn cpp6_assign_default_value_if_undefined(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    maybe_undefined: &Cpp6Reg,
    default_value: &crate::parser::nodes::Expression,
) {
    let is_not_undefined = generator.new_label();
    let is_undefined_dst = Some(generator.new_temporary());
    let is_undefined = generator.emit_is_undefined(is_undefined_dst, maybe_undefined.as_ref().unwrap());
    generator.emit_jump_if_false_raw(is_undefined.as_ref().unwrap(), &is_not_undefined);
    generator.emit_node_expression(maybe_undefined.clone(), default_value);
    generator.emit_label(&is_not_undefined);
}

impl crate::parser::nodes::ArrayPatternNode {
    pub fn bind_value(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        rhs: Cpp6Reg,
    ) {
        use crate::parser::nodes::ArrayPatternBindingType;

        if !generator.vm().is_safe_to_recurse() {
            generator.emit_throw_expression_too_deep_exception();
            return;
        }

        let iterable = rhs;
        let iterator = generator.new_temporary();
        let next_or_index = generator.new_temporary();
        {
            generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
            let symbol_dst = Some(generator.new_temporary());
            let iterator_symbol_name = generator.property_names().iterator_symbol.clone();
            let iterator_symbol = generator.emit_get_by_id(symbol_dst, iterable.clone(), &iterator_symbol_name);
            let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 0);
            generator.move_register(args.this_register().as_ref(), iterable.as_ref().unwrap());
            generator.emit_iterator_open(
                &iterator,
                &next_or_index,
                iterator_symbol.as_ref().unwrap(),
                &mut args,
                &self.throwable,
            );
        }

        if self.target_patterns.is_empty() {
            generator.emit_iterator_generic_close(
                &iterator,
                &self.throwable,
                crate::bytecompiler::bytecode_generator::EmitAwait::No,
            );
            return;
        }

        let bind_value_or_default_value_can_throw = self.target_patterns.iter().any(|target| {
            if let Some(pattern) = &target.pattern {
                if pattern.bind_value_can_throw(generator) {
                    return true;
                }
            }

            if let Some(default_value) = &target.default_value {
                if default_value.is_constant() {
                    return false;
                }
                if default_value.is_resolve_node()
                    && !default_value.as_resolve_node().unwrap().borrow().get_from_scope_can_throw(generator)
                {
                    return false;
                }
                return true;
            }

            false
        });

        let done = generator.new_temporary();

        let mut emit_bind_value = |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator| {
            for (i, target) in self.target_patterns.iter().enumerate() {
                let mut target_base_and_property_name = None;
                if let Some(crate::parser::nodes::DestructuringPatternNode::AssignmentElement(element)) =
                    &target.pattern
                {
                    target_base_and_property_name = element.borrow().emit_nodes_for_destructuring(generator, None, None);
                }

                match target.binding_type {
                    ArrayPatternBindingType::Elision | ArrayPatternBindingType::Element => {
                        let iteration_skipped = generator.new_label();
                        if i != 0 {
                            generator.emit_jump_if_true(&done, &iteration_skipped);
                        }

                        let value = generator.new_temporary();
                        {
                            let value_is_set = generator.new_label();
                            let mut next_args =
                                crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 0);
                            generator.move_register(next_args.this_register().as_ref(), &iterator);
                            if bind_value_or_default_value_can_throw {
                                // This implements steps 3-5 of https://tc39.es/ecma262/#sec-iteratornext and similar steps in its callers.
                                // On the fast path, only IteratorNext & friends can throw, resulting in iteratorRecord.[[Done]] being set
                                // to `true` and skipping IteratorClose. As an optimization, we are avoiding emitLoad() here because exception
                                // handlers are not emitted on the fast path and `done` won't be checked in case of an abrupt completion.
                                generator.emit_load_js_value(
                                    Some(done.clone()),
                                    crate::runtime::js_value::JSValue::Bool(true),
                                );
                            }
                            generator.emit_iterator_next(
                                &done,
                                &value,
                                iterable.as_ref().unwrap(),
                                &next_or_index,
                                &mut next_args,
                                &self.throwable,
                            );
                            generator.emit_jump_if_false(&done, &value_is_set);
                            generator.emit_label(&iteration_skipped);
                            generator.emit_load_js_value(
                                Some(value.clone()),
                                crate::runtime::js_value::JSValue::Undefined,
                            );
                            generator.emit_label(&value_is_set);
                        }

                        if target.binding_type == ArrayPatternBindingType::Element {
                            if let Some(default_value) = &target.default_value {
                                cpp6_assign_default_value_if_undefined(generator, &Some(value.clone()), default_value);
                            }

                            let pattern = target.pattern.as_ref().expect("o elemento tem padrão");
                            match (&target_base_and_property_name, pattern) {
                                (
                                    Some(emitted),
                                    crate::parser::nodes::DestructuringPatternNode::AssignmentElement(element),
                                ) => {
                                    element.borrow().bind_value_with_emitted_nodes(
                                        generator,
                                        emitted.clone(),
                                        Some(value.clone()),
                                    );
                                }
                                _ => pattern.bind_value(generator, Some(value.clone())),
                            }
                        }
                    }

                    ArrayPatternBindingType::RestElement => {
                        let array_dst = generator.new_temporary();
                        // `newTemporary()` cru no C++ (contagem 0): o primeiro `newTemporary()` do array reaproveita o slot.
                        let raw_array_dst = array_dst.get().clone();
                        let array = generator.with_raw_register(Some(&raw_array_dst), |generator| {
                            generator.emit_new_array(
                                Some(array_dst),
                                None,
                                0,
                                crate::runtime::indexing_type::ARRAY_WITH_UNDECIDED,
                            )
                        });

                        let iteration_done = generator.new_label();
                        if i != 0 {
                            generator.emit_jump_if_true(&done, &iteration_done);
                        }

                        let index = generator.new_temporary();
                        generator.emit_load_js_value(Some(index.clone()), crate::runtime::js_value::js_number_i32(0));
                        let loop_start = generator.new_label();
                        generator.emit_label(&loop_start);

                        let value = generator.new_temporary();
                        {
                            let mut next_args =
                                crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 0);
                            generator.move_register(next_args.this_register().as_ref(), &iterator);
                            if bind_value_or_default_value_can_throw {
                                // This implements steps 3-5 of https://tc39.es/ecma262/#sec-iteratornext and similar steps in its callers.
                                // On the fast path, only IteratorNext & friends can throw, resulting in iteratorRecord.[[Done]] being set
                                // to `true` and skipping IteratorClose. As an optimization, we are avoiding emitLoad() here because exception
                                // handlers are not emitted on the fast path and `done` won't be checked in case of an abrupt completion.
                                generator.emit_load_js_value(
                                    Some(done.clone()),
                                    crate::runtime::js_value::JSValue::Bool(true),
                                );
                            }
                            generator.emit_iterator_next(
                                &done,
                                &value,
                                iterable.as_ref().unwrap(),
                                &next_or_index,
                                &mut next_args,
                                &self.throwable,
                            );
                            generator.emit_jump_if_true(&done, &iteration_done);
                        }

                        generator.emit_direct_put_by_val(array.clone(), Some(index.clone()), Some(value.clone()));
                        generator.emit_inc(&index);
                        generator.emit_jump(&loop_start);

                        generator.emit_label(&iteration_done);
                        let pattern = target.pattern.as_ref().expect("o rest tem padrão");
                        match (&target_base_and_property_name, pattern) {
                            (
                                Some(emitted),
                                crate::parser::nodes::DestructuringPatternNode::AssignmentElement(element),
                            ) => {
                                element.borrow().bind_value_with_emitted_nodes(generator, emitted.clone(), array.clone());
                            }
                            _ => pattern.bind_value(generator, array.clone()),
                        }
                    }
                }
            }
        };

        let mut emit_iterator_close = |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator| {
            let iterator_closed = generator.new_label();
            generator.emit_jump_if_true(&done, &iterator_closed);
            generator.emit_iterator_generic_close(
                &iterator,
                &self.throwable,
                crate::bytecompiler::bytecode_generator::EmitAwait::No,
            );
            generator.emit_label(&iterator_closed);
        };

        if bind_value_or_default_value_can_throw {
            generator.emit_load_js_value(Some(done.clone()), crate::runtime::js_value::JSValue::Bool(false));
            generator.emit_try_with_finally_that_does_not_shadow_exception(&mut emit_bind_value, &mut emit_iterator_close);
        } else {
            emit_bind_value(generator);
            emit_iterator_close(generator);
        }
    }
}
