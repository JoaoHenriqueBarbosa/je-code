//! Tradução de `JavaScriptCore/parser/ASTBuilder.h`, primeira fatia (linhas 1 a 852: a struct, os tipos
//! associados e os métodos de `createSourceElements` até `createImportDeclaration`). A segunda fatia fica
//! em `ast_builder_part2.rs`, incluída por `include!` dentro do `impl TreeBuilder for ASTBuilder`.
//!
//! Modelo de posse (CONVENTIONS, modelo de dados, item 3). Os nós vivem como `NodeRef<T> = Rc<RefCell<T>>`
//! (`crate::parser::nodes`) e o `ASTBuilder` guarda e devolve esses mesmos ponteiros, como o C++ faz com os
//! `Node*` da arena: o pai só clona o ponteiro do filho (nunca o move), então o `Parser` pode manter o nó
//! que acabou de passar adiante e alterá-lo depois (`setIsOptionalChainBase`, `setEcmaName`, `m_next`).
//! O ponteiro nulo do C++ é `None`, e o `Default` do tipo associado é esse nulo.
//!
//! Tipos associados:
//!
//! - Famílias com enum (`Expression`, `Statement`, `DestructuringPatternNode`) e `FunctionBody`: o próprio
//!   `Option<..>` (o enum é a alça; `FunctionBody` é `Option<Rc<FunctionMetadataNode>>`, o `Rc` que o
//!   `FuncDeclNode`, o `BaseFuncExprNode` e a pilha de funções dividem).
//! - Struct concreta (`ArgumentsNode`, `PropertyNode`, `CaseClauseNode`, `SourceElements`, as listas...):
//!   `Link<T>`, o `Option<NodeRef<T>>` com igualdade de ponteiro. O `Option<NodeRef<T>>` cru não serve porque
//!   o `PartialEq` do `Rc<RefCell<T>>` compara os valores e as structs dos nós não o têm; o `Parser` testa
//!   `x != Default::default()`.
//! - Listas encadeadas (`PropertyList`, `ElementList`, `ArgumentsList`, `ClauseList`, as de template) são o
//!   ponteiro para um nó da cadeia, como no C++ (`typedef PropertyListNode* PropertyList`): o `create*`
//!   sem lista anterior devolve a cabeça, o `create*` com lista anterior liga o novo nó ao rabo recebido
//!   (`X::append`) e devolve o novo rabo. Quem guarda a cabeça é o `Parser`, que a entrega ao nó pai.
//! - `Comma` e `ClassExpression` são `Option<Expression>`; `ArrayPattern`, `ObjectPattern`, `RestPattern` e
//!   `DestructuringPattern` são todos `Option<DestructuringPatternNode>` (o C++ os converte por upcast
//!   implícito); `DefineField` é `Option<Statement>`.
//!
//! Desvios em relação ao C++ (todos mecânicos):
//!
//! - `ASTBuilder(VM&, ParserArena&, SourceCode*)` não guarda a arena nem o ponteiro: a arena só servia à
//!   alocação (que sumiu) e o `SourceCode` é copiado (um `Rc` por baixo). Assim o `Parser` não mantém
//!   empréstimo vivo de `self` enquanto o construtor existe.
//! - `BinaryExprContext` e `UnaryExprContext` não fazem nada no C++ (construtor vazio): são tipos unitários.
//! - `const Identifier*` que o C++ desreferencia sem teste (`*functionInfo.name`, `*classInfo.className`,
//!   `*propertyName`) e ponteiro de nó desreferenciado sem teste são `RELEASE_ASSERT` de que estão
//!   preenchidos (`non_null`, `non_null_ref`, `Link::get`).

// Fatias do `impl TreeBuilder`: o Rust não divide um impl de trait por `include!`, então cada uma é um
// `macro_rules!` expandido dentro do impl.
include!("ast_builder_part2.rs");
include!("ast_builder_part4.rs");

use std::cell::RefCell;
use std::rc::Rc;

use crate::parser::lexer::LexerFlagSet;
use crate::parser::nodes::{
    node as make, ArgumentListNode, ArgumentsNode, ArrayNode, ArrowFuncExprNode, AssignResolveNode,
    AssignmentContext, AwaitExprNode, BigIntNode, BlockNode, BooleanNode, BracketAccessorNode, BreakNode,
    BytecodeIntrinsicNode, BytecodeIntrinsicNodeType, CaseBlockNode, CaseClauseNode, ClassDeclNode,
    ClassElementTag, ClassExprNode, ClauseListNode, ConditionalNode, ContinueNode, DebuggerStatementNode,
    DeclarationStatement, DefineFieldNode, DefineFieldType, DestructuringAssignmentNode,
    DestructuringPatternNode, DoWhileNode, DotAccessorNode, DotType, DoubleNode, ElementNode,
    EmptyLetExpression, EmptyStatementNode, EmptyVarExpression, ExportSpecifierListNode,
    ExportSpecifierNode, Expression, ExprStatementNode, ForInNode, ForNode, ForOfNode, FuncDeclNode,
    FuncExprNode, FunctionMetadataNode, FunctionParameters, FunctionStack, IfElseNode,
    ImportAttributesListNode, ImportDeclarationNode, ImportMetaNode, ImportNode, ImportSpecifierListNode,
    ImportSpecifierNode, ImportType, IntegerNode, LabelNode, LogicalNotNode, MethodDefinitionNode,
    ModuleNameNode, NewExprNode, NewTargetNode, NodeRef, NullNode, ObjectLiteralNode,
    ObjectSpreadExpressionNode, Operator, OptionalChainNode, PrivateIdentifierNode, PropertyListNode,
    PropertyNode, PropertyNodeType, RegExpNode, ResolveNode, ReturnNode, SourceElements,
    SpreadExpressionNode, Statement, StringNode, SuperNode, SwitchNode, TaggedTemplateNode,
    TemplateExpressionListNode, TemplateLiteralNode, TemplateStringListNode, TemplateStringNode, ThisNode,
    ThrowNode, ThrowableExpressionData, TryNode, UnaryPlusNode, VoidNode, WhileNode, WithNode,
    YieldExprNode,
};
use crate::parser::parser::InferName;
use crate::parser::parser_arena::ParserArena;
use crate::parser::parser_function_info::{ParserClassInfo, ParserFunctionInfo};
use crate::parser::parser_modes::{
    LexicallyScopedFeatures, SourceParseMode, SuperBinding, ARGUMENTS_FEATURE, ARROW_FUNCTION_FEATURE,
    AWAIT_FEATURE, EVAL_FEATURE, NEW_TARGET_FEATURE, SUPER_CALL_FEATURE, SUPER_PROPERTY_FEATURE,
    THIS_FEATURE, WITH_FEATURE,
};
use crate::parser::parser_tokens::{JSTextPosition, JSTokenLocation};
use crate::parser::source_code::SourceCode;
use crate::parser::tree_builder::{TreeBuilder, TreeNodeHandle};
use crate::parser::variable_environment::VariableEnvironment;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::vm::VM;
use crate::wtf::math_extras::is_zero_or_unordered;
use crate::yarr::yarr_error_code::has_error;
use crate::yarr::yarr_syntax_checker::check_syntax;

/// `T*` de uma struct concreta de nó: ponteiro compartilhado com o nulo do C++ (`None`, o `Default`) e
/// igualdade de ponteiro (o `==` do C++). Ver a nota do módulo para o porquê de não ser `Option<NodeRef<T>>`.
pub struct Link<T>(Option<NodeRef<T>>);

impl<T> Link<T> {
    /// `new (parserArena) T(...)`.
    pub fn new(value: T) -> Link<T> {
        Link(Some(make(value)))
    }

    pub fn from_ref(node: NodeRef<T>) -> Link<T> {
        Link(Some(node))
    }

    /// `!node`.
    pub fn is_null(&self) -> bool {
        self.0.is_none()
    }

    /// O ponteiro, ou `None` quando nulo.
    pub fn opt(&self) -> Option<NodeRef<T>> {
        self.0.clone()
    }

    /// O ponteiro. `RELEASE_ASSERT`: não é nulo.
    pub fn get(&self) -> &NodeRef<T> {
        self.0.as_ref().expect("RELEASE_ASSERT: alça nula")
    }
}

