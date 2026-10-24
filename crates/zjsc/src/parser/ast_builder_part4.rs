macro_rules! ast_builder_part4 {
    () => {
// Quarta fatia de `parser/ASTBuilder.h`: os `make*Node` que o trait `TreeBuilder` declara e que o C++ define
// fora da classe (linhas 1230 a 1706 do `.h`). Incluída por `include!` DENTRO do `impl TreeBuilder for
// ASTBuilder`, ao lado de `ast_builder_part2.rs`; os auxiliares privados (`make_*_node` aritméticos,
// `create_*_number`) estão em `ast_builder_part3.rs`.

fn make_type_of_node(&mut self, location: &JSTokenLocation, expr: Option<Expression>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
    let expr = non_null(expr);
    if let Expression::Resolve(resolve) = &expr {
        let ident = resolve.borrow().ident.clone();
        return Some(Expression::TypeOfResolve(make(crate::parser::nodes::TypeOfResolveNode::new(location, ident, start, divot, end))));
    }
    Some(Expression::TypeOfValue(make(crate::parser::nodes::TypeOfValueNode::new(location, expr))))
}

fn make_delete_node(&mut self, location: &JSTokenLocation, expr: Option<Expression>, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
    let expr = non_null(expr);
    if let Expression::OptionalChain(optional_chain) = &expr {
        let inner = optional_chain.borrow().expr.clone();
        if inner.is_location() {
            debug_assert!(!inner.is_resolve_node());
            let deleted = non_null(self.make_delete_node(location, Some(inner), start, divot, end));
            optional_chain.borrow_mut().expr = deleted;
            return Some(expr);
        }
    }

    if !expr.is_location() {
        return Some(Expression::DeleteValue(make(crate::parser::nodes::DeleteValueNode::new(location, expr))));
    }
    match &expr {
        Expression::Resolve(resolve) => {
            let ident = resolve.borrow().ident.clone();
            Some(Expression::DeleteResolve(make(crate::parser::nodes::DeleteResolveNode::new(location, ident, divot, start, end))))
        }
        Expression::BracketAccessor(bracket) => {
            let (base, subscript) = {
                let bracket = bracket.borrow();
                (bracket.base_expr.clone(), bracket.subscript.clone())
            };
            Some(Expression::DeleteBracket(make(crate::parser::nodes::DeleteBracketNode::new(location, base, subscript, divot, start, end))))
        }
        Expression::DotAccessor(dot) => {
            self.check_arguments_length_modification(&Some(expr.clone()));
            let (base, ident) = {
                let dot = dot.borrow();
                (dot.base_expr.clone(), dot.ident.clone())
            };
            Some(Expression::DeleteDot(make(crate::parser::nodes::DeleteDotNode::new(location, base, ident, divot, start, end))))
        }
        // Invariante: o parser só chama aqui depois de testar `isDotAccessorNode()` (ASSERT no C++).
        _ => panic!("ASSERT(expr->isDotAccessorNode())"),
    }
}

fn make_negate_node(&mut self, location: &JSTokenLocation, n: Option<Expression>) -> Option<Expression> {
    let n = non_null(n);
    if n.is_number() {
        let value = -Self::number_value(&n);
        return Some(self.create_number_from_unary_operation(location, value, &n));
    }

    if let Expression::BigInt(big_int) = &n {
        let sign = !big_int.borrow().sign;
        return Some(self.create_big_int_from_unary_operation(location, sign, &n));
    }

    Some(Expression::Negate(make(crate::parser::nodes::NegateNode::new(location, n))))
}

fn make_bitwise_not_node(&mut self, location: &JSTokenLocation, expr: Option<Expression>) -> Option<Expression> {
    let expr = non_null(expr);
    if expr.is_number() {
        return Some(self.create_integer_like_number(location, !crate::runtime::math_common::to_int32(Self::number_value(&expr)) as f64));
    }
    Some(Expression::BitwiseNot(make(crate::parser::nodes::BitwiseNotNode::new(location, expr))))
}

fn make_static_block_function_call_node(&mut self, location: &JSTokenLocation, func: Option<Expression>, divot: JSTextPosition, divot_start: JSTextPosition, divot_end: JSTextPosition) -> Option<Expression> {
    Some(Expression::StaticBlockFunctionCall(make(crate::parser::nodes::StaticBlockFunctionCallNode::new(location, non_null(func), divot, divot_start, divot_end))))
}

fn make_function_call_node(&mut self, location: &JSTokenLocation, func: Option<Expression>, previous_base_was_super: bool, args: Link<ArgumentsNode>, divot_start: JSTextPosition, divot: JSTextPosition, divot_end: JSTextPosition, call_or_apply_child_depth: usize, is_optional_call: bool) -> Option<Expression> {
    use crate::parser::nodes::{ApplyFunctionCallDotNode, CallFunctionCallDotNode, EvalFunctionCallNode, FunctionCallBracketNode, FunctionCallDotNode, FunctionCallResolveNode, FunctionCallValueNode, HasOwnPropertyFunctionCallDotNode};

    debug_assert!(divot.offset >= divot.line_start_offset);
    let func = non_null(func);
    if func.is_super_node() {
        self.uses_super_call();
    }

    if let Expression::BytecodeIntrinsic(intrinsic) = &func {
        debug_assert!(!is_optional_call);
        let (type_, entry, ident) = {
            let intrinsic = intrinsic.borrow();
            (intrinsic.type_, intrinsic.entry, intrinsic.ident.clone())
        };
        if type_ == BytecodeIntrinsicNodeType::Constant && entry.type_() == crate::bytecode::bytecode_intrinsic_registry::Type::Emitter {
            return Some(Expression::BytecodeIntrinsic(make(BytecodeIntrinsicNode::new(BytecodeIntrinsicNodeType::Function, location, entry, ident, args.opt(), divot, divot_start, divot_end))));
        }
    }

    if let Expression::OptionalChain(optional_chain) = &func {
        let inner = optional_chain.borrow().expr.clone();
        if inner.is_location() {
            debug_assert!(!inner.is_resolve_node());
            // We must take care to preserve our `this` value in cases like `a?.b?.()` and `(a?.b)()`, respectively.
            if is_optional_call {
                return self.make_function_call_node(location, Some(inner), previous_base_was_super, args, divot_start, divot, divot_end, call_or_apply_child_depth, is_optional_call);
            }
            let called = non_null(self.make_function_call_node(location, Some(inner), previous_base_was_super, args, divot_start, divot, divot_end, call_or_apply_child_depth, is_optional_call));
            optional_chain.borrow_mut().expr = called;
            return Some(func);
        }
    }

    if !func.is_location() {
        return Some(Expression::FunctionCallValue(make(FunctionCallValueNode::new(location, func, args.get().clone(), divot, divot_start, divot_end, is_optional_call))));
    }
    match &func {
        Expression::Resolve(resolve) => {
            let identifier = resolve.borrow().ident.clone();
            if identifier == self.vm.property_names.eval && !is_optional_call {
                self.uses_eval();
                return Some(Expression::EvalFunctionCall(make(EvalFunctionCallNode::new(location, args.get().clone(), divot, divot_start, divot_end))));
            }
            Some(Expression::FunctionCallResolve(make(FunctionCallResolveNode::new(location, identifier, args.get().clone(), divot, divot_start, divot_end, is_optional_call))))
        }
        Expression::BracketAccessor(bracket) => {
            let (base, subscript, subscript_has_assignments, subexpression_divot, subexpression_end) = {
                let bracket = bracket.borrow();
                (bracket.base_expr.clone(), bracket.subscript.clone(), bracket.subscript_has_assignments, bracket.throwable.divot, bracket.throwable.divot_end.offset)
            };
            let mut node = FunctionCallBracketNode::new(location, base, subscript, subscript_has_assignments, args.get().clone(), divot, divot_start, divot_end, is_optional_call);
            node.throwable.set_subexpression_info(&subexpression_divot, subexpression_end);
            Some(Expression::FunctionCallBracket(make(node)))
        }
        Expression::DotAccessor(dot) => {
            let (base, ident, dot_type, subexpression_divot, subexpression_end) = {
                let dot = dot.borrow();
                (dot.base_expr.clone(), dot.ident.clone(), dot.type_, dot.throwable.divot, dot.throwable.divot_end.offset)
            };

            // Fecha o nó de chamada de ponto com o `setSubexpressionInfo` que o C++ aplica a todas as variantes.
            macro_rules! with_subexpression_info {
                ($variant:ident, $node:expr) => {{
                    let mut node = $node;
                    node.throwable.set_subexpression_info(&subexpression_divot, subexpression_end);
                    Expression::$variant(make(node))
                }};
            }

            let builtin_names = self.vm.property_names.builtin_names();
            let is_call = ident == *builtin_names.call_public_name() || ident == builtin_names.call_private_name();
            let is_apply = ident == *builtin_names.apply_public_name() || ident == builtin_names.apply_private_name();
            let base_is_reflect = match &base {
                Expression::Resolve(resolve) => resolve.borrow().ident == self.vm.property_names.reflect,
                _ => false,
            };
            let is_has_own_property_pattern = ident == self.vm.property_names.has_own_property
                && match args.get().borrow().list_node.as_ref() {
                    Some(list_node) => {
                        let list_node = list_node.borrow();
                        list_node.expr.is_resolve_node() && list_node.next.is_none()
                    }
                    None => false,
                }
                && (base.is_resolve_node() || base.is_this_node());

            let node = if !previous_base_was_super && is_call {
                Some(with_subexpression_info!(CallFunctionCallDot, CallFunctionCallDotNode::new(location, base.clone(), ident.clone(), dot_type, args.get().clone(), divot, divot_start, divot_end, is_optional_call, call_or_apply_child_depth)))
            } else if !previous_base_was_super && is_apply {
                // FIXME: This check is only needed because we haven't taught the bytecode generator to inline
                // Reflect.apply yet. See https://bugs.webkit.org/show_bug.cgi?id=190668.
                if !base_is_reflect {
                    Some(with_subexpression_info!(ApplyFunctionCallDot, ApplyFunctionCallDotNode::new(location, base.clone(), ident.clone(), dot_type, args.get().clone(), divot, divot_start, divot_end, is_optional_call, call_or_apply_child_depth)))
                } else {
                    None
                }
            } else if !previous_base_was_super && is_has_own_property_pattern {
                // We match the AST pattern:
                // <resolveNode|thisNode>.hasOwnProperty(<resolveNode>)
                // i.e:
                // o.hasOwnProperty(p)
                Some(with_subexpression_info!(HasOwnPropertyFunctionCallDot, HasOwnPropertyFunctionCallDotNode::new(location, base.clone(), ident.clone(), dot_type, args.get().clone(), divot, divot_start, divot_end, is_optional_call)))
            } else {
                None
            };
            Some(match node {
                Some(node) => node,
                None => with_subexpression_info!(FunctionCallDot, FunctionCallDotNode::new(location, base, ident, dot_type, args.get().clone(), divot, divot_start, divot_end, is_optional_call)),
            })
        }
        // Invariante: ramo selecionado pelo parser só para `isDotAccessorNode()` (ASSERT no C++).
        _ => panic!("ASSERT(func->isDotAccessorNode())"),
    }
}

fn make_assign_node(&mut self, location: &JSTokenLocation, loc: Option<Expression>, op: Operator, expr: Option<Expression>, loc_has_assignments: bool, expr_has_assignments: bool, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
    use crate::parser::nodes::{AssignBracketNode, AssignDotNode, AssignErrorNode, ReadModifyBracketNode, ReadModifyDotNode, ReadModifyResolveNode, ShortCircuitReadModifyBracketNode, ShortCircuitReadModifyDotNode, ShortCircuitReadModifyResolveNode};

    let loc = non_null(loc);
    let expr = non_null(expr);
    if !loc.is_location() {
        debug_assert!(loc.is_function_call());
        return Some(Expression::AssignError(make(AssignErrorNode::new(location, loc, divot, start, end))));
    }

    let is_short_circuit = op == Operator::CoalesceEq || op == Operator::OrEq || op == Operator::AndEq;

    match &loc {
        Expression::Resolve(resolve) => {
            let ident = resolve.borrow().ident.clone();

            if op == Operator::Equal {
                Self::set_ecma_name_of_function_or_class(&expr, &ident);
                let mut node = AssignResolveNode::new(location, ident, expr, AssignmentContext::AssignmentExpression);
                Self::set_exception_location(&mut node.throwable, start, divot, end);
                return Some(Expression::AssignResolve(make(node)));
            }

            if is_short_circuit {
                Self::set_ecma_name_of_function_or_class(&expr, &ident);
                return Some(Expression::ShortCircuitReadModifyResolve(make(ShortCircuitReadModifyResolveNode::new(location, ident, op, expr, expr_has_assignments, divot, start, end))));
            }

            Some(Expression::ReadModifyResolve(make(ReadModifyResolveNode::new(location, ident, op, expr, expr_has_assignments, divot, start, end))))
        }
        Expression::BracketAccessor(bracket) => {
            let (base, subscript, bracket_divot, bracket_divot_end) = {
                let bracket = bracket.borrow();
                (bracket.base_expr.clone(), bracket.subscript.clone(), bracket.throwable.divot, bracket.throwable.divot_end.offset)
            };

            if op == Operator::Equal {
                return Some(Expression::AssignBracket(make(AssignBracketNode::new(location, base, subscript, expr, loc_has_assignments, expr_has_assignments, bracket_divot, start, end))));
            }

            if is_short_circuit {
                let mut node = ShortCircuitReadModifyBracketNode::new(location, base, subscript, op, expr, loc_has_assignments, expr_has_assignments, divot, start, end);
                node.throwable.set_subexpression_info(&bracket_divot, bracket_divot_end);
                return Some(Expression::ShortCircuitReadModifyBracket(make(node)));
            }

            let mut node = ReadModifyBracketNode::new(location, base, subscript, op, expr, loc_has_assignments, expr_has_assignments, divot, start, end);
            node.throwable.set_subexpression_info(&bracket_divot, bracket_divot_end);
            Some(Expression::ReadModifyBracket(make(node)))
        }
        Expression::DotAccessor(dot) => {
            let (base, ident, dot_type, dot_divot, dot_divot_end) = {
                let dot = dot.borrow();
                (dot.base_expr.clone(), dot.ident.clone(), dot.type_, dot.throwable.divot, dot.throwable.divot_end.offset)
            };

            if op == Operator::Equal {
                return Some(Expression::AssignDot(make(AssignDotNode::new(location, base, ident, dot_type, expr, expr_has_assignments, dot_divot, start, end))));
            }

            if is_short_circuit {
                let mut node = ShortCircuitReadModifyDotNode::new(location, base, ident, dot_type, op, expr, expr_has_assignments, divot, start, end);
                node.throwable.set_subexpression_info(&dot_divot, dot_divot_end);
                return Some(Expression::ShortCircuitReadModifyDot(make(node)));
            }

            let mut node = ReadModifyDotNode::new(location, base, ident, dot_type, op, expr, expr_has_assignments, divot, start, end);
            node.throwable.set_subexpression_info(&dot_divot, dot_divot_end);
            Some(Expression::ReadModifyDot(make(node)))
        }
        // Invariante: ramo selecionado pelo parser só para `isDotAccessorNode()` (ASSERT no C++).
        _ => panic!("ASSERT(loc->isDotAccessorNode())"),
    }
}

fn make_prefix_node(&mut self, location: &JSTokenLocation, expr: Option<Expression>, op: Operator, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
    self.check_arguments_length_modification(&expr);
    Some(Expression::Prefix(make(crate::parser::nodes::PrefixNode::new(location, non_null(expr), op, divot, start, end))))
}

fn make_postfix_node(&mut self, location: &JSTokenLocation, expr: Option<Expression>, op: Operator, start: JSTextPosition, divot: JSTextPosition, end: JSTextPosition) -> Option<Expression> {
    self.check_arguments_length_modification(&expr);
    Some(Expression::Postfix(make(crate::parser::nodes::PostfixNode::new(location, non_null(expr), op, divot, start, end))))
}

    };
}
