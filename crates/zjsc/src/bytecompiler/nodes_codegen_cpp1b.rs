// Tradução de bytecompiler/NodesCodegen.cpp, linhas 555 a 1152 (de `ArrayNode::isSimpleArray` e
// `toArgumentList` até o fim de `BaseDotNode::emitGetPropertyValue` com `thisValue`).
// Incluído por include!, sem `use` no topo: caminhos completos em tudo.
//
// Convenções herdadas de nodes_codegen_cpp1.rs: `dst: Option<RegisterRef>`; métodos por struct concreta
// recebem `&self`; onde o C++ passa `this` como nó compartilhado, vem `this_node: &NodeRef<..>`.
// `GetterSetterMap` do C++ (hash por `UniquedStringImpl*`) vira `Vec` com busca linear por `Identifier`:
// a ordem de iteração do `privateAccessorMap` só decide a ordem dos registradores temporários.

// `ArrayNode::isSimpleArray` já vive em parser/nodes.rs (só lê a árvore).
impl crate::parser::nodes::ArrayNode {
    /// `ArrayNode::toArgumentList`: o `ParserArena` do C++ some, a posse é compartilhada.
    pub fn to_argument_list(
        &self,
        line_number: i32,
        start_position: u32,
    ) -> Option<crate::parser::nodes::NodeRef<crate::parser::nodes::ArgumentListNode>> {
        debug_assert!(self.elision == 0);
        let mut ptr = self.element.clone()?;
        let mut location = crate::parser::parser_tokens::JSTokenLocation::default();
        location.line = line_number;
        location.start_offset = start_position;
        let head = crate::parser::nodes::node(crate::parser::nodes::ArgumentListNode::new(&location, ptr.borrow().node.clone()));
        let mut tail = head.clone();
        let mut next = ptr.borrow().next.clone();
        while let Some(current) = next {
            debug_assert!(current.borrow().elision == 0);
            tail = crate::parser::nodes::ArgumentListNode::append(&tail, &location, current.borrow().node.clone());
            ptr = current.clone();
            next = ptr.borrow().next.clone();
        }
        Some(head)
    }
}

// ------------------------------ ObjectLiteralNode ----------------------------

impl crate::parser::nodes::ObjectLiteralNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let list = match &self.list {
            None => {
                if generator.is_ignored_dst(dst.as_ref()) {
                    return None;
                }
                let final_dst = generator.final_destination(dst.as_ref(), None);
                return generator.emit_new_object(Some(final_dst));
            }
            Some(list) => list.clone(),
        };

        let position = *self.position();
        let mut property_list = list;
        let mut new_object: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
        let first_is_spread = property_list.borrow().node.borrow().type_ & crate::parser::nodes::PropertyNode::SPREAD != 0;
        if first_is_spread {
            // Only one element and it is spread.
            if property_list.borrow().next.is_none() {
                let function = generator
                    .move_link_time_constant(None, crate::bytecode::link_time_constant::LinkTimeConstant::CloneObject)
                    .expect("moveLinkTimeConstant devolve registro");
                let spread_source = nodes_codegen_spread_source(&property_list);
                let src = generator.emit_node_expression_no_dst(&spread_source);
                let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 0);
                generator.move_register(args.this_register().as_ref(), &src.expect("emitNode devolve registro"));
                let final_dst = generator.final_destination(dst.as_ref(), Some(&function));
                return generator.emit_call(
                    Some(final_dst),
                    Some(function.clone()),
                    crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                    &mut args,
                    &position,
                    &position,
                    &position,
                    crate::bytecompiler::bytecode_generator::DebuggableCall::No,
                );
            }

            let mut found_non_constant = false;
            let mut p = property_list.borrow().next.clone();
            while let Some(current) = p {
                let type_ = current.borrow().node.borrow().type_;
                if type_ & crate::parser::nodes::PropertyNode::CONSTANT != 0
                    || type_ & crate::parser::nodes::PropertyNode::COMPUTED != 0
                    || type_ & crate::parser::nodes::PropertyNode::SPREAD != 0
                {
                    p = current.borrow().next.clone();
                    continue;
                }
                found_non_constant = true;
                break;
            }

            // All properties are simple constants, and the first property is spread.
            // Let's first clone an object and materialize the rest.
            if !found_non_constant {
                let function = generator
                    .move_link_time_constant(None, crate::bytecode::link_time_constant::LinkTimeConstant::CloneObject)
                    .expect("moveLinkTimeConstant devolve registro");
                let spread_source = nodes_codegen_spread_source(&property_list);
                let src = generator.emit_node_expression_no_dst(&spread_source);
                let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 0);
                generator.move_register(args.this_register().as_ref(), &src.expect("emitNode devolve registro"));
                let temp_dst = generator.temp_destination(dst.as_ref());
                new_object = generator.emit_call(
                    Some(temp_dst),
                    Some(function.clone()),
                    crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                    &mut args,
                    &position,
                    &position,
                    &position,
                    crate::bytecompiler::bytecode_generator::DebuggableCall::No,
                );
                let next = property_list.borrow().next.clone().expect("lista com mais de um elemento");
                property_list = next;
            }
        }

        let new_object = match new_object {
            Some(new_object) => new_object,
            None => {
                let temp_dst = generator.temp_destination(dst.as_ref());
                generator.emit_new_object(Some(temp_dst)).expect("emitNewObject devolve registro")
            }
        };
        crate::parser::nodes::PropertyListNode::emit_bytecode(&property_list, generator, Some(new_object.clone()), None, None, None);
        generator.move_register(dst.as_ref(), &new_object)
    }
}

