// Fatia do `Nodes.h` de `TypeOfValueNode` até `ScopeNode`, com os construtores do `NodeConstructors.h`.
// Incluída por `include!` em `nodes.rs`: compartilha os `use` e as macros do módulo.

pub struct TypeOfValueNode {
    pub base: ExpressionNode,
    pub expr: Expression,
}

inherit!(TypeOfValueNode => ExpressionNode);

impl TypeOfValueNode {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        TypeOfValueNode { base: ExpressionNode::with_result_type(location, ResultType::string_type()), expr }
    }
}

pub struct PrefixNode {
    pub base: ExpressionNode,
    pub throwable: ThrowablePrefixedSubExpressionData,
    pub expr: Expression,
    pub operator: Operator,
}

inherit!(PrefixNode => ExpressionNode);

impl PrefixNode {
    pub fn new(
        location: &JSTokenLocation,
        expr: Expression,
        operator: Operator,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        PrefixNode {
            base: ExpressionNode::new(location),
            throwable: ThrowablePrefixedSubExpressionData::new(divot, divot_start, divot_end),
            expr,
            operator,
        }
    }
}

pub struct PostfixNode {
    pub base: PrefixNode,
}

inherit!(PostfixNode => PrefixNode);

impl PostfixNode {
    pub fn new(
        location: &JSTokenLocation,
        expr: Expression,
        operator: Operator,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        PostfixNode { base: PrefixNode::new(location, expr, operator, divot, divot_start, divot_end) }
    }
}

pub struct UnaryOpNode {
    pub base: ExpressionNode,
    pub expr: Expression,
    pub opcode_id: OpcodeID,
}

inherit!(UnaryOpNode => ExpressionNode);

impl UnaryOpNode {
    pub fn new(location: &JSTokenLocation, result_type: ResultType, expr: Expression, opcode_id: OpcodeID) -> Self {
        UnaryOpNode { base: ExpressionNode::with_result_type(location, result_type), expr, opcode_id }
    }
}

/// Declara uma subclasse final de `UnaryOpNode` cujo construtor só escolhe o `ResultType` (a partir do
/// operando, quando precisa) e o `OpcodeID`.
macro_rules! unary_op_node {
    ($name:ident, $opcode:ident, ($operand:ident) => $result_type:expr) => {
        pub struct $name {
            pub base: UnaryOpNode,
        }

        inherit!($name => UnaryOpNode);

        impl $name {
            pub fn new(location: &JSTokenLocation, $operand: Expression) -> Self {
                let result_type = $result_type;
                $name { base: UnaryOpNode::new(location, result_type, $operand, OpcodeID::$opcode) }
            }
        }
    };
}

// `UnaryPlus` sempre devolve número, nunca BigInt (ECMA-262, sec-unary-plus-operator-runtime-semantics-evaluation).
unary_op_node!(UnaryPlusNode, op_to_number, (_expr) => ResultType::number_type());
unary_op_node!(NegateNode, op_negate, (expr) => ResultType::for_unary_arith(expr.result_descriptor()));
unary_op_node!(BitwiseNotNode, op_bitnot, (_expr) => ResultType::for_bit_op());
unary_op_node!(LogicalNotNode, op_not, (_expr) => ResultType::boolean_type());

pub struct BinaryOpNode {
    pub base: ExpressionNode,
    pub right_has_assignments: bool,
    pub should_to_unsigned_result: bool,
    pub opcode_id: OpcodeID,
    pub expr1: Expression,
    pub expr2: Expression,
}

inherit!(BinaryOpNode => ExpressionNode);

impl BinaryOpNode {
    pub fn new(
        location: &JSTokenLocation,
        expr1: Expression,
        expr2: Expression,
        opcode_id: OpcodeID,
        right_has_assignments: bool,
    ) -> Self {
        Self::from_base(ExpressionNode::new(location), expr1, expr2, opcode_id, right_has_assignments)
    }

    pub fn with_result_type(
        location: &JSTokenLocation,
        result_type: ResultType,
        expr1: Expression,
        expr2: Expression,
        opcode_id: OpcodeID,
        right_has_assignments: bool,
    ) -> Self {
        Self::from_base(
            ExpressionNode::with_result_type(location, result_type),
            expr1,
            expr2,
            opcode_id,
            right_has_assignments,
        )
    }

    fn from_base(
        base: ExpressionNode,
        expr1: Expression,
        expr2: Expression,
        opcode_id: OpcodeID,
        right_has_assignments: bool,
    ) -> Self {
        BinaryOpNode { base, right_has_assignments, should_to_unsigned_result: true, opcode_id, expr1, expr2 }
    }
}

