//! Tradução de `JavaScriptCore/parser/SyntaxChecker.h`.
//!
//! O `TreeBuilder` da pré-análise: não constrói árvore, devolve códigos inteiros (os `*Expr` e
//! `*Result` abaixo) que o `Parser` só compara. A interface é o trait
//! `crate::parser::tree_builder::TreeBuilder`; os desvios de forma estão descritos lá.

use crate::parser::lexer::{LexerFlagSet, LexerFlags};
use crate::parser::nodes::{
    AssignmentContext, ClassElementTag, DotType, FunctionStack, Operator, PropertyNode,
    PropertyNodeType,
};
use crate::parser::nodes::{DefineFieldType, ImportType};
use crate::parser::parser::InferName;
use crate::parser::parser_arena::ParserArena;
use crate::parser::parser_function_info::{ParserClassInfo, ParserFunctionInfo};
use crate::parser::parser_modes::{LexicallyScopedFeatures, SourceParseMode, SuperBinding};
use crate::parser::parser_tokens::{JSTextPosition, JSTokenLocation};
use crate::parser::tree_builder::{TreeBuilder, TreeNodeHandle};
use crate::parser::variable_environment::VariableEnvironment;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::vm::VM;
use crate::yarr::yarr_error_code::has_error;
use crate::yarr::yarr_syntax_checker::check_syntax;

/// `SyntaxChecker::Property`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Property {
    pub type_: PropertyNodeType,
    pub is_underscore_proto_setter: bool,
}

impl Property {
    /// `Property(PropertyNode::Type type)`.
    pub fn new(type_: PropertyNodeType) -> Property {
        Property { type_, is_underscore_proto_setter: false }
    }

    /// `Property(PropertyNode::Type type, bool isUnderscoreProtoSetter)`.
    pub fn with_underscore_proto_setter(type_: PropertyNodeType, is_underscore_proto_setter: bool) -> Property {
        Property { type_, is_underscore_proto_setter }
    }

    /// `operator!()`.
    pub fn is_null(&self) -> bool {
        self.type_ == 0
    }
}

/// `SyntaxChecker::BinaryExprContext`: o topo salvo de `m_topBinaryExpr`.
pub struct BinaryExprContext {
    token: i32,
}

/// `SyntaxChecker::UnaryExprContext`: o topo salvo de `m_topUnaryToken`.
pub struct UnaryExprContext {
    token: i32,
}

/// Os nós do `SyntaxChecker` são `int`, então não há o que guardar nem consultar.
impl TreeNodeHandle for i32 {
    fn set_end_offset(&self, _offset: i32) {}
    fn end_offset(&self) -> i32 {
        0
    }
    fn set_start_offset(&self, _offset: i32) {}
    fn breakpoint_location(&self) -> JSTextPosition {
        JSTextPosition::new(0, 0, 0)
    }
}

pub struct SyntaxChecker<'a> {
    vm: &'a VM,
    top_binary_expr: i32,
    top_unary_token: i32,
}