/// `static_cast<ObjectSpreadExpressionNode*>(propertyList->m_node->m_assign)->expression()`.
fn nodes_codegen_spread_source(
    property_list: &crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyListNode>,
) -> crate::parser::nodes::Expression {
    let property = property_list.borrow().node.clone();
    let assign = property.borrow().assign.clone().expect("spread tem expressão");
    match assign {
        crate::parser::nodes::Expression::ObjectSpreadExpression(spread) => spread.borrow().expression.clone(),
        _ => unreachable!("PropertyNode::Spread carrega ObjectSpreadExpressionNode"),
    }
}

// ------------------------------ PropertyListNode -----------------------------

fn emit_put_home_object(
    generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    function: &crate::bytecompiler::bytecode_generator::RegisterRef,
    home_object: &crate::bytecompiler::bytecode_generator::RegisterRef,
) {
    let home_object_name = generator.property_names().builtin_names().home_object_private_name().clone();
    generator.emit_put_by_id(Some(function.clone()), &home_object_name, Some(home_object.clone()));
}

/// `needsHomeObject(ExpressionNode*)`.
fn nodes_codegen_needs_home_object(node: &crate::parser::nodes::Expression) -> bool {
    match node {
        crate::parser::nodes::Expression::FuncExpr(n) => {
            n.borrow().metadata.super_binding == crate::parser::parser_modes::SuperBinding::Needed
        }
        crate::parser::nodes::Expression::ArrowFuncExpr(n) => {
            n.borrow().metadata.super_binding == crate::parser::parser_modes::SuperBinding::Needed
        }
        crate::parser::nodes::Expression::MethodDefinition(n) => {
            n.borrow().metadata.super_binding == crate::parser::parser_modes::SuperBinding::Needed
        }
        _ => false,
    }
}

/// `makeClassElementDefinition` (lambda de `PropertyListNode::emitBytecode`).
fn nodes_codegen_make_class_element_definition(
    list: &crate::parser::nodes::PropertyListNode,
) -> crate::bytecode::unlinked_function_executable::ClassElementDefinition {
    use crate::bytecode::unlinked_function_executable::ClassElementDefinitionKind as Kind;

    let node = list.node.borrow();
    let initializer_position = node.assign.as_ref().map(|initializer| *initializer.base().position());

    let mut kind = Kind::FieldWithLiteralPropertyKey;
    if node.is_static_class_block() {
        kind = Kind::StaticInitializationBlock;
    } else if node.has_computed_name() {
        kind = Kind::FieldWithComputedPropertyKey;
    } else if node.is_private() {
        kind = Kind::FieldWithPrivatePropertyKey;
    }

    crate::bytecode::unlinked_function_executable::ClassElementDefinition {
        ident: node.name.clone().expect("elemento de classe tem nome"),
        position: *list.position(),
        initializer_position,
        kind,
    }
}

type NodesCodegenGetterSetterPair = (
    Option<crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyNode>>,
    Option<crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyNode>>,
);
type NodesCodegenGetterSetterMap = Vec<(crate::runtime::identifier::Identifier, NodesCodegenGetterSetterPair)>;

/// `GetterSetterMap::find`.
fn nodes_codegen_map_find(map: &NodesCodegenGetterSetterMap, name: &crate::runtime::identifier::Identifier) -> Option<usize> {
    map.iter().position(|(key, _)| key == name)
}

impl crate::parser::nodes::PropertyListNode {
    pub fn is_computed_class_field(&self) -> bool {
        self.node.borrow().is_computed_class_field()
    }

    pub fn is_instance_class_field(&self) -> bool {
        self.node.borrow().is_instance_class_field()
    }

    pub fn is_static_class_element(&self) -> bool {
        self.node.borrow().is_static_class_element()
    }