/// Declara uma subclasse final de `BinaryOpNode` cujo construtor só escolhe o `ResultType` (a partir dos
/// tipos dos operandos, quando precisa) e o `OpcodeID`.
macro_rules! binary_op_node {
    ($name:ident, $opcode:ident, ($type1:ident, $type2:ident) => $result_type:expr) => {
        pub struct $name {
            pub base: BinaryOpNode,
        }

        inherit!($name => BinaryOpNode);

        impl $name {
            pub fn new(
                location: &JSTokenLocation,
                expr1: Expression,
                expr2: Expression,
                right_has_assignments: bool,
            ) -> Self {
                let $type1 = expr1.result_descriptor();
                let $type2 = expr2.result_descriptor();
                let result_type = $result_type;
                $name {
                    base: BinaryOpNode::with_result_type(
                        location,
                        result_type,
                        expr1,
                        expr2,
                        OpcodeID::$opcode,
                        right_has_assignments,
                    ),
                }
            }
        }
    };
}

binary_op_node!(PowNode, op_pow, (a, b) => ResultType::for_non_add_arith(a, b));
binary_op_node!(MultNode, op_mul, (a, b) => ResultType::for_non_add_arith(a, b));
binary_op_node!(DivNode, op_div, (a, b) => ResultType::for_non_add_arith(a, b));
binary_op_node!(ModNode, op_mod, (a, b) => ResultType::for_non_add_arith(a, b));
binary_op_node!(AddNode, op_add, (a, b) => ResultType::for_add(a, b));
binary_op_node!(SubNode, op_sub, (a, b) => ResultType::for_non_add_arith(a, b));
binary_op_node!(LeftShiftNode, op_lshift, (_a, _b) => ResultType::for_bit_op());
binary_op_node!(RightShiftNode, op_rshift, (_a, _b) => ResultType::for_bit_op());
binary_op_node!(UnsignedRightShiftNode, op_urshift, (_a, _b) => ResultType::number_type());
binary_op_node!(LessNode, op_less, (_a, _b) => ResultType::boolean_type());
binary_op_node!(GreaterNode, op_greater, (_a, _b) => ResultType::boolean_type());
binary_op_node!(LessEqNode, op_lesseq, (_a, _b) => ResultType::boolean_type());
binary_op_node!(GreaterEqNode, op_greatereq, (_a, _b) => ResultType::boolean_type());

pub struct ThrowableBinaryOpNode {
    pub base: BinaryOpNode,
    pub throwable: ThrowableExpressionData,
}

inherit!(ThrowableBinaryOpNode => BinaryOpNode);

impl ThrowableBinaryOpNode {
    pub fn with_result_type(
        location: &JSTokenLocation,
        result_type: ResultType,
        expr1: Expression,
        expr2: Expression,
        opcode_id: OpcodeID,
        right_has_assignments: bool,
    ) -> Self {
        ThrowableBinaryOpNode {
            base: BinaryOpNode::with_result_type(
                location,
                result_type,
                expr1,
                expr2,
                opcode_id,
                right_has_assignments,
            ),
            throwable: ThrowableExpressionData::default(),
        }
    }

    pub fn new(
        location: &JSTokenLocation,
        expr1: Expression,
        expr2: Expression,
        opcode_id: OpcodeID,
        right_has_assignments: bool,
    ) -> Self {
        ThrowableBinaryOpNode {
            base: BinaryOpNode::new(location, expr1, expr2, opcode_id, right_has_assignments),
            throwable: ThrowableExpressionData::default(),
        }
    }
}

pub struct InstanceOfNode {
    pub base: ThrowableBinaryOpNode,
}

inherit!(InstanceOfNode => ThrowableBinaryOpNode);

