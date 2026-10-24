//! Tradução de `parser/Nodes.h` e `parser/NodeConstructors.h`, primeira metade (até `TypeOfResolveNode`).
//!
//! Modelo da árvore sintática (a segunda metade e o `ASTBuilder` seguem estas regras):
//!
//! - Cada classe concreta do C++ vira uma struct com o MESMO nome. Campo `m_fooBar` vira `foo_bar`.
//!   Quando o nome colide com o campo `base` da herança, o membro do C++ ganha o sufixo `_expr`
//!   (`m_base` de `BracketAccessorNode` e `BaseDotNode` vira `base_expr`). `type` vira `type_`.
//! - Herança vira composição: o pai que tem campos é o primeiro campo, chamado `base`, e a struct
//!   implementa `Deref`/`DerefMut` para ele (macro `inherit!`), o que reproduz a busca de membros do C++
//!   (`nó.position`, `nó.result_type`, `nó.divot`). Classe abstrata SEM campos (`ConstantNode`,
//!   `MetaPropertyNode`) não vira struct: o filho aponta direto para o avô. A segunda herança
//!   (`ThrowableExpressionData` e variantes) é um campo `throwable`, sem `Deref`.
//! - O polimorfismo vira `enum Expression` e `enum Statement`, com UMA variante por classe concreta do
//!   `Nodes.h` inteiro (sem o sufixo `Node`), cada uma `NodeRef<Struct>`. O enum é a própria alça (clonar
//!   copia o ponteiro; igualdade é de ponteiro) e dá a struct base por `expr.base()`/`expr.base_mut()`
//!   (`Ref`/`RefMut` do `ExpressionNode`/`StatementNode`), não por `Deref`: `expr.base().position()`.
//!   Os métodos virtuais viram `match` no enum. `NumberNode` é abstrata: as variantes são `Double` e
//!   `Integer`. Nenhum `*Node` abstrato com campos vira variante.
//! - Ponteiro de nó vira `Expression`/`NodeRef<Struct>` (nunca nulo) ou `Option<..>` (nulo no C++).
//!   Lista encadeada do C++ mantém `next: Option<NodeRef<..>>`. O construtor `X(previous, ...)` que faz
//!   `previous->m_next = this` vira `X::append(&tail, ...) -> NodeRef<X>`, que cria o nó, o liga ao
//!   `tail` e devolve o novo rabo como alça clonada (o chamador anda com `tail = X::append(&tail, ..)`).
//! - `const Identifier&` vira `Identifier` (clonado pelo chamador). Posições (`JSTextPosition`) entram
//!   por valor; `JSTokenLocation` por referência, como no C++.
//! - Omitidos de propósito: `emitBytecode` e tudo que recebe `BytecodeGenerator`/`RegisterID` (camada do
//!   bytecompiler, inclusive `isPure`, cujos corpos não triviais estão no `NodesCodegen.cpp`, e
//!   `ArrayNode::toArgumentList`, que só o emissor usa e que compartilha filhos); `ASSERT`s;
//!   `ParserArenaFreeable`/`ParserArenaDeletable` (a posse compartilhada por `NodeRef` os substitui); acessor de campo
//!   puro (`value()`, `identifier()`, `lexicalVariables()`: use o campo `pub`) e função de repasse
//!   (`hasUsingDeclaration` do `VariableEnvironmentNode`, `isComputedClassField` do `PropertyListNode`:
//!   chame o alvo direto, `nó.lexical_variables.has_using_declaration()`, `nó.node.borrow().is_computed_class_field()`).
//! - A segunda metade deve: declarar `inherit!(Filho => Pai)` para cada struct com pai; definir os
//!   métodos `has_completion_value`/`has_early_break_or_continue` em `BlockNode` e `ScopeNode`; dar o
//!   campo `statement` a `LabelNode`; e usar `Rc<FunctionMetadataNode>` onde o C++ divide o ponteiro
//!   (ver `declaration_stacks::FunctionStack`), com mutabilidade interior em `FunctionMetadataNode`.

use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use crate::bytecode::bytecode_intrinsic_registry::Entry as BytecodeIntrinsicRegistryEntry;
use crate::parser::module_scope_data::ModuleScopeData;
use crate::parser::parser_arena::ParserArena;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::bytecode::opcode::OpcodeID;
use crate::parser::parser_modes::{
    CodeFeatures, FunctionMode, InnerArrowFunctionCodeFeatures, LexicallyScopedFeatures, PrivateBrandRequirement,
    SourceParseMode, SuperBinding, ARGUMENTS_FEATURE, AWAIT_FEATURE,
    ARGUMENTS_INNER_ARROW_FUNCTION_FEATURE, ARROW_FUNCTION_FEATURE, ASYNC_FUNCTION_WITHOUT_AWAIT_FEATURE,
    EVAL_FEATURE, EVAL_INNER_ARROW_FUNCTION_FEATURE, NEW_TARGET_FEATURE, NEW_TARGET_INNER_ARROW_FUNCTION_FEATURE,
    NON_SIMPLE_PARAMETER_LIST_FEATURE, NO_FEATURES, NO_INNER_ARROW_FUNCTION_FEATURES, SHADOWS_ARGUMENTS_FEATURE,
    STRICT_MODE_LEXICALLY_SCOPED_FEATURE, SUPER_CALL_FEATURE, SUPER_CALL_INNER_ARROW_FUNCTION_FEATURE,
    SUPER_PROPERTY_FEATURE, SUPER_PROPERTY_INNER_ARROW_FUNCTION_FEATURE, THIS_FEATURE,
    THIS_INNER_ARROW_FUNCTION_FEATURE, WITH_FEATURE,
};
use crate::parser::parser_tokens::{JSTextPosition, JSTokenLocation};
use crate::parser::result_type::ResultType;
use crate::parser::source_code::SourceCode;
use crate::parser::variable_environment::VariableEnvironment;
use crate::runtime::identifier::Identifier;
use crate::runtime::vm::VM;

// `typedef SmallSet<UniquedStringImpl*> UniquedStringImplPtrSet;` fica para o módulo que primeiro o
// usa (o `Parser`), junto do `SmallSet` e do `UniquedStringImpl` da WTF, que ainda não foram portados.

/// `Deref`/`DerefMut` de uma struct para o pai guardado no campo `base`.
macro_rules! inherit {
    ($child:ty => $parent:ty) => {
        impl Deref for $child {
            type Target = $parent;
            fn deref(&self) -> &$parent {
                &self.base
            }
        }
        impl DerefMut for $child {
            fn deref_mut(&mut self) -> &mut $parent {
                &mut self.base
            }
        }
    };
}
pub(crate) use inherit;

/// `NodeRef<T>`: o ponteiro de nó do C++. Os nós vivem enquanto alguém aponta para eles (no C++, na
/// arena do parser), e o `Parser`/`ASTBuilder` alteram nós que já estão na árvore; daí posse
/// compartilhada com mutação interior. Igualdade de ponteiro é `Rc::ptr_eq`.
pub type NodeRef<T> = Rc<RefCell<T>>;

/// `new (parserArena) T(...)`: cria o nó e devolve o ponteiro.
pub fn node<T>(value: T) -> NodeRef<T> {
    Rc::new(RefCell::new(value))
}

