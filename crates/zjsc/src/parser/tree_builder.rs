//! O `TreeBuilder` do `Parser`.
//!
//! No C++ o `Parser` é um template sobre o `TreeBuilder` (`ASTBuilder` ou `SyntaxChecker`), e cada
//! `typedef` do construtor (`Expression`, `Statement`, `Property`, ...) é um tipo associado aqui
//! (CONVENTIONS, modelo de dados, item 3). Os métodos são os que o `SyntaxChecker.h` define, com as
//! assinaturas do `ASTBuilder.h` onde o `SyntaxChecker` só traz `int` no lugar do tipo do nó.
//!
//! Desvios mecânicos em relação ao C++, todos por falta de sobrecarga em Rust:
//!
//! - Sobrecargas viram nomes distintos (`create_array_elisions`, `create_array_elements`,
//!   `create_array_elisions_elements`, e assim por diante). A forma é sempre a do `ASTBuilder.h`.
//! - `const Identifier*` que o `ASTBuilder` desreferencia sem teste vira `&Identifier`; o que admite
//!   ponteiro nulo vira `Option<&Identifier>`.
//! - `createImportDeclaration` leva o `ImportType` e `createAsyncFunctionBody` leva o `SourceParseMode`:
//!   são as assinaturas que o `Parser.cpp` chama (o `SyntaxChecker.h` está defasado nos dois, e como o
//!   `Parser` só instancia esses ramos com o `ASTBuilder`, o C++ nunca o notou).
//! - `BinaryExprContext` e `UnaryExprContext` (guardas RAII que salvam e zeram o topo da pilha e o
//!   restauram no destrutor) viram um par `begin_*_context` / `end_*_context`, com o estado salvo como
//!   tipo associado. Quem abre fecha no mesmo escopo, em todo caminho de saída.
//! - Os `Node*` que `setEndOffset`, `endOffset`, `setStartOffset` e `breakpointLocation` recebem viram
//!   um parâmetro genérico com o trait `TreeNodeHandle`.
//!
//! `createConstStatement` e `appendConstDecl` do `SyntaxChecker.h` não existem no `ASTBuilder.h` e o
//! `Parser.cpp` não os chama; ficam só como métodos próprios do `SyntaxChecker`.

use crate::parser::nodes::{
    AssignmentContext, ClassElementTag, DotType, FunctionStack, Operator, PropertyNodeType,
};
use crate::parser::nodes::{DefineFieldType, ImportType};
use crate::parser::lexer::LexerFlagSet;
use crate::parser::parser::InferName;
use crate::parser::parser_arena::ParserArena;
use crate::parser::parser_function_info::{ParserClassInfo, ParserFunctionInfo};
use crate::parser::parser_modes::{LexicallyScopedFeatures, SourceParseMode, SuperBinding};
use crate::parser::parser_tokens::{JSTextPosition, JSTokenLocation};
use crate::parser::variable_environment::VariableEnvironment;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::vm::VM;

/// O que um tipo de resultado do construtor precisa: cópia barata, valor nulo (o `0`/`nullptr` do C++
/// que o `Parser` testa com `!x`) e comparação com ele.
pub trait TreeNode: Clone + Default + PartialEq {}
impl<T: Clone + Default + PartialEq> TreeNode for T {}

/// O `Node*` que o `ASTBuilder` recebe em `setEndOffset`, `endOffset`, `setStartOffset` e
/// `breakpointLocation`.
pub trait TreeNodeHandle {
    fn set_end_offset(&self, offset: i32);
    fn end_offset(&self) -> i32;
    fn set_start_offset(&self, offset: i32);
    fn breakpoint_location(&self) -> JSTextPosition;
}

pub trait TreeBuilder: Sized {
    type Expression: TreeNode + TreeNodeHandle;
    type SourceElements: TreeNode;
    type Arguments: TreeNode;
    type Comma: TreeNode + Into<Self::Expression>;
    type Property: TreeNode;
    type PropertyList: TreeNode;
    type ElementList: TreeNode;
    type ArgumentsList: TreeNode;
    type TemplateExpressionList: TreeNode;
    type TemplateString: TreeNode;
    type TemplateStringList: TreeNode;
    type TemplateLiteral: TreeNode + Into<Self::Expression>;
    type FormalParameterList: TreeNode;
    type FunctionBody: TreeNode + TreeNodeHandle;
    type ClassExpression: TreeNode + Into<Self::Expression>;
    type ModuleName: TreeNode;
    type ImportSpecifier: TreeNode;
    type ImportSpecifierList: TreeNode;
    type ImportAttributesList: TreeNode;
    type ExportSpecifier: TreeNode;
    type ExportSpecifierList: TreeNode;
    type Statement: TreeNode + TreeNodeHandle;
    type ClauseList: TreeNode;
    type Clause: TreeNode + TreeNodeHandle;
    type BinaryOperand: TreeNode;
    type DestructuringPattern: TreeNode;
    type ArrayPattern: TreeNode + Into<Self::DestructuringPattern>;
    type ObjectPattern: TreeNode + Into<Self::DestructuringPattern>;
    type RestPattern: TreeNode + Into<Self::DestructuringPattern>;
    /// `DefineFieldNode*` (o `SyntaxChecker` devolve `int`).
    type DefineField: TreeNode + Into<Self::Statement>;
    /// Estado que `SyntaxChecker::BinaryExprContext` / `ASTBuilder::BinaryExprContext` salvam.
    type BinaryExprContext;
    /// Estado que `SyntaxChecker::UnaryExprContext` / `ASTBuilder::UnaryExprContext` salvam.
    type UnaryExprContext;