impl<T> Clone for Link<T> {
    fn clone(&self) -> Link<T> {
        Link(self.0.clone())
    }
}

impl<T> Default for Link<T> {
    fn default() -> Link<T> {
        Link(None)
    }
}

impl<T> PartialEq for Link<T> {
    fn eq(&self, other: &Link<T>) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

/// `RELEASE_ASSERT(node)`: o C++ desreferencia o ponteiro sem testar.
pub fn non_null<T>(value: Option<T>) -> T {
    value.expect("RELEASE_ASSERT: nó nulo")
}

/// `non_null` sem tomar posse.
pub fn non_null_ref<T>(value: &Option<T>) -> &T {
    value.as_ref().expect("RELEASE_ASSERT: nó nulo")
}

/// `Node*` do `ASTBuilder` sobre um `Expression`/`Statement` (e o `Option` deles, o ponteiro anulável).
macro_rules! impl_node_handle {
    ($ty:ty) => {
        impl TreeNodeHandle for $ty {
            fn set_end_offset(&self, offset: i32) {
                self.base_mut().set_end_offset(offset);
            }
            fn end_offset(&self) -> i32 {
                self.base().end_offset()
            }
            fn set_start_offset(&self, offset: i32) {
                self.base_mut().set_start_offset(offset);
            }
            fn breakpoint_location(&self) -> JSTextPosition {
                let mut node = self.base_mut();
                node.set_needs_debug_hook();
                *node.position()
            }
        }

        impl TreeNodeHandle for Option<$ty> {
            fn set_end_offset(&self, offset: i32) {
                non_null_ref(self).set_end_offset(offset);
            }
            fn end_offset(&self) -> i32 {
                non_null_ref(self).end_offset()
            }
            fn set_start_offset(&self, offset: i32) {
                non_null_ref(self).set_start_offset(offset);
            }
            fn breakpoint_location(&self) -> JSTextPosition {
                non_null_ref(self).breakpoint_location()
            }
        }
    };
}

impl_node_handle!(Expression);
impl_node_handle!(Statement);

/// `setStartOffset(CaseClauseNode*, int)`: o `CaseClauseNode` não é um `Node` e o `ASTBuilder` só o usa
/// com esta sobrecarga; as outras três operações não existem para ele no C++.
impl TreeNodeHandle for Link<CaseClauseNode> {
    fn set_end_offset(&self, _offset: i32) {
        panic!("RELEASE_ASSERT_NOT_REACHED");
    }
    fn end_offset(&self) -> i32 {
        panic!("RELEASE_ASSERT_NOT_REACHED");
    }
    fn set_start_offset(&self, offset: i32) {
        self.get().borrow_mut().start_offset = offset;
    }
    fn breakpoint_location(&self) -> JSTextPosition {
        panic!("RELEASE_ASSERT_NOT_REACHED");
    }
}

/// `Node*` do `ASTBuilder` sobre um `FunctionMetadataNode` (que é um `Node`).
impl TreeNodeHandle for Option<Rc<FunctionMetadataNode>> {
    fn set_end_offset(&self, offset: i32) {
        non_null_ref(self).base.borrow_mut().set_end_offset(offset);
    }
    fn end_offset(&self) -> i32 {
        non_null_ref(self).base.borrow().end_offset()
    }
    fn set_start_offset(&self, offset: i32) {
        non_null_ref(self).base.borrow_mut().set_start_offset(offset);
    }
    fn breakpoint_location(&self) -> JSTextPosition {
        let mut node = non_null_ref(self).base.borrow_mut();
        node.set_needs_debug_hook();
        *node.position()
    }
}

/// `ASTBuilder::BinaryOpInfo`.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct BinaryOpInfo {
    pub start: JSTextPosition,
    pub divot: JSTextPosition,
    pub end: JSTextPosition,
    pub has_assignment: bool,
}

impl BinaryOpInfo {
    /// `BinaryOpInfo(otherStart, otherDivot, otherEnd, rhsHasAssignment)`.
    pub fn new(start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition, has_assignment: bool) -> BinaryOpInfo {
        BinaryOpInfo { start, divot, end, has_assignment }
    }

    /// `BinaryOpInfo(lhs, rhs)`.
    pub fn from_operands(lhs: &BinaryOpInfo, rhs: &BinaryOpInfo) -> BinaryOpInfo {
        BinaryOpInfo { start: lhs.start, divot: rhs.start, end: rhs.end, has_assignment: lhs.has_assignment || rhs.has_assignment }
    }
}

/// `ASTBuilder::AssignmentInfo`.
#[derive(Clone)]
pub struct AssignmentInfo {
    pub node: Option<Expression>,
    pub start: JSTextPosition,
    pub divot: JSTextPosition,
    pub init_assignments: i32,
    pub op: Operator,
}

impl AssignmentInfo {
    pub fn new(node: Option<Expression>, start: JSTextPosition, divot: JSTextPosition, init_assignments: i32, op: Operator) -> AssignmentInfo {
        AssignmentInfo { node, start, divot, init_assignments, op }
    }
}

/// `ASTBuilder::BinaryExprContext`: o construtor do C++ não faz nada.
pub struct BinaryExprContext;

/// `ASTBuilder::UnaryExprContext`: idem.
pub struct UnaryExprContext;

/// `ASTBuilder::Scope`.
#[derive(Default)]
struct Scope {
    features: i32,
    num_constants: i32,
}

pub struct ASTBuilder {
    vm: Rc<VM>,
    source_code: SourceCode,
    scope: Scope,
    binary_operand_stack: Vec<(Option<Expression>, BinaryOpInfo)>,
    assignment_info_stack: Vec<AssignmentInfo>,
    binary_operator_stack: Vec<(i32, i32)>,
    unary_token_stack: Vec<(i32, JSTextPosition)>,
    eval_count: i32,
}

/// Membros que não são do `TreeBuilder`: construtor, acessores do escopo e os auxiliares privados do
/// `ASTBuilder.h` (linhas 1125 a 1222) que as duas fatias usam.
impl ASTBuilder {
    pub fn new(vm: Rc<VM>, _parser_arena: &mut ParserArena, source_code: &SourceCode) -> ASTBuilder {
        ASTBuilder {
            vm,
            source_code: source_code.clone(),
            scope: Scope::default(),
            binary_operand_stack: Vec::new(),
            assignment_info_stack: Vec::new(),
            binary_operator_stack: Vec::new(),
            unary_token_stack: Vec::new(),
            eval_count: 0,
        }
    }

    pub fn features(&self) -> i32 {
        self.scope.features
    }

    pub fn num_constants(&self) -> i32 {
        self.scope.num_constants
    }

    /// `ASTBuilder::checkArgumentsLengthModification`.
    fn check_arguments_length_modification(&mut self, node: &Option<Expression>) {
        // Since we exclude pattern `arguments.length` to enable ArgumentsFeature,
        // we need re-enable ArgumentsFeature for `arguments.length` modification.
        if node.as_ref().is_some_and(|expression| expression.is_arguments_length_access(&self.vm)) {
            self.uses_arguments();
        }
    }

    /// `ASTBuilder::setExceptionLocation(ThrowableExpressionData*, divotStart, divot, divotEnd)`.
    fn set_exception_location(node: &mut ThrowableExpressionData, divot_start: JSTextPosition, divot: JSTextPosition, divot_end: JSTextPosition) {
        node.set_exception_source_code(divot, divot_start, divot_end);
    }

    fn inc_constants(&mut self) {
        self.scope.num_constants += 1;
    }

    fn uses_this(&mut self) {
        self.scope.features |= THIS_FEATURE as i32;
    }

    fn uses_arrow_function(&mut self) {
        self.scope.features |= ARROW_FUNCTION_FEATURE as i32;
    }

    fn uses_arguments(&mut self) {
        self.scope.features |= ARGUMENTS_FEATURE as i32;
    }

    fn uses_with(&mut self) {
        self.scope.features |= WITH_FEATURE as i32;
    }

    fn uses_super_call(&mut self) {
        self.scope.features |= SUPER_CALL_FEATURE as i32;
    }