/// Define o enum de uma família (uma variante `NodeRef<Struct>` por classe concreta) e o acesso à
/// struct base da família, `base()`/`base_mut()`, no lugar da conversão implícita do C++ para a
/// classe base. Dois valores são iguais quando apontam para o mesmo nó.
macro_rules! define_node_enum {
    ($(#[$meta:meta])* $name:ident => $target:ident { $($variant:ident($ty:ident)),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone)]
        pub enum $name {
            $($variant(NodeRef<$ty>)),*
        }
        impl $name {
            /// Quantas alças `Rc` apontam para este nó (usado pelo `Drop` iterativo das listas encadeadas).
            pub fn strong_count(&self) -> usize {
                match self {
                    $($name::$variant(n) => Rc::strong_count(n)),*
                }
            }
            pub fn base(&self) -> std::cell::Ref<'_, $target> {
                match self {
                    $($name::$variant(n) => std::cell::Ref::map(n.borrow(), |n| -> &$target { n })),*
                }
            }
            pub fn base_mut(&self) -> std::cell::RefMut<'_, $target> {
                match self {
                    $($name::$variant(n) => std::cell::RefMut::map(n.borrow_mut(), |n| -> &mut $target { n })),*
                }
            }
        }
        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                match (self, other) {
                    $(($name::$variant(a), $name::$variant(b)) => Rc::ptr_eq(a, b),)*
                    _ => false,
                }
            }
        }
    };
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Equal,
    PlusEq,
    MinusEq,
    MultEq,
    DivEq,
    PlusPlus,
    MinusMinus,
    BitAndEq,
    BitXOrEq,
    BitOrEq,
    ModEq,
    PowEq,
    CoalesceEq,
    OrEq,
    AndEq,
    LShift,
    RShift,
    URShift,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalOperator {
    And,
    Or,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallThroughMode {
    FallThroughMeansTrue = 0,
    FallThroughMeansFalse = 1,
}

pub fn invert(fall_through_mode: FallThroughMode) -> FallThroughMode {
    match fall_through_mode {
        FallThroughMode::FallThroughMeansTrue => FallThroughMode::FallThroughMeansFalse,
        FallThroughMode::FallThroughMeansFalse => FallThroughMode::FallThroughMeansTrue,
    }
}

pub mod declaration_stacks {
    use std::rc::Rc;

    use super::FunctionMetadataNode;

    /// `Vector<FunctionMetadataNode*>`: o `FuncDeclNode` e a pilha de funções do escopo apontam para o
    /// mesmo `FunctionMetadataNode`, então a posse é compartilhada (`Rc`).
    pub type FunctionStack = Vec<Rc<FunctionMetadataNode>>;
}
pub use declaration_stacks::FunctionStack;

/// `FunctionMetadataNode*`: posse compartilhada em `Rc`, sem `RefCell` (só os campos que o parser
/// altera depois têm mutabilidade interior; ver `FunctionMetadataNode`).
pub type FunctionMetadataNodeRef = Rc<FunctionMetadataNode>;
/// `RestParameterNode*`.
pub type RestParameterNodeRef = NodeRef<RestParameterNode>;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchType {
    None,
    Immediate,
    Character,
    ImmediateList,
    CharacterList,
    String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchInfo {
    pub bytecode_offset: u32,
    pub switch_type: SwitchType,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentContext {
    DeclarationStatement,
    ConstDeclarationStatement,
    UsingDeclarationStatement,
    AwaitUsingDeclarationStatement,
    AssignmentExpression,
}

/// `ParserArenaRoot`: guarda a arena do parser (o construtor do C++ a troca com a recebida).
pub struct ParserArenaRoot {
    pub arena: ParserArena,
}

impl ParserArenaRoot {
    pub fn new(parser_arena: &mut ParserArena) -> Self {
        // `m_arena.swap(parserArena)`: a raiz fica com a arena do parser e o parser com uma vazia.
        ParserArenaRoot { arena: std::mem::take(parser_arena) }
    }
}

pub struct Node {
    pub position: JSTextPosition,
    pub end_offset: i32,
    pub needs_debug_hook: bool,
}

impl Node {
    pub fn new(location: &JSTokenLocation) -> Self {
        Node {
            position: JSTextPosition::new(
                location.line,
                location.start_offset as i32,
                location.line_start_offset as i32,
            ),
            end_offset: -1,
            needs_debug_hook: false,
        }
    }

    pub fn first_line(&self) -> i32 {
        self.position.line
    }

    pub fn start_offset(&self) -> i32 {
        self.position.offset
    }

    pub fn end_offset(&self) -> i32 {
        self.end_offset
    }

    pub fn line_start_offset(&self) -> i32 {
        self.position.line_start_offset
    }

    pub fn position(&self) -> &JSTextPosition {
        &self.position
    }

    pub fn set_end_offset(&mut self, offset: i32) {
        self.end_offset = offset;
    }

    pub fn set_start_offset(&mut self, offset: i32) {
        self.position.offset = offset;
    }

    pub fn needs_debug_hook(&self) -> bool {
        self.needs_debug_hook
    }

    pub fn set_needs_debug_hook(&mut self) {
        self.needs_debug_hook = true;
    }
}

pub struct ExpressionNode {
    pub base: Node,
    pub result_type: ResultType,
    pub is_optional_chain_base: bool,
}

inherit!(ExpressionNode => Node);

impl ExpressionNode {
    /// `ExpressionNode(location)` com o `ResultType::unknownType()` padrão.
    pub fn new(location: &JSTokenLocation) -> Self {
        Self::with_result_type(location, ResultType::unknown_type())
    }

    pub fn with_result_type(location: &JSTokenLocation, result_type: ResultType) -> Self {
        ExpressionNode { base: Node::new(location), result_type, is_optional_chain_base: false }
    }

    pub fn result_descriptor(&self) -> ResultType {
        self.result_type
    }

    pub fn set_is_optional_chain_base(&mut self) {
        self.is_optional_chain_base = true;
    }
}

pub struct StatementNode {
    pub base: Node,
    pub last_line: i32,
    pub next: Option<Statement>,
}

inherit!(StatementNode => Node);

impl StatementNode {
    pub fn new(location: &JSTokenLocation) -> Self {
        StatementNode { base: Node::new(location), last_line: -1, next: None }
    }

    /// `StatementNode::setLoc` (Nodes.cpp).
    pub fn set_loc(&mut self, first_line: u32, last_line: u32, start_offset: i32, line_start_offset: i32) {
        self.last_line = last_line as i32;
        self.base.position = JSTextPosition::new(first_line as i32, start_offset, line_start_offset);
    }

    pub fn last_line(&self) -> u32 {
        self.last_line as u32
    }

    pub fn next(&self) -> Option<Statement> {
        self.next.clone()
    }

    pub fn set_next(&mut self, next: Option<Statement>) {
        self.next = next;
    }
}

/// O C++ guarda os statements num `Vector` e destrói em laço. Aqui eles se encadeiam por `next`
/// (`Rc`), e o `Drop` padrão recursaria uma vez por statement (estouro de pilha com dezenas de
/// milhares). Solta a cadeia em laço: só desfaz o encadeamento dos nós que este elo possui sozinho.
impl Drop for StatementNode {
    fn drop(&mut self) {
        let mut current = self.next.take();
        while let Some(statement) = current {
            current = if statement.strong_count() == 1 { statement.base_mut().next.take() } else { None };
        }
    }
}

/// `VariableEnvironmentNode`: segunda herança dos nós com escopo léxico (`BlockNode`, `ForNode`,
/// `EnumerationNode`, `TryNode`, `ScopeNode`, `ClassExprNode`, `SwitchNode`), campo `variable_environment`.
#[derive(Default)]
pub struct VariableEnvironmentNode {
    pub lexical_variables: VariableEnvironment,
    pub function_stack: FunctionStack,
}

impl VariableEnvironmentNode {
    pub fn with_lexical_variables(lexical_variables: VariableEnvironment) -> Self {
        VariableEnvironmentNode { lexical_variables, function_stack: FunctionStack::new() }
    }

    pub fn with_function_stack(lexical_variables: VariableEnvironment, function_stack: FunctionStack) -> Self {
        VariableEnvironmentNode { lexical_variables, function_stack }
    }

    pub fn has_using_declaration(&self) -> bool {
        self.lexical_variables.has_using_declaration()
    }

    pub fn has_await_using_declaration(&self) -> bool {
        self.lexical_variables.has_await_using_declaration()
    }

    pub fn using_declaration_count(&self) -> u32 {
        self.lexical_variables.using_declaration_count()
    }
}

/// `JSValue(double).isInt32()`: o valor entra como int32 quando a conversão é exata e não é `-0`.
fn double_is_strict_int32(value: f64) -> bool {
    let as_int32 = value as i32;
    !(as_int32 as f64 != value || (as_int32 == 0 && value.is_sign_negative()))
}

pub struct NullNode {
    pub base: ExpressionNode,
}

inherit!(NullNode => ExpressionNode);

impl NullNode {
    pub fn new(location: &JSTokenLocation) -> Self {
        NullNode { base: ExpressionNode::with_result_type(location, ResultType::null_type()) }
    }
}

pub struct BooleanNode {
    pub base: ExpressionNode,
    pub value: bool,
}

inherit!(BooleanNode => ExpressionNode);

impl BooleanNode {
    pub fn new(location: &JSTokenLocation, value: bool) -> Self {
        BooleanNode { base: ExpressionNode::with_result_type(location, ResultType::boolean_type()), value }
    }
}

/// Classe abstrata: as concretas são `DoubleNode` e `IntegerNode`.
pub struct NumberNode {
    pub base: ExpressionNode,
    pub value: f64,
}

inherit!(NumberNode => ExpressionNode);

impl NumberNode {
    pub fn new(location: &JSTokenLocation, value: f64) -> Self {
        let result_type = if double_is_strict_int32(value) {
            ResultType::number_type_is_int32()
        } else {
            ResultType::number_type()
        };
        NumberNode { base: ExpressionNode::with_result_type(location, result_type), value }
    }
}

pub struct DoubleNode {
    pub base: NumberNode,
}

inherit!(DoubleNode => NumberNode);

impl DoubleNode {
    pub fn new(location: &JSTokenLocation, value: f64) -> Self {
        DoubleNode { base: NumberNode::new(location, value) }
    }
}

/// Número escrito como inteiro (`42` e não `42.`, `42.0`, `42e0`).
pub struct IntegerNode {
    pub base: DoubleNode,
}

inherit!(IntegerNode => DoubleNode);

impl IntegerNode {
    pub fn new(location: &JSTokenLocation, value: f64) -> Self {
        IntegerNode { base: DoubleNode::new(location, value) }
    }
}

pub struct StringNode {
    pub base: ExpressionNode,
    pub value: Identifier,
}

inherit!(StringNode => ExpressionNode);

impl StringNode {
    pub fn new(location: &JSTokenLocation, value: Identifier) -> Self {
        StringNode { base: ExpressionNode::with_result_type(location, ResultType::string_type()), value }
    }
}

pub struct BigIntNode {
    pub base: ExpressionNode,
    pub value: Identifier,
    pub radix: u8,
    pub sign: bool,
}

inherit!(BigIntNode => ExpressionNode);

impl BigIntNode {
    /// `BigIntNode(location, value, radix)`: sem sinal.
    pub fn new(location: &JSTokenLocation, value: Identifier, radix: u8) -> Self {
        Self::with_sign(location, value, radix, false)
    }

    pub fn with_sign(location: &JSTokenLocation, value: Identifier, radix: u8, sign: bool) -> Self {
        BigIntNode {
            base: ExpressionNode::with_result_type(location, ResultType::big_int_type()),
            value,
            radix,
            sign,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ThrowableExpressionData {
    pub divot: JSTextPosition,
    pub divot_start: JSTextPosition,
    pub divot_end: JSTextPosition,
}

impl ThrowableExpressionData {
    pub fn new(divot: JSTextPosition, start: JSTextPosition, end: JSTextPosition) -> Self {
        ThrowableExpressionData { divot, divot_start: start, divot_end: end }
    }

    pub fn set_exception_source_code(
        &mut self,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) {
        self.divot = divot;
        self.divot_start = divot_start;
        self.divot_end = divot_end;
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ThrowableSubExpressionData {
    pub base: ThrowableExpressionData,
    pub subexpression_divot_offset: u16,
    pub subexpression_end_offset: u16,
    pub subexpression_line_offset: u16,
    pub subexpression_line_start_offset: u16,
}

inherit!(ThrowableSubExpressionData => ThrowableExpressionData);

impl ThrowableSubExpressionData {
    pub fn new(divot: JSTextPosition, divot_start: JSTextPosition, divot_end: JSTextPosition) -> Self {
        ThrowableSubExpressionData {
            base: ThrowableExpressionData::new(divot, divot_start, divot_end),
            ..Default::default()
        }
    }

    pub fn set_subexpression_info(&mut self, subexpression_divot: &JSTextPosition, subexpression_offset: i32) {
        let divot = self.base.divot;
        let divot_end = self.base.divot_end;
        // Estouro significa que não dá para guardar com segurança: fica apontando para o divot primário.
        if (divot.offset.wrapping_sub(subexpression_divot.offset) & !0xFFFF) != 0 {
            return;
        }
        if (divot.line.wrapping_sub(subexpression_divot.line) & !0xFFFF) != 0 {
            return;
        }
        if (divot.line_start_offset.wrapping_sub(subexpression_divot.line_start_offset) & !0xFFFF) != 0 {
            return;
        }
        if (divot_end.offset.wrapping_sub(subexpression_offset) & !0xFFFF) != 0 {
            return;
        }
        self.subexpression_divot_offset = divot.offset.wrapping_sub(subexpression_divot.offset) as u16;
        self.subexpression_end_offset = divot_end.offset.wrapping_sub(subexpression_offset) as u16;
        self.subexpression_line_offset = divot.line.wrapping_sub(subexpression_divot.line) as u16;
        self.subexpression_line_start_offset =
            divot.line_start_offset.wrapping_sub(subexpression_divot.line_start_offset) as u16;
    }

    pub fn subexpression_divot(&self) -> JSTextPosition {
        let divot = self.base.divot;
        JSTextPosition::new(
            divot.line.wrapping_sub(self.subexpression_line_offset as i32),
            divot.offset.wrapping_sub(self.subexpression_divot_offset as i32),
            divot.line_start_offset.wrapping_sub(self.subexpression_line_start_offset as i32),
        )
    }

    pub fn subexpression_start(&self) -> JSTextPosition {
        self.base.divot_start
    }

    pub fn subexpression_end(&self) -> JSTextPosition {
        self.base.divot_end - (self.subexpression_end_offset as i32)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ThrowablePrefixedSubExpressionData {
    pub base: ThrowableExpressionData,
    pub subexpression_divot_offset: u16,
    pub subexpression_start_offset: u16,
    pub subexpression_line_offset: u16,
    pub subexpression_line_start_offset: u16,
}

inherit!(ThrowablePrefixedSubExpressionData => ThrowableExpressionData);

impl ThrowablePrefixedSubExpressionData {
    pub fn new(divot: JSTextPosition, start: JSTextPosition, end: JSTextPosition) -> Self {
        ThrowablePrefixedSubExpressionData {
            base: ThrowableExpressionData::new(divot, start, end),
            ..Default::default()
        }
    }

    pub fn set_subexpression_info(&mut self, subexpression_divot: &JSTextPosition, subexpression_offset: i32) {
        let divot = self.base.divot;
        let divot_start = self.base.divot_start;
        // Estouro significa que não dá para guardar com segurança: fica apontando para o divot primário.
        if (subexpression_divot.offset.wrapping_sub(divot.offset) & !0xFFFF) != 0 {
            return;
        }
        if (subexpression_divot.line.wrapping_sub(divot.line) & !0xFFFF) != 0 {
            return;
        }
        if (subexpression_divot.line_start_offset.wrapping_sub(divot.line_start_offset) & !0xFFFF) != 0 {
            return;
        }
        if (subexpression_offset.wrapping_sub(divot_start.offset) & !0xFFFF) != 0 {
            return;
        }
        self.subexpression_divot_offset = subexpression_divot.offset.wrapping_sub(divot.offset) as u16;
        self.subexpression_start_offset = subexpression_offset.wrapping_sub(divot_start.offset) as u16;
        self.subexpression_line_offset = subexpression_divot.line.wrapping_sub(divot.line) as u16;
        self.subexpression_line_start_offset =
            subexpression_divot.line_start_offset.wrapping_sub(divot.line_start_offset) as u16;
    }

    pub fn subexpression_divot(&self) -> JSTextPosition {
        let divot = self.base.divot;
        JSTextPosition::new(
            divot.line.wrapping_add(self.subexpression_line_offset as i32),
            divot.offset.wrapping_add(self.subexpression_divot_offset as i32),
            divot.line_start_offset.wrapping_add(self.subexpression_line_start_offset as i32),
        )
    }

    pub fn subexpression_start(&self) -> JSTextPosition {
        self.base.divot_start + (self.subexpression_start_offset as i32)
    }

    pub fn subexpression_end(&self) -> JSTextPosition {
        self.base.divot_end
    }
}

pub struct TemplateExpressionListNode {
    pub next: Option<NodeRef<TemplateExpressionListNode>>,
    pub node: Expression,
}

impl TemplateExpressionListNode {
    pub fn new(node: Expression) -> Self {
        TemplateExpressionListNode { next: None, node }
    }

    /// `TemplateExpressionListNode(previous, node)`.
    pub fn append(previous: &NodeRef<TemplateExpressionListNode>, expression: Expression) -> NodeRef<TemplateExpressionListNode> {
        let tail = node(TemplateExpressionListNode::new(expression));
        previous.borrow_mut().next = Some(tail.clone());
        tail
    }
}

pub struct TemplateStringNode {
    pub base: ExpressionNode,
    pub cooked: Option<Identifier>,
    pub raw: Option<Identifier>,
}

inherit!(TemplateStringNode => ExpressionNode);

impl TemplateStringNode {
    pub fn new(location: &JSTokenLocation, cooked: Option<Identifier>, raw: Option<Identifier>) -> Self {
        TemplateStringNode { base: ExpressionNode::new(location), cooked, raw }
    }
}

pub struct TemplateStringListNode {
    pub next: Option<NodeRef<TemplateStringListNode>>,
    pub node: NodeRef<TemplateStringNode>,
}

impl TemplateStringListNode {
    pub fn new(node: NodeRef<TemplateStringNode>) -> Self {
        TemplateStringListNode { next: None, node }
    }

    /// `TemplateStringListNode(previous, node)`.
    pub fn append(previous: &NodeRef<TemplateStringListNode>, string: NodeRef<TemplateStringNode>) -> NodeRef<TemplateStringListNode> {
        let tail = node(TemplateStringListNode::new(string));
        previous.borrow_mut().next = Some(tail.clone());
        tail
    }
}

pub struct TemplateLiteralNode {
    pub base: ExpressionNode,
    pub template_strings: Option<NodeRef<TemplateStringListNode>>,
    pub template_expressions: Option<NodeRef<TemplateExpressionListNode>>,
}

inherit!(TemplateLiteralNode => ExpressionNode);

impl TemplateLiteralNode {
    pub fn new(location: &JSTokenLocation, template_strings: Option<NodeRef<TemplateStringListNode>>) -> Self {
        Self::with_expressions(location, template_strings, None)
    }

    pub fn with_expressions(
        location: &JSTokenLocation,
        template_strings: Option<NodeRef<TemplateStringListNode>>,
        template_expressions: Option<NodeRef<TemplateExpressionListNode>>,
    ) -> Self {
        TemplateLiteralNode { base: ExpressionNode::new(location), template_strings, template_expressions }
    }
}

pub struct TaggedTemplateNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub tag: Expression,
    pub template_literal: NodeRef<TemplateLiteralNode>,
}

inherit!(TaggedTemplateNode => ExpressionNode);

impl TaggedTemplateNode {
    pub fn new(location: &JSTokenLocation, tag: Expression, template_literal: NodeRef<TemplateLiteralNode>) -> Self {
        TaggedTemplateNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            tag,
            template_literal,
        }
    }
}

pub struct RegExpNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub pattern: Identifier,
    pub flags: Identifier,
}

inherit!(RegExpNode => ExpressionNode);

impl RegExpNode {
    pub fn new(location: &JSTokenLocation, pattern: Identifier, flags: Identifier) -> Self {
        RegExpNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            pattern,
            flags,
        }
    }
}

pub struct ThisNode {
    pub base: ExpressionNode,
}

inherit!(ThisNode => ExpressionNode);

impl ThisNode {
    pub fn new(location: &JSTokenLocation) -> Self {
        ThisNode { base: ExpressionNode::new(location) }
    }
}

pub struct SuperNode {
    pub base: ExpressionNode,
}

inherit!(SuperNode => ExpressionNode);

impl SuperNode {
    pub fn new(location: &JSTokenLocation) -> Self {
        SuperNode { base: ExpressionNode::new(location) }
    }
}

pub struct ImportNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub expr: Expression,
    pub option: Option<Expression>,
    pub deferred: bool,
}

inherit!(ImportNode => ExpressionNode);

impl ImportNode {
    pub fn new(location: &JSTokenLocation, expr: Expression, option: Option<Expression>, deferred: bool) -> Self {
        ImportNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            expr,
            option,
            deferred,
        }
    }
}

/// `NewTargetNode : MetaPropertyNode`: `MetaPropertyNode` não tem campos, o pai é o `ExpressionNode`.
pub struct NewTargetNode {
    pub base: ExpressionNode,
}

inherit!(NewTargetNode => ExpressionNode);

impl NewTargetNode {
    pub fn new(location: &JSTokenLocation) -> Self {
        NewTargetNode { base: ExpressionNode::new(location) }
    }
}

pub struct ImportMetaNode {
    pub base: ExpressionNode,
    pub expr: Expression,
}

inherit!(ImportMetaNode => ExpressionNode);

impl ImportMetaNode {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        ImportMetaNode { base: ExpressionNode::new(location), expr }
    }
}

pub struct ResolveNode {
    pub base: ExpressionNode,
    pub ident: Identifier,
    pub start: JSTextPosition,
}

inherit!(ResolveNode => ExpressionNode);

impl ResolveNode {
    pub fn new(location: &JSTokenLocation, ident: Identifier, start: JSTextPosition) -> Self {
        ResolveNode { base: ExpressionNode::new(location), ident, start }
    }
}

/// Expressão fictícia que guarda o lado esquerdo de `#x in obj`.
pub struct PrivateIdentifierNode {
    pub base: ExpressionNode,
    pub ident: Identifier,
}

inherit!(PrivateIdentifierNode => ExpressionNode);

impl PrivateIdentifierNode {
    pub fn new(location: &JSTokenLocation, ident: Identifier) -> Self {
        PrivateIdentifierNode { base: ExpressionNode::new(location), ident }
    }
}

pub struct ElementNode {
    pub next: Option<NodeRef<ElementNode>>,
    pub node: Expression,
    pub elision: i32,
}

impl ElementNode {
    pub fn new(elision: i32, node: Expression) -> Self {
        ElementNode { next: None, node, elision }
    }

    /// `ElementNode(l, elision, node)`.
    pub fn append(l: &NodeRef<ElementNode>, elision: i32, expr: Expression) -> NodeRef<ElementNode> {
        let tail = node(ElementNode::new(elision, expr));
        l.borrow_mut().next = Some(tail.clone());
        tail
    }
}

pub struct ArrayNode {
    pub base: ExpressionNode,
    pub element: Option<NodeRef<ElementNode>>,
    pub elision: i32,
}

inherit!(ArrayNode => ExpressionNode);

impl ArrayNode {
    /// `ArrayNode(location, elision)`: sem elementos.
    pub fn new(location: &JSTokenLocation, elision: i32) -> Self {
        Self::with_elision(location, elision, None)
    }

    /// `ArrayNode(location, element)`: sem elisão.
    pub fn from_elements(location: &JSTokenLocation, element: Option<NodeRef<ElementNode>>) -> Self {
        Self::with_elision(location, 0, element)
    }

    pub fn with_elision(location: &JSTokenLocation, elision: i32, element: Option<NodeRef<ElementNode>>) -> Self {
        ArrayNode { base: ExpressionNode::new(location), element, elision }
    }

    /// `ArrayNode::isSimpleArray` (NodesCodegen.cpp, só lê a árvore).
    pub fn is_simple_array(&self) -> bool {
        if self.elision != 0 {
            return false;
        }
        let mut ptr = self.element.clone();
        while let Some(element) = ptr {
            let element_ref = element.borrow();
            if element_ref.elision != 0 {
                return false;
            }
            if element_ref.node.is_spread_expression() {
                return false;
            }
            ptr = element_ref.next.clone();
        }
        true
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassElementTag {
    No,
    Instance,
    Static,
    LastTag,
}

pub struct PropertyNode {
    pub name: Option<Identifier>,
    pub expression: Option<Expression>,
    pub assign: Option<Expression>,
    /// `PropertyNode::Type` (campo de 11 bits no C++): combinação das constantes `PropertyNode::CONSTANT` etc.
    pub type_: u16,
    pub needs_super_binding: bool,
    pub class_element_tag: ClassElementTag,
    pub is_overridden_by_duplicate: bool,
}

/// `PropertyNode::Type`: máscara de bits.
pub type PropertyNodeType = u16;

impl PropertyNode {
    pub const CONSTANT: PropertyNodeType = 1;
    pub const GETTER: PropertyNodeType = 2;
    pub const SETTER: PropertyNodeType = 4;
    pub const COMPUTED: PropertyNodeType = 8;
    pub const SHORTHAND: PropertyNodeType = 16;
    pub const SPREAD: PropertyNodeType = 32;
    pub const PRIVATE_FIELD: PropertyNodeType = 64;
    pub const PRIVATE_METHOD: PropertyNodeType = 128;
    pub const PRIVATE_SETTER: PropertyNodeType = 256;
    pub const PRIVATE_GETTER: PropertyNodeType = 512;
    pub const BLOCK: PropertyNodeType = 1024;

    fn from_parts(
        name: Option<Identifier>,
        expression: Option<Expression>,
        assign: Option<Expression>,
        type_: PropertyNodeType,
        super_binding: SuperBinding,
        tag: ClassElementTag,
    ) -> Self {
        PropertyNode {
            name,
            expression,
            assign,
            type_: type_ & 0x7FF,
            needs_super_binding: super_binding == SuperBinding::Needed,
            class_element_tag: tag,
            is_overridden_by_duplicate: false,
        }
    }

    /// `PropertyNode(const Identifier&, Type, SuperBinding, ClassElementTag)`.
    pub fn from_name(name: Identifier, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Self {
        Self::from_parts(Some(name), None, None, type_, super_binding, tag)
    }

    /// `PropertyNode(const Identifier&, ExpressionNode* assign, Type, SuperBinding, ClassElementTag)`.
    pub fn from_name_and_assign(
        name: Identifier,
        assign: Option<Expression>,
        type_: PropertyNodeType,
        super_binding: SuperBinding,
        tag: ClassElementTag,
    ) -> Self {
        Self::from_parts(Some(name), None, assign, type_, super_binding, tag)
    }

    /// `PropertyNode(ExpressionNode* assign, Type, SuperBinding, ClassElementTag)`.
    pub fn from_assign(assign: Expression, type_: PropertyNodeType, super_binding: SuperBinding, tag: ClassElementTag) -> Self {
        Self::from_parts(None, None, Some(assign), type_, super_binding, tag)
    }

    /// `PropertyNode(ExpressionNode* propertyName, ExpressionNode* assign, Type, SuperBinding, ClassElementTag)`.
    pub fn from_expression_and_assign(
        property_name: Expression,
        assign: Option<Expression>,
        type_: PropertyNodeType,
        super_binding: SuperBinding,
        tag: ClassElementTag,
    ) -> Self {
        Self::from_parts(None, Some(property_name), assign, type_, super_binding, tag)
    }

    /// `PropertyNode(const Identifier&, ExpressionNode* propertyName, ExpressionNode* assign, Type, SuperBinding, ClassElementTag)`.
    pub fn from_name_expression_and_assign(
        ident: Identifier,
        property_name: Expression,
        assign: Option<Expression>,
        type_: PropertyNodeType,
        super_binding: SuperBinding,
        tag: ClassElementTag,
    ) -> Self {
        Self::from_parts(Some(ident), Some(property_name), assign, type_, super_binding, tag)
    }

    pub fn is_class_property(&self) -> bool {
        self.class_element_tag != ClassElementTag::No
    }

    pub fn is_static_class_property(&self) -> bool {
        self.class_element_tag == ClassElementTag::Static
    }

    pub fn is_instance_class_property(&self) -> bool {
        self.class_element_tag == ClassElementTag::Instance
    }

    pub fn is_class_field(&self) -> bool {
        self.is_class_property() && !self.needs_super_binding
    }

    pub fn is_instance_class_field(&self) -> bool {
        self.is_instance_class_property() && !self.needs_super_binding
    }

    pub fn is_static_class_field(&self) -> bool {
        self.is_static_class_property() && !self.needs_super_binding
    }

    pub fn is_static_class_block(&self) -> bool {
        (self.type_ & Self::BLOCK) != 0
    }

    pub fn is_static_class_element(&self) -> bool {
        self.is_static_class_block() || self.is_static_class_field()
    }

    pub fn is_private(&self) -> bool {
        (self.type_ & (Self::PRIVATE_FIELD | Self::PRIVATE_METHOD | Self::PRIVATE_GETTER | Self::PRIVATE_SETTER)) != 0
    }

    pub fn has_computed_name(&self) -> bool {
        self.expression.is_some()
    }

    pub fn is_computed_class_field(&self) -> bool {
        self.is_class_field() && self.has_computed_name()
    }

    pub fn set_is_overridden_by_duplicate(&mut self) {
        self.is_overridden_by_duplicate = true;
    }

    pub fn is_underscore_proto_setter(vm: &VM, node: &PropertyNode) -> bool {
        Self::is_underscore_proto_setter_parts(
            vm,
            node.name.as_ref(),
            node.type_,
            node.needs_super_binding,
            node.is_class_property(),
        )
    }

    pub fn is_underscore_proto_setter_parts(
        vm: &VM,
        name: Option<&Identifier>,
        type_: PropertyNodeType,
        needs_super_binding: bool,
        is_class_property: bool,
    ) -> bool {
        match name {
            Some(name) => {
                *name == vm.property_names.underscore_proto
                    && type_ == Self::CONSTANT
                    && !needs_super_binding
                    && !is_class_property
            }
            None => false,
        }
    }
}

pub struct PropertyListNode {
    pub base: ExpressionNode,
    pub node: NodeRef<PropertyNode>,
    pub next: Option<NodeRef<PropertyListNode>>,
    pub has_private_accessors: bool,
}

inherit!(PropertyListNode => ExpressionNode);

impl PropertyListNode {
    pub fn new(location: &JSTokenLocation, node: NodeRef<PropertyNode>) -> Self {
        PropertyListNode { base: ExpressionNode::new(location), node, next: None, has_private_accessors: false }
    }

    /// `PropertyListNode(location, node, list)`.
    pub fn append(
        list: &NodeRef<PropertyListNode>,
        location: &JSTokenLocation,
        property: NodeRef<PropertyNode>,
    ) -> NodeRef<PropertyListNode> {
        let tail = node(PropertyListNode::new(location, property));
        list.borrow_mut().next = Some(tail.clone());
        tail
    }

    /// Anda a lista (este nó e os `next`) e diz se algum `PropertyNode` satisfaz `predicate`: o laço que o
    /// C++ repete em `hasStaticallyNamedProperty`, `hasInstanceFields` e `shouldCreateLexicalScopeForClass`.
    fn any_property(&self, mut predicate: impl FnMut(&PropertyNode) -> bool) -> bool {
        if predicate(&self.node.borrow()) {
            return true;
        }
        let mut list = self.next.clone();
        while let Some(current) = list {
            let current_ref = current.borrow();
            if predicate(&current_ref.node.borrow()) {
                return true;
            }
            list = current_ref.next.clone();
        }
        false
    }

    /// `PropertyListNode::hasStaticallyNamedProperty` (Nodes.cpp).
    pub fn has_statically_named_property(&self, prop_name: &Identifier) -> bool {
        self.any_property(|property| {
            property.is_static_class_property()
                && property.name.as_ref().is_some_and(|current_node_name| current_node_name == prop_name)
        })
    }

    /// `PropertyListNode::hasInstanceFields` (Nodes.cpp).
    pub fn has_instance_fields(&self) -> bool {
        self.any_property(|property| property.is_instance_class_field())
    }

    /// `PropertyListNode::shouldCreateLexicalScopeForClass` (Nodes.cpp).
    pub fn should_create_lexical_scope_for_class(list: Option<&NodeRef<PropertyListNode>>) -> bool {
        list.is_some_and(|list| {
            list.borrow().any_property(|property| property.is_computed_class_field() || property.is_private())
        })
    }
}

pub struct ObjectLiteralNode {
    pub base: ExpressionNode,
    pub list: Option<NodeRef<PropertyListNode>>,
}

inherit!(ObjectLiteralNode => ExpressionNode);

impl ObjectLiteralNode {
    pub fn new(location: &JSTokenLocation) -> Self {
        Self::with_list(location, None)
    }

    pub fn with_list(location: &JSTokenLocation, list: Option<NodeRef<PropertyListNode>>) -> Self {
        ObjectLiteralNode { base: ExpressionNode::new(location), list }
    }
}

pub struct BracketAccessorNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub base_expr: Expression,
    pub subscript: Expression,
    pub subscript_has_assignments: bool,
}

inherit!(BracketAccessorNode => ExpressionNode);

impl BracketAccessorNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        subscript: Expression,
        subscript_has_assignments: bool,
    ) -> Self {
        BracketAccessorNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            base_expr,
            subscript,
            subscript_has_assignments,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DotType {
    Name,
    PrivateMember,
}

/// Classe abstrata: pai de `DotAccessorNode`, `FunctionCallDotNode`, `AssignDotNode`,
/// `ReadModifyDotNode` e `ShortCircuitReadModifyDotNode`.
pub struct BaseDotNode {
    pub base: ExpressionNode,
    pub base_expr: Expression,
    pub ident: Identifier,
    pub type_: DotType,
}

inherit!(BaseDotNode => ExpressionNode);

impl BaseDotNode {
    pub fn new(location: &JSTokenLocation, base_expr: Expression, ident: Identifier, type_: DotType) -> Self {
        BaseDotNode { base: ExpressionNode::new(location), base_expr, ident, type_ }
    }

    pub fn is_private_member(&self) -> bool {
        self.type_ == DotType::PrivateMember
    }

    pub fn is_arguments_length_access(&self, vm: &VM) -> bool {
        self.base_expr.is_arguments(vm) && self.ident == vm.property_names.length
    }
}

pub struct DotAccessorNode {
    pub base: BaseDotNode,
    pub throwable: ThrowableExpressionData,
}

inherit!(DotAccessorNode => BaseDotNode);

impl DotAccessorNode {
    pub fn new(location: &JSTokenLocation, base_expr: Expression, ident: Identifier, type_: DotType) -> Self {
        DotAccessorNode {
            base: BaseDotNode::new(location, base_expr, ident, type_),
            throwable: ThrowableExpressionData::default(),
        }
    }
}

pub struct SpreadExpressionNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub expression: Expression,
}

inherit!(SpreadExpressionNode => ExpressionNode);

impl SpreadExpressionNode {
    pub fn new(location: &JSTokenLocation, expression: Expression) -> Self {
        SpreadExpressionNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            expression,
        }
    }
}

pub struct ObjectSpreadExpressionNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub expression: Expression,
}

inherit!(ObjectSpreadExpressionNode => ExpressionNode);

impl ObjectSpreadExpressionNode {
    pub fn new(location: &JSTokenLocation, expression: Expression) -> Self {
        ObjectSpreadExpressionNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            expression,
        }
    }
}

pub struct ArgumentListNode {
    pub base: ExpressionNode,
    pub next: Option<NodeRef<ArgumentListNode>>,
    pub expr: Expression,
}

inherit!(ArgumentListNode => ExpressionNode);

impl ArgumentListNode {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        ArgumentListNode { base: ExpressionNode::new(location), next: None, expr }
    }

    /// `ArgumentListNode(location, listNode, expr)`.
    pub fn append(
        list_node: &NodeRef<ArgumentListNode>,
        location: &JSTokenLocation,
        expr: Expression,
    ) -> NodeRef<ArgumentListNode> {
        let tail = node(ArgumentListNode::new(location, expr));
        list_node.borrow_mut().next = Some(tail.clone());
        tail
    }
}