    pub fn emit_declare_private_field_names(
        this: &crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyListNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        scope: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) {
        // Walk the list and declare any Private property names (e.g. `#foo`) in the provided scope.
        let mut create_private_symbol: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
        let mut list = Some(this.clone());
        while let Some(p) = list {
            let property = p.borrow().node.clone();
            // O C++ usa `position()` do próprio `this` (a cabeça da lista), não o do elemento corrente.
            let position = *this.borrow().position();
            if property.borrow().type_ & crate::parser::nodes::PropertyNode::PRIVATE_FIELD != 0 {
                if create_private_symbol.is_none() {
                    create_private_symbol = generator.move_link_time_constant(
                        None,
                        crate::bytecode::link_time_constant::LinkTimeConstant::CreatePrivateSymbol,
                    );
                }
                let create_private_symbol = create_private_symbol.clone().expect("moveLinkTimeConstant devolve registro");

                let name = property.borrow().name.clone().expect("campo privado tem nome");
                let mut arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 1);
                generator.emit_load_js_value(arguments.this_register(), crate::runtime::js_value::js_undefined());
                generator.emit_load_identifier(arguments.argument_register(0), &name);
                let final_dst = generator.final_destination(None, Some(&create_private_symbol));
                // O destino é o `RegisterID*` cru do C++ (contagem 0): o call frame reaproveita o slot.
                let raw_dst = final_dst.get().clone();
                let symbol = generator.with_raw_register(Some(&raw_dst), |generator| {
                    generator.emit_call(
                        Some(final_dst),
                        Some(create_private_symbol.clone()),
                        crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                        &mut arguments,
                        &position,
                        &position,
                        &position,
                        crate::bytecompiler::bytecode_generator::DebuggableCall::No,
                    )
                });

                let var = generator.variable(&name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                generator.emit_put_to_scope(
                    Some(scope.clone()),
                    &var,
                    symbol,
                    crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                    crate::runtime::get_put_info::InitializationMode::ConstInitialization,
                );
            }
            list = p.borrow().next.clone();
        }
    }

