// Despacho dos métodos virtuais de `ExpressionNode` e `StatementNode` (parser/Nodes.h) sobre os enums
// `Expression` e `Statement`: `isPure`, `emitBytecode` e `emitBytecodeInConditionContext`.
// Incluído por include! no fim de `nodes_codegen.rs`, sem `use` no topo: caminhos completos em tudo.
//
// Cada `match` é exaustivo de propósito (sem `_`): nó novo no enum quebra a compilação aqui, como a
// função virtual pura do C++ quebraria. Os corpos reais ficam nas fatias `nodes_codegen_cpp*`; aqui só
// se escolhe a struct concreta. As cinco classes com `emitBytecode(this)` (FuncExpr, ArrowFuncExpr,
// MethodDefinition, ClassExpr, ForOf) e as que precisam do próprio enum (DotAccessor, TaggedTemplate,
// FunctionCallDot) recebem a alça, como as fatias declaram.
//
// `needs_debug_hook` não ganha método no enum: é `expr.base().needs_debug_hook()` (campo do `Node`).

impl crate::parser::nodes::Expression {
    /// `ConstantNode::jsValue(generator)`. `None` quando o nó não é um `ConstantNode`; `Some(None)` quando
    /// `jsValue` devolve `JSValue()` (string ou BigInt enorme demais).
    fn constant_js_value(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
    ) -> Option<Option<crate::runtime::js_value::JSValue>> {
        match self {
            Self::Null(_) => Some(Some(crate::runtime::js_value::js_null())),
            Self::Boolean(n) => Some(Some(crate::runtime::js_value::js_boolean(n.borrow().value))),
            Self::Double(n) => Some(n.borrow().js_value(generator)),
            Self::Integer(n) => Some(n.borrow().js_value(generator)),
            Self::String(n) => Some(n.borrow().js_value(generator)),
            Self::BigInt(n) => Some(n.borrow().js_value(generator)),
            _ => None,
        }
    }

    /// `ExpressionNode::isPure`: falso por padrão; `ConstantNode` é puro e `ResolveNode` tem o seu.
    pub fn is_pure(&self, generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator) -> bool {
        match self {
            Self::Resolve(n) => n.borrow().is_pure(generator),
            _ => self.is_constant(),
        }
    }