impl InstanceOfNode {
    pub fn new(location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Self {
        InstanceOfNode {
            base: ThrowableBinaryOpNode::with_result_type(
                location,
                ResultType::boolean_type(),
                expr1,
                expr2,
                OpcodeID::op_instanceof,
                right_has_assignments,
            ),
        }
    }
}

pub struct InNode {
    pub base: ThrowableBinaryOpNode,
}

inherit!(InNode => ThrowableBinaryOpNode);

impl InNode {
    pub fn new(location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Self {
        InNode {
            base: ThrowableBinaryOpNode::new(location, expr1, expr2, OpcodeID::op_in_by_val, right_has_assignments),
        }
    }
}

binary_op_node!(EqualNode, op_eq, (_a, _b) => ResultType::boolean_type());
binary_op_node!(NotEqualNode, op_neq, (_a, _b) => ResultType::boolean_type());
binary_op_node!(StrictEqualNode, op_stricteq, (_a, _b) => ResultType::boolean_type());
binary_op_node!(NotStrictEqualNode, op_nstricteq, (_a, _b) => ResultType::boolean_type());
binary_op_node!(BitAndNode, op_bitand, (_a, _b) => ResultType::for_bit_op());
binary_op_node!(BitOrNode, op_bitor, (_a, _b) => ResultType::for_bit_op());
binary_op_node!(BitXOrNode, op_bitxor, (_a, _b) => ResultType::for_bit_op());

/// `m_expr1 && m_expr2`, `m_expr1 || m_expr2`.
pub struct LogicalOpNode {
    pub base: ExpressionNode,
    pub operator: LogicalOperator,
    pub expr1: Expression,
    pub expr2: Expression,
}

inherit!(LogicalOpNode => ExpressionNode);

impl LogicalOpNode {
    pub fn new(location: &JSTokenLocation, expr1: Expression, expr2: Expression, operator: LogicalOperator) -> Self {
        let result_type = ResultType::for_logical_op(expr1.result_descriptor(), expr2.result_descriptor());
        LogicalOpNode { base: ExpressionNode::with_result_type(location, result_type), operator, expr1, expr2 }
    }
}

pub struct CoalesceNode {
    pub base: ExpressionNode,
    pub expr1: Expression,
    pub expr2: Expression,
    pub has_absorbed_optional_chain: bool,
}

inherit!(CoalesceNode => ExpressionNode);

impl CoalesceNode {
    pub fn new(
        location: &JSTokenLocation,
        expr1: Expression,
        expr2: Expression,
        has_absorbed_optional_chain: bool,
    ) -> Self {
        let result_type = ResultType::for_coalesce(expr1.result_descriptor(), expr2.result_descriptor());
        CoalesceNode {
            base: ExpressionNode::with_result_type(location, result_type),
            expr1,
            expr2,
            has_absorbed_optional_chain,
        }
    }
}

pub struct OptionalChainNode {
    pub base: ExpressionNode,
    pub expr: Expression,
    pub is_outermost: bool,
}

inherit!(OptionalChainNode => ExpressionNode);

impl OptionalChainNode {
    pub fn new(location: &JSTokenLocation, expr: Expression, is_outermost: bool) -> Self {
        OptionalChainNode { base: ExpressionNode::new(location), expr, is_outermost }
    }
}

/// O operador ternário, `m_logical ? m_expr1 : m_expr2`.
pub struct ConditionalNode {
    pub base: ExpressionNode,
    pub logical: Expression,
    pub expr1: Expression,
    pub expr2: Expression,
}

inherit!(ConditionalNode => ExpressionNode);

impl ConditionalNode {
    pub fn new(location: &JSTokenLocation, logical: Expression, expr1: Expression, expr2: Expression) -> Self {
        ConditionalNode { base: ExpressionNode::new(location), logical, expr1, expr2 }
    }
}

pub struct ReadModifyResolveNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub ident: Identifier,
    pub right: Expression,
    pub operator: Operator,
    pub right_has_assignments: bool,
}

inherit!(ReadModifyResolveNode => ExpressionNode);

impl ReadModifyResolveNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: Identifier,
        operator: Operator,
        right: Expression,
        right_has_assignments: bool,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        ReadModifyResolveNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            ident,
            right,
            operator,
            right_has_assignments,
        }
    }
}

pub struct ShortCircuitReadModifyResolveNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub ident: Identifier,
    pub right: Expression,
    pub operator: Operator,
    pub right_has_assignments: bool,
}

inherit!(ShortCircuitReadModifyResolveNode => ExpressionNode);

impl ShortCircuitReadModifyResolveNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: Identifier,
        operator: Operator,
        right: Expression,
        right_has_assignments: bool,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        ShortCircuitReadModifyResolveNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            ident,
            right,
            operator,
            right_has_assignments,
        }
    }
}

pub struct AssignResolveNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub ident: Identifier,
    pub right: Expression,
    pub assignment_context: AssignmentContext,
}

inherit!(AssignResolveNode => ExpressionNode);

impl AssignResolveNode {
    pub fn new(
        location: &JSTokenLocation,
        ident: Identifier,
        right: Expression,
        assignment_context: AssignmentContext,
    ) -> Self {
        AssignResolveNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::default(),
            ident,
            right,
            assignment_context,
        }
    }
}

pub struct ReadModifyBracketNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableSubExpressionData,
    pub base_expr: Expression,
    pub subscript: Expression,
    pub right: Expression,
    pub operator: Operator,
    pub subscript_has_assignments: bool,
    pub right_has_assignments: bool,
}

inherit!(ReadModifyBracketNode => ExpressionNode);

impl ReadModifyBracketNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        subscript: Expression,
        operator: Operator,
        right: Expression,
        subscript_has_assignments: bool,
        right_has_assignments: bool,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        ReadModifyBracketNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableSubExpressionData::new(divot, divot_start, divot_end),
            base_expr,
            subscript,
            right,
            operator,
            subscript_has_assignments,
            right_has_assignments,
        }
    }
}

pub struct ShortCircuitReadModifyBracketNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableSubExpressionData,
    pub base_expr: Expression,
    pub subscript: Expression,
    pub right: Expression,
    pub operator: Operator,
    pub subscript_has_assignments: bool,
    pub right_has_assignments: bool,
}