pub struct ArgumentsNode {
    pub list_node: Option<NodeRef<ArgumentListNode>>,
    pub has_assignments: bool,
}

impl ArgumentsNode {
    pub fn new() -> Self {
        ArgumentsNode { list_node: None, has_assignments: false }
    }

    pub fn with_list(list_node: Option<NodeRef<ArgumentListNode>>, has_assignments: bool) -> Self {
        ArgumentsNode { list_node, has_assignments }
    }
}

impl Default for ArgumentsNode {
    fn default() -> Self {
        ArgumentsNode::new()
    }
}

pub struct NewExprNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub expr: Expression,
    pub args: Option<NodeRef<ArgumentsNode>>,
}

inherit!(NewExprNode => ExpressionNode);

impl NewExprNode {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        Self::with_args(location, expr, None)
    }

    pub fn with_args(location: &JSTokenLocation, expr: Expression, args: Option<NodeRef<ArgumentsNode>>) -> Self {
        NewExprNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            expr,
            args,
        }
    }
}

pub struct EvalFunctionCallNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub args: NodeRef<ArgumentsNode>,
}

inherit!(EvalFunctionCallNode => ExpressionNode);

impl EvalFunctionCallNode {
    pub fn new(
        location: &JSTokenLocation,
        args: NodeRef<ArgumentsNode>,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        EvalFunctionCallNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            args,
        }
    }
}