    /// `ExpressionNode::emitBytecode`.
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        use crate::parser::nodes::{ArrowFuncExprNode, ClassExprNode, FuncExprNode, MethodDefinitionNode, PropertyListNode};
        match self {
            Self::Null(_) | Self::Boolean(_) | Self::String(_) | Self::BigInt(_) => {
                // `ConstantNode::emitBytecode` checa `ignoredResult` ANTES de `jsValue(generator)`: um
                // `"use strict";` descartado não pode entrar na tabela de constantes.
                if generator.is_ignored_dst(dst.as_ref()) {
                    return None;
                }
                let constant = self.constant_js_value(generator).flatten();
                constant_node_emit_bytecode(constant, generator, dst)
            }
            Self::Double(n) => n.borrow().emit_bytecode(generator, dst, false),
            Self::Integer(n) => n.borrow().emit_bytecode(generator, dst, true),
            Self::TemplateString(n) => n.borrow().emit_bytecode(generator, dst),
            Self::TemplateLiteral(n) => n.borrow().emit_bytecode(generator, dst),
            Self::TaggedTemplate(n) => n.borrow().emit_bytecode(n, generator, dst),
            Self::RegExp(n) => n.borrow().emit_bytecode(generator, dst),
            Self::This(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Super(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Import(n) => n.borrow().emit_bytecode(generator, dst),
            Self::NewTarget(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ImportMeta(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Resolve(n) => n.borrow().emit_bytecode(generator, dst),
            // `RELEASE_ASSERT_NOT_REACHED()`: o parser nunca deixa um PrivateIdentifierNode chegar ao emissor.
            Self::PrivateIdentifier(_) => unreachable!("PrivateIdentifierNode::emitBytecode"),
            Self::Array(n) => n.borrow().emit_bytecode(generator, dst),
            Self::PropertyList(n) => PropertyListNode::emit_bytecode(n, generator, dst, None, None, None),
            Self::ObjectLiteral(n) => n.borrow().emit_bytecode(generator, dst),
            Self::BracketAccessor(n) => n.borrow().emit_bytecode(generator, dst),
            Self::DotAccessor(n) => n.borrow().emit_bytecode(self, generator, dst),
            Self::SpreadExpression(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ObjectSpreadExpression(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ArgumentList(n) => n.borrow().emit_bytecode(generator, dst),
            Self::NewExpr(n) => n.borrow().emit_bytecode(generator, dst),
            Self::EvalFunctionCall(n) => n.borrow().emit_bytecode(generator, dst),
            Self::FunctionCallValue(n) => n.borrow().emit_bytecode(generator, dst),
            Self::StaticBlockFunctionCall(n) => n.borrow().emit_bytecode(generator, dst),
            Self::FunctionCallResolve(n) => n.borrow().emit_bytecode(generator, dst),
            Self::FunctionCallBracket(n) => n.borrow().emit_bytecode(generator, dst),
            Self::FunctionCallDot(n) => n.borrow().emit_bytecode(n, generator, dst),
            Self::BytecodeIntrinsic(n) => n.borrow().emit_bytecode(generator, dst),
            Self::CallFunctionCallDot(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ApplyFunctionCallDot(n) => n.borrow().emit_bytecode(generator, dst),
            Self::HasOwnPropertyFunctionCallDot(n) => n.borrow().emit_bytecode(generator, dst),
            Self::DeleteResolve(n) => n.borrow().emit_bytecode(generator, dst),
            Self::DeleteBracket(n) => n.borrow().emit_bytecode(generator, dst),
            Self::DeleteDot(n) => n.borrow().emit_bytecode(generator, dst),
            Self::DeleteValue(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Void(n) => n.borrow().emit_bytecode(generator, dst),
            Self::TypeOfResolve(n) => n.borrow().emit_bytecode(generator, dst),
            Self::TypeOfValue(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Prefix(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Postfix(n) => n.borrow().emit_bytecode(generator, dst),
            // `UnaryPlusNode` tem o seu; `NegateNode`, `BitwiseNotNode` e `LogicalNotNode` usam o de `UnaryOpNode`.
            Self::UnaryPlus(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Negate(n) => n.borrow().emit_bytecode(generator, dst),
            Self::BitwiseNot(n) => n.borrow().emit_bytecode(generator, dst),
            Self::LogicalNot(n) => n.borrow().emit_bytecode(generator, dst),
            // `BinaryOpNode::emitBytecode`, salvo `EqualNode`, `StrictEqualNode`, `InstanceOfNode` e `InNode`,
            // que sobrescrevem; a busca de método pela cadeia de `Deref` escolhe o mais próximo, como o C++.
            Self::Pow(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Mult(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Div(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Mod(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Add(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Sub(n) => n.borrow().emit_bytecode(generator, dst),
            Self::LeftShift(n) => n.borrow().emit_bytecode(generator, dst),
            Self::RightShift(n) => n.borrow().emit_bytecode(generator, dst),
            Self::UnsignedRightShift(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Less(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Greater(n) => n.borrow().emit_bytecode(generator, dst),
            Self::LessEq(n) => n.borrow().emit_bytecode(generator, dst),
            Self::GreaterEq(n) => n.borrow().emit_bytecode(generator, dst),
            Self::InstanceOf(n) => n.borrow().emit_bytecode(generator, dst),
            Self::In(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Equal(n) => n.borrow().emit_bytecode(generator, dst),
            Self::NotEqual(n) => n.borrow().emit_bytecode(generator, dst),
            Self::StrictEqual(n) => n.borrow().emit_bytecode(generator, dst),
            Self::NotStrictEqual(n) => n.borrow().emit_bytecode(generator, dst),
            Self::BitAnd(n) => n.borrow().emit_bytecode(generator, dst),
            Self::BitOr(n) => n.borrow().emit_bytecode(generator, dst),
            Self::BitXOr(n) => n.borrow().emit_bytecode(generator, dst),
            Self::LogicalOp(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Coalesce(n) => n.borrow().emit_bytecode(generator, dst),
            Self::OptionalChain(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Conditional(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ReadModifyResolve(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ShortCircuitReadModifyResolve(n) => n.borrow().emit_bytecode(generator, dst),
            Self::AssignResolve(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ReadModifyBracket(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ShortCircuitReadModifyBracket(n) => n.borrow().emit_bytecode(generator, dst),
            Self::AssignBracket(n) => n.borrow().emit_bytecode(generator, dst),
            Self::AssignDot(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ReadModifyDot(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ShortCircuitReadModifyDot(n) => n.borrow().emit_bytecode(generator, dst),
            Self::AssignError(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Comma(n) => n.borrow().emit_bytecode(generator, dst),
            Self::EmptyVarExpression(n) => n.borrow().emit_bytecode(generator, dst),
            Self::EmptyLetExpression(n) => n.borrow().emit_bytecode(generator, dst),
            Self::FuncExpr(n) => FuncExprNode::emit_bytecode(n, generator, dst),
            Self::ArrowFuncExpr(n) => ArrowFuncExprNode::emit_bytecode(n, generator, dst),
            Self::MethodDefinition(n) => MethodDefinitionNode::emit_bytecode(n, generator, dst),
            Self::YieldExpr(n) => n.borrow().emit_bytecode(generator, dst),
            Self::AwaitExpr(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ClassExpr(n) => ClassExprNode::emit_bytecode(n, generator, dst),
            Self::DestructuringAssignment(n) => n.borrow().emit_bytecode(generator, dst),
        }
    }

    /// `ExpressionNode::emitBytecodeInConditionContext` virtual: `ConstantNode`, `LogicalNotNode`,
    /// `BinaryOpNode` (e os filhos), `LogicalOpNode`, `CoalesceNode`, `OptionalChainNode`, `ConditionalNode`
    /// e `CommaNode` sobrescrevem; o resto usa o padrão da família.
    pub fn emit_bytecode_in_condition_context(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        true_target: &crate::bytecompiler::label::LabelRef,
        false_target: &crate::bytecompiler::label::LabelRef,
        fall_through_mode: crate::parser::nodes::FallThroughMode,
    ) {
        if let Some(constant) = self.constant_js_value(generator) {
            constant_node_emit_bytecode_in_condition_context(
                self,
                constant,
                generator,
                true_target,
                false_target,
                fall_through_mode,
            );
            return;
        }
        match self {
            Self::LogicalNot(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Pow(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Mult(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Div(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Mod(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Add(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Sub(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::LeftShift(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::RightShift(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::UnsignedRightShift(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Less(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Greater(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::LessEq(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::GreaterEq(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::InstanceOf(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::In(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Equal(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::NotEqual(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::StrictEqual(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::NotStrictEqual(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::BitAnd(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::BitOr(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::BitXOr(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::LogicalOp(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Coalesce(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::OptionalChain(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Conditional(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            Self::Comma(n) => {
                n.borrow().emit_bytecode_in_condition_context(self, generator, true_target, false_target, fall_through_mode)
            }
            _ => expression_node_emit_bytecode_in_condition_context(
                self,
                generator,
                true_target,
                false_target,
                fall_through_mode,
            ),
        }
    }
}

impl crate::parser::nodes::Statement {
    /// `StatementNode::emitBytecode`.
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        match self {
            Self::Block(n) => n.borrow().emit_bytecode(generator, dst),
            Self::EmptyStatement(n) => n.borrow().emit_bytecode(generator, dst),
            Self::DebuggerStatement(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ExprStatement(n) => n.borrow().emit_bytecode(generator, dst),
            Self::DeclarationStatement(n) => n.borrow().emit_bytecode(generator, dst),
            Self::IfElse(n) => n.borrow().emit_bytecode(generator, dst),
            Self::DoWhile(n) => n.borrow().emit_bytecode(generator, dst),
            Self::While(n) => n.borrow().emit_bytecode(generator, dst),
            Self::For(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ForIn(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ForOf(n) => crate::parser::nodes::ForOfNode::emit_bytecode(n, generator, dst),
            Self::Continue(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Break(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Return(n) => n.borrow().emit_bytecode(generator, dst),
            Self::With(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Label(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Throw(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Try(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Program(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Eval(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ModuleProgram(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ImportDeclaration(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ExportAllDeclaration(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ExportDefaultDeclaration(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ExportLocalDeclaration(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ExportNamedDeclaration(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Function(n) => n.borrow().emit_bytecode(generator, dst),
            Self::DefineField(n) => n.borrow().emit_bytecode(generator, dst),
            Self::FuncDecl(n) => n.borrow().emit_bytecode(generator, dst),
            Self::ClassDecl(n) => n.borrow().emit_bytecode(generator, dst),
            Self::Switch(n) => n.borrow().emit_bytecode(generator, dst),
        }
    }
}