#[allow(non_upper_case_globals)]
impl<'a> SyntaxChecker<'a> {
    /// `SyntaxChecker(VM& vm, void*)`. Os dois membros de topo ficam sem inicializar no C++; o `Parser`
    /// sempre os escreve antes de ler, então zero é um valor seguro.
    pub fn new(vm: &'a VM) -> SyntaxChecker<'a> {
        SyntaxChecker { vm, top_binary_expr: 0, top_unary_token: 0 }
    }

    pub const META_PROPERTY_BIT: i32 = 0x8000_0000u32 as i32;

    pub const NONE_EXPR: i32 = 0;
    pub const RESOLVE_EVAL_EXPR: i32 = 1;
    pub const RESOLVE_EXPR: i32 = 2;
    pub const INTEGER_EXPR: i32 = 3;
    pub const DOUBLE_EXPR: i32 = 4;
    pub const STRING_EXPR: i32 = 5;
    pub const BIG_INT_EXPR: i32 = 6;
    pub const THIS_EXPR: i32 = 7;
    pub const NULL_EXPR: i32 = 8;
    pub const BOOL_EXPR: i32 = 9;
    pub const REG_EXP_EXPR: i32 = 10;
    pub const OBJECT_LITERAL_EXPR: i32 = 11;
    pub const FUNCTION_EXPR: i32 = 12;
    pub const CLASS_EXPR: i32 = 13;
    pub const SUPER_EXPR: i32 = 14;
    pub const IMPORT_EXPR: i32 = 15;
    pub const BRACKET_EXPR: i32 = 16;
    pub const DOT_EXPR: i32 = 17;
    pub const CALL_EXPR: i32 = 18;
    pub const NEW_EXPR: i32 = 19;
    pub const PRE_EXPR: i32 = 20;
    pub const POST_EXPR: i32 = 21;
    pub const UNARY_EXPR: i32 = 22;
    pub const BINARY_EXPR: i32 = 23;
    pub const OPTIONAL_CHAIN: i32 = 24;
    pub const PRIVATE_DOT_EXPR: i32 = 25;
    pub const CONDITIONAL_EXPR: i32 = 26;
    pub const ASSIGNMENT_EXPR: i32 = 27;
    pub const TYPEOF_EXPR: i32 = 28;
    pub const DELETE_EXPR: i32 = 29;
    pub const ARRAY_LITERAL_EXPR: i32 = 30;
    pub const BINDING_DESTRUCTURING: i32 = 31;
    pub const REST_PARAMETER: i32 = 32;
    pub const ARRAY_DESTRUCTURING: i32 = 33;
    pub const OBJECT_DESTRUCTURING: i32 = 34;
    pub const SOURCE_ELEMENTS_RESULT: i32 = 35;
    pub const FUNCTION_BODY_RESULT: i32 = 36;
    pub const SPREAD_EXPR: i32 = 37;
    pub const OBJECT_SPREAD_EXPR: i32 = 38;
    pub const ARGUMENTS_RESULT: i32 = 39;
    pub const PROPERTY_LIST_RESULT: i32 = 40;
    pub const ARGUMENTS_LIST_RESULT: i32 = 41;
    pub const ELEMENTS_LIST_RESULT: i32 = 42;
    pub const STATEMENT_RESULT: i32 = 43;
    pub const FORMAL_PARAMETER_LIST_RESULT: i32 = 44;
    pub const CLAUSE_RESULT: i32 = 45;
    pub const CLAUSE_LIST_RESULT: i32 = 46;
    pub const COMMA_EXPR: i32 = 47;
    pub const DESTRUCTURING_ASSIGNMENT: i32 = 48;
    pub const TEMPLATE_STRING_RESULT: i32 = 49;
    pub const TEMPLATE_STRING_LIST_RESULT: i32 = 50;
    pub const TEMPLATE_EXPRESSION_LIST_RESULT: i32 = 51;
    pub const TEMPLATE_EXPR: i32 = 52;
    pub const TAGGED_TEMPLATE_EXPR: i32 = 53;
    pub const YIELD_EXPR: i32 = 54;
    pub const AWAIT_EXPR: i32 = 55;
    pub const MODULE_NAME_RESULT: i32 = 56;
    pub const PRIVATE_IDENTIFIER: i32 = 57;
    pub const IMPORT_SPECIFIER_RESULT: i32 = 58;
    pub const IMPORT_SPECIFIER_LIST_RESULT: i32 = 59;
    pub const IMPORT_ATTRIBUTES_LIST_RESULT: i32 = 60;
    pub const EXPORT_SPECIFIER_RESULT: i32 = 61;
    pub const EXPORT_SPECIFIER_LIST_RESULT: i32 = 62;

    pub const NEW_TARGET_EXPR: i32 = Self::META_PROPERTY_BIT;
    pub const IMPORT_META_EXPR: i32 = Self::META_PROPERTY_BIT | 1;

    /// `createIfStatement(location, condition, trueBlock, start, end)`: sobrecarga sem `else` que o
    /// `Parser` não chama (ele passa `0` na forma de seis argumentos); fica por fidelidade.
    pub fn create_if_statement_no_else(&mut self, _location: &JSTokenLocation, _condition: i32, _true_block: i32, _start: i32, _end: i32) -> i32 {
        Self::STATEMENT_RESULT
    }

    /// `createConstStatement`: não existe no `ASTBuilder` nem é chamado pelo `Parser`.
    pub fn create_const_statement(&mut self, _location: &JSTokenLocation, _a: i32, _start: i32, _end: i32) -> i32 {
        Self::STATEMENT_RESULT
    }

    /// `appendConstDecl`: idem `create_const_statement`.
    pub fn append_const_decl(&mut self, _location: &JSTokenLocation, _a: i32, _ident: Option<&Identifier>, _b: i32) -> i32 {
        Self::STATEMENT_RESULT
    }
}

impl<'a> TreeBuilder for SyntaxChecker<'a> {
    type Expression = i32;
    type SourceElements = i32;
    type Arguments = i32;
    type Comma = i32;
    type Property = Property;
    type PropertyList = i32;
    type ElementList = i32;
    type ArgumentsList = i32;
    type TemplateExpressionList = i32;
    type TemplateString = i32;
    type TemplateStringList = i32;
    type TemplateLiteral = i32;
    type FormalParameterList = i32;
    type FunctionBody = i32;
    type ClassExpression = i32;
    type ModuleName = i32;
    type ImportSpecifier = i32;
    type ImportSpecifierList = i32;
    type ImportAttributesList = i32;
    type ExportSpecifier = i32;
    type ExportSpecifierList = i32;
    type Statement = i32;
    type ClauseList = i32;
    type Clause = i32;
    type BinaryOperand = i32;
    type DestructuringPattern = i32;
    type ArrayPattern = i32;
    type ObjectPattern = i32;
    type RestPattern = i32;
    type DefineField = i32;
    type BinaryExprContext = BinaryExprContext;
    type UnaryExprContext = UnaryExprContext;

    const CREATES_AST: bool = false;
    const NEEDS_FREE_VARIABLE_INFO: bool = false;
    const CAN_USE_FUNCTION_CACHE: bool = true;
    const DONT_BUILD_KEYWORDS: LexerFlagSet = LexerFlagSet::from_raw(LexerFlags::DontBuildKeywords as u8);
    const DONT_BUILD_STRINGS: LexerFlagSet = LexerFlagSet::from_raw(LexerFlags::DontBuildStrings as u8);