pub struct FunctionCallValueNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub expr: Expression,
    pub args: NodeRef<ArgumentsNode>,
    pub is_optional_call: bool,
}

inherit!(FunctionCallValueNode => ExpressionNode);

impl FunctionCallValueNode {
    pub fn new(
        location: &JSTokenLocation,
        expr: Expression,
        args: NodeRef<ArgumentsNode>,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
        is_optional_call: bool,
    ) -> Self {
        FunctionCallValueNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            expr,
            args,
            is_optional_call,
        }
    }
}

pub struct StaticBlockFunctionCallNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub expr: Option<Expression>,
}

inherit!(StaticBlockFunctionCallNode => ExpressionNode);

impl StaticBlockFunctionCallNode {
    pub fn new(
        location: &JSTokenLocation,
        expr: Expression,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        StaticBlockFunctionCallNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            expr: Some(expr),
        }
    }
}

pub struct FunctionCallResolveNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub ident: Identifier,
    pub args: NodeRef<ArgumentsNode>,
    pub is_optional_call: bool,
}

inherit!(FunctionCallResolveNode => ExpressionNode);

impl FunctionCallResolveNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: Identifier,
        args: NodeRef<ArgumentsNode>,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
        is_optional_call: bool,
    ) -> Self {
        FunctionCallResolveNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            ident,
            args,
            is_optional_call,
        }
    }
}

