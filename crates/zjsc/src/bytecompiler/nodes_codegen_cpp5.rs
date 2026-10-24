// NodesCodegen.cpp, linhas 4335 a 4429: `DoWhileNode::emitBytecode`, `WhileNode::emitBytecode` e
// `ForNode::emitBytecode` (incluído por include!, sem `use` no topo). A próxima função é
// `ForInNode::tryGetBoundLocal` (linha 4431), que fica para a fatia seguinte.
//
// Convenções desta fatia (as mesmas da cpp2): registrador é `Option<RegisterRef>` (o `RegisterID*`
// nulo do C++). O `Ref<LabelScope>` é o `Rc<LabelScope>` de `new_label_scope`; os alvos de salto
// saem do `LabelScope` como `LabelRef` e entram em `emit_label` direto, ou como `&Label` (via
// `borrow()`) em `emit_node_in_condition_context`.

type Cpp5Reg = Option<crate::bytecompiler::bytecode_generator::RegisterRef>;

impl crate::parser::nodes::DoWhileNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5Reg,
    ) {
        if generator.should_be_concerned_with_completion_value() && self.statement.has_early_break_or_continue() {
            generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
        }

        let scope = generator.new_label_scope(crate::bytecompiler::label_scope::LabelScopeType::Loop, None);

        let top_of_loop = generator.new_label();
        generator.emit_label(&top_of_loop);
        generator.emit_loop_hint();

        generator.emit_node_in_tail_position_statement(dst, &self.statement);

        generator.emit_label(scope.continue_target().expect("o laço sempre tem continueTarget"));
        generator.emit_node_in_condition_context(
            &self.expr,
            &top_of_loop,
            scope.break_target(),
            crate::parser::nodes::FallThroughMode::FallThroughMeansFalse,
        );

        generator.emit_label(scope.break_target());
    }
}

impl crate::parser::nodes::WhileNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5Reg,
    ) {
        if generator.should_be_concerned_with_completion_value() && self.statement.has_early_break_or_continue() {
            generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
        }

        let scope = generator.new_label_scope(crate::bytecompiler::label_scope::LabelScopeType::Loop, None);
        let top_of_loop = generator.new_label();

        generator.emit_node_in_condition_context(
            &self.expr,
            &top_of_loop,
            scope.break_target(),
            crate::parser::nodes::FallThroughMode::FallThroughMeansTrue,
        );

        generator.emit_label(&top_of_loop);
        generator.emit_loop_hint();

        generator.emit_profile_control_flow(self.statement.base().start_offset());
        generator.emit_node_in_tail_position_statement(dst, &self.statement);

        generator.emit_label(scope.continue_target().expect("o laço sempre tem continueTarget"));

        generator.emit_node_in_condition_context(
            &self.expr,
            &top_of_loop,
            scope.break_target(),
            crate::parser::nodes::FallThroughMode::FallThroughMeansFalse,
        );

        generator.emit_label(scope.break_target());

        generator.emit_profile_control_flow(self.statement.base().end_offset() + if self.statement.is_block() { 1 } else { 0 });
    }
}

impl crate::parser::nodes::ForNode {
    pub fn emit_bytecode(
        &self,
        generator: &mut crate::bytecompiler::bytecode_generator::BytecodeGenerator,
        dst: Cpp5Reg,
    ) {
        if generator.should_be_concerned_with_completion_value() && self.statement.has_early_break_or_continue() {
            generator.emit_load_js_value(dst.clone(), crate::runtime::js_value::JSValue::Undefined);
        }

        let mut for_loop_symbol_table: Cpp5Reg = None;
        generator.push_lexical_scope(
            &self.variable_environment,
            crate::bytecompiler::bytecode_generator::ScopeType::LetConstScope,
            crate::bytecompiler::bytecode_generator::TDZCheckOptimization::Optimize,
            crate::bytecompiler::bytecode_generator::NestedScopeType::IsNested,
            Some(&mut for_loop_symbol_table),
            true,
        );

        let using_count = self.variable_environment.using_declaration_count();
        let has_await_using = self.variable_environment.has_await_using_declaration();
        generator.emit_body_with_using_if_needed(using_count, has_await_using, &mut |generator| {
            let scope = generator.new_label_scope(crate::bytecompiler::label_scope::LabelScopeType::Loop, None);

            if let Some(expr1) = &self.expr1 {
                generator.emit_node_in_ignore_result_position_expression(expr1);
                if self.initializer_contains_closure {
                    generator.prepare_lexical_scope_for_next_for_loop_iteration(
                        &self.variable_environment,
                        for_loop_symbol_table.clone(),
                    );
                }
            }

            let top_of_loop = generator.new_label();
            if let Some(expr2) = &self.expr2 {
                generator.emit_node_in_condition_context(
                    expr2,
                    &top_of_loop,
                    scope.break_target(),
                    crate::parser::nodes::FallThroughMode::FallThroughMeansTrue,
                );
            }

            generator.emit_label(&top_of_loop);
            generator.emit_loop_hint();
            generator.emit_profile_control_flow(self.statement.base().start_offset());

            generator.emit_node_in_tail_position_statement(dst.clone(), &self.statement);

            generator.emit_label(scope.continue_target().expect("o laço sempre tem continueTarget"));
            generator.prepare_lexical_scope_for_next_for_loop_iteration(
                &self.variable_environment,
                for_loop_symbol_table.clone(),
            );
            if let Some(expr3) = &self.expr3 {
                generator.emit_node_in_ignore_result_position_expression(expr3);
            }

            if let Some(expr2) = &self.expr2 {
                generator.emit_node_in_condition_context(
                    expr2,
                    &top_of_loop,
                    scope.break_target(),
                    crate::parser::nodes::FallThroughMode::FallThroughMeansFalse,
                );
            } else {
                generator.emit_jump(&top_of_loop);
            }

            generator.emit_label(scope.break_target());
        });

        generator.pop_lexical_scope(&self.variable_environment);
        generator.emit_profile_control_flow(self.statement.base().end_offset() + if self.statement.is_block() { 1 } else { 0 });
    }
}