    const CREATES_AST: bool;
    const NEEDS_FREE_VARIABLE_INFO: bool;
    const CAN_USE_FUNCTION_CACHE: bool;
    const DONT_BUILD_KEYWORDS: LexerFlagSet;
    const DONT_BUILD_STRINGS: LexerFlagSet;

    /// Construtor da guarda `BinaryExprContext`: salva o topo e o zera.
    fn begin_binary_expr_context(&mut self) -> Self::BinaryExprContext;
    /// Destrutor da guarda `BinaryExprContext`: restaura o topo salvo.
    fn end_binary_expr_context(&mut self, saved: Self::BinaryExprContext);
    /// Construtor da guarda `UnaryExprContext`.
    fn begin_unary_expr_context(&mut self) -> Self::UnaryExprContext;
    /// Destrutor da guarda `UnaryExprContext`.
    fn end_unary_expr_context(&mut self, saved: Self::UnaryExprContext);

    fn create_source_elements(&mut self) -> Self::SourceElements;
    fn make_static_block_function_call_node(&mut self, location: &JSTokenLocation, func: Self::Expression, divot: JSTextPosition, divot_start: JSTextPosition, divot_end: JSTextPosition) -> Self::Expression;
    #[allow(clippy::too_many_arguments)]
    fn make_function_call_node(&mut self, location: &JSTokenLocation, func: Self::Expression, previous_base_was_super: bool, args: Self::Arguments, divot_start: JSTextPosition, divot: JSTextPosition, divot_end: JSTextPosition, call_or_apply_child_depth: usize, is_optional_call: bool) -> Self::Expression;
    fn create_comma_expr(&mut self, location: &JSTokenLocation, node: Self::Expression) -> Self::Comma;
    fn append_to_comma_expr(&mut self, location: &JSTokenLocation, tail: Self::Comma, next: Self::Expression) -> Self::Comma;
    #[allow(clippy::too_many_arguments)]
    fn make_assign_node(&mut self, location: &JSTokenLocation, left: Self::Expression, op: Operator, right: Self::Expression, left_has_assignments: bool, right_has_assignments: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn make_prefix_node(&mut self, location: &JSTokenLocation, expr: Self::Expression, op: Operator, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn make_postfix_node(&mut self, location: &JSTokenLocation, expr: Self::Expression, op: Operator, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn make_type_of_node(&mut self, location: &JSTokenLocation, expr: Self::Expression, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn make_delete_node(&mut self, location: &JSTokenLocation, expr: Self::Expression, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn make_negate_node(&mut self, location: &JSTokenLocation, expr: Self::Expression) -> Self::Expression;
    fn make_bitwise_not_node(&mut self, location: &JSTokenLocation, expr: Self::Expression) -> Self::Expression;
    fn create_logical_not(&mut self, location: &JSTokenLocation, expr: Self::Expression) -> Self::Expression;
    fn create_unary_plus(&mut self, location: &JSTokenLocation, expr: Self::Expression) -> Self::Expression;
    fn create_void(&mut self, location: &JSTokenLocation, expr: Self::Expression) -> Self::Expression;
    #[allow(clippy::too_many_arguments)]
    fn create_import_expr(&mut self, location: &JSTokenLocation, expr: Self::Expression, option: Self::Expression, deferred: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn create_this_expr(&mut self, location: &JSTokenLocation) -> Self::Expression;
    fn create_super_expr(&mut self, location: &JSTokenLocation) -> Self::Expression;
    fn create_new_target_expr(&mut self, location: &JSTokenLocation) -> Self::Expression;
    fn create_import_meta_expr(&mut self, location: &JSTokenLocation, expr: Self::Expression) -> Self::Expression;
    fn is_meta_property(&mut self, expr: &Self::Expression) -> bool;
    fn is_new_target(&mut self, expr: &Self::Expression) -> bool;
    fn is_import_meta(&mut self, expr: &Self::Expression) -> bool;
    fn create_resolve(&mut self, location: &JSTokenLocation, ident: &Identifier, start: JSTextPosition, end: JSTextPosition, need_to_check_uses_arguments: bool) -> Self::Expression;
    fn create_private_identifier_node(&mut self, location: &JSTokenLocation, ident: &Identifier) -> Self::Expression;
    fn create_object_literal(&mut self, location: &JSTokenLocation) -> Self::Expression;
    fn create_object_literal_with_properties(&mut self, location: &JSTokenLocation, properties: Self::PropertyList) -> Self::Expression;
    /// `createArray(location, int elisions)`.
    fn create_array_elisions(&mut self, location: &JSTokenLocation, elisions: i32) -> Self::Expression;
    /// `createArray(location, ElementNode* elems)`.
    fn create_array_elements(&mut self, location: &JSTokenLocation, elems: Self::ElementList) -> Self::Expression;
    /// `createArray(location, int elisions, ElementNode* elems)`.
    fn create_array_elisions_elements(&mut self, location: &JSTokenLocation, elisions: i32, elems: Self::ElementList) -> Self::Expression;
    fn create_double_expr(&mut self, location: &JSTokenLocation, d: f64) -> Self::Expression;
    fn create_integer_expr(&mut self, location: &JSTokenLocation, d: f64) -> Self::Expression;
    fn create_big_int(&mut self, location: &JSTokenLocation, big_int: &Identifier, radix: u8) -> Self::Expression;
    fn create_string(&mut self, location: &JSTokenLocation, string: &Identifier) -> Self::Expression;
    fn create_boolean(&mut self, location: &JSTokenLocation, b: bool) -> Self::Expression;
    fn create_null(&mut self, location: &JSTokenLocation) -> Self::Expression;
    #[allow(clippy::too_many_arguments)]
    fn create_bracket_access(&mut self, location: &JSTokenLocation, base: Self::Expression, property: Self::Expression, property_has_assignments: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    #[allow(clippy::too_many_arguments)]
    fn create_dot_access(&mut self, location: &JSTokenLocation, base: Self::Expression, property: &Identifier, type_: DotType, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn create_reg_exp(&mut self, location: &JSTokenLocation, pattern: &Identifier, flags: &Identifier, start: JSTextPosition, skip_syntax_check: bool) -> Self::Expression;
    /// `createNewExpr(location, expr, arguments, start, divot, end)`.
    #[allow(clippy::too_many_arguments)]
    fn create_new_expr(&mut self, location: &JSTokenLocation, expr: Self::Expression, arguments: Self::Arguments, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    /// `createNewExpr(location, expr, start, divot, end)`.
    fn create_new_expr_no_arguments(&mut self, location: &JSTokenLocation, expr: Self::Expression, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn create_optional_chain(&mut self, location: &JSTokenLocation, base: Self::Expression, expr: Self::Expression, is_outermost: bool) -> Self::Expression;
    fn create_conditional_expr(&mut self, location: &JSTokenLocation, condition: Self::Expression, lhs: Self::Expression, rhs: Self::Expression) -> Self::Expression;
    #[allow(clippy::too_many_arguments)]
    fn create_assign_resolve(&mut self, location: &JSTokenLocation, ident: &Identifier, rhs: Self::Expression, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition, assignment_context: AssignmentContext) -> Self::Expression;
    fn create_empty_var_expression(&mut self, location: &JSTokenLocation, ident: &Identifier) -> Self::Expression;
    fn create_empty_let_expression(&mut self, location: &JSTokenLocation, ident: &Identifier) -> Self::Expression;
    /// `createYield(location)`.
    fn create_yield(&mut self, location: &JSTokenLocation) -> Self::Expression;
    /// `createYield(location, argument, delegate, start, divot, end)`.
    #[allow(clippy::too_many_arguments)]
    fn create_yield_argument(&mut self, location: &JSTokenLocation, argument: Self::Expression, delegate: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn create_await(&mut self, location: &JSTokenLocation, argument: Self::Expression, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    #[allow(clippy::too_many_arguments)]
    fn create_class_expr(&mut self, location: &JSTokenLocation, class_info: &ParserClassInfo<Self>, class_head_environment: VariableEnvironment, class_environment: VariableEnvironment, constructor: Self::Expression, parent_class: Self::Expression, class_elements: Self::PropertyList, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::ClassExpression;
    fn create_function_expr(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<Self>) -> Self::Expression;
    fn create_generator_function_body(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<Self>, name: &Identifier) -> Self::Expression;
    fn create_async_function_body(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<Self>, parse_mode: SourceParseMode, name: &Identifier) -> Self::Expression;
    #[allow(clippy::too_many_arguments)]
    fn create_function_metadata(&mut self, start_location: &JSTokenLocation, end_location: &JSTokenLocation, start_column: u32, end_column: u32, function_start: u32, function_name_start: i32, parameters_start: i32, implementation_visibility: ImplementationVisibility, lexically_scoped_features: LexicallyScopedFeatures, constructor_kind: ConstructorKind, super_binding: SuperBinding, parameter_count: u32, mode: SourceParseMode, is_arrow_function_body_expression: bool) -> Self::FunctionBody;
    fn create_arrow_function_expr(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<Self>) -> Self::Expression;
    fn create_method_definition(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<Self>) -> Self::Expression;
    fn set_function_name_start(&mut self, body: &Self::FunctionBody, function_name_start: i32);
    fn create_arguments(&mut self) -> Self::Arguments;
    fn create_arguments_with_list(&mut self, args: Self::ArgumentsList, has_assignments: bool) -> Self::Arguments;
    fn create_spread_expression(&mut self, location: &JSTokenLocation, expression: Self::Expression, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn create_object_spread_expression(&mut self, location: &JSTokenLocation, expression: Self::Expression, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;
    fn create_template_string(&mut self, location: &JSTokenLocation, cooked: Option<&Identifier>, raw: Option<&Identifier>) -> Self::TemplateString;
    /// `createTemplateStringList(templateString)`.
    fn create_template_string_list(&mut self, template_string: Self::TemplateString) -> Self::TemplateStringList;
    /// `createTemplateStringList(templateStringList, templateString)`.
    fn create_template_string_list_append(&mut self, template_string_list: Self::TemplateStringList, template_string: Self::TemplateString) -> Self::TemplateStringList;
    /// `createTemplateExpressionList(expression)`.
    fn create_template_expression_list(&mut self, expression: Self::Expression) -> Self::TemplateExpressionList;
    /// `createTemplateExpressionList(templateExpressionList, expression)`.
    fn create_template_expression_list_append(&mut self, template_expression_list: Self::TemplateExpressionList, expression: Self::Expression) -> Self::TemplateExpressionList;
    /// `createTemplateLiteral(location, templateStringList)`.
    fn create_template_literal(&mut self, location: &JSTokenLocation, template_string_list: Self::TemplateStringList) -> Self::TemplateLiteral;
    /// `createTemplateLiteral(location, templateStringList, templateExpressionList)`.
    fn create_template_literal_with_expressions(&mut self, location: &JSTokenLocation, template_string_list: Self::TemplateStringList, template_expression_list: Self::TemplateExpressionList) -> Self::TemplateLiteral;
    fn create_tagged_template(&mut self, location: &JSTokenLocation, base: Self::Expression, template_literal: Self::TemplateLiteral, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Self::Expression;

    /// `createArgumentsList(location, arg)`.
    fn create_arguments_list(&mut self, location: &JSTokenLocation, arg: Self::Expression) -> Self::ArgumentsList;
    /// `createArgumentsList(location, args, arg)`.
    fn create_arguments_list_append(&mut self, location: &JSTokenLocation, args: Self::ArgumentsList, arg: Self::Expression) -> Self::ArgumentsList;
    /// `createProperty(const Identifier* name, ExpressionNode* node, type, superBinding, inferName, tag)`.
    #[allow(clippy::too_many_arguments)]
    fn create_property_named(&mut self, name: Option<&Identifier>, node: Self::Expression, type_: PropertyNodeType, super_binding: SuperBinding, infer_name: InferName, tag: ClassElementTag) -> Self::Property;
    /// `createProperty(ExpressionNode* node, type, superBinding, tag)`.
    fn create_property_expression(&mut self, node: Self::Expression, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Self::Property;
    /// `createProperty(VM&, ParserArena&, double propertyName, ExpressionNode* node, type, superBinding, tag)`.
    #[allow(clippy::too_many_arguments)]
    fn create_property_number(&mut self, vm: &VM, parser_arena: &mut ParserArena, property_name: f64, node: Self::Expression, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Self::Property;
    /// `createProperty(ExpressionNode* propertyName, ExpressionNode* node, type, superBinding, tag)`.
    fn create_property_computed(&mut self, property_name: Self::Expression, node: Self::Expression, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Self::Property;
    /// `createProperty(const Identifier*, ExpressionNode* propertyName, ExpressionNode* node, type, superBinding, tag)`.
    #[allow(clippy::too_many_arguments)]
    fn create_property_identifier_computed(&mut self, identifier: &Identifier, property_name: Self::Expression, node: Self::Expression, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Self::Property;
    /// `createProperty(const Identifier* propertyName, type, superBinding, tag)`.
    fn create_property_identifier(&mut self, property_name: &Identifier, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Self::Property;
    /// `createPropertyList(location, property)`.
    fn create_property_list(&mut self, location: &JSTokenLocation, property: Self::Property) -> Self::PropertyList;
    /// `createPropertyList(location, property, tail)`.
    fn create_property_list_append(&mut self, location: &JSTokenLocation, property: Self::Property, tail: Self::PropertyList) -> Self::PropertyList;
    /// `createElementList(elisions, expr)`.
    fn create_element_list(&mut self, elisions: i32, expr: Self::Expression) -> Self::ElementList;
    /// `createElementList(elems, elisions, expr)`.
    fn create_element_list_append(&mut self, elems: Self::ElementList, elisions: i32, expr: Self::Expression) -> Self::ElementList;
    /// `createElementList(ArgumentListNode* elems)`.
    fn create_element_list_from_arguments(&mut self, elems: Self::ArgumentsList) -> Self::ElementList;
    fn create_formal_parameter_list(&mut self) -> Self::FormalParameterList;
    fn append_parameter(&mut self, list: &Self::FormalParameterList, pattern: Self::DestructuringPattern, default_value: Self::Expression);
    fn create_clause(&mut self, expr: Self::Expression, statements: Self::SourceElements) -> Self::Clause;
    /// `createClauseList(clause)`.
    fn create_clause_list(&mut self, clause: Self::Clause) -> Self::ClauseList;
    /// `createClauseList(tail, clause)`.
    fn create_clause_list_append(&mut self, tail: Self::ClauseList, clause: Self::Clause) -> Self::ClauseList;
    fn create_func_decl_statement(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<Self>) -> Self::Statement;
    fn create_define_field(&mut self, location: &JSTokenLocation, ident: &Identifier, initializer: Self::Expression, type_: DefineFieldType) -> Self::DefineField;
    fn create_class_decl_statement(&mut self, location: &JSTokenLocation, class_expression: Self::ClassExpression, class_start: JSTextPosition, class_end: JSTextPosition, start_line: u32, end_line: u32) -> Self::Statement;
    fn create_block_statement(&mut self, location: &JSTokenLocation, elements: Self::SourceElements, start_line: i32, end_line: i32, lexical_variables: VariableEnvironment, function_stack: FunctionStack) -> Self::Statement;
    fn create_expr_statement(&mut self, location: &JSTokenLocation, expr: Self::Expression, start: JSTextPosition, end: i32) -> Self::Statement;
    /// `createIfStatement(location, condition, trueBlock, falseBlock, start, end)`. O `Parser.cpp` passa
    /// `0` em `falseBlock` quando não há `else`, que é o valor nulo (`Default`) do tipo `Statement`.
    fn create_if_statement(&mut self, location: &JSTokenLocation, condition: Self::Expression, true_block: Self::Statement, false_block: Self::Statement, start: i32, end: i32) -> Self::Statement;
    #[allow(clippy::too_many_arguments)]
    fn create_for_loop(&mut self, location: &JSTokenLocation, initializer: Self::Expression, condition: Self::Expression, iter: Self::Expression, statements: Self::Statement, start: i32, end: i32, lexical_variables: VariableEnvironment, initializer_contains_closure: bool) -> Self::Statement;
    /// `createForInLoop` com `ExpressionNode* lhs`.
    #[allow(clippy::too_many_arguments)]
    fn create_for_in_loop(&mut self, location: &JSTokenLocation, lhs: Self::Expression, iter: Self::Expression, statements: Self::Statement, decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> Self::Statement;
    /// `createForInLoop` com `DestructuringPatternNode* pattern`.
    #[allow(clippy::too_many_arguments)]
    fn create_for_in_loop_pattern(&mut self, location: &JSTokenLocation, pattern: Self::DestructuringPattern, iter: Self::Expression, statements: Self::Statement, decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> Self::Statement;
    /// `createForOfLoop` com `ExpressionNode* lhs`.
    #[allow(clippy::too_many_arguments)]
    fn create_for_of_loop(&mut self, is_for_await: bool, location: &JSTokenLocation, lhs: Self::Expression, iter: Self::Expression, statements: Self::Statement, decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> Self::Statement;
    /// `createForOfLoop` com `DestructuringPatternNode* pattern`.
    #[allow(clippy::too_many_arguments)]
    fn create_for_of_loop_pattern(&mut self, is_for_await: bool, location: &JSTokenLocation, pattern: Self::DestructuringPattern, iter: Self::Expression, statements: Self::Statement, decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> Self::Statement;
    fn create_empty_statement(&mut self, location: &JSTokenLocation) -> Self::Statement;
    fn create_declaration_statement(&mut self, location: &JSTokenLocation, expr: Self::Expression, start: i32, end: i32) -> Self::Statement;
    fn create_return_statement(&mut self, location: &JSTokenLocation, expression: Self::Expression, start: JSTextPosition, end: JSTextPosition) -> Self::Statement;
    /// `createBreakStatement(location, start, end)` (sem rótulo).
    fn create_break_statement(&mut self, location: &JSTokenLocation, start: JSTextPosition, end: JSTextPosition) -> Self::Statement;
    /// `createBreakStatement(location, ident, start, end)`.
    fn create_break_statement_label(&mut self, location: &JSTokenLocation, ident: &Identifier, start: JSTextPosition, end: JSTextPosition) -> Self::Statement;
    /// `createContinueStatement(location, start, end)` (sem rótulo).
    fn create_continue_statement(&mut self, location: &JSTokenLocation, start: JSTextPosition, end: JSTextPosition) -> Self::Statement;
    /// `createContinueStatement(location, ident, start, end)`.
    fn create_continue_statement_label(&mut self, location: &JSTokenLocation, ident: &Identifier, start: JSTextPosition, end: JSTextPosition) -> Self::Statement;
    #[allow(clippy::too_many_arguments)]
    fn create_try_statement(&mut self, location: &JSTokenLocation, try_block: Self::Statement, catch_pattern: Self::DestructuringPattern, catch_block: Self::Statement, finally_block: Self::Statement, start_line: i32, end_line: i32, catch_environment: VariableEnvironment) -> Self::Statement;
    #[allow(clippy::too_many_arguments)]
    fn create_switch_statement(&mut self, location: &JSTokenLocation, expr: Self::Expression, first_clauses: Self::ClauseList, default_clause: Self::Clause, second_clauses: Self::ClauseList, start_line: i32, end_line: i32, lexical_variables: VariableEnvironment, function_stack: FunctionStack) -> Self::Statement;
    fn create_while_statement(&mut self, location: &JSTokenLocation, expr: Self::Expression, statement: Self::Statement, start_line: i32, end_line: i32) -> Self::Statement;
    #[allow(clippy::too_many_arguments)]
    fn create_with_statement(&mut self, location: &JSTokenLocation, expr: Self::Expression, statement: Self::Statement, start: u32, end: JSTextPosition, start_line: u32, end_line: u32) -> Self::Statement;
    fn create_do_while_statement(&mut self, location: &JSTokenLocation, statement: Self::Statement, expr: Self::Expression, start_line: i32, end_line: i32) -> Self::Statement;
    fn create_label_statement(&mut self, location: &JSTokenLocation, ident: &Identifier, statement: Self::Statement, start: JSTextPosition, end: JSTextPosition) -> Self::Statement;
    fn create_throw_statement(&mut self, location: &JSTokenLocation, expr: Self::Expression, start: JSTextPosition, end: JSTextPosition) -> Self::Statement;
    fn create_debugger(&mut self, location: &JSTokenLocation, start_line: i32, end_line: i32) -> Self::Statement;
    fn create_module_name(&mut self, location: &JSTokenLocation, module_name: &Identifier) -> Self::ModuleName;
    fn create_import_specifier(&mut self, location: &JSTokenLocation, imported_name: &Identifier, local_name: &Identifier) -> Self::ImportSpecifier;
    fn create_import_specifier_list(&mut self) -> Self::ImportSpecifierList;
    fn append_import_specifier(&mut self, specifier_list: &Self::ImportSpecifierList, specifier: Self::ImportSpecifier);
    fn create_import_attributes_list(&mut self) -> Self::ImportAttributesList;
    fn append_import_assertion(&mut self, attributes_list: &Self::ImportAttributesList, key: &Identifier, value: &Identifier);
    fn create_import_declaration(&mut self, location: &JSTokenLocation, type_: ImportType, import_specifier_list: Self::ImportSpecifierList, module_name: Self::ModuleName, import_attributes_list: Self::ImportAttributesList) -> Self::Statement;
    fn create_export_all_declaration(&mut self, location: &JSTokenLocation, module_name: Self::ModuleName, import_attributes_list: Self::ImportAttributesList) -> Self::Statement;
    fn create_export_default_declaration(&mut self, location: &JSTokenLocation, declaration: Self::Statement, local_name: &Identifier) -> Self::Statement;
    fn create_export_local_declaration(&mut self, location: &JSTokenLocation, declaration: Self::Statement) -> Self::Statement;
    fn create_export_named_declaration(&mut self, location: &JSTokenLocation, export_specifier_list: Self::ExportSpecifierList, module_name: Self::ModuleName, import_attributes_list: Self::ImportAttributesList) -> Self::Statement;
    fn create_export_specifier(&mut self, location: &JSTokenLocation, local_name: &Identifier, exported_name: &Identifier) -> Self::ExportSpecifier;
    fn create_export_specifier_list(&mut self) -> Self::ExportSpecifierList;
    fn append_export_specifier(&mut self, specifier_list: &Self::ExportSpecifierList, specifier: Self::ExportSpecifier);

    /// `createGetterOrSetterProperty(location, type, const Identifier* name, functionInfo, tag)`.
    fn create_getter_or_setter_property(&mut self, location: &JSTokenLocation, type_: PropertyNodeType, name: &Identifier, function_info: &ParserFunctionInfo<Self>, tag: ClassElementTag) -> Self::Property;
    /// `createGetterOrSetterProperty(location, type, ExpressionNode* name, functionInfo, tag)`.
    fn create_getter_or_setter_property_computed(&mut self, location: &JSTokenLocation, type_: PropertyNodeType, name: Self::Expression, function_info: &ParserFunctionInfo<Self>, tag: ClassElementTag) -> Self::Property;
    /// `createGetterOrSetterProperty(vm, parserArena, location, type, double name, functionInfo, tag)`.
    #[allow(clippy::too_many_arguments)]
    fn create_getter_or_setter_property_number(&mut self, vm: &VM, parser_arena: &mut ParserArena, location: &JSTokenLocation, type_: PropertyNodeType, name: f64, function_info: &ParserFunctionInfo<Self>, tag: ClassElementTag) -> Self::Property;

    fn append_statement(&mut self, elements: &Self::SourceElements, statement: Self::Statement);
    fn eval_count(&self) -> i32;
    fn append_binary_expression_info(&mut self, operand_stack_depth: &mut i32, current: Self::Expression, expr_start: JSTextPosition, lhs: JSTextPosition, rhs: JSTextPosition, has_assignments: bool);

    // Estruturas de dados usadas no parse de expressões binárias.
    fn operator_stack_pop(&mut self, operator_stack_depth: &mut i32);
    fn operator_stack_should_reduce(&mut self, precedence: i32) -> bool;
    fn get_from_operand_stack(&mut self, i: i32) -> Self::BinaryOperand;
    fn shrink_operand_stack_by(&mut self, operand_stack_depth: &mut i32, amount: i32);
    fn append_binary_operation(&mut self, location: &JSTokenLocation, operand_stack_depth: &mut i32, operator_stack_depth: &mut i32, lhs: Self::BinaryOperand, rhs: Self::BinaryOperand);
    fn operator_stack_append(&mut self, operator_stack_depth: &mut i32, op: i32, precedence: i32);
    fn pop_operand_stack(&mut self, operand_stack_depth: &mut i32) -> Self::Expression;

    fn append_unary_token(&mut self, stack_depth: &mut i32, type_: i32, start: JSTextPosition);
    fn unary_token_stack_last_type(&mut self, stack_depth: &mut i32) -> i32;
    fn unary_token_stack_last_start(&mut self, stack_depth: &mut i32) -> JSTextPosition;
    fn unary_token_stack_remove_last(&mut self, stack_depth: &mut i32);
    fn unary_token_stack_depth(&self) -> i32;
    fn set_unary_token_stack_depth(&mut self, depth: i32);

    fn assignment_stack_append(&mut self, assignment_stack_depth: &mut i32, node: Self::Expression, start: JSTextPosition, divot: JSTextPosition, assignment_count: i32, op: Operator);
    fn create_assignment(&mut self, location: &JSTokenLocation, assignment_stack_depth: &mut i32, rhs: Self::Expression, initial_assignment_count: i32, current_assignment_count: i32, last_token_end: JSTextPosition) -> Self::Expression;
    fn get_type(&self, property: &Self::Property) -> PropertyNodeType;
    fn is_underscore_proto_setter(&self, property: &Self::Property) -> bool;
    fn is_resolve(&self, expr: &Self::Expression) -> bool;
    fn create_destructuring_assignment(&mut self, location: &JSTokenLocation, pattern: Self::DestructuringPattern, initializer: Self::Expression) -> Self::Expression;

    fn create_array_pattern(&mut self, location: &JSTokenLocation) -> Self::ArrayPattern;
    fn append_array_pattern_skip_entry(&mut self, node: &Self::ArrayPattern, location: &JSTokenLocation);
    fn append_array_pattern_entry(&mut self, node: &Self::ArrayPattern, location: &JSTokenLocation, pattern: Self::DestructuringPattern, default_value: Self::Expression);
    fn append_array_pattern_rest_entry(&mut self, node: &Self::ArrayPattern, location: &JSTokenLocation, pattern: Self::DestructuringPattern);
    fn finish_array_pattern(&mut self, node: &Self::ArrayPattern, divot_start: JSTextPosition, divot: JSTextPosition, divot_end: JSTextPosition);
    fn create_object_pattern(&mut self, location: &JSTokenLocation) -> Self::ObjectPattern;
    /// `appendObjectPatternEntry(node, location, wasString, identifier, pattern, defaultValue)`.
    fn append_object_pattern_entry(&mut self, node: &Self::ObjectPattern, location: &JSTokenLocation, was_string: bool, identifier: &Identifier, pattern: Self::DestructuringPattern, default_value: Self::Expression);
    /// `appendObjectPatternEntry(vm, node, location, propertyExpression, pattern, defaultValue)`.
    fn append_object_pattern_computed_entry(&mut self, vm: &VM, node: &Self::ObjectPattern, location: &JSTokenLocation, property_expression: Self::Expression, pattern: Self::DestructuringPattern, default_value: Self::Expression);
    fn append_object_pattern_rest_entry(&mut self, vm: &VM, node: &Self::ObjectPattern, location: &JSTokenLocation, pattern: Self::DestructuringPattern);
    fn set_contains_object_rest_element(&mut self, node: &Self::ObjectPattern, contains_rest_element: bool);
    fn set_contains_computed_property(&mut self, node: &Self::ObjectPattern, contains_computed_property: bool);
    fn finish_object_pattern(&mut self, node: &Self::ObjectPattern, divot_start: JSTextPosition, divot: JSTextPosition, divot_end: JSTextPosition);

    // As conversões de ponteiro derivado para base do C++ (`Comma*` para `ExpressionNode*`,
    // `ArrayPatternNode*` para `DestructuringPatternNode*`...) são os `Into` dos tipos associados acima.
    // Os métodos abaixo só existem no `ASTBuilder` (o C++ os chama dentro de `if constexpr` de `ASTBuilder`);
    // o `SyntaxChecker` fica com o corpo vazio do trait.

    /// `static_cast<DotAccessorNode*>(expression)->identifier()`: `None` quando não é um acesso por ponto.
    fn dot_accessor_identifier(&self, _expression: &Self::Expression) -> Option<Identifier> {
        None
    }
    /// `classElements->setHasPrivateAccessors(...)`.
    fn set_has_private_accessors(&self, _class_elements: &Self::PropertyList, _has_private_accessors: bool) {}
    /// `functionInfo.body->setEcmaName(name)`.
    fn set_function_body_ecma_name(_body: &Self::FunctionBody, _name: &Identifier) {}
    /// `getMetadata(ParserFunctionInfo<...>&)`: a sobrecarga por tipo de construtor.
    fn get_metadata(function_info: &ParserFunctionInfo<Self>) -> std::rc::Rc<crate::parser::nodes::FunctionMetadataNode>;

    fn create_binding_location(&mut self, location: &JSTokenLocation, bound_property: &Identifier, start: JSTextPosition, end: JSTextPosition, context: AssignmentContext) -> Self::DestructuringPattern;
    fn create_rest_parameter(&mut self, pattern: Self::DestructuringPattern, num_parameters_to_skip: usize) -> Self::RestPattern;
    fn create_assignment_element(&mut self, assignment_target: &Self::Expression, start: JSTextPosition, end: JSTextPosition) -> Self::DestructuringPattern;

    fn is_binding_node(&self, pattern: &Self::DestructuringPattern) -> bool;
    fn is_location(&self, expr: &Self::Expression) -> bool;
    fn is_private_location(&self, expr: &Self::Expression) -> bool;
    fn is_assignment_location(&self, expr: &Self::Expression) -> bool;
    fn is_object_literal(&self, expr: &Self::Expression) -> bool;
    fn is_array_literal(&self, expr: &Self::Expression) -> bool;
    fn is_object_or_array_literal(&self, expr: &Self::Expression) -> bool;
    fn is_function_call(&self, expr: &Self::Expression) -> bool;

    fn should_skip_pause_location(&self, statement: &Self::Statement) -> bool;

    fn set_end_offset<N: TreeNodeHandle>(&mut self, node: &N, offset: i32);
    fn end_offset<N: TreeNodeHandle>(&mut self, node: &N) -> i32;
    fn set_start_offset<N: TreeNodeHandle>(&mut self, node: &N, offset: i32);

    fn breakpoint_location<N: TreeNodeHandle>(&mut self, node: &N) -> JSTextPosition;

    fn propagate_arguments_use(&mut self);

    fn has_arguments_feature(&self) -> bool;
}