pub struct FunctionCallBracketNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableSubExpressionData,
    pub base_expr: Expression,
    pub subscript: Expression,
    pub args: NodeRef<ArgumentsNode>,
    pub subscript_has_assignments: bool,
    pub is_optional_call: bool,
}

inherit!(FunctionCallBracketNode => ExpressionNode);

impl FunctionCallBracketNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        subscript: Expression,
        subscript_has_assignments: bool,
        args: NodeRef<ArgumentsNode>,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
        is_optional_call: bool,
    ) -> Self {
        FunctionCallBracketNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableSubExpressionData::new(divot, divot_start, divot_end),
            base_expr,
            subscript,
            args,
            subscript_has_assignments,
            is_optional_call,
        }
    }
}

pub struct FunctionCallDotNode {
    pub base: BaseDotNode,
    pub throwable: ThrowableSubExpressionData,
    pub args: NodeRef<ArgumentsNode>,
    pub is_optional_call: bool,
}

inherit!(FunctionCallDotNode => BaseDotNode);

impl FunctionCallDotNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        ident: Identifier,
        type_: DotType,
        args: NodeRef<ArgumentsNode>,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
        is_optional_call: bool,
    ) -> Self {
        FunctionCallDotNode {
            base: BaseDotNode::new(location, base_expr, ident, type_),
            throwable: ThrowableSubExpressionData::new(divot, divot_start, divot_end),
            args,
            is_optional_call,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BytecodeIntrinsicNodeType {
    Constant,
    Function,
}

pub struct BytecodeIntrinsicNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub entry: BytecodeIntrinsicRegistryEntry,
    pub ident: Identifier,
    pub args: Option<NodeRef<ArgumentsNode>>,
    pub type_: BytecodeIntrinsicNodeType,
}

inherit!(BytecodeIntrinsicNode => ExpressionNode);

impl BytecodeIntrinsicNode {
    pub fn new(
        type_: BytecodeIntrinsicNodeType,
        location: &JSTokenLocation,
        entry: BytecodeIntrinsicRegistryEntry,
        ident: Identifier,
        args: Option<NodeRef<ArgumentsNode>>,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        BytecodeIntrinsicNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            entry,
            ident,
            args,
            type_,
        }
    }
}