inherit!(ShortCircuitReadModifyBracketNode => ExpressionNode);

impl ShortCircuitReadModifyBracketNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        subscript: Expression,
        operator: Operator,
        right: Expression,
        subscript_has_assignments: bool,
        right_has_assignments: bool,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        ShortCircuitReadModifyBracketNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableSubExpressionData::new(divot, divot_start, divot_end),
            base_expr,
            subscript,
            right,
            operator,
            subscript_has_assignments,
            right_has_assignments,
        }
    }
}

pub struct AssignBracketNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub base_expr: Expression,
    pub subscript: Expression,
    pub right: Expression,
    pub subscript_has_assignments: bool,
    pub right_has_assignments: bool,
}

inherit!(AssignBracketNode => ExpressionNode);

impl AssignBracketNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        subscript: Expression,
        right: Expression,
        subscript_has_assignments: bool,
        right_has_assignments: bool,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        AssignBracketNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            base_expr,
            subscript,
            right,
            subscript_has_assignments,
            right_has_assignments,
        }
    }
}

pub struct AssignDotNode {
    pub base: BaseDotNode,
    pub throwable: ThrowableExpressionData,
    pub right: Expression,
    pub right_has_assignments: bool,
}

inherit!(AssignDotNode => BaseDotNode);

impl AssignDotNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        ident: Identifier,
        type_: DotType,
        right: Expression,
        right_has_assignments: bool,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        AssignDotNode {
            base: BaseDotNode::new(location, base_expr, ident, type_),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            right,
            right_has_assignments,
        }
    }
}

pub struct ReadModifyDotNode {
    pub base: BaseDotNode,
    pub throwable: ThrowableSubExpressionData,
    pub right: Expression,
    pub operator: Operator,
    pub right_has_assignments: bool,
}

inherit!(ReadModifyDotNode => BaseDotNode);

impl ReadModifyDotNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        ident: Identifier,
        type_: DotType,
        operator: Operator,
        right: Expression,
        right_has_assignments: bool,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        ReadModifyDotNode {
            base: BaseDotNode::new(location, base_expr, ident, type_),
            throwable: ThrowableSubExpressionData::new(divot, divot_start, divot_end),
            right,
            operator,
            right_has_assignments,
        }
    }
}

pub struct ShortCircuitReadModifyDotNode {
    pub base: BaseDotNode,
    pub throwable: ThrowableSubExpressionData,
    pub right: Expression,
    pub operator: Operator,
    pub right_has_assignments: bool,
}

inherit!(ShortCircuitReadModifyDotNode => BaseDotNode);

impl ShortCircuitReadModifyDotNode {
    pub fn new(
        location: &JSTokenLocation,
        base_expr: Expression,
        ident: Identifier,
        type_: DotType,
        operator: Operator,
        right: Expression,
        right_has_assignments: bool,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        ShortCircuitReadModifyDotNode {
            base: BaseDotNode::new(location, base_expr, ident, type_),
            throwable: ThrowableSubExpressionData::new(divot, divot_start, divot_end),
            right,
            operator,
            right_has_assignments,
        }
    }
}

pub struct AssignErrorNode {
    pub base: ExpressionNode,
    pub throwable: ThrowableExpressionData,
    pub left: Expression,
}

inherit!(AssignErrorNode => ExpressionNode);

impl AssignErrorNode {
    pub fn new(
        location: &JSTokenLocation,
        left: Expression,
        divot: JSTextPosition,
        divot_start: JSTextPosition,
        divot_end: JSTextPosition,
    ) -> Self {
        AssignErrorNode {
            base: ExpressionNode::new(location),
            throwable: ThrowableExpressionData::new(divot, divot_start, divot_end),
            left,
        }
    }
}

pub struct CommaNode {
    pub base: ExpressionNode,
    pub expr: Expression,
    pub next: Option<NodeRef<CommaNode>>,
}

inherit!(CommaNode => ExpressionNode);

impl CommaNode {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        CommaNode { base: ExpressionNode::new(location), expr, next: None }
    }
}

/// `SourceElements`: a lista de statements de um bloco ou escopo.
///
/// Como no C++: os statements se encadeiam pelo `m_next` de cada `StatementNode`, e `head`/`tail` são alças
/// para os mesmos nós, de modo que o `append` é em tempo constante.
#[derive(Default)]
pub struct SourceElements {
    head: Option<Statement>,
    tail: Option<Statement>,
}

impl SourceElements {
    pub fn new() -> Self {
        SourceElements { head: None, tail: None }
    }

    pub fn append(&mut self, statement: Statement) {
        if statement.is_empty_statement() {
            return;
        }

        match self.tail.replace(statement.clone()) {
            Some(tail) => tail.base_mut().set_next(Some(statement)),
            None => self.head = Some(statement),
        }
    }

