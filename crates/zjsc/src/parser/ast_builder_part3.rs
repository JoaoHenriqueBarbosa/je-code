// Terceira fatia de `parser/ASTBuilder.h`: as definições fora da classe (`makeBinaryNode`, `makeCoalesceNode`,
// os `make*Node` aritméticos) e os auxiliares privados que as partes 1 e 2 chamam (`createIntegerLikeNumber`
// e irmãos, `static_cast<ArrayPatternNode*>`/`ObjectPatternNode*`). Incluída por `include!` no fim de
// `ast_builder.rs`, FORA do `impl TreeBuilder`: não há `use` aqui, o que `ast_builder.rs` não importa vai por
// caminho completo. Os `make*` que o trait `TreeBuilder` declara (`makeFunctionCallNode`, `makeAssignNode`,
// `makeDeleteNode`...) ficam em `ast_builder_part4.rs`, que entra DENTRO do `impl TreeBuilder`.
//
// Os `make*Node` privados trabalham com `Expression` (o C++ desreferencia o ponteiro sem testar), o
// `Option<Expression>` só aparece na fronteira do trait.

impl ASTBuilder {
    /// `static_cast<ArrayPatternNode*>(node)`.
    fn array_pattern_of(node: &Option<DestructuringPatternNode>) -> &crate::parser::nodes::NodeRef<crate::parser::nodes::ArrayPatternNode> {
        match node {
            Some(DestructuringPatternNode::ArrayPattern(pattern)) => pattern,
            _ => panic!("static_cast<ArrayPatternNode*> sobre um padrão que não é de array"),
        }
    }

    /// `static_cast<ObjectPatternNode*>(node)`.
    fn object_pattern_of(node: &Option<DestructuringPatternNode>) -> &crate::parser::nodes::NodeRef<crate::parser::nodes::ObjectPatternNode> {
        match node {
            Some(DestructuringPatternNode::ObjectPattern(pattern)) => pattern,
            _ => panic!("static_cast<ObjectPatternNode*> sobre um padrão que não é de objeto"),
        }
    }

    /// `NumberNode::value()` de um `Expression` que o C++ já testou com `isNumber()`.
    fn number_value(expr: &Expression) -> f64 {
        match expr {
            Expression::Double(node) => node.borrow().value,
            Expression::Integer(node) => node.borrow().value,
            _ => panic!("static_cast<NumberNode*> sobre um nó que não é número"),
        }
    }

    /// `createIntegerLikeNumber`.
    fn create_integer_like_number(&mut self, location: &JSTokenLocation, d: f64) -> Expression {
        Expression::Integer(make(IntegerNode::new(location, d)))
    }

    /// `createDoubleLikeNumber`.
    fn create_double_like_number(&mut self, location: &JSTokenLocation, d: f64) -> Expression {
        Expression::Double(make(DoubleNode::new(location, d)))
    }

    /// `createBigIntWithSign`.
    fn create_big_int_with_sign(&mut self, location: &JSTokenLocation, big_int: &Identifier, radix: u8, sign: bool) -> Expression {
        Expression::BigInt(make(BigIntNode::with_sign(location, big_int.clone(), radix, sign)))
    }

    /// `createNumberFromBinaryOperation`.
    fn create_number_from_binary_operation(&mut self, location: &JSTokenLocation, value: f64, original_node_a: &Expression, original_node_b: &Expression) -> Expression {
        if original_node_a.is_integer_node() && original_node_b.is_integer_node() {
            return self.create_integer_like_number(location, value);
        }
        self.create_double_like_number(location, value)
    }

    /// `createNumberFromUnaryOperation`.
    fn create_number_from_unary_operation(&mut self, location: &JSTokenLocation, value: f64, original_node: &Expression) -> Expression {
        if original_node.is_integer_node() {
            return self.create_integer_like_number(location, value);
        }
        self.create_double_like_number(location, value)
    }

    /// `createBigIntFromUnaryOperation`.
    fn create_big_int_from_unary_operation(&mut self, location: &JSTokenLocation, sign: bool, original_node: &Expression) -> Expression {
        let Expression::BigInt(big_int) = original_node else {
            panic!("static_cast<const BigIntNode&> sobre um nó que não é BigInt");
        };
        let (value, radix) = {
            let big_int = big_int.borrow();
            (big_int.value.clone(), big_int.radix)
        };
        self.create_big_int_with_sign(location, &value, radix, sign)
    }

    /// O corpo que `makeBitOrNode`, `makeBitAndNode`, `makeBitXOrNode` e os três deslocamentos repetem:
    /// se os dois operandos são número, o resultado é o `createIntegerLikeNumber` de `fold(a, b)`.
    fn fold_integer_like(&mut self, location: &JSTokenLocation, expr1: &Expression, expr2: &Expression, fold: impl Fn(f64, f64) -> f64) -> Option<Expression> {
        if expr1.is_number() && expr2.is_number() {
            let value = fold(Self::number_value(expr1), Self::number_value(expr2));
            return Some(self.create_integer_like_number(location, value));
        }
        None
    }