    fn uses_super_property(&mut self) {
        self.scope.features |= SUPER_PROPERTY_FEATURE as i32;
    }

    fn uses_eval(&mut self) {
        self.eval_count += 1;
        self.scope.features |= EVAL_FEATURE as i32;
    }

    fn uses_new_target(&mut self) {
        self.scope.features |= NEW_TARGET_FEATURE as i32;
    }

    fn uses_await(&mut self) {
        self.scope.features |= AWAIT_FEATURE as i32;
    }

    /// `static_cast<BaseFuncExprNode*>(node)->metadata()->setEcmaName(ident)`.
    fn set_metadata_ecma_name(metadata: &FunctionMetadataNode, ident: &Identifier) {
        *metadata.ecma_name.borrow_mut() = ident.clone();
    }

    /// O par de ramos que o C++ repete em `createAssignResolve`, `createDefineField`, `createProperty`,
    /// `makeAssignNode` e `tryInferNameInPatternWithIdentifier`:
    /// `if (node->isBaseFuncExprNode()) metadata->setEcmaName(ident); else if (node->isClassExprNode()) classExpr->setEcmaName(ident);`.
    fn set_ecma_name_of_function_or_class(node: &Expression, ident: &Identifier) {
        match node {
            Expression::FuncExpr(n) => Self::set_metadata_ecma_name(&n.borrow().metadata, ident),
            Expression::ArrowFuncExpr(n) => Self::set_metadata_ecma_name(&n.borrow().metadata, ident),
            Expression::MethodDefinition(n) => Self::set_metadata_ecma_name(&n.borrow().metadata, ident),
            Expression::ClassExpr(n) => n.borrow_mut().set_ecma_name(ident),
            _ => {}
        }
    }

    /// `ASTBuilder::tryInferNameInPattern` (com `tryInferNameInPatternWithIdentifier` no corpo).
    fn try_infer_name_in_pattern(pattern: &DestructuringPatternNode, default_value: &Option<Expression>) {
        let Some(default_value) = default_value else {
            return;
        };

        let ident = match pattern {
            DestructuringPatternNode::Binding(binding) => Some(binding.borrow().bound_property.clone()),
            DestructuringPatternNode::AssignmentElement(element) => match &element.borrow().assignment_target {
                Expression::Resolve(resolve) => Some(resolve.borrow().ident.clone()),
                _ => None,
            },
            _ => None,
        };
        if let Some(ident) = ident {
            Self::set_ecma_name_of_function_or_class(default_value, &ident);
        }
    }

    /// `*functionInfo.name` e `*classInfo.className`: o C++ desreferencia sem teste.
    fn non_null_name(name: &Option<Identifier>) -> &Identifier {
        name.as_ref().expect("RELEASE_ASSERT: nome nulo")
    }

    /// `functionInfo.body`, que o C++ desreferencia sem teste.
    fn body_of(function_info: &ParserFunctionInfo<ASTBuilder>) -> &Rc<FunctionMetadataNode> {
        function_info.body.as_ref().expect("RELEASE_ASSERT: FunctionMetadataNode nulo")
    }

    /// `m_sourceCode->subExpression(functionInfo.startOffset, endOffset, functionInfo.startLine, functionInfo.parametersStartColumn)`.
    fn function_source(&self, function_info: &ParserFunctionInfo<ASTBuilder>, end_offset: u32) -> SourceCode {
        self.source_code.sub_expression(function_info.start_offset, end_offset, function_info.start_line, function_info.parameters_start_column as i32)
    }

    /// O `endOffset` das funções seta: `isArrowFunctionBodyExpression() ? endOffset - 1 : endOffset`.
    fn arrow_function_end_offset(function_info: &ParserFunctionInfo<ASTBuilder>) -> u32 {
        if Self::body_of(function_info).is_arrow_function_body_expression {
            function_info.end_offset - 1
        } else {
            function_info.end_offset
        }
    }

    /// `functionInfo.body->setLoc(functionInfo.startLine, functionInfo.endLine, location.startOffset, location.lineStartOffset)`.
    fn set_function_body_loc(function_info: &ParserFunctionInfo<ASTBuilder>, location: &JSTokenLocation) {
        Self::body_of(function_info).set_loc(function_info.start_line as u32, function_info.end_line as u32, location.start_offset as i32, location.line_start_offset as i32);
    }

    /// `result->setLoc(first, last, location.startOffset, location.lineStartOffset)` dos statements.
    fn located_statement(result: Statement, first_line: u32, last_line: u32, location: &JSTokenLocation) -> Option<Statement> {
        result.base_mut().set_loc(first_line, last_line, location.start_offset as i32, location.line_start_offset as i32);
        Some(result)
    }

    /// `result->setLoc(start.line, lastLine, start.offset, start.lineStartOffset)` dos statements com posição.
    fn statement_at(result: Statement, start: JSTextPosition, last_line: u32) -> Option<Statement> {
        result.base_mut().set_loc(start.line as u32, last_line, start.offset, start.line_start_offset);
        Some(result)
    }

    /// O corpo comum de `createFunctionExpr` e das duas rotas de `createGeneratorFunctionBody` e
    /// `createAsyncFunctionBody` que o chamam: o `FuncExprNode` e o `setLoc` do metadado.
    fn build_func_expr(&self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> Expression {
        let source = self.function_source(function_info, function_info.end_offset);
        let result = FuncExprNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(Self::body_of(function_info)), &source);
        Self::set_function_body_loc(function_info, location);
        Expression::FuncExpr(make(result))
    }

    /// `getter` e `setter` com nome fixo ou numérico: o `MethodDefinitionNode` com o corpo.
    fn build_accessor_method_definition(&self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>, null_identifier: &Identifier) -> Expression {
        let source = self.function_source(function_info, function_info.end_offset);
        Expression::MethodDefinition(make(MethodDefinitionNode::new(location, null_identifier, Rc::clone(Self::body_of(function_info)), &source)))
    }
}

/// `TemplateLiteralNode*` para `ExpressionNode*` (o `TemplateLiteralNode` é uma expressão).
impl From<Link<TemplateLiteralNode>> for Option<Expression> {
    fn from(literal: Link<TemplateLiteralNode>) -> Option<Expression> {
        literal.opt().map(Expression::TemplateLiteral)
    }
}

impl TreeBuilder for ASTBuilder {
    type Expression = Option<Expression>;
    type SourceElements = Link<SourceElements>;
    type Arguments = Link<ArgumentsNode>;
    type Comma = Option<Expression>;
    type Property = Link<PropertyNode>;
    type PropertyList = Link<PropertyListNode>;
    type ElementList = Link<ElementNode>;
    type ArgumentsList = Link<ArgumentListNode>;
    type TemplateExpressionList = Link<TemplateExpressionListNode>;
    type TemplateString = Link<TemplateStringNode>;
    type TemplateStringList = Link<TemplateStringListNode>;
    type TemplateLiteral = Link<TemplateLiteralNode>;
    type FormalParameterList = Link<FunctionParameters>;
    type FunctionBody = Option<Rc<FunctionMetadataNode>>;
    type ClassExpression = Option<Expression>;
    type ModuleName = Link<ModuleNameNode>;
    type ImportSpecifier = Link<ImportSpecifierNode>;
    type ImportSpecifierList = Link<ImportSpecifierListNode>;
    type ImportAttributesList = Link<ImportAttributesListNode>;
    type ExportSpecifier = Link<ExportSpecifierNode>;
    type ExportSpecifierList = Link<ExportSpecifierListNode>;
    type Statement = Option<Statement>;
    type ClauseList = Link<ClauseListNode>;
    type Clause = Link<CaseClauseNode>;
    type BinaryOperand = (Option<Expression>, BinaryOpInfo);
    type DestructuringPattern = Option<DestructuringPatternNode>;
    type ArrayPattern = Option<DestructuringPatternNode>;
    type ObjectPattern = Option<DestructuringPatternNode>;
    type RestPattern = Option<DestructuringPatternNode>;
    type DefineField = Option<Statement>;
    type BinaryExprContext = BinaryExprContext;
    type UnaryExprContext = UnaryExprContext;