    /// Os statements na ordem do `m_next`, a partir do `m_head`.
    pub fn iter(&self) -> impl Iterator<Item = Statement> {
        std::iter::successors(self.head.clone(), |statement| statement.base().next())
    }

    /// `m_head == m_tail ? m_head : nullptr`: com a lista vazia os dois são nulos e o resultado é nulo.
    pub fn single_statement(&self) -> Option<Statement> {
        match (&self.head, &self.tail) {
            (Some(head), Some(tail)) if head == tail => Some(head.clone()),
            _ => None,
        }
    }

    pub fn first_statement(&self) -> Option<Statement> {
        self.head.clone()
    }

    pub fn last_statement(&self) -> Option<Statement> {
        self.tail.clone()
    }

    pub fn has_completion_value(&self) -> bool {
        self.iter().any(|statement| statement.has_completion_value())
    }

    pub fn has_early_break_or_continue(&self) -> bool {
        for statement in self.iter() {
            if statement.has_early_break_or_continue() {
                return true;
            }
            if statement.has_completion_value() {
                return false;
            }
        }

        false
    }
}

pub struct BlockNode {
    pub base: StatementNode,
    pub variable_environment: VariableEnvironmentNode,
    pub statements: Option<NodeRef<SourceElements>>,
}

inherit!(BlockNode => StatementNode);

impl BlockNode {
    pub fn new(
        location: &JSTokenLocation,
        statements: Option<NodeRef<SourceElements>>,
        lexical_variables: VariableEnvironment,
        function_stack: FunctionStack,
    ) -> Self {
        BlockNode {
            base: StatementNode::new(location),
            variable_environment: VariableEnvironmentNode::with_function_stack(lexical_variables, function_stack),
            statements,
        }
    }

    pub fn last_statement(&self) -> Option<Statement> {
        self.statements.as_ref().and_then(|statements| statements.borrow().last_statement())
    }

    pub fn single_statement(&self) -> Option<Statement> {
        self.statements.as_ref().and_then(|statements| statements.borrow().single_statement())
    }

    pub fn has_completion_value(&self) -> bool {
        self.statements.as_ref().is_some_and(|statements| statements.borrow().has_completion_value())
    }

    pub fn has_early_break_or_continue(&self) -> bool {
        self.statements.as_ref().is_some_and(|statements| statements.borrow().has_early_break_or_continue())
    }
}

pub struct EmptyStatementNode {
    pub base: StatementNode,
}

inherit!(EmptyStatementNode => StatementNode);

impl EmptyStatementNode {
    pub fn new(location: &JSTokenLocation) -> Self {
        EmptyStatementNode { base: StatementNode::new(location) }
    }
}

pub struct DebuggerStatementNode {
    pub base: StatementNode,
}

inherit!(DebuggerStatementNode => StatementNode);

impl DebuggerStatementNode {
    pub fn new(location: &JSTokenLocation) -> Self {
        DebuggerStatementNode { base: StatementNode::new(location) }
    }
}

pub struct ExprStatementNode {
    pub base: StatementNode,
    pub expr: Expression,
}

inherit!(ExprStatementNode => StatementNode);

impl ExprStatementNode {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        ExprStatementNode { base: StatementNode::new(location), expr }
    }
}

pub struct DeclarationStatement {
    pub base: StatementNode,
    pub expr: Expression,
}

inherit!(DeclarationStatement => StatementNode);

impl DeclarationStatement {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        DeclarationStatement { base: StatementNode::new(location), expr }
    }
}

pub struct EmptyVarExpression {
    pub base: ExpressionNode,
    pub ident: Identifier,
}

inherit!(EmptyVarExpression => ExpressionNode);

impl EmptyVarExpression {
    pub fn new(location: &JSTokenLocation, ident: Identifier) -> Self {
        EmptyVarExpression { base: ExpressionNode::new(location), ident }
    }
}

pub struct EmptyLetExpression {
    pub base: ExpressionNode,
    pub ident: Identifier,
}

inherit!(EmptyLetExpression => ExpressionNode);

impl EmptyLetExpression {
    pub fn new(location: &JSTokenLocation, ident: Identifier) -> Self {
        EmptyLetExpression { base: ExpressionNode::new(location), ident }
    }
}

pub struct IfElseNode {
    pub base: StatementNode,
    pub condition: Expression,
    pub if_block: Statement,
    pub else_block: Option<Statement>,
}

inherit!(IfElseNode => StatementNode);

impl IfElseNode {
    pub fn new(
        location: &JSTokenLocation,
        condition: Expression,
        if_block: Statement,
        else_block: Option<Statement>,
    ) -> Self {
        IfElseNode { base: StatementNode::new(location), condition, if_block, else_block }
    }
}

pub struct DoWhileNode {
    pub base: StatementNode,
    pub statement: Statement,
    pub expr: Expression,
}