pub struct CallFunctionCallDotNode {
    pub base: FunctionCallDotNode,
    pub distance_to_innermost_call_or_apply: usize,
}

inherit!(CallFunctionCallDotNode => FunctionCallDotNode);

impl CallFunctionCallDotNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        ident: Identifier,
        type_: DotType,
        args: NodeRef<ArgumentsNode>,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
        is_optional_call: bool,
        distance_to_innermost_call_or_apply: usize,
    ) -> Self {
        CallFunctionCallDotNode {
            base: FunctionCallDotNode::new(
                location,
                base_expr,
                ident,
                type_,
                args,
                divot,
                divot_start,
                divot_end,
                is_optional_call,
            ),
            distance_to_innermost_call_or_apply,
        }
    }
}

pub struct ApplyFunctionCallDotNode {
    pub base: FunctionCallDotNode,
    pub distance_to_innermost_call_or_apply: usize,
}

inherit!(ApplyFunctionCallDotNode => FunctionCallDotNode);

impl ApplyFunctionCallDotNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        ident: Identifier,
        type_: DotType,
        args: NodeRef<ArgumentsNode>,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
        is_optional_call: bool,
        distance_to_innermost_call_or_apply: usize,
    ) -> Self {
        ApplyFunctionCallDotNode {
            base: FunctionCallDotNode::new(
                location,
                base_expr,
                ident,
                type_,
                args,
                divot,
                divot_start,
                divot_end,
                is_optional_call,
            ),
            distance_to_innermost_call_or_apply,
        }
    }
}

pub struct HasOwnPropertyFunctionCallDotNode {
    pub base: FunctionCallDotNode,
}

inherit!(HasOwnPropertyFunctionCallDotNode => FunctionCallDotNode);

impl HasOwnPropertyFunctionCallDotNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        ident: Identifier,
        type_: DotType,
        args: NodeRef<ArgumentsNode>,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
        is_optional_call: bool,
    ) -> Self {
        HasOwnPropertyFunctionCallDotNode {
            base: FunctionCallDotNode::new(
                location,
                base_expr,
                ident,
                type_,
                args,
                divot,
                divot_start,
                divot_end,
                is_optional_call,
            ),
        }
    }
}

pub struct DeleteResolveNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub ident: Identifier,
}

inherit!(DeleteResolveNode => ExpressionNode);

impl DeleteResolveNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: Identifier,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        DeleteResolveNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            ident,
        }
    }
}

pub struct DeleteBracketNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub base_expr: Expression,
    pub subscript: Expression,
}

inherit!(DeleteBracketNode => ExpressionNode);

impl DeleteBracketNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        subscript: Expression,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        DeleteBracketNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            base_expr,
            subscript,
        }
    }
}

pub struct DeleteDotNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub base_expr: Expression,
    pub ident: Identifier,
}

inherit!(DeleteDotNode => ExpressionNode);

impl DeleteDotNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        ident: Identifier,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        DeleteDotNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            base_expr,
            ident,
        }
    }
}

pub struct DeleteValueNode {
    pub base: ExpressionNode,
    pub expr: Expression,
}

inherit!(DeleteValueNode => ExpressionNode);

impl DeleteValueNode {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        DeleteValueNode { base: ExpressionNode::new(location), expr }
    }
}

pub struct VoidNode {
    pub base: ExpressionNode,
    pub expr: Expression,
}

inherit!(VoidNode => ExpressionNode);

impl VoidNode {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        VoidNode { base: ExpressionNode::new(location), expr }
    }
}

pub struct TypeOfResolveNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub ident: Identifier,
}

inherit!(TypeOfResolveNode => ExpressionNode);

impl TypeOfResolveNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: Identifier,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        TypeOfResolveNode {
            base: ExpressionNode::with_result_type(location, ResultType::string_type()),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            ident,
        }
    }
}