    /// `static NO_RETURN_DUE_TO_CRASH FunctionMetadataNode* getMetadata(ParserFunctionInfo<SyntaxChecker>&)`.
    fn get_metadata(_function_info: &ParserFunctionInfo<SyntaxChecker<'a>>) -> std::rc::Rc<crate::parser::nodes::FunctionMetadataNode> {
        // Invariante: `getMetadata` só é chamado com `CREATES_AST`, que o SyntaxChecker não tem.
        panic!("RELEASE_ASSERT_NOT_REACHED");
    }

    fn begin_binary_expr_context(&mut self) -> BinaryExprContext {
        let token = self.top_binary_expr;
        self.top_binary_expr = 0;
        BinaryExprContext { token }
    }
    fn end_binary_expr_context(&mut self, saved: BinaryExprContext) {
        self.top_binary_expr = saved.token;
    }
    fn begin_unary_expr_context(&mut self) -> UnaryExprContext {
        let token = self.top_unary_token;
        self.top_unary_token = 0;
        UnaryExprContext { token }
    }
    fn end_unary_expr_context(&mut self, saved: UnaryExprContext) {
        self.top_unary_token = saved.token;
    }

    fn create_source_elements(&mut self) -> i32 { Self::SOURCE_ELEMENTS_RESULT }
    fn make_static_block_function_call_node(&mut self, _l: &JSTokenLocation, _func: i32, _divot: JSTextPosition, _divot_start: JSTextPosition, _divot_end: JSTextPosition) -> i32 { Self::CALL_EXPR }
    fn make_function_call_node(&mut self, _l: &JSTokenLocation, _func: i32, _previous_base_was_super: bool, _args: i32, _divot_start: JSTextPosition, _divot: JSTextPosition, _divot_end: JSTextPosition, _call_or_apply_child_depth: usize, _is_optional_call: bool) -> i32 { Self::CALL_EXPR }
    fn create_comma_expr(&mut self, _l: &JSTokenLocation, _node: i32) -> i32 { Self::COMMA_EXPR }
    fn append_to_comma_expr(&mut self, _l: &JSTokenLocation, _tail: i32, _next: i32) -> i32 { Self::COMMA_EXPR }
    fn make_assign_node(&mut self, _l: &JSTokenLocation, _left: i32, _op: Operator, _right: i32, _lh: bool, _rh: bool, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::ASSIGNMENT_EXPR }
    fn make_prefix_node(&mut self, _l: &JSTokenLocation, _expr: i32, _op: Operator, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::PRE_EXPR }
    fn make_postfix_node(&mut self, _l: &JSTokenLocation, _expr: i32, _op: Operator, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::POST_EXPR }
    fn make_type_of_node(&mut self, _l: &JSTokenLocation, _expr: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::TYPEOF_EXPR }
    fn make_delete_node(&mut self, _l: &JSTokenLocation, _expr: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::DELETE_EXPR }
    fn make_negate_node(&mut self, _l: &JSTokenLocation, _expr: i32) -> i32 { Self::UNARY_EXPR }
    fn make_bitwise_not_node(&mut self, _l: &JSTokenLocation, _expr: i32) -> i32 { Self::UNARY_EXPR }
    fn create_logical_not(&mut self, _l: &JSTokenLocation, _expr: i32) -> i32 { Self::UNARY_EXPR }
    fn create_unary_plus(&mut self, _l: &JSTokenLocation, _expr: i32) -> i32 { Self::UNARY_EXPR }
    fn create_void(&mut self, _l: &JSTokenLocation, _expr: i32) -> i32 { Self::UNARY_EXPR }
    fn create_import_expr(&mut self, _l: &JSTokenLocation, _expr: i32, _option: i32, _deferred: bool, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::IMPORT_EXPR }
    fn create_this_expr(&mut self, _l: &JSTokenLocation) -> i32 { Self::THIS_EXPR }
    fn create_super_expr(&mut self, _l: &JSTokenLocation) -> i32 { Self::SUPER_EXPR }
    fn create_new_target_expr(&mut self, _l: &JSTokenLocation) -> i32 { Self::NEW_TARGET_EXPR }
    fn create_import_meta_expr(&mut self, _l: &JSTokenLocation, _expr: i32) -> i32 { Self::IMPORT_META_EXPR }
    fn is_meta_property(&mut self, type_: &i32) -> bool { (*type_ & Self::META_PROPERTY_BIT) != 0 }
    fn is_new_target(&mut self, type_: &i32) -> bool { *type_ == Self::NEW_TARGET_EXPR }
    fn is_import_meta(&mut self, type_: &i32) -> bool { *type_ == Self::IMPORT_META_EXPR }
    fn create_resolve(&mut self, _l: &JSTokenLocation, _ident: &Identifier, _start: JSTextPosition, _end: JSTextPosition, _need_to_check_uses_arguments: bool) -> i32 { Self::RESOLVE_EXPR }
    fn create_private_identifier_node(&mut self, _l: &JSTokenLocation, _ident: &Identifier) -> i32 { Self::PRIVATE_IDENTIFIER }
    fn create_object_literal(&mut self, _l: &JSTokenLocation) -> i32 { Self::OBJECT_LITERAL_EXPR }
    fn create_object_literal_with_properties(&mut self, _l: &JSTokenLocation, _properties: i32) -> i32 { Self::OBJECT_LITERAL_EXPR }
    fn create_array_elisions(&mut self, _l: &JSTokenLocation, _elisions: i32) -> i32 { Self::ARRAY_LITERAL_EXPR }
    fn create_array_elements(&mut self, _l: &JSTokenLocation, _elems: i32) -> i32 { Self::ARRAY_LITERAL_EXPR }
    fn create_array_elisions_elements(&mut self, _l: &JSTokenLocation, _elisions: i32, _elems: i32) -> i32 { Self::ARRAY_LITERAL_EXPR }
    fn create_double_expr(&mut self, _l: &JSTokenLocation, _d: f64) -> i32 { Self::DOUBLE_EXPR }
    fn create_integer_expr(&mut self, _l: &JSTokenLocation, _d: f64) -> i32 { Self::INTEGER_EXPR }
    fn create_big_int(&mut self, _l: &JSTokenLocation, _big_int: &Identifier, _radix: u8) -> i32 { Self::BIG_INT_EXPR }
    fn create_string(&mut self, _l: &JSTokenLocation, _string: &Identifier) -> i32 { Self::STRING_EXPR }
    fn create_boolean(&mut self, _l: &JSTokenLocation, _b: bool) -> i32 { Self::BOOL_EXPR }
    fn create_null(&mut self, _l: &JSTokenLocation) -> i32 { Self::NULL_EXPR }
    fn create_bracket_access(&mut self, _l: &JSTokenLocation, _base: i32, _property: i32, _property_has_assignments: bool, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::BRACKET_EXPR }
    fn create_dot_access(&mut self, _l: &JSTokenLocation, _base: i32, _property: &Identifier, type_: DotType, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 {
        if type_ == DotType::PrivateMember { Self::PRIVATE_DOT_EXPR } else { Self::DOT_EXPR }
    }
    fn create_reg_exp(&mut self, _l: &JSTokenLocation, pattern: &Identifier, flags: &Identifier, _start: JSTextPosition, skip_syntax_check: bool) -> i32 {
        if !skip_syntax_check && has_error(check_syntax(pattern.string(), flags.string())) { 0 } else { Self::REG_EXP_EXPR }
    }
    fn create_new_expr(&mut self, _l: &JSTokenLocation, _expr: i32, _arguments: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::NEW_EXPR }
    fn create_new_expr_no_arguments(&mut self, _l: &JSTokenLocation, _expr: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::NEW_EXPR }
    fn create_optional_chain(&mut self, _l: &JSTokenLocation, _base: i32, _expr: i32, _is_outermost: bool) -> i32 { Self::OPTIONAL_CHAIN }
    fn create_conditional_expr(&mut self, _l: &JSTokenLocation, _condition: i32, _lhs: i32, _rhs: i32) -> i32 { Self::CONDITIONAL_EXPR }
    fn create_assign_resolve(&mut self, _l: &JSTokenLocation, _ident: &Identifier, _rhs: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition, _ctx: AssignmentContext) -> i32 { Self::ASSIGNMENT_EXPR }
    fn create_empty_var_expression(&mut self, _l: &JSTokenLocation, _ident: &Identifier) -> i32 { Self::ASSIGNMENT_EXPR }
    fn create_empty_let_expression(&mut self, _l: &JSTokenLocation, _ident: &Identifier) -> i32 { Self::ASSIGNMENT_EXPR }
    fn create_yield(&mut self, _l: &JSTokenLocation) -> i32 { Self::YIELD_EXPR }
    fn create_yield_argument(&mut self, _l: &JSTokenLocation, _argument: i32, _delegate: bool, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::YIELD_EXPR }
    fn create_await(&mut self, _l: &JSTokenLocation, _argument: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::AWAIT_EXPR }
    fn create_class_expr(&mut self, _l: &JSTokenLocation, _class_info: &ParserClassInfo<Self>, _head: VariableEnvironment, _env: VariableEnvironment, _constructor: i32, _parent_class: i32, _class_elements: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::CLASS_EXPR }
    fn create_function_expr(&mut self, _l: &JSTokenLocation, _info: &ParserFunctionInfo<Self>) -> i32 { Self::FUNCTION_EXPR }
    fn create_generator_function_body(&mut self, _l: &JSTokenLocation, _info: &ParserFunctionInfo<Self>, _name: &Identifier) -> i32 { Self::FUNCTION_EXPR }
    fn create_async_function_body(&mut self, _l: &JSTokenLocation, _info: &ParserFunctionInfo<Self>, _parse_mode: SourceParseMode, _name: &Identifier) -> i32 { Self::FUNCTION_EXPR }
    fn create_function_metadata(&mut self, _sl: &JSTokenLocation, _el: &JSTokenLocation, _sc: u32, _ec: u32, _fs: u32, _fns: i32, _ps: i32, _iv: ImplementationVisibility, _lsf: LexicallyScopedFeatures, _ck: ConstructorKind, _sb: SuperBinding, _pc: u32, _mode: SourceParseMode, _arrow_body_expr: bool) -> i32 { Self::FUNCTION_BODY_RESULT }
    fn create_arrow_function_expr(&mut self, _l: &JSTokenLocation, _info: &ParserFunctionInfo<Self>) -> i32 { Self::FUNCTION_EXPR }
    fn create_method_definition(&mut self, _l: &JSTokenLocation, _info: &ParserFunctionInfo<Self>) -> i32 { Self::FUNCTION_EXPR }
    fn set_function_name_start(&mut self, _body: &i32, _function_name_start: i32) {}
    fn create_arguments(&mut self) -> i32 { Self::ARGUMENTS_RESULT }
    fn create_arguments_with_list(&mut self, _args: i32, _has_assignments: bool) -> i32 { Self::ARGUMENTS_RESULT }
    fn create_spread_expression(&mut self, _l: &JSTokenLocation, _e: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::SPREAD_EXPR }
    fn create_object_spread_expression(&mut self, _l: &JSTokenLocation, _e: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::OBJECT_SPREAD_EXPR }
    fn create_template_string(&mut self, _l: &JSTokenLocation, _cooked: Option<&Identifier>, _raw: Option<&Identifier>) -> i32 { Self::TEMPLATE_STRING_RESULT }
    fn create_template_string_list(&mut self, _ts: i32) -> i32 { Self::TEMPLATE_STRING_LIST_RESULT }
    fn create_template_string_list_append(&mut self, _list: i32, _ts: i32) -> i32 { Self::TEMPLATE_STRING_LIST_RESULT }
    fn create_template_expression_list(&mut self, _e: i32) -> i32 { Self::TEMPLATE_EXPRESSION_LIST_RESULT }
    fn create_template_expression_list_append(&mut self, _list: i32, _e: i32) -> i32 { Self::TEMPLATE_EXPRESSION_LIST_RESULT }
    fn create_template_literal(&mut self, _l: &JSTokenLocation, _list: i32) -> i32 { Self::TEMPLATE_EXPR }
    fn create_template_literal_with_expressions(&mut self, _l: &JSTokenLocation, _list: i32, _exprs: i32) -> i32 { Self::TEMPLATE_EXPR }
    fn create_tagged_template(&mut self, _l: &JSTokenLocation, _base: i32, _literal: i32, _start: JSTextPosition, _divot: JSTextPosition, _end: JSTextPosition) -> i32 { Self::TAGGED_TEMPLATE_EXPR }

    fn create_arguments_list(&mut self, _l: &JSTokenLocation, _arg: i32) -> i32 { Self::ARGUMENTS_LIST_RESULT }
    fn create_arguments_list_append(&mut self, _l: &JSTokenLocation, _args: i32, _arg: i32) -> i32 { Self::ARGUMENTS_LIST_RESULT }
    fn create_property_named(&mut self, name: Option<&Identifier>, _node: i32, type_: PropertyNodeType, super_binding: SuperBinding, _infer_name: InferName, tag: ClassElementTag) -> Property {
        let needs_super_binding = super_binding == SuperBinding::Needed;
        let is_class_property = tag != ClassElementTag::No;
        Property::with_underscore_proto_setter(
            type_,
            PropertyNode::is_underscore_proto_setter_parts(self.vm, name, type_, needs_super_binding, is_class_property),
        )
    }
    fn create_property_expression(&mut self, _node: i32, type_: PropertyNodeType, _sb: SuperBinding, _tag: ClassElementTag) -> Property { Property::new(type_) }
    fn create_property_number(&mut self, _vm: &VM, _arena: &mut ParserArena, _name: f64, _node: i32, type_: PropertyNodeType, _sb: SuperBinding, _tag: ClassElementTag) -> Property { Property::new(type_) }
    fn create_property_computed(&mut self, _name: i32, _node: i32, type_: PropertyNodeType, _sb: SuperBinding, _tag: ClassElementTag) -> Property { Property::new(type_) }
    fn create_property_identifier_computed(&mut self, _ident: &Identifier, _name: i32, _node: i32, type_: PropertyNodeType, _sb: SuperBinding, _tag: ClassElementTag) -> Property { Property::new(type_) }
    fn create_property_identifier(&mut self, _name: &Identifier, type_: PropertyNodeType, _sb: SuperBinding, _tag: ClassElementTag) -> Property { Property::new(type_) }
    fn create_property_list(&mut self, _l: &JSTokenLocation, _property: Property) -> i32 { Self::PROPERTY_LIST_RESULT }
    fn create_property_list_append(&mut self, _l: &JSTokenLocation, _property: Property, _tail: i32) -> i32 { Self::PROPERTY_LIST_RESULT }
    fn create_element_list(&mut self, _elisions: i32, _expr: i32) -> i32 { Self::ELEMENTS_LIST_RESULT }
    fn create_element_list_append(&mut self, _elems: i32, _elisions: i32, _expr: i32) -> i32 { Self::ELEMENTS_LIST_RESULT }
    fn create_element_list_from_arguments(&mut self, _elems: i32) -> i32 { Self::ELEMENTS_LIST_RESULT }
    fn create_formal_parameter_list(&mut self) -> i32 { Self::FORMAL_PARAMETER_LIST_RESULT }
    fn append_parameter(&mut self, _list: &i32, _pattern: i32, _default_value: i32) {}
    fn create_clause(&mut self, _expr: i32, _statements: i32) -> i32 { Self::CLAUSE_RESULT }
    fn create_clause_list(&mut self, _clause: i32) -> i32 { Self::CLAUSE_LIST_RESULT }
    fn create_clause_list_append(&mut self, _tail: i32, _clause: i32) -> i32 { Self::CLAUSE_LIST_RESULT }
    fn create_func_decl_statement(&mut self, _l: &JSTokenLocation, _info: &ParserFunctionInfo<Self>) -> i32 { Self::STATEMENT_RESULT }
    fn create_define_field(&mut self, _l: &JSTokenLocation, _ident: &Identifier, _initializer: i32, _type: DefineFieldType) -> i32 { 0 }
    fn create_class_decl_statement(&mut self, _l: &JSTokenLocation, _class_expression: i32, _start: JSTextPosition, _end: JSTextPosition, _sl: u32, _el: u32) -> i32 { Self::STATEMENT_RESULT }
    fn create_block_statement(&mut self, _l: &JSTokenLocation, _elements: i32, _sl: i32, _el: i32, _lexical: VariableEnvironment, _stack: FunctionStack) -> i32 { Self::STATEMENT_RESULT }
    fn create_expr_statement(&mut self, _l: &JSTokenLocation, _expr: i32, _start: JSTextPosition, _end: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_if_statement(&mut self, _l: &JSTokenLocation, _condition: i32, _true_block: i32, _false_block: i32, _start: i32, _end: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_for_loop(&mut self, _l: &JSTokenLocation, _init: i32, _cond: i32, _iter: i32, _stmts: i32, _start: i32, _end: i32, _lexical: VariableEnvironment, _closure: bool) -> i32 { Self::STATEMENT_RESULT }
    fn create_for_in_loop(&mut self, _l: &JSTokenLocation, _lhs: i32, _iter: i32, _stmts: i32, _dl: &JSTokenLocation, _es: JSTextPosition, _ed: JSTextPosition, _ee: JSTextPosition, _start: i32, _end: i32, _lexical: VariableEnvironment) -> i32 { Self::STATEMENT_RESULT }
    fn create_for_in_loop_pattern(&mut self, _l: &JSTokenLocation, _pattern: i32, _iter: i32, _stmts: i32, _dl: &JSTokenLocation, _es: JSTextPosition, _ed: JSTextPosition, _ee: JSTextPosition, _start: i32, _end: i32, _lexical: VariableEnvironment) -> i32 { Self::STATEMENT_RESULT }
    fn create_for_of_loop(&mut self, _await: bool, _l: &JSTokenLocation, _lhs: i32, _iter: i32, _stmts: i32, _dl: &JSTokenLocation, _es: JSTextPosition, _ed: JSTextPosition, _ee: JSTextPosition, _start: i32, _end: i32, _lexical: VariableEnvironment) -> i32 { Self::STATEMENT_RESULT }
    fn create_for_of_loop_pattern(&mut self, _await: bool, _l: &JSTokenLocation, _pattern: i32, _iter: i32, _stmts: i32, _dl: &JSTokenLocation, _es: JSTextPosition, _ed: JSTextPosition, _ee: JSTextPosition, _start: i32, _end: i32, _lexical: VariableEnvironment) -> i32 { Self::STATEMENT_RESULT }
    fn create_empty_statement(&mut self, _l: &JSTokenLocation) -> i32 { Self::STATEMENT_RESULT }
    fn create_declaration_statement(&mut self, _l: &JSTokenLocation, _expr: i32, _start: i32, _end: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_return_statement(&mut self, _l: &JSTokenLocation, _expr: i32, _start: JSTextPosition, _end: JSTextPosition) -> i32 { Self::STATEMENT_RESULT }
    fn create_break_statement(&mut self, _l: &JSTokenLocation, _start: JSTextPosition, _end: JSTextPosition) -> i32 { Self::STATEMENT_RESULT }
    fn create_break_statement_label(&mut self, _l: &JSTokenLocation, _ident: &Identifier, _start: JSTextPosition, _end: JSTextPosition) -> i32 { Self::STATEMENT_RESULT }
    fn create_continue_statement(&mut self, _l: &JSTokenLocation, _start: JSTextPosition, _end: JSTextPosition) -> i32 { Self::STATEMENT_RESULT }
    fn create_continue_statement_label(&mut self, _l: &JSTokenLocation, _ident: &Identifier, _start: JSTextPosition, _end: JSTextPosition) -> i32 { Self::STATEMENT_RESULT }
    fn create_try_statement(&mut self, _l: &JSTokenLocation, _try_block: i32, _pattern: i32, _catch_block: i32, _finally_block: i32, _sl: i32, _el: i32, _env: VariableEnvironment) -> i32 { Self::STATEMENT_RESULT }
    fn create_switch_statement(&mut self, _l: &JSTokenLocation, _expr: i32, _first: i32, _default: i32, _second: i32, _sl: i32, _el: i32, _lexical: VariableEnvironment, _stack: FunctionStack) -> i32 { Self::STATEMENT_RESULT }
    fn create_while_statement(&mut self, _l: &JSTokenLocation, _expr: i32, _stmt: i32, _sl: i32, _el: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_with_statement(&mut self, _l: &JSTokenLocation, _expr: i32, _stmt: i32, _start: u32, _end: JSTextPosition, _sl: u32, _el: u32) -> i32 { Self::STATEMENT_RESULT }
    fn create_do_while_statement(&mut self, _l: &JSTokenLocation, _stmt: i32, _expr: i32, _sl: i32, _el: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_label_statement(&mut self, _l: &JSTokenLocation, _ident: &Identifier, _stmt: i32, _start: JSTextPosition, _end: JSTextPosition) -> i32 { Self::STATEMENT_RESULT }
    fn create_throw_statement(&mut self, _l: &JSTokenLocation, _expr: i32, _start: JSTextPosition, _end: JSTextPosition) -> i32 { Self::STATEMENT_RESULT }
    fn create_debugger(&mut self, _l: &JSTokenLocation, _sl: i32, _el: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_module_name(&mut self, _l: &JSTokenLocation, _module_name: &Identifier) -> i32 { Self::MODULE_NAME_RESULT }
    fn create_import_specifier(&mut self, _l: &JSTokenLocation, _imported: &Identifier, _local: &Identifier) -> i32 { Self::IMPORT_SPECIFIER_RESULT }
    fn create_import_specifier_list(&mut self) -> i32 { Self::IMPORT_SPECIFIER_LIST_RESULT }
    fn append_import_specifier(&mut self, _list: &i32, _specifier: i32) {}
    fn create_import_attributes_list(&mut self) -> i32 { Self::IMPORT_ATTRIBUTES_LIST_RESULT }
    fn append_import_assertion(&mut self, _list: &i32, _key: &Identifier, _value: &Identifier) {}
    fn create_import_declaration(&mut self, _l: &JSTokenLocation, _type: ImportType, _specifiers: i32, _module_name: i32, _attributes: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_export_all_declaration(&mut self, _l: &JSTokenLocation, _module_name: i32, _attributes: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_export_default_declaration(&mut self, _l: &JSTokenLocation, _declaration: i32, _local_name: &Identifier) -> i32 { Self::STATEMENT_RESULT }
    fn create_export_local_declaration(&mut self, _l: &JSTokenLocation, _declaration: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_export_named_declaration(&mut self, _l: &JSTokenLocation, _specifiers: i32, _module_name: i32, _attributes: i32) -> i32 { Self::STATEMENT_RESULT }
    fn create_export_specifier(&mut self, _l: &JSTokenLocation, _local: &Identifier, _exported: &Identifier) -> i32 { Self::EXPORT_SPECIFIER_RESULT }
    fn create_export_specifier_list(&mut self) -> i32 { Self::EXPORT_SPECIFIER_LIST_RESULT }
    fn append_export_specifier(&mut self, _list: &i32, _specifier: i32) {}

    fn create_getter_or_setter_property(&mut self, _l: &JSTokenLocation, type_: PropertyNodeType, _name: &Identifier, _info: &ParserFunctionInfo<Self>, _tag: ClassElementTag) -> Property { Property::new(type_) }
    fn create_getter_or_setter_property_computed(&mut self, _l: &JSTokenLocation, type_: PropertyNodeType, _name: i32, _info: &ParserFunctionInfo<Self>, _tag: ClassElementTag) -> Property { Property::new(type_) }
    fn create_getter_or_setter_property_number(&mut self, _vm: &VM, _arena: &mut ParserArena, _l: &JSTokenLocation, type_: PropertyNodeType, _name: f64, _info: &ParserFunctionInfo<Self>, _tag: ClassElementTag) -> Property { Property::new(type_) }

    fn append_statement(&mut self, _elements: &i32, _statement: i32) {}
    fn eval_count(&self) -> i32 { 0 }
    fn append_binary_expression_info(&mut self, operand_stack_depth: &mut i32, expr: i32, _expr_start: JSTextPosition, _lhs: JSTextPosition, _rhs: JSTextPosition, _has_assignments: bool) {
        if self.top_binary_expr == 0 {
            self.top_binary_expr = expr;
        } else {
            self.top_binary_expr = Self::BINARY_EXPR;
        }
        *operand_stack_depth += 1;
    }

    // Estruturas de dados usadas no parse de expressões binárias.
    fn operator_stack_pop(&mut self, operator_stack_depth: &mut i32) { *operator_stack_depth -= 1; }
    fn operator_stack_should_reduce(&mut self, _precedence: i32) -> bool { true }
    fn get_from_operand_stack(&mut self, _i: i32) -> i32 { self.top_binary_expr }
    fn shrink_operand_stack_by(&mut self, operand_stack_depth: &mut i32, amount: i32) { *operand_stack_depth -= amount; }
    fn append_binary_operation(&mut self, _l: &JSTokenLocation, operand_stack_depth: &mut i32, _operator_stack_depth: &mut i32, _lhs: i32, _rhs: i32) { *operand_stack_depth += 1; }
    fn operator_stack_append(&mut self, operator_stack_depth: &mut i32, _op: i32, _precedence: i32) { *operator_stack_depth += 1; }
    fn pop_operand_stack(&mut self, _operand_stack_depth: &mut i32) -> i32 {
        let res = self.top_binary_expr;
        self.top_binary_expr = 0;
        res
    }

    fn append_unary_token(&mut self, stack_depth: &mut i32, tok: i32, _start: JSTextPosition) {
        *stack_depth = 1;
        self.top_unary_token = tok;
    }
    fn unary_token_stack_last_type(&mut self, _stack_depth: &mut i32) -> i32 { self.top_unary_token }
    fn unary_token_stack_last_start(&mut self, _stack_depth: &mut i32) -> JSTextPosition { JSTextPosition::new(0, 0, 0) }
    fn unary_token_stack_remove_last(&mut self, stack_depth: &mut i32) { *stack_depth = 0; }
    fn unary_token_stack_depth(&self) -> i32 { 0 }
    fn set_unary_token_stack_depth(&mut self, _depth: i32) {}

    fn assignment_stack_append(&mut self, assignment_stack_depth: &mut i32, _node: i32, _start: JSTextPosition, _divot: JSTextPosition, _assignment_count: i32, _op: Operator) { *assignment_stack_depth = 1; }
    fn create_assignment(&mut self, _l: &JSTokenLocation, assignment_stack_depth: &mut i32, _rhs: i32, _initial: i32, _current: i32, _last_token_end: JSTextPosition) -> i32 {
        *assignment_stack_depth = 0;
        Self::ASSIGNMENT_EXPR
    }
    fn get_type(&self, property: &Property) -> PropertyNodeType { property.type_ }
    fn is_underscore_proto_setter(&self, property: &Property) -> bool { property.is_underscore_proto_setter }
    fn is_resolve(&self, expr: &i32) -> bool { *expr == Self::RESOLVE_EXPR || *expr == Self::RESOLVE_EVAL_EXPR }
    fn create_destructuring_assignment(&mut self, _l: &JSTokenLocation, _pattern: i32, _initializer: i32) -> i32 { Self::DESTRUCTURING_ASSIGNMENT }

    fn create_array_pattern(&mut self, _l: &JSTokenLocation) -> i32 { Self::ARRAY_DESTRUCTURING }
    fn append_array_pattern_skip_entry(&mut self, _node: &i32, _l: &JSTokenLocation) {}
    fn append_array_pattern_entry(&mut self, _node: &i32, _l: &JSTokenLocation, _pattern: i32, _default_value: i32) {}
    fn append_array_pattern_rest_entry(&mut self, _node: &i32, _l: &JSTokenLocation, _pattern: i32) {}
    fn finish_array_pattern(&mut self, _node: &i32, _divot_start: JSTextPosition, _divot: JSTextPosition, _divot_end: JSTextPosition) {}
    fn create_object_pattern(&mut self, _l: &JSTokenLocation) -> i32 { Self::OBJECT_DESTRUCTURING }
    fn append_object_pattern_entry(&mut self, _node: &i32, _l: &JSTokenLocation, _was_string: bool, _identifier: &Identifier, _pattern: i32, _default_value: i32) {}
    fn append_object_pattern_computed_entry(&mut self, _vm: &VM, _node: &i32, _l: &JSTokenLocation, _property_expression: i32, _pattern: i32, _default_value: i32) {}
    fn append_object_pattern_rest_entry(&mut self, _vm: &VM, _node: &i32, _l: &JSTokenLocation, _pattern: i32) {}
    fn set_contains_object_rest_element(&mut self, _node: &i32, _contains: bool) {}
    fn set_contains_computed_property(&mut self, _node: &i32, _contains: bool) {}
    fn finish_object_pattern(&mut self, _node: &i32, _divot_start: JSTextPosition, _divot: JSTextPosition, _divot_end: JSTextPosition) {}

    fn create_binding_location(&mut self, _l: &JSTokenLocation, _bound_property: &Identifier, _start: JSTextPosition, _end: JSTextPosition, _ctx: AssignmentContext) -> i32 { Self::BINDING_DESTRUCTURING }
    fn create_rest_parameter(&mut self, _pattern: i32, _num_parameters_to_skip: usize) -> i32 { Self::REST_PARAMETER }
    fn create_assignment_element(&mut self, _target: &i32, _start: JSTextPosition, _end: JSTextPosition) -> i32 { Self::BINDING_DESTRUCTURING }

    fn is_binding_node(&self, pattern: &i32) -> bool { *pattern == Self::BINDING_DESTRUCTURING }
    fn is_location(&self, type_: &i32) -> bool {
        *type_ == Self::RESOLVE_EXPR || *type_ == Self::DOT_EXPR || *type_ == Self::PRIVATE_DOT_EXPR || *type_ == Self::BRACKET_EXPR
    }
    fn is_private_location(&self, type_: &i32) -> bool { *type_ == Self::PRIVATE_DOT_EXPR }
    fn is_assignment_location(&self, type_: &i32) -> bool { self.is_location(type_) || *type_ == Self::DESTRUCTURING_ASSIGNMENT }
    fn is_object_literal(&self, type_: &i32) -> bool { *type_ == Self::OBJECT_LITERAL_EXPR }
    fn is_array_literal(&self, type_: &i32) -> bool { *type_ == Self::ARRAY_LITERAL_EXPR }
    fn is_object_or_array_literal(&self, type_: &i32) -> bool { self.is_object_literal(type_) || self.is_array_literal(type_) }
    fn is_function_call(&self, type_: &i32) -> bool { *type_ == Self::CALL_EXPR }

    fn should_skip_pause_location(&self, _statement: &i32) -> bool { true }

    fn set_end_offset<N: TreeNodeHandle>(&mut self, _node: &N, _offset: i32) {}
    fn end_offset<N: TreeNodeHandle>(&mut self, _node: &N) -> i32 { 0 }
    fn set_start_offset<N: TreeNodeHandle>(&mut self, _node: &N, _offset: i32) {}

    fn breakpoint_location<N: TreeNodeHandle>(&mut self, _node: &N) -> JSTextPosition { JSTextPosition::new(0, 0, 0) }

    fn propagate_arguments_use(&mut self) {}

    fn has_arguments_feature(&self) -> bool { true }
}