    pub fn emit_bytecode(
        this: &crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyListNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst_or_constructor: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        prototype: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        mut instance_element_definitions: Option<
            &mut Vec<crate::bytecode::unlinked_function_executable::ClassElementDefinition>,
        >,
        mut static_element_definitions: Option<
            &mut Vec<crate::bytecode::unlinked_function_executable::ClassElementDefinition>,
        >,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        use crate::parser::nodes::PropertyNode;

        if this.borrow().has_private_accessors {
            let mut private_accessor_map: NodesCodegenGetterSetterMap = Vec::new();

            let mut property_list = Some(this.clone());
            while let Some(current) = property_list {
                let node = current.borrow().node.clone();
                let is_private_accessor =
                    node.borrow().type_ & (PropertyNode::PRIVATE_GETTER | PropertyNode::PRIVATE_SETTER) != 0;
                if is_private_accessor {
                    // We group private getters and setters to store them in a object
                    let name = node.borrow().name.clone().expect("acessor privado tem nome");
                    match nodes_codegen_map_find(&private_accessor_map, &name) {
                        // If the map already contains an element with node->name(),
                        // we need to store this node in the second part.
                        Some(index) => private_accessor_map[index].1 .1 = Some(node.clone()),
                        None => private_accessor_map.push((name, (Some(node.clone()), None))),
                    }
                }
                property_list = current.borrow().next.clone();
            }

            // Then we declare private accessors
            for (_, pair) in &private_accessor_map {
                // FIXME: Use GetterSetter to store private accessors
                // https://bugs.webkit.org/show_bug.cgi?id=221915
                let new_temporary = generator.new_temporary();
                let getter_setter_obj = generator.emit_new_object(Some(new_temporary)).expect("emitNewObject");

                let mut emit_put_accessor = |generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
                                             property_node: &crate::parser::nodes::NodeRef<PropertyNode>| {
                    let base = if property_node.borrow().is_instance_class_property() {
                        prototype.clone()
                    } else {
                        dst_or_constructor.clone()
                    };

                    let assign = property_node.borrow().assign.clone().expect("acessor tem função");
                    let value = generator.emit_node_expression_no_dst(&assign).expect("emitNode devolve registro");
                    if property_node.borrow().needs_super_binding {
                        emit_put_home_object(generator, &value, &base.expect("base do acessor"));
                    }
                    let setter_or_getter_ident =
                        if property_node.borrow().type_ & PropertyNode::PRIVATE_GETTER != 0 {
                            generator.property_names().builtin_names().get_private_name().clone()
                        } else {
                            generator.property_names().builtin_names().set_dup_private_name().clone()
                        };
                    generator.emit_direct_put_by_id(Some(getter_setter_obj.clone()), &setter_or_getter_ident, Some(value));
                };

                if let Some(first) = &pair.0 {
                    emit_put_accessor(generator, first);
                }

                if let Some(second) = &pair.1 {
                    emit_put_accessor(generator, second);
                }

                let first_name = pair.0.as_ref().expect("par sempre tem o primeiro").borrow().name.clone().expect("nome");
                let var = generator.variable(&first_name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                let scope_register = generator.scope_register();
                generator.emit_put_to_scope(
                    scope_register,
                    &var,
                    Some(getter_setter_obj),
                    crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                    crate::runtime::get_put_info::InitializationMode::ConstInitialization,
                );
            }
        }

        let mut p = Some(this.clone());
        let mut dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>;

        // Fast case: this loop just handles regular value properties.
        while let Some(current) = p.clone() {
            let node = current.borrow().node.clone();
            if node.borrow().type_ & PropertyNode::CONSTANT == 0 {
                break;
            }
            dst = if node.borrow().is_instance_class_property() { prototype.clone() } else { dst_or_constructor.clone() };

            if node.borrow().type_ & (PropertyNode::PRIVATE_GETTER | PropertyNode::PRIVATE_SETTER) != 0 {
                p = current.borrow().next.clone();
                continue;
            }

            if current.borrow().is_computed_class_field() {
                Self::emit_save_computed_field_name(&current, generator, &node);
            }

            if current.borrow().is_instance_class_field() && node.borrow().type_ & PropertyNode::PRIVATE_METHOD == 0 {
                let definitions = instance_element_definitions.as_deref_mut().expect("ASSERT(instanceElementDefinitions)");
                definitions.push(nodes_codegen_make_class_element_definition(&current.borrow()));
                p = current.borrow().next.clone();
                continue;
            }

            if current.borrow().is_static_class_element() {
                let definitions = static_element_definitions.as_deref_mut().expect("ASSERT(staticElementDefinitions)");
                definitions.push(nodes_codegen_make_class_element_definition(&current.borrow()));
                p = current.borrow().next.clone();
                continue;
            }

            Self::emit_put_constant_property(&current, generator, dst.clone(), &node);
            p = current.borrow().next.clone();
        }

        // Were there any get/set properties?
        if let Some(first_remaining) = p.clone() {
            // Build a list of getter/setter pairs to try to put them at the same time. If we encounter
            // a constant property by the same name as accessor or a computed property or a spread,
            // just emit everything as that may override previous values.
            let mut can_override_properties = false;

            let mut instance_map: NodesCodegenGetterSetterMap = Vec::new();
            let mut static_map: NodesCodegenGetterSetterMap = Vec::new();

            // Build a map, pairing get/set values together.
            let mut q = Some(first_remaining);
            while let Some(current) = q {
                let node = current.borrow().node.clone();
                let type_ = node.borrow().type_;
                if type_ & PropertyNode::COMPUTED != 0 || type_ & PropertyNode::SPREAD != 0 {
                    can_override_properties = true;
                    break;
                }

                let map = if node.borrow().is_static_class_property() { &mut static_map } else { &mut instance_map };
                let name = node.borrow().name.clone();
                if type_ & PropertyNode::CONSTANT != 0 {
                    if let Some(name) = &name {
                        if nodes_codegen_map_find(map, name).is_some() {
                            can_override_properties = true;
                            break;
                        }
                    }
                    q = current.borrow().next.clone();
                    continue;
                }

                // Duplicates are possible.
                let name = name.expect("acessor tem nome");
                match nodes_codegen_map_find(map, &name) {
                    None => map.push((name, (Some(node.clone()), None))),
                    Some(index) => {
                        let first = map[index].1 .0.clone().expect("primeiro do par");
                        if first.borrow().type_ == type_ {
                            first.borrow_mut().set_is_overridden_by_duplicate();
                            map[index].1 .0 = Some(node.clone());
                        } else {
                            if let Some(second) = &map[index].1 .1 {
                                second.borrow_mut().set_is_overridden_by_duplicate();
                            }
                            map[index].1 .1 = Some(node.clone());
                        }
                    }
                }
                q = current.borrow().next.clone();
            }

            // Iterate over the remaining properties in the list.
            while let Some(current) = p {
                let node = current.borrow().node.clone();
                dst = if node.borrow().is_instance_class_property() { prototype.clone() } else { dst_or_constructor.clone() };
                let dst_register = dst.clone().expect("destino da propriedade");

                if current.borrow().is_computed_class_field() {
                    Self::emit_save_computed_field_name(&current, generator, &node);
                }

                if node.borrow().type_ & (PropertyNode::PRIVATE_GETTER | PropertyNode::PRIVATE_SETTER) != 0 {
                    p = current.borrow().next.clone();
                    continue;
                }

                if current.borrow().is_instance_class_field() {
                    let definitions =
                        instance_element_definitions.as_deref_mut().expect("ASSERT(instanceElementDefinitions)");
                    debug_assert!(node.borrow().type_ & PropertyNode::CONSTANT != 0);
                    definitions.push(nodes_codegen_make_class_element_definition(&current.borrow()));
                    p = current.borrow().next.clone();
                    continue;
                }

                if current.borrow().is_static_class_element() {
                    let definitions = static_element_definitions.as_deref_mut().expect("ASSERT(staticElementDefinitions)");
                    definitions.push(nodes_codegen_make_class_element_definition(&current.borrow()));
                    p = current.borrow().next.clone();
                    continue;
                }

                let type_ = node.borrow().type_;
                let assign = node.borrow().assign.clone().expect("propriedade tem expressão");

                // Handle regular values.
                if type_ & PropertyNode::CONSTANT != 0 {
                    Self::emit_put_constant_property(&current, generator, dst.clone(), &node);
                    p = current.borrow().next.clone();
                    continue;
                } else if type_ & PropertyNode::SPREAD != 0 {
                    generator.emit_node_expression(dst.clone(), &assign);
                    p = current.borrow().next.clone();
                    continue;
                }

                let value = generator.emit_node_expression_no_dst(&assign).expect("emitNode devolve registro");
                if nodes_codegen_needs_home_object(&assign) {
                    emit_put_home_object(generator, &value, &dst_register);
                }

                let attributes: u32 = if node.borrow().is_class_property() {
                    crate::runtime::property_attribute::ACCESSOR | crate::runtime::property_attribute::DONT_ENUM
                } else {
                    crate::runtime::property_attribute::ACCESSOR
                };

                debug_assert!(type_ & (PropertyNode::GETTER | PropertyNode::SETTER) != 0);

                // This is a get/set property which may be overridden by a computed property or spread later.
                if can_override_properties {
                    // Computed accessors.
                    if type_ & PropertyNode::COMPUTED != 0 {
                        let expression = node.borrow().expression.clone().expect("acessor computado tem expressão");
                        let mut property_name = generator.emit_node_expression_no_dst(&expression);
                        if generator.should_set_function_name(&assign) {
                            let temporary = generator.new_temporary();
                            property_name = generator.emit_to_property_key(Some(temporary), property_name);
                            generator.emit_set_function_name(Some(value.clone()), property_name.clone());
                        }
                        if type_ & PropertyNode::GETTER != 0 {
                            generator.emit_put_getter_by_val(dst.clone(), property_name, attributes, Some(value));
                        } else {
                            generator.emit_put_setter_by_val(dst.clone(), property_name, attributes, Some(value));
                        }
                        p = current.borrow().next.clone();
                        continue;
                    }

                    let name = node.borrow().name.clone().expect("acessor tem nome");
                    if type_ & PropertyNode::GETTER != 0 {
                        generator.emit_put_getter_by_id(dst.clone(), &name, attributes, Some(value));
                    } else {
                        generator.emit_put_setter_by_id(dst.clone(), &name, attributes, Some(value));
                    }
                    p = current.borrow().next.clone();
                    continue;
                }

                // This is a get/set property pair.
                let map = if node.borrow().is_static_class_property() { &static_map } else { &instance_map };
                let name = node.borrow().name.clone().expect("acessor tem nome");
                let index = nodes_codegen_map_find(map, &name).expect("ASSERT(it != map.end())");
                let pair = map[index].1.clone();

                // Was this already generated as a part of its partner?
                let is_second = match &pair.1 {
                    Some(second) => std::rc::Rc::ptr_eq(second, &node),
                    None => false,
                };
                if is_second || node.borrow().is_overridden_by_duplicate {
                    p = current.borrow().next.clone();
                    continue;
                }

                // Generate the paired node now.
                let getter_reg: Option<crate::bytecompiler::bytecode_generator::RegisterRef>;
                let setter_reg: Option<crate::bytecompiler::bytecode_generator::RegisterRef>;
                let mut second_reg: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;

                if type_ & PropertyNode::GETTER != 0 {
                    getter_reg = Some(value.clone());
                    if let Some(second) = &pair.1 {
                        debug_assert!(second.borrow().type_ & PropertyNode::SETTER != 0);
                        let second_assign = second.borrow().assign.clone().expect("setter tem função");
                        setter_reg = generator.emit_node_expression_no_dst(&second_assign);
                        second_reg = setter_reg.clone();
                    } else {
                        setter_reg = generator.emit_load_js_value(None, crate::runtime::js_value::js_undefined());
                    }
                } else {
                    debug_assert!(type_ & PropertyNode::SETTER != 0);
                    setter_reg = Some(value.clone());
                    if let Some(second) = &pair.1 {
                        debug_assert!(second.borrow().type_ & PropertyNode::GETTER != 0);
                        let second_assign = second.borrow().assign.clone().expect("getter tem função");
                        getter_reg = generator.emit_node_expression_no_dst(&second_assign);
                        second_reg = getter_reg.clone();
                    } else {
                        getter_reg = generator.emit_load_js_value(None, crate::runtime::js_value::js_undefined());
                    }
                }

                if let Some(second) = &pair.1 {
                    let second_assign = second.borrow().assign.clone().expect("par tem função");
                    if nodes_codegen_needs_home_object(&second_assign) {
                        emit_put_home_object(generator, &second_reg.expect("segundo registrador"), &dst_register);
                    }
                }

                generator.emit_put_getter_setter(dst.clone(), &name, attributes, getter_reg, setter_reg);
                p = current.borrow().next.clone();
            }
        }

        dst_or_constructor
    }

    pub fn emit_put_constant_property(
        this: &crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyListNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        new_obj: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        node: &crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyNode>,
    ) {
        use crate::parser::nodes::PropertyNode;

        // Private fields are handled in a synthetic classFieldInitializer function, not here.
        debug_assert!(node.borrow().type_ & PropertyNode::PRIVATE_FIELD == 0);
        let position = *this.borrow().position();
        let assign = node.borrow().assign.clone().expect("propriedade constante tem expressão");

        if PropertyNode::is_underscore_proto_setter(generator.vm(), &node.borrow()) {
            let prototype = generator.emit_node_expression_no_dst(&assign);
            generator.emit_direct_set_prototype_of(
                crate::bytecompiler::bytecode_generator::InvalidPrototypeMode::Ignore,
                new_obj,
                prototype,
                &position,
                &position,
                &position,
            );
            return;
        }

        let should_set_function_name = generator.should_set_function_name(&assign);

        let mut property_name: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
        if node.borrow().name.is_none() {
            let temporary = generator.new_temporary();
            property_name = Some(temporary.clone());
            let expression = node.borrow().expression.clone().expect("nome computado tem expressão");
            if should_set_function_name {
                let key_source = generator.emit_node_expression_no_dst(&expression);
                generator.emit_to_property_key(Some(temporary), key_source);
            } else {
                generator.emit_node_expression(Some(temporary), &expression);
            }
        }

        let value = generator.emit_node_expression_no_dst(&assign).expect("emitNode devolve registro");
        if nodes_codegen_needs_home_object(&assign) {
            emit_put_home_object(generator, &value, new_obj.as_ref().expect("objeto de destino"));
        }

        if node.borrow().is_class_property() {
            debug_assert!(node.borrow().needs_super_binding);
            debug_assert!(node.borrow().type_ & PropertyNode::PRIVATE_SETTER == 0);
            debug_assert!(node.borrow().type_ & PropertyNode::PRIVATE_GETTER == 0);

            if node.borrow().type_ & PropertyNode::PRIVATE_METHOD != 0 {
                let name = node.borrow().name.clone().expect("método privado tem nome");
                let var = generator.variable(&name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                let scope_register = generator.scope_register();
                generator.emit_put_to_scope(
                    scope_register,
                    &var,
                    Some(value),
                    crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                    crate::runtime::get_put_info::InitializationMode::ConstInitialization,
                );
                return;
            }

            if let Some(name) = node.borrow().name.clone() {
                property_name = generator.emit_load_identifier(None, &name);
            }

            if should_set_function_name {
                generator.emit_set_function_name(Some(value.clone()), property_name.clone());
            }
            generator.emit_call_define_property(
                new_obj,
                property_name,
                Some(value),
                None,
                None,
                crate::bytecompiler::bytecode_generator::PROPERTY_CONFIGURABLE
                    | crate::bytecompiler::bytecode_generator::PROPERTY_WRITABLE,
                &position,
            );
            return;
        }

        let identifier = node.borrow().name.clone();
        if let Some(identifier) = identifier {
            debug_assert!(property_name.is_none());
            let optional_index = crate::runtime::identifier::parse_index_identifier(&identifier);
            match optional_index {
                None => {
                    generator.emit_direct_put_by_id(new_obj, &identifier, Some(value));
                }
                Some(index) => {
                    let property_name =
                        generator.emit_load_js_value(None, crate::runtime::js_value::js_number(index as f64));
                    generator.emit_direct_put_by_val(new_obj, property_name, Some(value));
                }
            }
            return;
        }

        if should_set_function_name {
            generator.emit_set_function_name(Some(value.clone()), property_name.clone());
        }
        generator.emit_direct_put_by_val(new_obj, property_name, Some(value));
    }

    pub fn emit_save_computed_field_name(
        _this: &crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyListNode>,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        node: &crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyNode>,
    ) {
        debug_assert!(node.borrow().is_computed_class_field());

        // The 'name' refers to a synthetic private name in the class scope, where the property key is saved for later use.
        let description = node.borrow().name.clone().expect("campo computado tem nome sintético");
        let var = generator.variable(&description, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        debug_assert!(var.local().is_none());

        let expression = node.borrow().expression.clone().expect("campo computado tem expressão");
        let property_expr = generator.emit_node_expression_no_dst(&expression);
        let temporary = generator.new_temporary();
        let property_name = generator.emit_to_property_key(Some(temporary), property_expr);

        if node.borrow().is_static_class_field() {
            let valid_property_name_label = generator.new_label();
            let prototype_name = generator.property_names().prototype.clone();
            let prototype_constant = generator.add_string_constant(&prototype_name);
            let prototype_string = generator
                .emit_load_js_value(None, crate::runtime::js_value::JSValue::from_js_string(prototype_constant));
            let comparison_dst = generator.new_temporary();
            let comparison = generator.emit_binary_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                Some(comparison_dst),
                prototype_string,
                property_name.clone(),
                crate::parser::result_type::OperandTypes::new(
                    crate::parser::result_type::ResultType::string_type(),
                    crate::parser::result_type::ResultType::string_type(),
                ),
            );
            generator.emit_jump_if_false_raw(&comparison.expect("emitBinaryOp devolve registro"), &valid_property_name_label);
            generator.emit_throw_type_error("Cannot declare a static field named 'prototype'");
            generator.emit_label(&valid_property_name_label);
        }

        let scope = generator.emit_resolve_scope(None, &var);
        generator.emit_put_to_scope(
            scope,
            &var,
            property_name,
            crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
            crate::runtime::get_put_info::InitializationMode::ConstInitialization,
        );
    }
}

// ------------------------------ BracketAccessorNode --------------------------------

/// `isNonIndexStringElement(ExpressionNode&)`.
fn nodes_codegen_is_non_index_string_element(element: &crate::parser::nodes::Expression) -> bool {
    match element {
        crate::parser::nodes::Expression::String(node) => {
            crate::runtime::identifier::parse_index_identifier(&node.borrow().value).is_none()
        }
        _ => false,
    }
}

/// `static_cast<StringNode*>(element)->value()`.
fn nodes_codegen_string_element_value(
    element: &crate::parser::nodes::Expression,
) -> crate::runtime::identifier::Identifier {
    match element {
        crate::parser::nodes::Expression::String(node) => node.borrow().value.clone(),
        _ => unreachable!("isNonIndexStringElement garante StringNode"),
    }
}

impl crate::parser::nodes::BracketAccessorNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if self.base_expr.is_super_node() {
            let final_dest = generator.final_destination(dst.as_ref(), None);
            let this_value = generator.ensure_this();
            let super_base = emit_super_base_for_callee(generator);

            if nodes_codegen_is_non_index_string_element(&self.subscript) {
                let id = nodes_codegen_string_element_value(&self.subscript);
                generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
                generator.emit_get_by_id_with_this(Some(final_dest.clone()), super_base, Some(this_value), &id);
            } else {
                let subscript = generator.emit_node_for_property(&self.subscript);
                generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
                generator.emit_get_by_val_with_this(Some(final_dest.clone()), super_base, Some(this_value), subscript);
            }

            generator.emit_profile_type_divots(
                Some(final_dest.clone()),
                &self.throwable.divot_start,
                &self.throwable.divot_end,
            );
            return Some(final_dest);
        }

        let final_dest = generator.final_destination(dst.as_ref(), None);

        let subscript_is_non_index_string = nodes_codegen_is_non_index_string_element(&self.subscript);
        let base = if subscript_is_non_index_string {
            generator.emit_node_expression_no_dst(&self.base_expr)
        } else {
            let subscript_is_pure = self.subscript.is_pure(generator);
            generator.emit_node_for_left_hand_side(&self.base_expr, self.subscript_has_assignments, subscript_is_pure)
        };

        if self.base_expr.base().is_optional_chain_base {
            generator.emit_optional_check(base.as_ref().expect("base avaliada"));
        }

        let ret;
        if subscript_is_non_index_string {
            generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
            let id = nodes_codegen_string_element_value(&self.subscript);
            ret = generator.emit_get_by_id(Some(final_dest.clone()), base, &id);
        } else {
            let property = generator.emit_node_for_property(&self.subscript);
            generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
            ret = generator.emit_get_by_val(Some(final_dest.clone()), base, property);
        }

        generator.emit_profile_type_divots(Some(final_dest), &self.throwable.divot_start, &self.throwable.divot_end);
        ret
    }
}