    fn make_pow_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        let stripped_expr1 = expr1.strip_unary_plus();
        let stripped_expr2 = expr2.strip_unary_plus();

        if stripped_expr1.is_number() && stripped_expr2.is_number() {
            let value = crate::runtime::math_common::operation_math_pow(Self::number_value(&stripped_expr1), Self::number_value(&stripped_expr2));
            return self.create_number_from_binary_operation(location, value, &stripped_expr1, &stripped_expr2);
        }

        let expr1 = if stripped_expr1.is_number() { stripped_expr1 } else { expr1 };
        let expr2 = if stripped_expr2.is_number() { stripped_expr2 } else { expr2 };

        Expression::Pow(make(crate::parser::nodes::PowNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_mult_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        // FIXME: Unary + change the evaluation order.
        // https://bugs.webkit.org/show_bug.cgi?id=159968
        let expr1 = expr1.strip_unary_plus();
        let expr2 = expr2.strip_unary_plus();

        if expr1.is_number() && expr2.is_number() {
            let value = Self::number_value(&expr1) * Self::number_value(&expr2);
            return self.create_number_from_binary_operation(location, value, &expr1, &expr2);
        }

        if expr1.is_number() && Self::number_value(&expr1) == 1.0 {
            return Expression::UnaryPlus(make(UnaryPlusNode::new(location, expr2)));
        }

        if expr2.is_number() && Self::number_value(&expr2) == 1.0 {
            return Expression::UnaryPlus(make(UnaryPlusNode::new(location, expr1)));
        }

        Expression::Mult(make(crate::parser::nodes::MultNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_div_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        // FIXME: Unary + change the evaluation order.
        // https://bugs.webkit.org/show_bug.cgi?id=159968
        let expr1 = expr1.strip_unary_plus();
        let expr2 = expr2.strip_unary_plus();

        if expr1.is_number() && expr2.is_number() {
            let result = Self::number_value(&expr1) / Self::number_value(&expr2);
            if crate::wtf::math_extras::truncate_double_to_int64(result) as f64 == result {
                return self.create_number_from_binary_operation(location, result, &expr1, &expr2);
            }
            return self.create_double_like_number(location, result);
        }
        Expression::Div(make(crate::parser::nodes::DivNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_mod_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        // FIXME: Unary + change the evaluation order.
        // https://bugs.webkit.org/show_bug.cgi?id=159968
        let expr1 = expr1.strip_unary_plus();
        let expr2 = expr2.strip_unary_plus();

        if expr1.is_number() && expr2.is_number() {
            // `%` de `f64` é o `fmod` do C.
            return self.create_integer_like_number(location, Self::number_value(&expr1) % Self::number_value(&expr2));
        }
        Expression::Mod(make(crate::parser::nodes::ModNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_add_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        if expr1.is_number() && expr2.is_number() {
            let value = Self::number_value(&expr1) + Self::number_value(&expr2);
            return self.create_number_from_binary_operation(location, value, &expr1, &expr2);
        }
        Expression::Add(make(crate::parser::nodes::AddNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_sub_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        // FIXME: Unary + change the evaluation order.
        // https://bugs.webkit.org/show_bug.cgi?id=159968
        let expr1 = expr1.strip_unary_plus();
        let expr2 = expr2.strip_unary_plus();

        if expr1.is_number() && expr2.is_number() {
            let value = Self::number_value(&expr1) - Self::number_value(&expr2);
            return self.create_number_from_binary_operation(location, value, &expr1, &expr2);
        }
        Expression::Sub(make(crate::parser::nodes::SubNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_left_shift_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        use crate::runtime::math_common::{to_int32, to_uint32};
        if let Some(folded) = self.fold_integer_like(location, &expr1, &expr2, |a, b| to_int32(a).wrapping_shl(to_uint32(b) & 0x1f) as f64) {
            return folded;
        }
        Expression::LeftShift(make(crate::parser::nodes::LeftShiftNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_right_shift_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        use crate::runtime::math_common::{to_int32, to_uint32};
        if let Some(folded) = self.fold_integer_like(location, &expr1, &expr2, |a, b| to_int32(a).wrapping_shr(to_uint32(b) & 0x1f) as f64) {
            return folded;
        }
        Expression::RightShift(make(crate::parser::nodes::RightShiftNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_u_right_shift_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        use crate::runtime::math_common::to_uint32;
        if let Some(folded) = self.fold_integer_like(location, &expr1, &expr2, |a, b| to_uint32(a).wrapping_shr(to_uint32(b) & 0x1f) as f64) {
            return folded;
        }
        Expression::UnsignedRightShift(make(crate::parser::nodes::UnsignedRightShiftNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_bit_or_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        use crate::runtime::math_common::to_int32;
        if let Some(folded) = self.fold_integer_like(location, &expr1, &expr2, |a, b| (to_int32(a) | to_int32(b)) as f64) {
            return folded;
        }
        Expression::BitOr(make(crate::parser::nodes::BitOrNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_bit_and_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        use crate::runtime::math_common::to_int32;
        if let Some(folded) = self.fold_integer_like(location, &expr1, &expr2, |a, b| (to_int32(a) & to_int32(b)) as f64) {
            return folded;
        }
        Expression::BitAnd(make(crate::parser::nodes::BitAndNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_bit_x_or_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression, right_has_assignments: bool) -> Expression {
        use crate::runtime::math_common::to_int32;
        if let Some(folded) = self.fold_integer_like(location, &expr1, &expr2, |a, b| (to_int32(a) ^ to_int32(b)) as f64) {
            return folded;
        }
        Expression::BitXOr(make(crate::parser::nodes::BitXOrNode::new(location, expr1, expr2, right_has_assignments)))
    }

    fn make_coalesce_node(&mut self, location: &JSTokenLocation, expr1: Expression, expr2: Expression) -> Expression {
        // Optimization for `x?.y ?? z`.
        if let Expression::OptionalChain(optional_chain) = &expr1 {
            let inner = optional_chain.borrow().expr.clone();
            if !inner.is_delete_node() {
                let has_absorbed_optional_chain = true;
                return Expression::Coalesce(make(crate::parser::nodes::CoalesceNode::new(location, inner, expr2, has_absorbed_optional_chain)));
            }
        }
        let has_absorbed_optional_chain = false;
        Expression::Coalesce(make(crate::parser::nodes::CoalesceNode::new(location, expr1, expr2, has_absorbed_optional_chain)))
    }

    /// `ASTBuilder::makeBinaryNode`.
    fn make_binary_node(&mut self, location: &JSTokenLocation, token: i32, lhs: &(Option<Expression>, BinaryOpInfo), rhs: &(Option<Expression>, BinaryOpInfo)) -> Option<Expression> {
        use crate::parser::nodes::LogicalOperator;
        use crate::parser::parser_tokens as t;

        let left = non_null_ref(&lhs.0).clone();
        let right = non_null_ref(&rhs.0).clone();
        let has_assignment = rhs.1.has_assignment;

        let node = match token as t::JSTokenType {
            t::COALESCE => self.make_coalesce_node(location, left, right),
            t::OR => Expression::LogicalOp(make(crate::parser::nodes::LogicalOpNode::new(location, left, right, LogicalOperator::Or))),
            t::AND => Expression::LogicalOp(make(crate::parser::nodes::LogicalOpNode::new(location, left, right, LogicalOperator::And))),
            t::BITOR => self.make_bit_or_node(location, left, right, has_assignment),
            t::BITXOR => self.make_bit_x_or_node(location, left, right, has_assignment),
            t::BITAND => self.make_bit_and_node(location, left, right, has_assignment),
            t::EQEQ => Expression::Equal(make(crate::parser::nodes::EqualNode::new(location, left, right, has_assignment))),
            t::NE => Expression::NotEqual(make(crate::parser::nodes::NotEqualNode::new(location, left, right, has_assignment))),
            t::STREQ => Expression::StrictEqual(make(crate::parser::nodes::StrictEqualNode::new(location, left, right, has_assignment))),
            t::STRNEQ => Expression::NotStrictEqual(make(crate::parser::nodes::NotStrictEqualNode::new(location, left, right, has_assignment))),
            t::LT => Expression::Less(make(crate::parser::nodes::LessNode::new(location, left, right, has_assignment))),
            t::GT => Expression::Greater(make(crate::parser::nodes::GreaterNode::new(location, left, right, has_assignment))),
            t::LE => Expression::LessEq(make(crate::parser::nodes::LessEqNode::new(location, left, right, has_assignment))),
            t::GE => Expression::GreaterEq(make(crate::parser::nodes::GreaterEqNode::new(location, left, right, has_assignment))),
            t::INSTANCEOF => {
                let mut node = crate::parser::nodes::InstanceOfNode::new(location, left, right, has_assignment);
                Self::set_exception_location(&mut node.throwable, lhs.1.start, rhs.1.start, rhs.1.end);
                Expression::InstanceOf(make(node))
            }
            t::INTOKEN => {
                let mut node = crate::parser::nodes::InNode::new(location, left, right, has_assignment);
                Self::set_exception_location(&mut node.throwable, lhs.1.start, rhs.1.start, rhs.1.end);
                Expression::In(make(node))
            }
            t::LSHIFT => self.make_left_shift_node(location, left, right, has_assignment),
            t::RSHIFT => self.make_right_shift_node(location, left, right, has_assignment),
            t::URSHIFT => self.make_u_right_shift_node(location, left, right, has_assignment),
            t::PLUS => self.make_add_node(location, left, right, has_assignment),
            t::MINUS => self.make_sub_node(location, left, right, has_assignment),
            t::TIMES => self.make_mult_node(location, left, right, has_assignment),
            t::DIVIDE => self.make_div_node(location, left, right, has_assignment),
            t::MOD => self.make_mod_node(location, left, right, has_assignment),
            t::POW => self.make_pow_node(location, left, right, has_assignment),
            // Invariante: o parser só entrega tokens da tabela de precedência binária (CRASH() no C++).
            _ => panic!("CRASH: operador binário desconhecido"),
        };
        Some(node)
    }
}