inherit!(DoWhileNode => StatementNode);

impl DoWhileNode {
    pub fn new(location: &JSTokenLocation, statement: Statement, expr: Expression) -> Self {
        DoWhileNode { base: StatementNode::new(location), statement, expr }
    }
}

pub struct WhileNode {
    pub base: StatementNode,
    pub expr: Expression,
    pub statement: Statement,
}

inherit!(WhileNode => StatementNode);

impl WhileNode {
    pub fn new(location: &JSTokenLocation, expr: Expression, statement: Statement) -> Self {
        WhileNode { base: StatementNode::new(location), expr, statement }
    }
}

pub struct ForNode {
    pub base: StatementNode,
    pub variable_environment: VariableEnvironmentNode,
    pub expr1: Option<Expression>,
    pub expr2: Option<Expression>,
    pub expr3: Option<Expression>,
    pub statement: Statement,
    pub initializer_contains_closure: bool,
}

inherit!(ForNode => StatementNode);

impl ForNode {
    pub fn new(
        location: &JSTokenLocation,
        expr1: Option<Expression>,
        expr2: Option<Expression>,
        expr3: Option<Expression>,
        statement: Statement,
        lexical_variables: VariableEnvironment,
        initializer_contains_closure: bool,
    ) -> Self {
        ForNode {
            base: StatementNode::new(location),
            variable_environment: VariableEnvironmentNode::with_lexical_variables(lexical_variables),
            expr1,
            expr2,
            expr3,
            statement,
            initializer_contains_closure,
        }
    }
}

pub struct EnumerationNode {
    pub base: StatementNode,
    pub throwable: ThrowableExpressionData,
    pub variable_environment: VariableEnvironmentNode,
    pub lexpr: Expression,
    pub expr: Expression,
    pub statement: Statement,
}

inherit!(EnumerationNode => StatementNode);

impl EnumerationNode {
    pub fn new(
        location: &JSTokenLocation,
        lexpr: Expression,
        expr: Expression,
        statement: Statement,
        lexical_variables: VariableEnvironment,
    ) -> Self {
        EnumerationNode {
            base: StatementNode::new(location),
            throwable: ThrowableExpressionData::default(),
            variable_environment: VariableEnvironmentNode::with_lexical_variables(lexical_variables),
            lexpr,
            expr,
            statement,
        }
    }
}

pub struct ForInNode {
    pub base: EnumerationNode,
}

inherit!(ForInNode => EnumerationNode);

impl ForInNode {
    pub fn new(
        location: &JSTokenLocation,
        lexpr: Expression,
        expr: Expression,
        statement: Statement,
        lexical_variables: VariableEnvironment,
    ) -> Self {
        ForInNode { base: EnumerationNode::new(location, lexpr, expr, statement, lexical_variables) }
    }
}

pub struct ForOfNode {
    pub base: EnumerationNode,
    pub is_for_await: bool,
}

inherit!(ForOfNode => EnumerationNode);

impl ForOfNode {
    pub fn new(
        is_for_await: bool,
        location: &JSTokenLocation,
        lexpr: Expression,
        expr: Expression,
        statement: Statement,
        lexical_variables: VariableEnvironment,
    ) -> Self {
        ForOfNode { base: EnumerationNode::new(location, lexpr, expr, statement, lexical_variables), is_for_await }
    }
}

pub struct ContinueNode {
    pub base: StatementNode,
    pub throwable: ThrowableExpressionData,
    pub ident: Identifier,
}

inherit!(ContinueNode => StatementNode);

impl ContinueNode {
    pub fn new(location: &JSTokenLocation, ident: Identifier) -> Self {
        ContinueNode { base: StatementNode::new(location), throwable: ThrowableExpressionData::default(), ident }
    }
}

pub struct BreakNode {
    pub base: StatementNode,
    pub throwable: ThrowableExpressionData,
    pub ident: Identifier,
}

inherit!(BreakNode => StatementNode);

impl BreakNode {
    pub fn new(location: &JSTokenLocation, ident: Identifier) -> Self {
        BreakNode { base: StatementNode::new(location), throwable: ThrowableExpressionData::default(), ident }
    }
}

pub struct ReturnNode {
    pub base: StatementNode,
    pub throwable: ThrowableExpressionData,
    pub value: Option<Expression>,
}

inherit!(ReturnNode => StatementNode);

impl ReturnNode {
    pub fn new(location: &JSTokenLocation, value: Option<Expression>) -> Self {
        ReturnNode { base: StatementNode::new(location), throwable: ThrowableExpressionData::default(), value }
    }
}

pub struct WithNode {
    pub base: StatementNode,
    pub expr: Expression,
    pub statement: Statement,
    pub divot: JSTextPosition,
    pub expression_length: u32,
}

inherit!(WithNode => StatementNode);