// ------------------------------ DotAccessorNode --------------------------------

impl crate::parser::nodes::DotAccessorNode {
    pub fn emit_bytecode(
        &self,
        this_node: &crate::parser::nodes::Expression,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let final_dest = generator.final_destination(dst.as_ref(), None);

        if generator.should_get_arguments_dot_length_fast(this_node) {
            return generator.emit_argument_count(Some(final_dest));
        }

        let base_is_super = self.base.base_expr.is_super_node();

        let base;
        if base_is_super {
            base = emit_super_base_for_callee(generator);
        } else {
            base = generator.emit_node_expression_no_dst(&self.base.base_expr);
            if self.base.base_expr.base().is_optional_chain_base {
                generator.emit_optional_check(base.as_ref().expect("base avaliada"));
            }
        }

        generator.emit_expression_info(&self.throwable.divot, &self.throwable.divot_start, &self.throwable.divot_end);
        let ret = self.base.emit_get_property_value(generator, Some(final_dest.clone()), base);

        generator.emit_profile_type_divots(Some(final_dest), &self.throwable.divot_start, &self.throwable.divot_end);
        ret
    }
}

impl crate::parser::nodes::BaseDotNode {
    /// `BaseDotNode::emitGetPropertyValue(generator, dst, base, thisValue)`: a sobrecarga de três
    /// argumentos do C++ (a de quatro) recebe o `RefPtr<RegisterID>& thisValue` como `&mut Option<..>`.
    pub fn emit_get_property_value_with_this(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_value: &mut Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let position = *self.position();
        if self.is_private_member() {
            let identifier_name = self.ident.clone();
            let private_traits = generator.get_private_traits(&identifier_name);
            if private_traits.is_method() {
                let var = generator.variable(&identifier_name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                let scope = generator.emit_resolve_scope(None, &var);
                debug_assert!(scope.is_some()); // Private names are always captured.
                let temporary = generator.new_temporary();
                let private_brand_symbol =
                    generator.emit_get_private_brand(Some(temporary), scope.clone(), private_traits.is_static());
                generator.emit_check_private_brand(base, private_brand_symbol, private_traits.is_static());

                return generator.emit_get_from_scope(
                    dst,
                    scope,
                    &var,
                    crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
                );
            }

            if private_traits.is_getter() {
                let var = generator.variable(&identifier_name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                let scope = generator.emit_resolve_scope(None, &var);
                debug_assert!(scope.is_some()); // Private names are always captured.
                let temporary = generator.new_temporary();
                let private_brand_symbol =
                    generator.emit_get_private_brand(Some(temporary), scope.clone(), private_traits.is_static());
                generator.emit_check_private_brand(base.clone(), private_brand_symbol, private_traits.is_static());

                let temporary = generator.new_temporary();
                let getter_setter_obj = generator.emit_get_from_scope(
                    Some(temporary),
                    scope,
                    &var,
                    crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
                );
                let temporary = generator.new_temporary();
                let get_private_name = generator.property_names().builtin_names().get_private_name().clone();
                let getter_function = generator
                    .emit_direct_get_by_id(Some(temporary), getter_setter_obj, &get_private_name)
                    .expect("emitDirectGetById devolve registro");
                let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(generator, None, 0);
                generator.move_register(args.this_register().as_ref(), &base.expect("base avaliada"));
                return generator.emit_call(
                    dst,
                    Some(getter_function.clone()),
                    crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                    &mut args,
                    &position,
                    &position,
                    &position,
                    crate::bytecompiler::bytecode_generator::DebuggableCall::Yes,
                );
            }

            if private_traits.is_setter() {
                // We need to perform brand check to follow the spec
                let var = generator.variable(&identifier_name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                let scope = generator.emit_resolve_scope(None, &var);
                debug_assert!(scope.is_some()); // Private names are always captured.
                let temporary = generator.new_temporary();
                let private_brand_symbol =
                    generator.emit_get_private_brand(Some(temporary), scope, private_traits.is_static());
                generator.emit_check_private_brand(base, private_brand_symbol, private_traits.is_static());
                generator.emit_throw_type_error("Trying to access an undefined private getter");
                return dst;
            }

            debug_assert!(private_traits.is_field());
            let var = generator.variable(&self.ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            debug_assert!(var.local().is_none(), "Private Field names must be stored in captured variables");

            let scope = generator.emit_resolve_scope(None, &var);
            debug_assert!(scope.is_some()); // Private names are always captured.
            let private_name = generator.new_temporary();
            generator.emit_get_from_scope(
                Some(private_name.clone()),
                scope,
                &var,
                crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
            );
            return generator.emit_get_private_name(dst, base, Some(private_name));
        }

        if self.base_expr.is_super_node() {
            if this_value.is_none() {
                *this_value = Some(generator.ensure_this());
            }
            return generator.emit_get_by_id_with_this(dst, base, this_value.clone(), &self.ident);
        }

        generator.emit_get_by_id(dst, base, &self.ident)
    }
}