    const CREATES_AST: bool = true;
    const NEEDS_FREE_VARIABLE_INFO: bool = true;
    const CAN_USE_FUNCTION_CACHE: bool = true;
    const DONT_BUILD_KEYWORDS: LexerFlagSet = LexerFlagSet::empty();
    const DONT_BUILD_STRINGS: LexerFlagSet = LexerFlagSet::empty();

    fn begin_binary_expr_context(&mut self) -> BinaryExprContext {
        BinaryExprContext
    }
    fn end_binary_expr_context(&mut self, _saved: BinaryExprContext) {}
    fn begin_unary_expr_context(&mut self) -> UnaryExprContext {
        UnaryExprContext
    }
    fn end_unary_expr_context(&mut self, _saved: UnaryExprContext) {}

    fn dot_accessor_identifier(&self, expression: &Option<Expression>) -> Option<Identifier> {
        match expression {
            Some(Expression::DotAccessor(dot)) => Some(dot.borrow().ident.clone()),
            _ => None,
        }
    }

    /// `classElements->setHasPrivateAccessors(...)` (`Parser.cpp`): o `classElements` do C++ é a cabeça.
    fn set_has_private_accessors(&self, class_elements: &Link<PropertyListNode>, has_private_accessors: bool) {
        if let Some(head) = class_elements.opt() {
            head.borrow_mut().has_private_accessors = has_private_accessors;
        }
    }

    fn set_function_body_ecma_name(body: &Option<Rc<FunctionMetadataNode>>, name: &Identifier) {
        Self::set_metadata_ecma_name(body.as_ref().expect("RELEASE_ASSERT: FunctionMetadataNode nulo"), name);
    }

    /// `static FunctionMetadataNode* getMetadata(ParserFunctionInfo<ASTBuilder>& info)`.
    fn get_metadata(function_info: &ParserFunctionInfo<ASTBuilder>) -> Rc<FunctionMetadataNode> {
        Self::body_of(function_info).clone()
    }

    fn create_source_elements(&mut self) -> Link<SourceElements> {
        Link::new(SourceElements::new())
    }

    fn create_logical_not(&mut self, location: &JSTokenLocation, expr: Option<Expression>) -> Option<Expression> {
        let expr = non_null(expr);
        let number = match &expr {
            Expression::Double(node) => Some(node.borrow().value),
            Expression::Integer(node) => Some(node.borrow().value),
            _ => None,
        };
        if let Some(value) = number {
            return self.create_boolean(location, is_zero_or_unordered(value));
        }

        Some(Expression::LogicalNot(make(LogicalNotNode::new(location, expr))))
    }

    fn create_unary_plus(&mut self, location: &JSTokenLocation, expr: Option<Expression>) -> Option<Expression> {
        Some(Expression::UnaryPlus(make(UnaryPlusNode::new(location, non_null(expr)))))
    }

    fn create_void(&mut self, location: &JSTokenLocation, expr: Option<Expression>) -> Option<Expression> {
        self.inc_constants();
        Some(Expression::Void(make(VoidNode::new(location, non_null(expr)))))
    }

    fn create_this_expr(&mut self, location: &JSTokenLocation) -> Option<Expression> {
        self.uses_this();
        Some(Expression::This(make(ThisNode::new(location))))
    }

    fn create_super_expr(&mut self, location: &JSTokenLocation) -> Option<Expression> {
        Some(Expression::Super(make(SuperNode::new(location))))
    }