impl WithNode {
    pub fn new(
        location: &JSTokenLocation,
        expr: Expression,
        statement: Statement,
        divot: JSTextPosition,
        expression_length: u32,
    ) -> Self {
        WithNode { base: StatementNode::new(location), expr, statement, divot, expression_length }
    }
}

pub struct LabelNode {
    pub base: StatementNode,
    pub throwable: ThrowableExpressionData,
    pub name: Identifier,
    pub statement: Statement,
}

inherit!(LabelNode => StatementNode);

impl LabelNode {
    pub fn new(location: &JSTokenLocation, name: Identifier, statement: Statement) -> Self {
        LabelNode {
            base: StatementNode::new(location),
            throwable: ThrowableExpressionData::default(),
            name,
            statement,
        }
    }
}

pub struct ThrowNode {
    pub base: StatementNode,
    pub throwable: ThrowableExpressionData,
    pub expr: Expression,
}

inherit!(ThrowNode => StatementNode);

impl ThrowNode {
    pub fn new(location: &JSTokenLocation, expr: Expression) -> Self {
        ThrowNode { base: StatementNode::new(location), throwable: ThrowableExpressionData::default(), expr }
    }
}

pub struct TryNode {
    pub base: StatementNode,
    pub variable_environment: VariableEnvironmentNode,
    pub try_block: Statement,
    pub catch_pattern: Option<DestructuringPatternNode>,
    pub catch_block: Option<Statement>,
    pub finally_block: Option<Statement>,
}

inherit!(TryNode => StatementNode);

impl TryNode {
    pub fn new(
        location: &JSTokenLocation,
        try_block: Statement,
        catch_pattern: Option<DestructuringPatternNode>,
        catch_block: Option<Statement>,
        catch_environment: VariableEnvironment,
        finally_block: Option<Statement>,
    ) -> Self {
        TryNode {
            base: StatementNode::new(location),
            variable_environment: VariableEnvironmentNode::with_lexical_variables(catch_environment),
            try_block,
            catch_pattern,
            catch_block,
            finally_block,
        }
    }
}

/// Classe abstrata: pai de `ProgramNode`, `EvalNode`, `ModuleProgramNode` e `FunctionNode`. Nunca é
/// instanciada diretamente.
pub struct ScopeNode {
    pub base: StatementNode,
    pub arena_root: ParserArenaRoot,
    pub variable_environment: VariableEnvironmentNode,
    pub start_line_number: i32,
    pub start_start_offset: u32,
    pub start_line_start_offset: u32,
    pub features: CodeFeatures,
    pub lexically_scoped_features: LexicallyScopedFeatures,
    pub inner_arrow_function_code_features: InnerArrowFunctionCodeFeatures,
    pub source: SourceCode,
    pub var_declarations: VariableEnvironment,
    pub num_constants: i32,
    pub statements: Option<NodeRef<SourceElements>>,
}

inherit!(ScopeNode => StatementNode);

impl ScopeNode {
    /// `ScopeNode(parserArena, start, end, lexicallyScopedFeatures)`: sem statements.
    pub fn new(
        parser_arena: &mut ParserArena,
        start_location: &JSTokenLocation,
        end_location: &JSTokenLocation,
        lexically_scoped_features: LexicallyScopedFeatures,
    ) -> Self {
        ScopeNode {
            base: StatementNode::new(end_location),
            arena_root: ParserArenaRoot::new(parser_arena),
            variable_environment: VariableEnvironmentNode::default(),
            start_line_number: start_location.line as i32,
            start_start_offset: start_location.start_offset as u32,
            start_line_start_offset: start_location.line_start_offset as u32,
            features: NO_FEATURES,
            lexically_scoped_features,
            inner_arrow_function_code_features: NO_INNER_ARROW_FUNCTION_FEATURES,
            source: SourceCode::default(),
            var_declarations: VariableEnvironment::default(),
            num_constants: 0,
            statements: None,
        }
    }

    /// `ScopeNode(parserArena, start, end, source, children, varEnvironment, funcStack, lexicalVariables, ...)`.
    pub fn with_statements(
        parser_arena: &mut ParserArena,
        start_location: &JSTokenLocation,
        end_location: &JSTokenLocation,
        source: &SourceCode,
        children: Option<NodeRef<SourceElements>>,
        var_environment: VariableEnvironment,
        func_stack: FunctionStack,
        lexical_variables: VariableEnvironment,
        features: CodeFeatures,
        lexically_scoped_features: LexicallyScopedFeatures,
        inner_arrow_function_code_features: InnerArrowFunctionCodeFeatures,
        num_constants: i32,
    ) -> Self {
        ScopeNode {
            base: StatementNode::new(end_location),
            arena_root: ParserArenaRoot::new(parser_arena),
            variable_environment: VariableEnvironmentNode::with_function_stack(lexical_variables, func_stack),
            start_line_number: start_location.line as i32,
            start_start_offset: start_location.start_offset as u32,
            start_line_start_offset: start_location.line_start_offset as u32,
            features,
            lexically_scoped_features,
            inner_arrow_function_code_features,
            source: source.clone(),
            var_declarations: var_environment,
            num_constants,
            statements: children,
        }
    }

