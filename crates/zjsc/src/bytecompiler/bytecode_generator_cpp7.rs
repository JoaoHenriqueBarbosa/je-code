// Fragmento de bytecompiler/BytecodeGenerator.h (linha 495): a variante de `StatementNode` do
// `emitNodeInIgnoreResultPosition`. A de `ExpressionNode` está em bytecode_generator_part2.rs.
// Juntada por include!; o `SetForScope` do WTF vira salvar o valor e restaurá-lo na saída.

impl BytecodeGenerator {
    // BytecodeGenerator.h:495
    pub fn emit_node_in_ignore_result_position_statement(&mut self, n: &crate::parser::nodes::Statement) {
        let saved_tail = self.allow_tail_call_optimization;
        let saved_ignore = self.allow_call_ignore_result_optimization;
        self.allow_tail_call_optimization = false;
        // Volta ao valor padrão.
        self.allow_call_ignore_result_optimization = self.default_allow_call_ignore_result_optimization;
        let ignored = self.ignored_result();
        self.emit_node_in_tail_position_statement(Some(ignored), n);
        self.allow_call_ignore_result_optimization = saved_ignore;
        self.allow_tail_call_optimization = saved_tail;
    }
}
