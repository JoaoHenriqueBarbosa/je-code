macro_rules! ast_builder_part2 {
    () => {
// Segunda fatia do `impl TreeBuilder for ASTBuilder` (`ASTBuilder.h`, de `createExportAllDeclaration` até o
// fim da classe, linha 1229). Incluída por `include!` dentro do `impl`: compartilha os `use` de
// `ast_builder.rs`; o que ele não importa vai por caminho completo. Os auxiliares que não são do trait
// (`array_pattern_of`, `object_pattern_of`, `make_binary_node`) ficam no `impl ASTBuilder` da parte 3.

fn create_export_all_declaration(&mut self, location: &JSTokenLocation, module_name: Link<ModuleNameNode>, import_attributes_list: Link<ImportAttributesListNode>) -> Option<Statement> {
    let node = crate::parser::nodes::ExportAllDeclarationNode::new(location, module_name.get().clone(), import_attributes_list.opt());
    Some(Statement::ExportAllDeclaration(make(node)))
}

fn create_export_default_declaration(&mut self, location: &JSTokenLocation, declaration: Option<Statement>, local_name: &Identifier) -> Option<Statement> {
    let node = crate::parser::nodes::ExportDefaultDeclarationNode::new(location, non_null(declaration), local_name.clone());
    Some(Statement::ExportDefaultDeclaration(make(node)))
}

fn create_export_local_declaration(&mut self, location: &JSTokenLocation, declaration: Option<Statement>) -> Option<Statement> {
    let node = crate::parser::nodes::ExportLocalDeclarationNode::new(location, non_null(declaration));
    Some(Statement::ExportLocalDeclaration(make(node)))
}

fn create_export_named_declaration(&mut self, location: &JSTokenLocation, export_specifier_list: Link<ExportSpecifierListNode>, module_name: Link<ModuleNameNode>, import_attributes_list: Link<ImportAttributesListNode>) -> Option<Statement> {
    let node = crate::parser::nodes::ExportNamedDeclarationNode::new(location, export_specifier_list.get().clone(), module_name.opt(), import_attributes_list.opt());
    Some(Statement::ExportNamedDeclaration(make(node)))
}

fn create_export_specifier(&mut self, location: &JSTokenLocation, local_name: &Identifier, exported_name: &Identifier) -> Link<ExportSpecifierNode> {
    Link::new(ExportSpecifierNode::new(location, local_name.clone(), exported_name.clone()))
}

fn create_export_specifier_list(&mut self) -> Link<ExportSpecifierListNode> {
    Link::new(ExportSpecifierListNode::default())
}

fn append_export_specifier(&mut self, specifier_list: &Link<ExportSpecifierListNode>, specifier: Link<ExportSpecifierNode>) {
    specifier_list.get().borrow_mut().append(specifier.get().clone());
}

fn append_statement(&mut self, elements: &Link<SourceElements>, statement: Option<Statement>) {
    elements.get().borrow_mut().append(non_null(statement));
}

fn create_comma_expr(&mut self, location: &JSTokenLocation, node: Option<Expression>) -> Option<Expression> {
    Some(Expression::Comma(make(crate::parser::nodes::CommaNode::new(location, non_null(node)))))
}

fn append_to_comma_expr(&mut self, location: &JSTokenLocation, tail: Option<Expression>, next: Option<Expression>) -> Option<Expression> {
    // Invariante: `tail` sempre nasceu de `create_comma_expr` (ASSERT no C++).
    let Some(Expression::Comma(tail)) = tail else {
        panic!("ASSERT: tail->isCommaNode()");
    };
    let new_tail = make(crate::parser::nodes::CommaNode::new(location, non_null(next)));
    tail.borrow_mut().next = Some(Rc::clone(&new_tail));
    Some(Expression::Comma(new_tail))
}

fn eval_count(&self) -> i32 {
    self.eval_count
}

fn append_binary_expression_info(&mut self, operand_stack_depth: &mut i32, current: Option<Expression>, expr_start: JSTextPosition, lhs: JSTextPosition, rhs: JSTextPosition, has_assignments: bool) {
    *operand_stack_depth += 1;
    self.binary_operand_stack.push((current, BinaryOpInfo::new(expr_start, lhs, rhs, has_assignments)));
}

// Logic to handle datastructures used during parsing of binary expressions
fn operator_stack_pop(&mut self, operator_stack_depth: &mut i32) {
    *operator_stack_depth -= 1;
    self.binary_operator_stack.pop();
}

fn operator_stack_should_reduce(&mut self, precedence: i32) -> bool {
    let last = non_null(self.binary_operator_stack.last().copied());
    // If the current precedence of the operator stack is the same to the one of the given operator,
    // it depends on the associative whether we reduce the stack.
    // If the operator is right associative, we should not reduce the stack right now.
    if precedence == last.1 {
        return (last.0 & crate::parser::parser_tokens::RIGHT_ASSOCIATIVE_BINARY_OP_TOKEN_FLAG as i32) == 0;
    }
    precedence < last.1
}

fn get_from_operand_stack(&mut self, i: i32) -> (Option<Expression>, BinaryOpInfo) {
    self.binary_operand_stack[(self.binary_operand_stack.len() as i32 + i) as usize].clone()
}

fn shrink_operand_stack_by(&mut self, operand_stack_depth: &mut i32, amount: i32) {
    *operand_stack_depth -= amount;
    self.binary_operand_stack.truncate(self.binary_operand_stack.len() - amount as usize);
}

fn append_binary_operation(&mut self, location: &JSTokenLocation, operand_stack_depth: &mut i32, _operator_stack_depth: &mut i32, lhs: (Option<Expression>, BinaryOpInfo), rhs: (Option<Expression>, BinaryOpInfo)) {
    *operand_stack_depth += 1;
    let op = non_null(self.binary_operator_stack.last().copied()).0;
    let node = self.make_binary_node(location, op, &lhs, &rhs);
    self.binary_operand_stack.push((node, BinaryOpInfo::from_operands(&lhs.1, &rhs.1)));
}

fn operator_stack_append(&mut self, operator_stack_depth: &mut i32, op: i32, precedence: i32) {
    *operator_stack_depth += 1;
    self.binary_operator_stack.push((op, precedence));
}

fn pop_operand_stack(&mut self, _operand_stack_depth: &mut i32) -> Option<Expression> {
    non_null(self.binary_operand_stack.pop()).0
}

fn append_unary_token(&mut self, token_stack_depth: &mut i32, type_: i32, start: JSTextPosition) {
    *token_stack_depth += 1;
    self.unary_token_stack.push((type_, start));
}

fn unary_token_stack_last_type(&mut self, _token_stack_depth: &mut i32) -> i32 {
    non_null(self.unary_token_stack.last()).0
}

fn unary_token_stack_last_start(&mut self, _token_stack_depth: &mut i32) -> JSTextPosition {
    non_null(self.unary_token_stack.last()).1
}

fn unary_token_stack_remove_last(&mut self, token_stack_depth: &mut i32) {
    *token_stack_depth -= 1;
    self.unary_token_stack.pop();
}

fn unary_token_stack_depth(&self) -> i32 {
    self.unary_token_stack.len() as i32
}

fn set_unary_token_stack_depth(&mut self, old_depth: i32) {
    self.unary_token_stack.truncate(old_depth as usize);
}

fn assignment_stack_append(&mut self, assignment_stack_depth: &mut i32, node: Option<Expression>, start: JSTextPosition, divot: JSTextPosition, assignment_count: i32, op: Operator) {
    *assignment_stack_depth += 1;
    debug_assert!(start.offset >= start.line_start_offset);
    debug_assert!(divot.offset >= divot.line_start_offset);
    self.assignment_info_stack.push(AssignmentInfo::new(node, start, divot, assignment_count, op));
}

fn create_assignment(&mut self, location: &JSTokenLocation, assignment_stack_depth: &mut i32, rhs: Option<Expression>, initial_assignment_count: i32, current_assignment_count: i32, last_token_end: JSTextPosition) -> Option<Expression> {
    let info = non_null(self.assignment_info_stack.last().cloned());
    self.check_arguments_length_modification(&info.node);
    let result = self.make_assign_node(location, info.node, info.op, rhs, info.init_assignments != initial_assignment_count, info.init_assignments != current_assignment_count, info.start, info.divot + 1, last_token_end);
    self.assignment_info_stack.pop();
    *assignment_stack_depth -= 1;
    result
}

fn get_type(&self, property: &Link<PropertyNode>) -> PropertyNodeType {
    property.get().borrow().type_
}

fn is_underscore_proto_setter(&self, property: &Link<PropertyNode>) -> bool {
    PropertyNode::is_underscore_proto_setter(&self.vm, &property.get().borrow())
}

fn is_resolve(&self, expr: &Option<Expression>) -> bool {
    non_null_ref(expr).is_resolve_node()
}

fn create_destructuring_assignment(&mut self, location: &JSTokenLocation, pattern: Option<DestructuringPatternNode>, initializer: Option<Expression>) -> Option<Expression> {
    Some(Expression::DestructuringAssignment(make(DestructuringAssignmentNode::new(location, non_null(pattern), initializer))))
}

fn create_array_pattern(&mut self, _location: &JSTokenLocation) -> Option<DestructuringPatternNode> {
    Some(DestructuringPatternNode::ArrayPattern(make(crate::parser::nodes::ArrayPatternNode::new())))
}

fn append_array_pattern_skip_entry(&mut self, node: &Option<DestructuringPatternNode>, location: &JSTokenLocation) {
    Self::array_pattern_of(node).borrow_mut().append_index(crate::parser::nodes::ArrayPatternBindingType::Elision, location, None, None);
}

fn append_array_pattern_entry(&mut self, node: &Option<DestructuringPatternNode>, location: &JSTokenLocation, pattern: Option<DestructuringPatternNode>, default_value: Option<Expression>) {
    Self::array_pattern_of(node).borrow_mut().append_index(crate::parser::nodes::ArrayPatternBindingType::Element, location, pattern.clone(), default_value.clone());
    if default_value.is_some() {
        Self::try_infer_name_in_pattern(non_null_ref(&pattern), &default_value);
    }
}

fn append_array_pattern_rest_entry(&mut self, node: &Option<DestructuringPatternNode>, location: &JSTokenLocation, pattern: Option<DestructuringPatternNode>) {
    Self::array_pattern_of(node).borrow_mut().append_index(crate::parser::nodes::ArrayPatternBindingType::RestElement, location, pattern, None);
}

fn finish_array_pattern(&mut self, node: &Option<DestructuringPatternNode>, divot_start: JSTextPosition, divot: JSTextPosition, divot_end: JSTextPosition) {
    Self::set_exception_location(&mut Self::array_pattern_of(node).borrow_mut().throwable, divot_start, divot, divot_end);
}

fn create_object_pattern(&mut self, _location: &JSTokenLocation) -> Option<DestructuringPatternNode> {
    Some(DestructuringPatternNode::ObjectPattern(make(crate::parser::nodes::ObjectPatternNode::new())))
}

fn append_object_pattern_entry(&mut self, node: &Option<DestructuringPatternNode>, location: &JSTokenLocation, was_string: bool, identifier: &Identifier, pattern: Option<DestructuringPatternNode>, default_value: Option<Expression>) {
    Self::object_pattern_of(node).borrow_mut().append_entry(location, identifier.clone(), was_string, non_null(pattern.clone()), default_value.clone(), crate::parser::nodes::ObjectPatternBindingType::Element);
    if default_value.is_some() {
        Self::try_infer_name_in_pattern(non_null_ref(&pattern), &default_value);
    }
}

fn append_object_pattern_computed_entry(&mut self, vm: &VM, node: &Option<DestructuringPatternNode>, location: &JSTokenLocation, property_expression: Option<Expression>, pattern: Option<DestructuringPatternNode>, default_value: Option<Expression>) {
    Self::object_pattern_of(node).borrow_mut().append_entry_with_expression(vm, location, non_null(property_expression), non_null(pattern.clone()), default_value.clone(), crate::parser::nodes::ObjectPatternBindingType::Element);
    if default_value.is_some() {
        Self::try_infer_name_in_pattern(non_null_ref(&pattern), &default_value);
    }
}

fn append_object_pattern_rest_entry(&mut self, vm: &VM, node: &Option<DestructuringPatternNode>, location: &JSTokenLocation, pattern: Option<DestructuringPatternNode>) {
    // `appendEntry(vm, location, nullptr, pattern, nullptr, RestElement)`: com a expressão nula, o
    // `ObjectPatternNode::appendEntry` do C++ grava `nullIdentifier`, a expressão nula e `wasString` falso, que é
    // exatamente a entrada que a sobrecarga por identificador monta com `nullIdentifier` e `false`.
    let null_identifier = vm.property_names.null_identifier.clone();
    Self::object_pattern_of(node).borrow_mut().append_entry(location, null_identifier, false, non_null(pattern), None, crate::parser::nodes::ObjectPatternBindingType::RestElement);
}

fn set_contains_object_rest_element(&mut self, node: &Option<DestructuringPatternNode>, contains_rest_element: bool) {
    Self::object_pattern_of(node).borrow_mut().contains_rest_element = contains_rest_element;
}

fn set_contains_computed_property(&mut self, node: &Option<DestructuringPatternNode>, contains_computed_property: bool) {
    Self::object_pattern_of(node).borrow_mut().contains_computed_property = contains_computed_property;
}

fn finish_object_pattern(&mut self, node: &Option<DestructuringPatternNode>, divot_start: JSTextPosition, divot: JSTextPosition, divot_end: JSTextPosition) {
    Self::set_exception_location(&mut Self::object_pattern_of(node).borrow_mut().throwable, divot_start, divot, divot_end);
}

fn create_binding_location(&mut self, _location: &JSTokenLocation, bound_property: &Identifier, start: JSTextPosition, end: JSTextPosition, context: AssignmentContext) -> Option<DestructuringPatternNode> {
    if matches!(context, AssignmentContext::AwaitUsingDeclarationStatement) {
        self.uses_await();
    }
    Some(DestructuringPatternNode::Binding(make(crate::parser::nodes::BindingNode::new(bound_property.clone(), start, end, context))))
}

fn create_rest_parameter(&mut self, pattern: Option<DestructuringPatternNode>, num_parameters_to_skip: usize) -> Option<DestructuringPatternNode> {
    Some(DestructuringPatternNode::RestParameter(make(crate::parser::nodes::RestParameterNode::new(non_null(pattern), num_parameters_to_skip as u32))))
}

fn create_assignment_element(&mut self, assignment_target: &Option<Expression>, start: JSTextPosition, end: JSTextPosition) -> Option<DestructuringPatternNode> {
    self.check_arguments_length_modification(assignment_target);
    Some(DestructuringPatternNode::AssignmentElement(make(crate::parser::nodes::AssignmentElementNode::new(non_null(assignment_target.clone()), start, end))))
}

fn set_end_offset<N: TreeNodeHandle>(&mut self, node: &N, offset: i32) {
    node.set_end_offset(offset);
}

fn end_offset<N: TreeNodeHandle>(&mut self, node: &N) -> i32 {
    node.end_offset()
}

fn set_start_offset<N: TreeNodeHandle>(&mut self, node: &N, offset: i32) {
    node.set_start_offset(offset);
}

fn breakpoint_location<N: TreeNodeHandle>(&mut self, node: &N) -> JSTextPosition {
    node.breakpoint_location()
}

fn propagate_arguments_use(&mut self) {
    self.uses_arguments();
}

fn has_arguments_feature(&self) -> bool {
    (self.scope.features & ARGUMENTS_FEATURE as i32) != 0
}

    };
}