define_node_enum! {
    /// Toda classe concreta de `ExpressionNode` do `Nodes.h` (as structs das variantes da segunda metade
    /// vêm depois neste módulo).
    Expression => ExpressionNode {
        Null(NullNode),
        Boolean(BooleanNode),
        Double(DoubleNode),
        Integer(IntegerNode),
        String(StringNode),
        BigInt(BigIntNode),
        TemplateString(TemplateStringNode),
        TemplateLiteral(TemplateLiteralNode),
        TaggedTemplate(TaggedTemplateNode),
        RegExp(RegExpNode),
        This(ThisNode),
        Super(SuperNode),
        Import(ImportNode),
        NewTarget(NewTargetNode),
        ImportMeta(ImportMetaNode),
        Resolve(ResolveNode),
        PrivateIdentifier(PrivateIdentifierNode),
        Array(ArrayNode),
        PropertyList(PropertyListNode),
        ObjectLiteral(ObjectLiteralNode),
        BracketAccessor(BracketAccessorNode),
        DotAccessor(DotAccessorNode),
        SpreadExpression(SpreadExpressionNode),
        ObjectSpreadExpression(ObjectSpreadExpressionNode),
        ArgumentList(ArgumentListNode),
        NewExpr(NewExprNode),
        EvalFunctionCall(EvalFunctionCallNode),
        FunctionCallValue(FunctionCallValueNode),
        StaticBlockFunctionCall(StaticBlockFunctionCallNode),
        FunctionCallResolve(FunctionCallResolveNode),
        FunctionCallBracket(FunctionCallBracketNode),
        FunctionCallDot(FunctionCallDotNode),
        BytecodeIntrinsic(BytecodeIntrinsicNode),
        CallFunctionCallDot(CallFunctionCallDotNode),
        ApplyFunctionCallDot(ApplyFunctionCallDotNode),
        HasOwnPropertyFunctionCallDot(HasOwnPropertyFunctionCallDotNode),
        DeleteResolve(DeleteResolveNode),
        DeleteBracket(DeleteBracketNode),
        DeleteDot(DeleteDotNode),
        DeleteValue(DeleteValueNode),
        Void(VoidNode),
        TypeOfResolve(TypeOfResolveNode),
        TypeOfValue(TypeOfValueNode),
        Prefix(PrefixNode),
        Postfix(PostfixNode),
        UnaryPlus(UnaryPlusNode),
        Negate(NegateNode),
        BitwiseNot(BitwiseNotNode),
        LogicalNot(LogicalNotNode),
        Pow(PowNode),
        Mult(MultNode),
        Div(DivNode),
        Mod(ModNode),
        Add(AddNode),
        Sub(SubNode),
        LeftShift(LeftShiftNode),
        RightShift(RightShiftNode),
        UnsignedRightShift(UnsignedRightShiftNode),
        Less(LessNode),
        Greater(GreaterNode),
        LessEq(LessEqNode),
        GreaterEq(GreaterEqNode),
        InstanceOf(InstanceOfNode),
        In(InNode),
        Equal(EqualNode),
        NotEqual(NotEqualNode),
        StrictEqual(StrictEqualNode),
        NotStrictEqual(NotStrictEqualNode),
        BitAnd(BitAndNode),
        BitOr(BitOrNode),
        BitXOr(BitXOrNode),
        LogicalOp(LogicalOpNode),
        Coalesce(CoalesceNode),
        OptionalChain(OptionalChainNode),
        Conditional(ConditionalNode),
        ReadModifyResolve(ReadModifyResolveNode),
        ShortCircuitReadModifyResolve(ShortCircuitReadModifyResolveNode),
        AssignResolve(AssignResolveNode),
        ReadModifyBracket(ReadModifyBracketNode),
        ShortCircuitReadModifyBracket(ShortCircuitReadModifyBracketNode),
        AssignBracket(AssignBracketNode),
        AssignDot(AssignDotNode),
        ReadModifyDot(ReadModifyDotNode),
        ShortCircuitReadModifyDot(ShortCircuitReadModifyDotNode),
        AssignError(AssignErrorNode),
        Comma(CommaNode),
        EmptyVarExpression(EmptyVarExpression),
        EmptyLetExpression(EmptyLetExpression),
        FuncExpr(FuncExprNode),
        ArrowFuncExpr(ArrowFuncExprNode),
        MethodDefinition(MethodDefinitionNode),
        YieldExpr(YieldExprNode),
        AwaitExpr(AwaitExprNode),
        ClassExpr(ClassExprNode),
        DestructuringAssignment(DestructuringAssignmentNode),
    }
}

define_node_enum! {
    /// Toda classe concreta de `StatementNode` do `Nodes.h` (as structs vêm na segunda metade).
    Statement => StatementNode {
        Block(BlockNode),
        EmptyStatement(EmptyStatementNode),
        DebuggerStatement(DebuggerStatementNode),
        ExprStatement(ExprStatementNode),
        DeclarationStatement(DeclarationStatement),
        IfElse(IfElseNode),
        DoWhile(DoWhileNode),
        While(WhileNode),
        For(ForNode),
        ForIn(ForInNode),
        ForOf(ForOfNode),
        Continue(ContinueNode),
        Break(BreakNode),
        Return(ReturnNode),
        With(WithNode),
        Label(LabelNode),
        Throw(ThrowNode),
        Try(TryNode),
        Program(ProgramNode),
        Eval(EvalNode),
        ModuleProgram(ModuleProgramNode),
        ImportDeclaration(ImportDeclarationNode),
        ExportAllDeclaration(ExportAllDeclarationNode),
        ExportDefaultDeclaration(ExportDefaultDeclarationNode),
        ExportLocalDeclaration(ExportLocalDeclarationNode),
        ExportNamedDeclaration(ExportNamedDeclarationNode),
        Function(FunctionNode),
        DefineField(DefineFieldNode),
        FuncDecl(FuncDeclNode),
        ClassDecl(ClassDeclNode),
        Switch(SwitchNode),
    }
}

impl Expression {
    pub fn is_number(&self) -> bool {
        matches!(self, Expression::Double(_) | Expression::Integer(_))
    }

    pub fn is_integer_node(&self) -> bool {
        matches!(self, Expression::Integer(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Expression::String(_))
    }

    pub fn is_big_int(&self) -> bool {
        matches!(self, Expression::BigInt(_))
    }

    pub fn is_object_literal(&self) -> bool {
        matches!(self, Expression::ObjectLiteral(_))
    }

    pub fn is_array_literal(&self) -> bool {
        matches!(self, Expression::Array(_))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Expression::Null(_))
    }

    /// `ExpressionNode::resultDescriptor`.
    pub fn result_descriptor(&self) -> ResultType {
        self.base().result_descriptor()
    }

    /// `ConstantNode::isConstant`: `Null`, `Boolean`, `Double`, `Integer`, `String` e `BigInt`.
    pub fn is_constant(&self) -> bool {
        matches!(
            self,
            Expression::Null(_)
                | Expression::Boolean(_)
                | Expression::Double(_)
                | Expression::Integer(_)
                | Expression::String(_)
                | Expression::BigInt(_)
        )
    }

    pub fn is_location(&self) -> bool {
        matches!(self, Expression::Resolve(_) | Expression::BracketAccessor(_) | Expression::DotAccessor(_))
    }

    pub fn is_private_location(&self) -> bool {
        match self {
            Expression::DotAccessor(n) => n.borrow().type_ == DotType::PrivateMember,
            _ => false,
        }
    }

    pub fn is_assignment_location(&self) -> bool {
        matches!(self, Expression::DestructuringAssignment(_)) || self.is_location()
    }

    pub fn is_resolve_node(&self) -> bool {
        matches!(self, Expression::Resolve(_))
    }

    pub fn is_assign_resolve_node(&self) -> bool {
        matches!(self, Expression::AssignResolve(_))
    }

    pub fn is_bracket_accessor_node(&self) -> bool {
        matches!(self, Expression::BracketAccessor(_))
    }

    pub fn is_dot_accessor_node(&self) -> bool {
        matches!(self, Expression::DotAccessor(_))
    }

    pub fn is_destructuring_node(&self) -> bool {
        matches!(self, Expression::DestructuringAssignment(_))
    }

    pub fn is_base_func_expr_node(&self) -> bool {
        matches!(self, Expression::FuncExpr(_) | Expression::ArrowFuncExpr(_) | Expression::MethodDefinition(_))
    }

    pub fn is_func_expr_node(&self) -> bool {
        matches!(self, Expression::FuncExpr(_) | Expression::MethodDefinition(_))
    }

    pub fn is_arrow_func_expr_node(&self) -> bool {
        matches!(self, Expression::ArrowFuncExpr(_))
    }

    pub fn is_class_expr_node(&self) -> bool {
        matches!(self, Expression::ClassExpr(_))
    }

    pub fn is_comma_node(&self) -> bool {
        matches!(self, Expression::Comma(_))
    }

    pub fn is_simple_array(&self) -> bool {
        match self {
            Expression::Array(n) => n.borrow().is_simple_array(),
            _ => false,
        }
    }

    pub fn is_add(&self) -> bool {
        matches!(self, Expression::Add(_))
    }

