//! Os `static_cast` do `ExpressionNode` que o `NodesCodegen` faz depois de `isResolveNode()` e afins.
//!
//! No C++ cada `isXNode()` autoriza `static_cast<XNode*>(this)`. Aqui o par vira `as_x_node`, que devolve
//! `Some` só na variante correspondente do `enum Expression` (o `NodeRef` é clonado, então aponta para o
//! mesmo nó). As classes abstratas com mais de uma variante concreta (`NumberNode`, `FuncExprNode`) devolvem
//! um `Ref` já convertido para a classe base, porque `Double` e `Integer` (e `FuncExpr` e `MethodDefinition`)
//! são tipos distintos.

use std::cell::Ref;

use crate::parser::nodes::{
    AssignResolveNode, BracketAccessorNode, DestructuringAssignmentNode, DotAccessorNode, Expression, FuncExprNode,
    NodeRef, NumberNode, ResolveNode, StringNode,
};

macro_rules! single_variant_accessors {
    ($($method:ident => $variant:ident($node:ty);)*) => {
        impl Expression {
            $(
                pub fn $method(&self) -> Option<NodeRef<$node>> {
                    match self {
                        Expression::$variant(n) => Some(n.clone()),
                        _ => None,
                    }
                }
            )*
        }
    };
}

single_variant_accessors! {
    as_resolve_node => Resolve(ResolveNode);
    as_assign_resolve_node => AssignResolve(AssignResolveNode);
    as_dot_accessor_node => DotAccessor(DotAccessorNode);
    as_bracket_accessor_node => BracketAccessor(BracketAccessorNode);
    as_destructuring_node => DestructuringAssignment(DestructuringAssignmentNode);
    as_string_node => String(StringNode);
}

impl Expression {
    /// `static_cast<NumberNode*>`: `DoubleNode` e `IntegerNode`.
    pub fn as_number_node(&self) -> Option<Ref<'_, NumberNode>> {
        match self {
            Expression::Double(n) => Some(Ref::map(n.borrow(), |n| &n.base)),
            Expression::Integer(n) => Some(Ref::map(n.borrow(), |n| &n.base.base)),
            _ => None,
        }
    }

    /// `static_cast<FuncExprNode*>`: `FuncExprNode` e `MethodDefinitionNode` (que herda dele).
    pub fn as_func_expr_node(&self) -> Option<Ref<'_, FuncExprNode>> {
        match self {
            Expression::FuncExpr(n) => Some(n.borrow()),
            Expression::MethodDefinition(n) => Some(Ref::map(n.borrow(), |n| &n.base)),
            _ => None,
        }
    }
}