    fn create_import_expr(&mut self, location: &JSTokenLocation, expr: Option<Expression>, option: Option<Expression>, deferred: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let mut node = ImportNode::new(location, non_null(expr), option, deferred);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::Import(make(node)))
    }

    fn create_new_target_expr(&mut self, location: &JSTokenLocation) -> Option<Expression> {
        self.uses_new_target();
        Some(Expression::NewTarget(make(NewTargetNode::new(location))))
    }

    fn create_import_meta_expr(&mut self, location: &JSTokenLocation, expr: Option<Expression>) -> Option<Expression> {
        Some(Expression::ImportMeta(make(ImportMetaNode::new(location, non_null(expr)))))
    }

    fn is_meta_property(&mut self, expr: &Option<Expression>) -> bool {
        non_null_ref(expr).is_meta_property()
    }

    fn is_new_target(&mut self, expr: &Option<Expression>) -> bool {
        non_null_ref(expr).is_new_target()
    }

    fn is_import_meta(&mut self, expr: &Option<Expression>) -> bool {
        non_null_ref(expr).is_import_meta()
    }

    fn create_resolve(&mut self, location: &JSTokenLocation, ident: &Identifier, start: JSTextPosition, end: JSTextPosition, need_to_check_uses_arguments: bool) -> Option<Expression> {
        if need_to_check_uses_arguments && self.vm.property_names.arguments == *ident {
            self.uses_arguments();
        }

        if ident.is_symbol() {
            if let Some(entry) = self.vm.bytecode_intrinsic_registry().lookup(ident) {
                return Some(Expression::BytecodeIntrinsic(make(BytecodeIntrinsicNode::new(BytecodeIntrinsicNodeType::Constant, location, entry, ident.clone(), None, start, start, end))));
            }
        }

        Some(Expression::Resolve(make(ResolveNode::new(location, ident.clone(), start))))
    }

    fn create_private_identifier_node(&mut self, location: &JSTokenLocation, ident: &Identifier) -> Option<Expression> {
        Some(Expression::PrivateIdentifier(make(PrivateIdentifierNode::new(location, ident.clone()))))
    }

    fn create_object_literal(&mut self, location: &JSTokenLocation) -> Option<Expression> {
        Some(Expression::ObjectLiteral(make(ObjectLiteralNode::new(location))))
    }

    fn create_object_literal_with_properties(&mut self, location: &JSTokenLocation, properties: Link<PropertyListNode>) -> Option<Expression> {
        Some(Expression::ObjectLiteral(make(ObjectLiteralNode::with_list(location, properties.opt()))))
    }

    fn create_array_elisions(&mut self, location: &JSTokenLocation, elisions: i32) -> Option<Expression> {
        if elisions != 0 {
            self.inc_constants();
        }
        Some(Expression::Array(make(ArrayNode::new(location, elisions))))
    }

    fn create_array_elements(&mut self, location: &JSTokenLocation, elems: Link<ElementNode>) -> Option<Expression> {
        Some(Expression::Array(make(ArrayNode::from_elements(location, elems.opt()))))
    }

    fn create_array_elisions_elements(&mut self, location: &JSTokenLocation, elisions: i32, elems: Link<ElementNode>) -> Option<Expression> {
        if elisions != 0 {
            self.inc_constants();
        }
        Some(Expression::Array(make(ArrayNode::with_elision(location, elisions, elems.opt()))))
    }

    fn create_double_expr(&mut self, location: &JSTokenLocation, d: f64) -> Option<Expression> {
        self.inc_constants();
        Some(Expression::Double(make(DoubleNode::new(location, d))))
    }

    fn create_integer_expr(&mut self, location: &JSTokenLocation, d: f64) -> Option<Expression> {
        self.inc_constants();
        Some(Expression::Integer(make(IntegerNode::new(location, d))))
    }

    fn create_big_int(&mut self, location: &JSTokenLocation, big_int: &Identifier, radix: u8) -> Option<Expression> {
        self.inc_constants();
        Some(Expression::BigInt(make(BigIntNode::new(location, big_int.clone(), radix))))
    }

    fn create_string(&mut self, location: &JSTokenLocation, string: &Identifier) -> Option<Expression> {
        self.inc_constants();
        Some(Expression::String(make(StringNode::new(location, string.clone()))))
    }

    fn create_boolean(&mut self, location: &JSTokenLocation, b: bool) -> Option<Expression> {
        self.inc_constants();
        Some(Expression::Boolean(make(BooleanNode::new(location, b))))
    }

    fn create_null(&mut self, location: &JSTokenLocation) -> Option<Expression> {
        self.inc_constants();
        Some(Expression::Null(make(NullNode::new(location))))
    }

    fn create_bracket_access(&mut self, location: &JSTokenLocation, base: Option<Expression>, property: Option<Expression>, property_has_assignments: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let base = non_null(base);
        if base.is_super_node() {
            self.uses_super_property();
        }

        let mut node = BracketAccessorNode::new(location, base, non_null(property), property_has_assignments);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::BracketAccessor(make(node)))
    }

    fn create_dot_access(&mut self, location: &JSTokenLocation, base: Option<Expression>, property: &Identifier, type_: DotType, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let base = non_null(base);
        if base.is_super_node() {
            self.uses_super_property();
        }

        let mut node = DotAccessorNode::new(location, base, property.clone(), type_);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::DotAccessor(make(node)))
    }

    fn create_spread_expression(&mut self, location: &JSTokenLocation, expression: Option<Expression>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let mut node = SpreadExpressionNode::new(location, non_null(expression));
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::SpreadExpression(make(node)))
    }

    fn create_object_spread_expression(&mut self, location: &JSTokenLocation, expression: Option<Expression>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let mut node = ObjectSpreadExpressionNode::new(location, non_null(expression));
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::ObjectSpreadExpression(make(node)))
    }

    fn create_template_string(&mut self, location: &JSTokenLocation, cooked: Option<&Identifier>, raw: Option<&Identifier>) -> Link<TemplateStringNode> {
        Link::new(TemplateStringNode::new(location, cooked.cloned(), raw.cloned()))
    }

    fn create_template_string_list(&mut self, template_string: Link<TemplateStringNode>) -> Link<TemplateStringListNode> {
        Link::new(TemplateStringListNode::new(template_string.get().clone()))
    }

    fn create_template_string_list_append(&mut self, template_string_list: Link<TemplateStringListNode>, template_string: Link<TemplateStringNode>) -> Link<TemplateStringListNode> {
        Link::from_ref(TemplateStringListNode::append(template_string_list.get(), template_string.get().clone()))
    }

    fn create_template_expression_list(&mut self, expression: Option<Expression>) -> Link<TemplateExpressionListNode> {
        Link::new(TemplateExpressionListNode::new(non_null(expression)))
    }

    fn create_template_expression_list_append(&mut self, template_expression_list: Link<TemplateExpressionListNode>, expression: Option<Expression>) -> Link<TemplateExpressionListNode> {
        Link::from_ref(TemplateExpressionListNode::append(template_expression_list.get(), non_null(expression)))
    }

    fn create_template_literal(&mut self, location: &JSTokenLocation, template_string_list: Link<TemplateStringListNode>) -> Link<TemplateLiteralNode> {
        Link::new(TemplateLiteralNode::new(location, template_string_list.opt()))
    }

    fn create_template_literal_with_expressions(&mut self, location: &JSTokenLocation, template_string_list: Link<TemplateStringListNode>, template_expression_list: Link<TemplateExpressionListNode>) -> Link<TemplateLiteralNode> {
        Link::new(TemplateLiteralNode::with_expressions(location, template_string_list.opt(), template_expression_list.opt()))
    }

    fn create_tagged_template(&mut self, location: &JSTokenLocation, base: Option<Expression>, template_literal: Link<TemplateLiteralNode>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let mut node = TaggedTemplateNode::new(location, non_null(base), template_literal.get().clone());
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        node.set_end_offset(end.offset);
        Some(Expression::TaggedTemplate(make(node)))
    }

    fn create_reg_exp(&mut self, location: &JSTokenLocation, pattern: &Identifier, flags: &Identifier, start: JSTextPosition, skip_syntax_check: bool) -> Option<Expression> {
        if !skip_syntax_check && has_error(check_syntax(pattern.string(), flags.string())) {
            return None;
        }
        let mut node = RegExpNode::new(location, pattern.clone(), flags.clone());
        let size = pattern.length() as i32 + 2; // + 2 for the two /'s
        let end = start + size;
        Self::set_exception_location(&mut node.throwable, start, end, end);
        Some(Expression::RegExp(make(node)))
    }

    fn create_new_expr(&mut self, location: &JSTokenLocation, expr: Option<Expression>, arguments: Link<ArgumentsNode>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let mut node = NewExprNode::with_args(location, non_null(expr), arguments.opt());
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::NewExpr(make(node)))
    }

    fn create_new_expr_no_arguments(&mut self, location: &JSTokenLocation, expr: Option<Expression>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let mut node = NewExprNode::new(location, non_null(expr));
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::NewExpr(make(node)))
    }

    fn create_optional_chain(&mut self, location: &JSTokenLocation, base: Option<Expression>, expr: Option<Expression>, is_outermost: bool) -> Option<Expression> {
        if let Some(base) = &base {
            base.base_mut().set_is_optional_chain_base();
        }
        Some(Expression::OptionalChain(make(OptionalChainNode::new(location, non_null(expr), is_outermost))))
    }

    fn create_conditional_expr(&mut self, location: &JSTokenLocation, condition: Option<Expression>, lhs: Option<Expression>, rhs: Option<Expression>) -> Option<Expression> {
        Some(Expression::Conditional(make(ConditionalNode::new(location, non_null(condition), non_null(lhs), non_null(rhs)))))
    }

    fn create_assign_resolve(&mut self, location: &JSTokenLocation, ident: &Identifier, rhs: Option<Expression>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition, assignment_context: AssignmentContext) -> Option<Expression> {
        let rhs = non_null(rhs);
        Self::set_ecma_name_of_function_or_class(&rhs, ident);
        if assignment_context == AssignmentContext::AwaitUsingDeclarationStatement {
            self.uses_await();
        }
        let mut node = AssignResolveNode::new(location, ident.clone(), rhs, assignment_context);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::AssignResolve(make(node)))
    }

    fn create_empty_var_expression(&mut self, location: &JSTokenLocation, ident: &Identifier) -> Option<Expression> {
        Some(Expression::EmptyVarExpression(make(EmptyVarExpression::new(location, ident.clone()))))
    }

    fn create_empty_let_expression(&mut self, location: &JSTokenLocation, ident: &Identifier) -> Option<Expression> {
        Some(Expression::EmptyLetExpression(make(EmptyLetExpression::new(location, ident.clone()))))
    }

    fn create_yield(&mut self, location: &JSTokenLocation) -> Option<Expression> {
        Some(Expression::YieldExpr(make(YieldExprNode::new(location, None, false))))
    }

    fn create_yield_argument(&mut self, location: &JSTokenLocation, argument: Option<Expression>, delegate: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let mut node = YieldExprNode::new(location, argument, delegate);
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::YieldExpr(make(node)))
    }

    fn create_await(&mut self, location: &JSTokenLocation, argument: Option<Expression>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        self.uses_await();
        let mut node = AwaitExprNode::new(location, non_null(argument));
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::AwaitExpr(make(node)))
    }

    fn create_define_field(&mut self, location: &JSTokenLocation, ident: &Identifier, initializer: Option<Expression>, type_: DefineFieldType) -> Option<Statement> {
        if let Some(initializer) = &initializer {
            if type_ != DefineFieldType::ComputedName {
                Self::set_ecma_name_of_function_or_class(initializer, ident);
            }
        }
        Some(Statement::DefineField(make(DefineFieldNode::new(location, ident.clone(), initializer, type_))))
    }

    fn create_class_expr(&mut self, location: &JSTokenLocation, class_info: &ParserClassInfo<ASTBuilder>, class_head_environment: VariableEnvironment, class_environment: VariableEnvironment, constructor: Option<Expression>, parent_class: Option<Expression>, class_elements: Link<PropertyListNode>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
        let source = self.source_code.sub_expression(class_info.start_offset, class_info.end_offset, class_info.start_line, class_info.start_column as i32);
        let mut node = ClassExprNode::new(location, Self::non_null_name(&class_info.class_name).clone(), source, class_head_environment, class_environment, constructor, parent_class, class_elements.opt());
        Self::set_exception_location(&mut node.throwable, start, divot, end);
        Some(Expression::ClassExpr(make(node)))
    }

    fn create_function_expr(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> Option<Expression> {
        Some(self.build_func_expr(location, function_info))
    }

    fn create_generator_function_body(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>, name: &Identifier) -> Option<Expression> {
        let result = self.build_func_expr(location, function_info);
        if !name.is_null() {
            Self::set_metadata_ecma_name(Self::body_of(function_info), name);
        }
        Some(result)
    }

    fn create_async_function_body(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>, parse_mode: SourceParseMode, name: &Identifier) -> Option<Expression> {
        if parse_mode == SourceParseMode::AsyncArrowFunctionBodyMode {
            let source = self.function_source(function_info, Self::arrow_function_end_offset(function_info));
            let result = FuncExprNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(Self::body_of(function_info)), &source);
            if !name.is_null() {
                Self::set_metadata_ecma_name(Self::body_of(function_info), name);
            }
            Self::set_function_body_loc(function_info, location);
            return Some(Expression::FuncExpr(make(result)));
        }
        let result = self.build_func_expr(location, function_info);
        if !name.is_null() {
            Self::set_metadata_ecma_name(Self::body_of(function_info), name);
        }
        Some(result)
    }

    fn create_method_definition(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> Option<Expression> {
        let source = self.function_source(function_info, function_info.end_offset);
        let result = MethodDefinitionNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(Self::body_of(function_info)), &source);
        Self::set_function_body_loc(function_info, location);
        Some(Expression::MethodDefinition(make(result)))
    }

    fn create_function_metadata(&mut self, start_location: &JSTokenLocation, end_location: &JSTokenLocation, start_column: u32, end_column: u32, function_start: u32, function_name_start: i32, parameters_start: i32, implementation_visibility: ImplementationVisibility, lexically_scoped_features: LexicallyScopedFeatures, constructor_kind: ConstructorKind, super_binding: SuperBinding, parameter_count: u32, mode: SourceParseMode, is_arrow_function_body_expression: bool) -> Option<Rc<FunctionMetadataNode>> {
        Some(Rc::new(FunctionMetadataNode::new(start_location, end_location, start_column, end_column, function_start, function_name_start, parameters_start, implementation_visibility, lexically_scoped_features, constructor_kind, super_binding, parameter_count, mode, is_arrow_function_body_expression)))
    }

    fn create_arrow_function_expr(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> Option<Expression> {
        self.uses_arrow_function();
        let source = self.function_source(function_info, Self::arrow_function_end_offset(function_info));
        let result = ArrowFuncExprNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(Self::body_of(function_info)), &source);
        Self::set_function_body_loc(function_info, location);
        Some(Expression::ArrowFuncExpr(make(result)))
    }

    fn set_function_name_start(&mut self, _body: &Option<Rc<FunctionMetadataNode>>, _function_name_start: i32) {
        // O `ASTBuilder.h` não declara `setFunctionNameStart`: só o `SyntaxChecker.h` o traz (`{ }`) e o
        // `Parser.cpp` não o chama. O trait o herda do `SyntaxChecker`, então aqui também não faz nada.
    }

    fn create_arguments(&mut self) -> Link<ArgumentsNode> {
        Link::new(ArgumentsNode::new())
    }

    fn create_arguments_with_list(&mut self, args: Link<ArgumentListNode>, has_assignments: bool) -> Link<ArgumentsNode> {
        Link::new(ArgumentsNode::with_list(args.opt(), has_assignments))
    }

    fn create_arguments_list(&mut self, location: &JSTokenLocation, arg: Option<Expression>) -> Link<ArgumentListNode> {
        Link::new(ArgumentListNode::new(location, non_null(arg)))
    }

    fn create_arguments_list_append(&mut self, location: &JSTokenLocation, args: Link<ArgumentListNode>, arg: Option<Expression>) -> Link<ArgumentListNode> {
        Link::from_ref(ArgumentListNode::append(args.get(), location, non_null(arg)))
    }

    fn create_getter_or_setter_property(&mut self, location: &JSTokenLocation, type_: PropertyNodeType, name: &Identifier, function_info: &ParserFunctionInfo<ASTBuilder>, tag: ClassElementTag) -> Link<PropertyNode> {
        Self::set_function_body_loc(function_info, location);
        Self::set_metadata_ecma_name(Self::body_of(function_info), name);
        let null_identifier = self.vm.property_names.null_identifier.clone();
        let method_def = self.build_accessor_method_definition(location, function_info, &null_identifier);
        Link::new(PropertyNode::from_name_and_assign(name.clone(), Some(method_def), type_, SuperBinding::Needed, tag))
    }

    fn create_getter_or_setter_property_computed(&mut self, location: &JSTokenLocation, type_: PropertyNodeType, name: Option<Expression>, function_info: &ParserFunctionInfo<ASTBuilder>, tag: ClassElementTag) -> Link<PropertyNode> {
        Self::set_function_body_loc(function_info, location);
        let null_identifier = self.vm.property_names.null_identifier.clone();
        let method_def = self.build_accessor_method_definition(location, function_info, &null_identifier);
        Link::new(PropertyNode::from_expression_and_assign(non_null(name), Some(method_def), type_, SuperBinding::Needed, tag))
    }

    fn create_getter_or_setter_property_number(&mut self, vm: &VM, parser_arena: &mut ParserArena, location: &JSTokenLocation, type_: PropertyNodeType, name: f64, function_info: &ParserFunctionInfo<ASTBuilder>, tag: ClassElementTag) -> Link<PropertyNode> {
        Self::set_function_body_loc(function_info, location);
        let ident = parser_arena.identifier_arena().borrow_mut().make_numeric_identifier(vm, name);
        Self::set_metadata_ecma_name(Self::body_of(function_info), &ident);
        let method_def = self.build_accessor_method_definition(location, function_info, &vm.property_names.null_identifier);
        Link::new(PropertyNode::from_name_and_assign(ident, Some(method_def), type_, SuperBinding::Needed, tag))
    }

    fn create_property_identifier(&mut self, property_name: &Identifier, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Link<PropertyNode> {
        Link::new(PropertyNode::from_name(property_name.clone(), type_, super_binding, tag))
    }

    fn create_property_named(&mut self, name: Option<&Identifier>, node: Option<Expression>, type_: PropertyNodeType, super_binding: SuperBinding, infer_name: InferName, tag: ClassElementTag) -> Link<PropertyNode> {
        let property_name = name.expect("RELEASE_ASSERT: nome de propriedade nulo");
        if let (InferName::Allowed, Some(node)) = (infer_name, &node) {
            Self::set_ecma_name_of_function_or_class(node, property_name);
        }
        Link::new(PropertyNode::from_name_and_assign(property_name.clone(), node, type_, super_binding, tag))
    }

    fn create_property_expression(&mut self, node: Option<Expression>, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Link<PropertyNode> {
        Link::new(PropertyNode::from_assign(non_null(node), type_, super_binding, tag))
    }

    fn create_property_number(&mut self, vm: &VM, parser_arena: &mut ParserArena, property_name: f64, node: Option<Expression>, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Link<PropertyNode> {
        let ident = parser_arena.identifier_arena().borrow_mut().make_numeric_identifier(vm, property_name);
        Link::new(PropertyNode::from_name_and_assign(ident, node, type_, super_binding, tag))
    }

    fn create_property_computed(&mut self, property_name: Option<Expression>, node: Option<Expression>, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Link<PropertyNode> {
        Link::new(PropertyNode::from_expression_and_assign(non_null(property_name), node, type_, super_binding, tag))
    }

    fn create_property_identifier_computed(&mut self, identifier: &Identifier, property_name: Option<Expression>, node: Option<Expression>, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Link<PropertyNode> {
        Link::new(PropertyNode::from_name_expression_and_assign(identifier.clone(), non_null(property_name), node, type_, super_binding, tag))
    }

    fn create_property_list(&mut self, location: &JSTokenLocation, property: Link<PropertyNode>) -> Link<PropertyListNode> {
        Link::new(PropertyListNode::new(location, property.get().clone()))
    }

    fn create_property_list_append(&mut self, location: &JSTokenLocation, property: Link<PropertyNode>, tail: Link<PropertyListNode>) -> Link<PropertyListNode> {
        Link::from_ref(PropertyListNode::append(tail.get(), location, property.get().clone()))
    }

    fn create_element_list(&mut self, elisions: i32, expr: Option<Expression>) -> Link<ElementNode> {
        Link::new(ElementNode::new(elisions, non_null(expr)))
    }

    fn create_element_list_append(&mut self, elems: Link<ElementNode>, elisions: i32, expr: Option<Expression>) -> Link<ElementNode> {
        Link::from_ref(ElementNode::append(elems.get(), elisions, non_null(expr)))
    }

    fn create_element_list_from_arguments(&mut self, elems: Link<ArgumentListNode>) -> Link<ElementNode> {
        let first = elems.get().borrow();
        let head = Link::new(ElementNode::new(0, first.expr.clone()));
        let mut tail = head.get().clone();
        let mut next = first.next.clone();
        while let Some(current) = next {
            let current = current.borrow();
            tail = ElementNode::append(&tail, 0, current.expr.clone());
            next = current.next.clone();
        }
        head
    }

    fn create_formal_parameter_list(&mut self) -> Link<FunctionParameters> {
        Link::new(FunctionParameters::new())
    }

    fn append_parameter(&mut self, list: &Link<FunctionParameters>, pattern: Option<DestructuringPatternNode>, default_value: Option<Expression>) {
        Self::try_infer_name_in_pattern(non_null_ref(&pattern), &default_value);
        list.get().borrow_mut().append(non_null(pattern), default_value);
    }

    fn create_clause(&mut self, expr: Option<Expression>, statements: Link<SourceElements>) -> Link<CaseClauseNode> {
        Link::new(CaseClauseNode::new(expr, statements.opt()))
    }

    fn create_clause_list(&mut self, clause: Link<CaseClauseNode>) -> Link<ClauseListNode> {
        Link::new(ClauseListNode::new(clause.get().clone()))
    }

    fn create_clause_list_append(&mut self, tail: Link<ClauseListNode>, clause: Link<CaseClauseNode>) -> Link<ClauseListNode> {
        Link::from_ref(ClauseListNode::append(tail.get(), clause.get().clone()))
    }

    fn create_func_decl_statement(&mut self, location: &JSTokenLocation, function_info: &ParserFunctionInfo<ASTBuilder>) -> Option<Statement> {
        let source = self.function_source(function_info, function_info.end_offset);
        let decl = FuncDeclNode::new(location, Self::non_null_name(&function_info.name), Rc::clone(Self::body_of(function_info)), &source);
        if *Self::non_null_name(&function_info.name) == self.vm.property_names.arguments {
            self.uses_arguments();
        }
        Self::set_function_body_loc(function_info, location);
        Some(Statement::FuncDecl(make(decl)))
    }

    fn create_class_decl_statement(&mut self, location: &JSTokenLocation, class_expression: Option<Expression>, class_start: JSTextPosition, class_end: JSTextPosition, start_line: u32, end_line: u32) -> Option<Statement> {
        let class_expression = non_null(class_expression);
        let name = match &class_expression {
            Expression::ClassExpr(class) => class.borrow().name.clone(),
            // Invariante: o parser só passa um ClassExpr aqui (RELEASE_ASSERT_NOT_REACHED no C++).
            _ => panic!("RELEASE_ASSERT_NOT_REACHED"),
        };
        let assign = self.create_assign_resolve(location, &name, Some(class_expression), class_start, class_start + 1i32, class_end, AssignmentContext::DeclarationStatement);
        Self::located_statement(Statement::ClassDecl(make(ClassDeclNode::new(location, non_null(assign)))), start_line, end_line, location)
    }

    fn create_block_statement(&mut self, location: &JSTokenLocation, elements: Link<SourceElements>, start_line: i32, end_line: i32, lexical_variables: VariableEnvironment, function_stack: FunctionStack) -> Option<Statement> {
        let block = Statement::Block(make(BlockNode::new(location, elements.opt(), lexical_variables, function_stack)));
        Self::located_statement(block, start_line as u32, end_line as u32, location)
    }

    fn create_expr_statement(&mut self, location: &JSTokenLocation, expr: Option<Expression>, start: JSTextPosition, end: i32) -> Option<Statement> {
        Self::statement_at(Statement::ExprStatement(make(ExprStatementNode::new(location, non_null(expr)))), start, end as u32)
    }

    fn create_if_statement(&mut self, location: &JSTokenLocation, condition: Option<Expression>, true_block: Option<Statement>, false_block: Option<Statement>, start: i32, end: i32) -> Option<Statement> {
        let result = Statement::IfElse(make(IfElseNode::new(location, non_null(condition), non_null(true_block), false_block)));
        Self::located_statement(result, start as u32, end as u32, location)
    }

    fn create_for_loop(&mut self, location: &JSTokenLocation, initializer: Option<Expression>, condition: Option<Expression>, iter: Option<Expression>, statements: Option<Statement>, start: i32, end: i32, lexical_variables: VariableEnvironment, initializer_contains_closure: bool) -> Option<Statement> {
        let result = Statement::For(make(ForNode::new(location, initializer, condition, iter, non_null(statements), lexical_variables, initializer_contains_closure)));
        Self::located_statement(result, start as u32, end as u32, location)
    }

    fn create_for_in_loop(&mut self, location: &JSTokenLocation, lhs: Option<Expression>, iter: Option<Expression>, statements: Option<Statement>, _decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> Option<Statement> {
        let mut node = ForInNode::new(location, non_null(lhs), non_null(iter), non_null(statements), lexical_variables);
        Self::set_exception_location(&mut node.throwable, e_start, e_divot, e_end);
        Self::located_statement(Statement::ForIn(make(node)), start as u32, end as u32, location)
    }

    fn create_for_in_loop_pattern(&mut self, location: &JSTokenLocation, pattern: Option<DestructuringPatternNode>, iter: Option<Expression>, statements: Option<Statement>, decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> Option<Statement> {
        let lexpr = Some(Expression::DestructuringAssignment(make(DestructuringAssignmentNode::new(decl_location, non_null(pattern), None))));
        self.create_for_in_loop(location, lexpr, iter, statements, decl_location, e_start, e_divot, e_end, start, end, lexical_variables)
    }

    fn create_for_of_loop(&mut self, is_for_await: bool, location: &JSTokenLocation, lhs: Option<Expression>, iter: Option<Expression>, statements: Option<Statement>, _decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> Option<Statement> {
        let mut node = ForOfNode::new(is_for_await, location, non_null(lhs), non_null(iter), non_null(statements), lexical_variables);
        Self::set_exception_location(&mut node.throwable, e_start, e_divot, e_end);
        let result = Self::located_statement(Statement::ForOf(make(node)), start as u32, end as u32, location);
        if is_for_await {
            self.uses_await();
        }
        result
    }

    fn create_for_of_loop_pattern(&mut self, is_for_await: bool, location: &JSTokenLocation, pattern: Option<DestructuringPatternNode>, iter: Option<Expression>, statements: Option<Statement>, decl_location: &JSTokenLocation, e_start: JSTextPosition, e_divot: JSTextPosition, e_end: JSTextPosition, start: i32, end: i32, lexical_variables: VariableEnvironment) -> Option<Statement> {
        let lexpr = Some(Expression::DestructuringAssignment(make(DestructuringAssignmentNode::new(decl_location, non_null(pattern), None))));
        self.create_for_of_loop(is_for_await, location, lexpr, iter, statements, decl_location, e_start, e_divot, e_end, start, end, lexical_variables)
    }

    fn is_binding_node(&self, pattern: &Option<DestructuringPatternNode>) -> bool {
        non_null_ref(pattern).is_binding_node()
    }

    fn is_location(&self, expr: &Option<Expression>) -> bool {
        non_null_ref(expr).is_location()
    }

    fn is_assignment_location(&self, expr: &Option<Expression>) -> bool {
        non_null_ref(expr).is_assignment_location()
    }

    fn is_private_location(&self, expr: &Option<Expression>) -> bool {
        non_null_ref(expr).is_private_location()
    }

    fn is_object_literal(&self, expr: &Option<Expression>) -> bool {
        non_null_ref(expr).is_object_literal()
    }

    fn is_array_literal(&self, expr: &Option<Expression>) -> bool {
        non_null_ref(expr).is_array_literal()
    }

    fn is_object_or_array_literal(&self, expr: &Option<Expression>) -> bool {
        self.is_object_literal(expr) || self.is_array_literal(expr)
    }

    fn is_function_call(&self, expr: &Option<Expression>) -> bool {
        non_null_ref(expr).is_function_call()
    }

    fn should_skip_pause_location(&self, statement: &Option<Statement>) -> bool {
        statement.as_ref().map_or(true, |statement| statement.is_label())
    }

    fn create_empty_statement(&mut self, location: &JSTokenLocation) -> Option<Statement> {
        Some(Statement::EmptyStatement(make(EmptyStatementNode::new(location))))
    }

    fn create_declaration_statement(&mut self, location: &JSTokenLocation, expr: Option<Expression>, start: i32, end: i32) -> Option<Statement> {
        let result = Statement::DeclarationStatement(make(DeclarationStatement::new(location, non_null(expr))));
        Self::located_statement(result, start as u32, end as u32, location)
    }

    fn create_return_statement(&mut self, location: &JSTokenLocation, expression: Option<Expression>, start: JSTextPosition, end: JSTextPosition) -> Option<Statement> {
        let mut node = ReturnNode::new(location, expression);
        Self::set_exception_location(&mut node.throwable, start, end, end);
        Self::statement_at(Statement::Return(make(node)), start, end.line as u32)
    }

    fn create_break_statement(&mut self, location: &JSTokenLocation, start: JSTextPosition, end: JSTextPosition) -> Option<Statement> {
        let null_identifier = self.vm.property_names.null_identifier.clone();
        self.create_break_statement_label(location, &null_identifier, start, end)
    }

    fn create_break_statement_label(&mut self, location: &JSTokenLocation, ident: &Identifier, start: JSTextPosition, end: JSTextPosition) -> Option<Statement> {
        let mut node = BreakNode::new(location, ident.clone());
        Self::set_exception_location(&mut node.throwable, start, end, end);
        Self::statement_at(Statement::Break(make(node)), start, end.line as u32)
    }

    fn create_continue_statement(&mut self, location: &JSTokenLocation, start: JSTextPosition, end: JSTextPosition) -> Option<Statement> {
        let null_identifier = self.vm.property_names.null_identifier.clone();
        self.create_continue_statement_label(location, &null_identifier, start, end)
    }

    fn create_continue_statement_label(&mut self, location: &JSTokenLocation, ident: &Identifier, start: JSTextPosition, end: JSTextPosition) -> Option<Statement> {
        let mut node = ContinueNode::new(location, ident.clone());
        Self::set_exception_location(&mut node.throwable, start, end, end);
        Self::statement_at(Statement::Continue(make(node)), start, end.line as u32)
    }

    fn create_try_statement(&mut self, location: &JSTokenLocation, try_block: Option<Statement>, catch_pattern: Option<DestructuringPatternNode>, catch_block: Option<Statement>, finally_block: Option<Statement>, start_line: i32, end_line: i32, catch_environment: VariableEnvironment) -> Option<Statement> {
        let node = TryNode::new(location, non_null(try_block), catch_pattern, catch_block, catch_environment, finally_block);
        Self::located_statement(Statement::Try(make(node)), start_line as u32, end_line as u32, location)
    }

    fn create_switch_statement(&mut self, location: &JSTokenLocation, expr: Option<Expression>, first_clauses: Link<ClauseListNode>, default_clause: Link<CaseClauseNode>, second_clauses: Link<ClauseListNode>, start_line: i32, end_line: i32, lexical_variables: VariableEnvironment, function_stack: FunctionStack) -> Option<Statement> {
        let cases = CaseBlockNode::new(first_clauses.opt(), default_clause.opt(), second_clauses.opt());
        let result = Statement::Switch(make(SwitchNode::new(location, non_null(expr), make(cases), lexical_variables, function_stack)));
        Self::located_statement(result, start_line as u32, end_line as u32, location)
    }

    fn create_while_statement(&mut self, location: &JSTokenLocation, expr: Option<Expression>, statement: Option<Statement>, start_line: i32, end_line: i32) -> Option<Statement> {
        let result = Statement::While(make(WhileNode::new(location, non_null(expr), non_null(statement))));
        Self::located_statement(result, start_line as u32, end_line as u32, location)
    }

    fn create_with_statement(&mut self, location: &JSTokenLocation, expr: Option<Expression>, statement: Option<Statement>, start: u32, end: JSTextPosition, start_line: u32, end_line: u32) -> Option<Statement> {
        self.uses_with();
        let node = WithNode::new(location, non_null(expr), non_null(statement), end, (end - start).as_int() as u32);
        Self::located_statement(Statement::With(make(node)), start_line, end_line, location)
    }

    fn create_do_while_statement(&mut self, location: &JSTokenLocation, statement: Option<Statement>, expr: Option<Expression>, start_line: i32, end_line: i32) -> Option<Statement> {
        let result = Statement::DoWhile(make(DoWhileNode::new(location, non_null(statement), non_null(expr))));
        Self::located_statement(result, start_line as u32, end_line as u32, location)
    }

    fn create_label_statement(&mut self, location: &JSTokenLocation, ident: &Identifier, statement: Option<Statement>, start: JSTextPosition, end: JSTextPosition) -> Option<Statement> {
        let mut node = LabelNode::new(location, ident.clone(), non_null(statement));
        Self::set_exception_location(&mut node.throwable, start, end, end);
        Some(Statement::Label(make(node)))
    }

    fn create_throw_statement(&mut self, location: &JSTokenLocation, expr: Option<Expression>, start: JSTextPosition, end: JSTextPosition) -> Option<Statement> {
        let mut node = ThrowNode::new(location, non_null(expr));
        Self::set_exception_location(&mut node.throwable, start, end, end);
        Self::statement_at(Statement::Throw(make(node)), start, end.line as u32)
    }

    fn create_debugger(&mut self, location: &JSTokenLocation, start_line: i32, end_line: i32) -> Option<Statement> {
        let result = Statement::DebuggerStatement(make(DebuggerStatementNode::new(location)));
        Self::located_statement(result, start_line as u32, end_line as u32, location)
    }

    fn create_module_name(&mut self, location: &JSTokenLocation, module_name: &Identifier) -> Link<ModuleNameNode> {
        Link::new(ModuleNameNode::new(location, module_name.clone()))
    }

    fn create_import_specifier(&mut self, location: &JSTokenLocation, imported_name: &Identifier, local_name: &Identifier) -> Link<ImportSpecifierNode> {
        Link::new(ImportSpecifierNode::new(location, imported_name.clone(), local_name.clone()))
    }

    fn create_import_specifier_list(&mut self) -> Link<ImportSpecifierListNode> {
        Link::new(ImportSpecifierListNode::default())
    }

    fn append_import_specifier(&mut self, specifier_list: &Link<ImportSpecifierListNode>, specifier: Link<ImportSpecifierNode>) {
        specifier_list.get().borrow_mut().append(specifier.get().clone());
    }

    fn create_import_attributes_list(&mut self) -> Link<ImportAttributesListNode> {
        Link::new(ImportAttributesListNode::default())
    }

    fn append_import_assertion(&mut self, attributes_list: &Link<ImportAttributesListNode>, key: &Identifier, value: &Identifier) {
        attributes_list.get().borrow_mut().append(key.clone(), value.clone());
    }

    fn create_import_declaration(&mut self, location: &JSTokenLocation, type_: ImportType, import_specifier_list: Link<ImportSpecifierListNode>, module_name: Link<ModuleNameNode>, import_attributes_list: Link<ImportAttributesListNode>) -> Option<Statement> {
        let node = ImportDeclarationNode::new(location, type_, import_specifier_list.get().clone(), module_name.get().clone(), import_attributes_list.opt());
        Some(Statement::ImportDeclaration(make(node)))
    }

    // Segunda fatia (`ASTBuilder.h`, do `createExportAllDeclaration` em diante, mais as definições de
    // `ASTBuilder.h` para `makeBinaryNode` e afins): os métodos restantes do trait.
    ast_builder_part2!();
    ast_builder_part4!();
}

include!("ast_builder_part3.rs");