    pub fn is_subtract(&self) -> bool {
        matches!(self, Expression::Sub(_))
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, Expression::Boolean(_))
    }

    pub fn is_this_node(&self) -> bool {
        matches!(self, Expression::This(_))
    }

    pub fn is_spread_expression(&self) -> bool {
        matches!(self, Expression::SpreadExpression(_))
    }

    pub fn is_super_node(&self) -> bool {
        matches!(self, Expression::Super(_))
    }

    pub fn is_import_node(&self) -> bool {
        matches!(self, Expression::Import(_))
    }

    pub fn is_meta_property(&self) -> bool {
        matches!(self, Expression::NewTarget(_) | Expression::ImportMeta(_))
    }

    pub fn is_new_target(&self) -> bool {
        matches!(self, Expression::NewTarget(_))
    }

    pub fn is_import_meta(&self) -> bool {
        matches!(self, Expression::ImportMeta(_))
    }

    pub fn is_bytecode_intrinsic_node(&self) -> bool {
        matches!(self, Expression::BytecodeIntrinsic(_))
    }

    /// `BinaryOpNode::isBinaryOpNode`: todas as subclasses de `BinaryOpNode` (inclusive `InstanceOf` e `In`
    /// pela `ThrowableBinaryOpNode`). `LogicalOp` e `Coalesce` não são `BinaryOpNode`.
    pub fn is_binary_op_node(&self) -> bool {
        matches!(
            self,
            Expression::Pow(_)
                | Expression::Mult(_)
                | Expression::Div(_)
                | Expression::Mod(_)
                | Expression::Add(_)
                | Expression::Sub(_)
                | Expression::LeftShift(_)
                | Expression::RightShift(_)
                | Expression::UnsignedRightShift(_)
                | Expression::Less(_)
                | Expression::Greater(_)
                | Expression::LessEq(_)
                | Expression::GreaterEq(_)
                | Expression::InstanceOf(_)
                | Expression::In(_)
                | Expression::Equal(_)
                | Expression::NotEqual(_)
                | Expression::StrictEqual(_)
                | Expression::NotStrictEqual(_)
                | Expression::BitAnd(_)
                | Expression::BitOr(_)
                | Expression::BitXOr(_)
        )
    }

    pub fn is_function_call(&self) -> bool {
        match self {
            Expression::EvalFunctionCall(_)
            | Expression::FunctionCallValue(_)
            | Expression::StaticBlockFunctionCall(_)
            | Expression::FunctionCallResolve(_)
            | Expression::FunctionCallBracket(_)
            | Expression::FunctionCallDot(_)
            | Expression::CallFunctionCallDot(_)
            | Expression::ApplyFunctionCallDot(_)
            | Expression::HasOwnPropertyFunctionCallDot(_) => true,
            Expression::BytecodeIntrinsic(n) => n.borrow().type_ == BytecodeIntrinsicNodeType::Function,
            _ => false,
        }
    }

    pub fn is_delete_node(&self) -> bool {
        matches!(
            self,
            Expression::DeleteResolve(_)
                | Expression::DeleteBracket(_)
                | Expression::DeleteDot(_)
                | Expression::DeleteValue(_)
        )
    }

    pub fn is_optional_chain(&self) -> bool {
        matches!(self, Expression::OptionalChain(_))
    }

    pub fn is_optional_call(&self) -> bool {
        match self {
            Expression::FunctionCallValue(n) => n.borrow().is_optional_call,
            Expression::FunctionCallResolve(n) => n.borrow().is_optional_call,
            Expression::FunctionCallBracket(n) => n.borrow().is_optional_call,
            Expression::FunctionCallDot(n) => n.borrow().is_optional_call,
            Expression::CallFunctionCallDot(n) => n.borrow().is_optional_call,
            Expression::ApplyFunctionCallDot(n) => n.borrow().is_optional_call,
            Expression::HasOwnPropertyFunctionCallDot(n) => n.borrow().is_optional_call,
            _ => false,
        }
    }

    pub fn is_private_identifier(&self) -> bool {
        matches!(self, Expression::PrivateIdentifier(_))
    }

    /// `BaseDotNode::isArgumentsLengthAccess`: toda subclasse de `BaseDotNode`.
    pub fn is_arguments_length_access(&self, vm: &VM) -> bool {
        match self {
            Expression::DotAccessor(n) => n.borrow().is_arguments_length_access(vm),
            Expression::FunctionCallDot(n) => n.borrow().is_arguments_length_access(vm),
            Expression::CallFunctionCallDot(n) => n.borrow().is_arguments_length_access(vm),
            Expression::ApplyFunctionCallDot(n) => n.borrow().is_arguments_length_access(vm),
            Expression::HasOwnPropertyFunctionCallDot(n) => n.borrow().is_arguments_length_access(vm),
            Expression::AssignDot(n) => n.borrow().is_arguments_length_access(vm),
            Expression::ReadModifyDot(n) => n.borrow().is_arguments_length_access(vm),
            Expression::ShortCircuitReadModifyDot(n) => n.borrow().is_arguments_length_access(vm),
            _ => false,
        }
    }

    pub fn is_arguments(&self, vm: &VM) -> bool {
        match self {
            Expression::Resolve(n) => n.borrow().ident == vm.property_names.arguments,
            _ => false,
        }
    }

    /// `ExpressionNode::stripUnaryPlus`: o `UnaryPlusNode` devolve o operando, os demais a si mesmos
    /// (a alça clonada: o mesmo nó).
    pub fn strip_unary_plus(&self) -> Expression {
        match self {
            Expression::UnaryPlus(n) => n.borrow().expr.clone(),
            _ => self.clone(),
        }
    }
}

impl Statement {
    pub fn has_completion_value(&self) -> bool {
        match self {
            Statement::Block(n) => n.borrow().has_completion_value(),
            Statement::Program(n) => n.borrow().has_completion_value(),
            Statement::Eval(n) => n.borrow().has_completion_value(),
            Statement::ModuleProgram(n) => n.borrow().has_completion_value(),
            Statement::Function(n) => n.borrow().has_completion_value(),
            Statement::Label(n) => n.borrow().statement.has_completion_value(),
            Statement::EmptyStatement(_)
            | Statement::DebuggerStatement(_)
            | Statement::DeclarationStatement(_)
            | Statement::Continue(_)
            | Statement::Break(_)
            | Statement::ImportDeclaration(_)
            | Statement::ExportAllDeclaration(_)
            | Statement::ExportDefaultDeclaration(_)
            | Statement::ExportLocalDeclaration(_)
            | Statement::ExportNamedDeclaration(_)
            | Statement::FuncDecl(_)
            | Statement::ClassDecl(_) => false,
            _ => true,
        }
    }

    pub fn has_early_break_or_continue(&self) -> bool {
        match self {
            Statement::Block(n) => n.borrow().has_early_break_or_continue(),
            Statement::Program(n) => n.borrow().has_early_break_or_continue(),
            Statement::Eval(n) => n.borrow().has_early_break_or_continue(),
            Statement::ModuleProgram(n) => n.borrow().has_early_break_or_continue(),
            Statement::Function(n) => n.borrow().has_early_break_or_continue(),
            Statement::Label(n) => n.borrow().statement.has_early_break_or_continue(),
            Statement::Continue(_) | Statement::Break(_) => true,
            _ => false,
        }
    }

    pub fn is_empty_statement(&self) -> bool {
        matches!(self, Statement::EmptyStatement(_))
    }

    pub fn is_debugger_statement(&self) -> bool {
        matches!(self, Statement::DebuggerStatement(_))
    }

    pub fn is_function_node(&self) -> bool {
        matches!(self, Statement::Function(_))
    }

    pub fn is_return_node(&self) -> bool {
        matches!(self, Statement::Return(_))
    }

    pub fn is_expr_statement(&self) -> bool {
        matches!(self, Statement::ExprStatement(_))
    }

    pub fn is_break(&self) -> bool {
        matches!(self, Statement::Break(_))
    }

    pub fn is_continue(&self) -> bool {
        matches!(self, Statement::Continue(_))
    }

    pub fn is_label(&self) -> bool {
        matches!(self, Statement::Label(_))
    }

    pub fn is_block(&self) -> bool {
        matches!(self, Statement::Block(_))
    }

    pub fn is_func_decl_node(&self) -> bool {
        matches!(self, Statement::FuncDecl(_))
    }

    pub fn is_module_declaration_node(&self) -> bool {
        matches!(
            self,
            Statement::ImportDeclaration(_)
                | Statement::ExportAllDeclaration(_)
                | Statement::ExportDefaultDeclaration(_)
                | Statement::ExportLocalDeclaration(_)
                | Statement::ExportNamedDeclaration(_)
        )
    }

    pub fn is_for_of_node(&self) -> bool {
        matches!(self, Statement::ForOf(_))
    }

    pub fn is_define_field_node(&self) -> bool {
        matches!(self, Statement::DefineField(_))
    }
}

/// `ScopeNode*` do C++: ponteiro para qualquer subclasse concreta de `ScopeNode`. O gerador de
/// bytecode recebe `ProgramNode`, `EvalNode`, `ModuleProgramNode` ou `FunctionNode` pelo mesmo campo,
/// então o ponteiro polimórfico vira um enum fechado de `NodeRef` das quatro subclasses (a hierarquia
/// é fechada no JSC). `borrow()` devolve o `ScopeNode` base, como o `->` do C++.
#[derive(Clone)]
pub enum ScopeNodeRef {
    Program(NodeRef<ProgramNode>),
    Eval(NodeRef<EvalNode>),
    ModuleProgram(NodeRef<ModuleProgramNode>),
    Function(NodeRef<FunctionNode>),
}

impl ScopeNodeRef {
    /// `ScopeNode::isFunctionNode()` (virtual): `false` na base, `true` só em `FunctionNode`.
    pub fn is_function_node(&self) -> bool {
        matches!(self, ScopeNodeRef::Function(_))
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, ScopeNode> {
        match self {
            ScopeNodeRef::Program(node) => std::cell::Ref::map(node.borrow(), |n| &**n),
            ScopeNodeRef::Eval(node) => std::cell::Ref::map(node.borrow(), |n| &**n),
            ScopeNodeRef::ModuleProgram(node) => std::cell::Ref::map(node.borrow(), |n| &**n),
            ScopeNodeRef::Function(node) => std::cell::Ref::map(node.borrow(), |n| &**n),
        }
    }
}

// Segunda metade do `Nodes.h`, em arquivos de fatia incluídos no mesmo módulo.
include!("nodes_part2.rs");
include!("nodes_part3.rs");