    pub fn start_line(&self) -> i32 {
        self.start_line_number
    }

    pub fn start_start_offset(&self) -> i32 {
        self.start_start_offset as i32
    }

    pub fn start_line_start_offset(&self) -> i32 {
        self.start_line_start_offset as i32
    }

    pub fn do_any_inner_arrow_functions_use_any_feature(&self) -> bool {
        self.inner_arrow_function_code_features != NO_INNER_ARROW_FUNCTION_FEATURES
    }

    pub fn do_any_inner_arrow_functions_use_arguments(&self) -> bool {
        (self.inner_arrow_function_code_features & ARGUMENTS_INNER_ARROW_FUNCTION_FEATURE) != 0
    }

    pub fn do_any_inner_arrow_functions_use_super_call(&self) -> bool {
        (self.inner_arrow_function_code_features & SUPER_CALL_INNER_ARROW_FUNCTION_FEATURE) != 0
    }

    pub fn do_any_inner_arrow_functions_use_super_property(&self) -> bool {
        (self.inner_arrow_function_code_features & SUPER_PROPERTY_INNER_ARROW_FUNCTION_FEATURE) != 0
    }

    pub fn do_any_inner_arrow_functions_use_eval(&self) -> bool {
        (self.inner_arrow_function_code_features & EVAL_INNER_ARROW_FUNCTION_FEATURE) != 0
    }

    pub fn do_any_inner_arrow_functions_use_this(&self) -> bool {
        (self.inner_arrow_function_code_features & THIS_INNER_ARROW_FUNCTION_FEATURE) != 0
    }

    pub fn do_any_inner_arrow_functions_use_new_target(&self) -> bool {
        (self.inner_arrow_function_code_features & NEW_TARGET_INNER_ARROW_FUNCTION_FEATURE) != 0
    }

    pub fn uses_eval(&self) -> bool {
        (self.features & EVAL_FEATURE) != 0
    }

    pub fn has_shadows_arguments_feature(&self) -> bool {
        (self.features & SHADOWS_ARGUMENTS_FEATURE) != 0
    }

    pub fn uses_arguments(&self) -> bool {
        (self.features & ARGUMENTS_FEATURE) != 0 && (self.features & SHADOWS_ARGUMENTS_FEATURE) == 0
    }

    pub fn uses_arrow_function(&self) -> bool {
        (self.features & ARROW_FUNCTION_FEATURE) != 0
    }

    pub fn is_strict_mode(&self) -> bool {
        (self.lexically_scoped_features & STRICT_MODE_LEXICALLY_SCOPED_FEATURE) != 0
    }

    pub fn uses_this(&self) -> bool {
        (self.features & THIS_FEATURE) != 0
    }

    pub fn uses_super_call(&self) -> bool {
        (self.features & SUPER_CALL_FEATURE) != 0
    }

    pub fn uses_super_property(&self) -> bool {
        (self.features & SUPER_PROPERTY_FEATURE) != 0
    }

    pub fn uses_new_target(&self) -> bool {
        (self.features & NEW_TARGET_FEATURE) != 0
    }

    pub fn is_async_function_without_await(&self) -> bool {
        (self.features & ASYNC_FUNCTION_WITHOUT_AWAIT_FEATURE) != 0
    }

    pub fn needs_activation(&self) -> bool {
        self.var_declarations.has_captured_variables() || (self.features & (EVAL_FEATURE | WITH_FEATURE)) != 0
    }

    pub fn uses_non_simple_parameter_list(&self) -> bool {
        (self.features & NON_SIMPLE_PARAMETER_LIST_FEATURE) != 0
    }

    pub fn needs_new_target_register_for_this_scope(&self) -> bool {
        self.uses_super_call() || self.uses_new_target()
    }

    pub fn needed_constants(&self) -> i32 {
        // Podem ser precisas 2 constantes a mais que a contagem do parser, pelos vários usos de
        // `jsUndefined()` e `jsNull()`.
        self.num_constants + 2
    }

    pub fn single_statement(&self) -> Option<Statement> {
        self.statements.as_ref().and_then(|statements| statements.borrow().single_statement())
    }

    pub fn is_empty_body(&self) -> bool {
        self.statements.is_none()
    }

    pub fn has_completion_value(&self) -> bool {
        self.statements.as_ref().is_some_and(|statements| statements.borrow().has_completion_value())
    }

    pub fn has_early_break_or_continue(&self) -> bool {
        self.statements.as_ref().is_some_and(|statements| statements.borrow().has_early_break_or_continue())
    }
}
