// Parte 2 de bytecompiler/BytecodeGenerator.h (linhas 502 a 999 do .h). Juntada por include!.
// Convenção: `Option<Rc<RefCell<RegisterID>>>` é o `RegisterID*` anulável do C++; o `RefPtr<RegisterID>`
// é `Rc<RefCell<RegisterID>>`. O `SetForScope` do WTF vira salvar o valor e restaurá-lo na saída do escopo.

impl BytecodeGenerator {
    // BytecodeGenerator.h:502
    pub fn emit_node_in_tail_position_statement(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        n: &crate::parser::nodes::Statement,
    ) {
        // Node::emitCode assume que dst, se dado, é um local ou um temporário referenciado.
        if !self.vm.is_safe_to_recurse() {
            self.emit_throw_expression_too_deep_exception();
            return;
        }
        if n.base().needs_debug_hook() {
            self.emit_debug_hook_statement_data(n, None);
        }
        n.emit_bytecode(self, dst);
    }

    // BytecodeGenerator.h:519: `add_metadata_for` é o método do `OpWriter` (bytecode_generator.rs).

    // BytecodeGenerator.h:524
    pub fn next_value_profile_index(&mut self) -> u32 {
        self.code_block.metadata().add_value_profile()
    }

    // BytecodeGenerator.h:529
    pub fn emit_node_statement(&mut self, n: &crate::parser::nodes::Statement) {
        self.emit_node(None, n);
    }

    // BytecodeGenerator.h:534
    pub fn emit_node_in_tail_position_statement_no_dst(&mut self, n: &crate::parser::nodes::Statement) {
        self.emit_node_in_tail_position_statement(None, n);
    }

    // BytecodeGenerator.h:539
    pub fn emit_node_expression(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        n: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let saved_tail = self.allow_tail_call_optimization;
        let saved_ignore = self.allow_call_ignore_result_optimization;
        self.allow_tail_call_optimization = false;
        self.allow_call_ignore_result_optimization = false;
        let result = self.emit_node_in_tail_position_expression(dst, n);
        self.allow_call_ignore_result_optimization = saved_ignore;
        self.allow_tail_call_optimization = saved_tail;
        result
    }

    // BytecodeGenerator.h:546
    pub fn emit_node_in_tail_position_from_return_node(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        n: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let saved_ignore = self.allow_call_ignore_result_optimization;
        self.allow_call_ignore_result_optimization = false;
        let result = self.emit_node_in_tail_position_expression(dst, n);
        self.allow_call_ignore_result_optimization = saved_ignore;
        result
    }

    // BytecodeGenerator.h:552
    pub fn emit_node_in_tail_position_from_expr_statement_node(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        n: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let saved_tail = self.allow_tail_call_optimization;
        self.allow_tail_call_optimization = false;
        let result = self.emit_node_in_tail_position_expression(dst, n);
        self.allow_tail_call_optimization = saved_tail;
        result
    }

    // BytecodeGenerator.h:558
    pub fn emit_node_in_ignore_result_position_expression(
        &mut self,
        n: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let saved_tail = self.allow_tail_call_optimization;
        let saved_ignore = self.allow_call_ignore_result_optimization;
        self.allow_tail_call_optimization = false;
        // Volta ao valor padrão.
        self.allow_call_ignore_result_optimization = self.default_allow_call_ignore_result_optimization;
        let result = self.emit_node_in_tail_position_expression(Some(self.ignored_result()), n);
        self.allow_call_ignore_result_optimization = saved_ignore;
        self.allow_tail_call_optimization = saved_tail;
        result
    }

    // BytecodeGenerator.h:564
    pub fn emit_node_in_tail_position_expression(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        n: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // Node::emitCode assume que dst, se dado, é um local ou um temporário referenciado.
        if !self.vm.is_safe_to_recurse() {
            return self.emit_throw_expression_too_deep_exception();
        }
        if n.base().needs_debug_hook() {
            self.emit_debug_hook_expression_data(n, None);
        }
        n.emit_bytecode(self, dst)
    }

    // BytecodeGenerator.h:576
    pub fn emit_node_expression_no_dst(
        &mut self,
        n: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_node_expression(None, n)
    }

    // BytecodeGenerator.h:581
    pub fn emit_node_in_tail_position_expression_no_dst(
        &mut self,
        n: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_node_in_tail_position_expression(None, n)
    }

    // BytecodeGenerator.h:586
    pub fn emit_define_class_elements(
        &mut self,
        n: &crate::parser::nodes::NodeRef<crate::parser::nodes::PropertyListNode>,
        constructor: &crate::bytecompiler::bytecode_generator::RegisterRef,
        prototype: &crate::bytecompiler::bytecode_generator::RegisterRef,
        instance_element_definitions: &mut Vec<crate::bytecode::unlinked_function_executable::ClassElementDefinition>,
        static_element_definitions: &mut Vec<crate::bytecode::unlinked_function_executable::ClassElementDefinition>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if !self.vm.is_safe_to_recurse() {
            return self.emit_throw_expression_too_deep_exception();
        }
        if n.borrow().needs_debug_hook() {
            self.emit_debug_hook_expression_data(&crate::parser::nodes::Expression::PropertyList(n.clone()), None);
        }
        crate::parser::nodes::PropertyListNode::emit_bytecode(
            n,
            self,
            Some(constructor.clone()),
            Some(prototype.clone()),
            Some(instance_element_definitions),
            Some(static_element_definitions),
        )
    }

    // BytecodeGenerator.h:596
    pub fn emit_node_for_property_dst(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        node: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if node.is_string() {
            if let crate::parser::nodes::Expression::String(string_node) = node {
                if let Some(index) = crate::runtime::identifier::parse_index_identifier(&string_node.borrow().value) {
                    return self.emit_load_js_value(dst, crate::runtime::js_value::JSValue::from_u32(index));
                }
            }
        }
        self.emit_node_expression(dst, node)
    }

    // BytecodeGenerator.h:604
    pub fn emit_node_for_property(
        &mut self,
        n: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_node_for_property_dst(None, n)
    }

    // BytecodeGenerator.h:609
    pub fn emit_node_in_condition_context(
        &mut self,
        n: &crate::parser::nodes::Expression,
        true_target: &crate::bytecompiler::label::LabelRef,
        false_target: &crate::bytecompiler::label::LabelRef,
        fall_through_mode: crate::parser::nodes::FallThroughMode,
    ) {
        if !self.vm.is_safe_to_recurse() {
            self.emit_throw_expression_too_deep_exception();
            return;
        }
        n.emit_bytecode_in_condition_context(self, true_target, false_target, fall_through_mode);
    }

    // BytecodeGenerator.h:618
    pub fn emit_expression_info(
        &mut self,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
    ) {
        // Não emite expression info se os dados puderem causar uma falha depois. Nesse caso só se usa
        // a informação errada numa mensagem de erro, sem falhar.
        if !divot.is_set() || !divot_start.is_set() || !divot_end.is_set() {
            return;
        }

        if self.is_builtin_function() {
            return;
        }
        let source_offset = self.scope_node.borrow().source.start_offset() as u32;
        let first_line = self.scope_node.borrow().source.first_line().one_based_int() as u32;

        let divot_offset = (divot.offset as u32).wrapping_sub(source_offset);
        let start_offset = (divot.offset as u32).wrapping_sub(divot_start.offset as u32);
        let end_offset = (divot_end.offset as u32).wrapping_sub(divot.offset as u32);

        let mut line = divot.line as u32;
        line = line.wrapping_sub(first_line);

        let mut line_start = divot.line_start_offset as u32;
        if line_start > source_offset {
            line_start -= source_offset;
        } else {
            line_start = 0;
        }

        if divot_offset < line_start {
            return;
        }

        let column = divot_offset - line_start;

        let instruction_offset = self.instructions().size_in_bytes() as u32;
        self.code_block.add_expression_info(
            instruction_offset,
            divot_offset,
            start_offset,
            end_offset,
            crate::bytecode::line_column::LineColumn { line, column },
        );
    }

    // BytecodeGenerator.h:657
    pub fn left_hand_side_needs_copy(&self, right_has_assignments: bool, right_is_pure: bool) -> bool {
        (self.code_type != crate::bytecode::code_type::CodeType::FunctionCode || right_has_assignments)
            && !right_is_pure
    }

    // BytecodeGenerator.h:662
    pub fn emit_node_for_left_hand_side(
        &mut self,
        n: &crate::parser::nodes::Expression,
        right_has_assignments: bool,
        right_is_pure: bool,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if self.left_hand_side_needs_copy(right_has_assignments, right_is_pure) {
            let dst = self.new_temporary();
            self.emit_node_expression(Some(dst.clone()), n);
            return Some(dst);
        }

        self.emit_node_expression_no_dst(n)
    }

    // BytecodeGenerator.h:673
    pub fn emit_node_for_left_hand_side_for_property(
        &mut self,
        n: &crate::parser::nodes::Expression,
        right_has_assignments: bool,
        right_is_pure: bool,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if self.left_hand_side_needs_copy(right_has_assignments, right_is_pure) {
            let dst = self.new_temporary();
            self.emit_node_for_property_dst(Some(dst.clone()), n);
            return Some(dst);
        }

        self.emit_node_for_property(n)
    }

    // BytecodeGenerator.h:684
    // pub fn hoist_sloppy_mode_function_if_necessary(&mut self, _: &NodeRef<FunctionMetadataNode>);  // .cpp
    // BytecodeGenerator.h:686
    // pub fn find_for_in_context(&mut self, property: &Rc<RefCell<RegisterID>>) -> Option<&mut ForInContext>;  // .cpp

    // BytecodeGenerator.h:690 (private)
    // fn emit_type_profiler_expression_info(&mut self, start_divot: &JSTextPosition, end_divot: &JSTextPosition);  // .cpp
    // BytecodeGenerator.h:692 (private): enum IsNotTypeofUndefined { Yes, No }, definido abaixo do impl.
    // BytecodeGenerator.h:693 (private)
    // fn try_emit_typeof_is_undefined_for_string_comparison<const IS_NOT_TYPEOF_UNDEFINED: bool>(
    //     &mut self, dst: &Rc<RefCell<RegisterID>>, src1: &Rc<RefCell<RegisterID>>, src2: &Rc<RefCell<RegisterID>>) -> bool;  // .cpp

    // BytecodeGenerator.h:698
    // pub fn emit_profile_type_flag(&mut self, register_to_profile: &Rc<RefCell<RegisterID>>, flag: ProfileTypeBytecodeFlag);  // .cpp
    // BytecodeGenerator.h:700
    // pub fn emit_profile_type_variable(&mut self, register_to_profile: &Rc<RefCell<RegisterID>>, variable: &Variable, start_divot: &JSTextPosition, end_divot: &JSTextPosition);  // .cpp
    // BytecodeGenerator.h:702
    // pub fn emit_profile_type_flag_divots(&mut self, register_to_profile: &Rc<RefCell<RegisterID>>, flag: ProfileTypeBytecodeFlag, start_divot: &JSTextPosition, end_divot: &JSTextPosition);  // .cpp
    // BytecodeGenerator.h:704
    // pub fn emit_profile_type_divots(&mut self, register_to_profile: &Rc<RefCell<RegisterID>>, start_divot: &JSTextPosition, end_divot: &JSTextPosition);  // .cpp
    // BytecodeGenerator.h:706
    // pub fn emit_profile_control_flow(&mut self, text_offset: i32);  // .cpp

    // BytecodeGenerator.h:708
    // pub fn emit_load_arrow_function_lexical_environment(&mut self, id: &Identifier) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // BytecodeGenerator.h:709
    // pub fn ensure_this(&mut self) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // BytecodeGenerator.h:710
    // pub fn emit_load_this_from_arrow_function_lexical_environment(&mut self);  // .cpp
    // BytecodeGenerator.h:711
    // pub fn emit_load_new_target_from_arrow_function_lexical_environment(&mut self) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:713
    // pub fn add_constant_index(&mut self) -> u32;  // .cpp
    // BytecodeGenerator.h:714
    // pub fn emit_load_bool(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, value: bool) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // BytecodeGenerator.h:715
    // pub fn emit_load_identifier(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, id: &Identifier) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // BytecodeGenerator.h:716
    // pub fn emit_load_js_value_with_representation(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, value: JSValue, representation: SourceCodeRepresentation) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    //   (o argumento padrão SourceCodeRepresentation::Other vira o wrapper emit_load_js_value, definido na parte do .cpp)
    // BytecodeGenerator.h:717
    // pub fn emit_load_excluded_list(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, excluded_list: IdentifierSet) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:719 a 727: template<UnaryOp> requires (opcodeID != op_negate)
    pub fn emit_unary_op<U: crate::bytecode::bytecode_ops::UnaryOpcode>(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if U::OPCODE_ID == crate::bytecode::opcode::OpcodeID::op_unsigned {
            let profile = self.code_block.add_unary_arith_profile();
            U::emit_with_profile(self, dst.clone(), src, profile);
        } else {
            U::emit(self, dst.clone(), src);
        }
        dst
    }

    // BytecodeGenerator.h:729
    // pub fn emit_unary_op_dynamic(&mut self, opcode_id: OpcodeID, dst: Option<Rc<RefCell<RegisterID>>>, src: Option<Rc<RefCell<RegisterID>>>, result_type: ResultType) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:731 a 742
    pub fn emit_binary_op<B: crate::bytecode::bytecode_ops::BinaryOpcode>(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src1: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src2: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        types: crate::parser::result_type::OperandTypes,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        use crate::bytecode::opcode::OpcodeID;
        let id = B::OPCODE_ID;
        if id == OpcodeID::op_add
            || id == OpcodeID::op_mul
            || id == OpcodeID::op_sub
            || id == OpcodeID::op_div
            || id == OpcodeID::op_bitand
            || id == OpcodeID::op_bitor
            || id == OpcodeID::op_bitxor
        {
            let profile = self.code_block.add_binary_arith_profile();
            B::emit_with_profile_and_types(self, dst.clone(), src1, src2, profile, types);
        } else if id == OpcodeID::op_lshift || id == OpcodeID::op_rshift {
            let profile = self.code_block.add_binary_arith_profile();
            B::emit_with_profile(self, dst.clone(), src1, src2, profile);
        } else {
            B::emit(self, dst.clone(), src1, src2);
        }
        dst
    }

    // BytecodeGenerator.h:744
    // pub fn emit_binary_op_dynamic(&mut self, opcode_id: OpcodeID, dst: Option<Rc<RefCell<RegisterID>>>, src1: Option<Rc<RefCell<RegisterID>>>, src2: Option<Rc<RefCell<RegisterID>>>, types: OperandTypes) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:746 a 752: template<EqOp>
    pub fn emit_equality_op<E: crate::bytecode::bytecode_ops::BinaryOpcode>(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src1: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src2: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // static_assert(EqOp::opcodeID == op_eq || EqOp::opcodeID == op_stricteq)
        if !self.emit_equality_op_impl(dst.clone(), src1.clone(), src2.clone()) {
            E::emit(self, dst.clone(), src1, src2);
        }
        dst
    }

    // BytecodeGenerator.h:754
    // pub fn emit_equality_op_impl(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, src1: Option<Rc<RefCell<RegisterID>>>, src2: Option<Rc<RefCell<RegisterID>>>) -> bool;  // .cpp
    // (o corpo do template acima o chama; a assinatura fica aqui para a parte do .cpp)

    // BytecodeGenerator.h:756 a 762
    // pub fn emit_create_this(&mut self, dst: Option<Rc<RefCell<RegisterID>>>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_create_promise(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, new_target: Option<Rc<RefCell<RegisterID>>>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_create_generator(&mut self, dst: ..., new_target: ...) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_create_async_generator(&mut self, dst: ..., new_target: ...) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // BytecodeGenerator.h:761
    // pub fn emit_instance_field_initialization_if_needed(&mut self, dst, constructor, divot: &JSTextPosition, divot_start: &JSTextPosition, divot_end: &JSTextPosition) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // BytecodeGenerator.h:762 a 768
    // pub fn emit_tdz_check(&mut self, target: &Rc<RefCell<RegisterID>>);  // .cpp
    // pub fn emit_tdz_check_variable(&mut self, target: &Rc<RefCell<RegisterID>>, variable: &Variable);  // .cpp
    // pub fn needs_tdz_check(&mut self, variable: &Variable) -> bool;  // .cpp
    // pub fn emit_tdz_check_if_necessary(&mut self, variable: &Variable, target: Option<Rc<RefCell<RegisterID>>>, scope: Option<Rc<RefCell<RegisterID>>>);  // .cpp
    // pub fn lift_tdz_check_if_possible(&mut self, variable: &Variable);  // .cpp
    // BytecodeGenerator.h:768 a 780: emit_new_object, emit_new_promise, emit_new_generator, emit_new_async_function_generator
    //   (todas: fn(&mut self, dst: Option<Rc<RefCell<RegisterID>>>) -> Option<Rc<RefCell<RegisterID>>>)  // .cpp
    // pub fn emit_new_array(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, elements: Option<NodeRef<ElementNode>>, length: u32, recommended_indexing_type: IndexingType) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp (para no primeiro elision)
    // pub fn emit_new_array_buffer(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, butterfly: CellId, recommended_indexing_type: IndexingType) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // FIXME do C++: new_array_with_spread deveria usar um array allocation profile e receber um recommendedIndexingType
    // pub fn emit_new_array_with_spread(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, elements: Option<NodeRef<ElementNode>>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_new_array_with_size(&mut self, dst: ..., length: Option<Rc<RefCell<RegisterID>>>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_new_array_with_species(&mut self, dst: ..., length: ..., array: ...) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:782 a 789
    // pub fn emit_new_function(&mut self, dst: ..., metadata: &NodeRef<FunctionMetadataNode>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_new_function_expression(&mut self, dst: ..., func_expr: &NodeRef<FuncExprNode>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_new_default_constructor(&mut self, dst: ..., constructor_kind: ConstructorKind, name: &Identifier, ecma_name: &Identifier, class_source: &SourceCode, needs_class_field_initializer: NeedsClassFieldInitializer, private_brand_requirement: PrivateBrandRequirement) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_new_class_field_initializer_function(&mut self, dst: ..., definitions: Vec<ClassElementDefinition>, is_derived: bool) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_new_arrow_function_expression(&mut self, dst: ..., arrow: &NodeRef<ArrowFuncExprNode>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_new_method_definition(&mut self, dst: ..., method: &NodeRef<MethodDefinitionNode>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_new_reg_exp(&mut self, dst: ..., regexp: Rc<RegExp>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:791 a 793
    // pub fn should_set_function_name(&mut self, node: &Expression) -> bool;  // .cpp
    // pub fn emit_set_function_name(&mut self, value: &Rc<RefCell<RegisterID>>, name: &Rc<RefCell<RegisterID>>);  // .cpp
    // pub fn emit_set_function_name_identifier(&mut self, value: &Rc<RefCell<RegisterID>>, name: &Identifier);  // .cpp

    // BytecodeGenerator.h:795 a 797
    // pub fn emit_async_iterator_open(&mut self, iterator: &Rc<RefCell<RegisterID>>, next: &Rc<RefCell<RegisterID>>, symbol_iterator: &Rc<RefCell<RegisterID>>, iterable: &mut CallArguments, node: &ThrowableExpressionData);  // .cpp
    // pub fn emit_get_generic_async_iterator(&mut self, iterator: ..., next: ..., subject: ..., node: &ThrowableExpressionData);  // .cpp
    // pub fn emit_async_iterator_next(&mut self, dst: ..., next: ..., iterator: ..., value: ..., node: &ThrowableExpressionData) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:799 a 800
    // pub fn move_link_time_constant(&mut self, dst: Option<Rc<RefCell<RegisterID>>>, constant: LinkTimeConstant) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn move_empty_value(&mut self, dst: Option<Rc<RefCell<RegisterID>>>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:802 a 808
    // emit_to_number, emit_to_numeric, emit_to_string: fn(&mut self, dst: &Rc<RefCell<RegisterID>>, src: &Rc<RefCell<RegisterID>>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_to_object(&mut self, dst: ..., src: ..., message: &Identifier) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_to_this(&mut self, src_dst: &Rc<RefCell<RegisterID>>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_inc(&mut self, src_dst: &Rc<RefCell<RegisterID>>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_dec(&mut self, src_dst: &Rc<RefCell<RegisterID>>) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:810 a 813
    // pub fn emit_instanceof(&mut self, dst, value, constructor, has_instance_or_prototype) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_type_of(&mut self, dst, src) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_in_by_val(&mut self, dst, property, base) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp
    // pub fn emit_in_by_id(&mut self, dst, base, property: &Identifier) -> Option<Rc<RefCell<RegisterID>>>;  // .cpp

    // BytecodeGenerator.h:815 a 828 (todas .cpp; mesma forma de retorno, Option<Rc<RefCell<RegisterID>>>)
    // emit_get_length(dst, base); emit_get_by_id(dst, base, property: &Identifier);
    // emit_get_by_id_with_this(dst, base, this_val, property: &Identifier); emit_direct_get_by_id(dst, base, property);
    // emit_put_by_id(base, property: &Identifier, value); emit_put_by_id_with_this(base, this_value, property, value);
    // emit_direct_put_by_id(base, property, value); emit_delete_by_id(dst, base, property: &Identifier);
    // emit_get_by_val(dst, base, property); emit_get_by_val_with_this(dst, base, this_value, property);
    // emit_get_prototype_of(dst, value)

    // BytecodeGenerator.h:830 a 842
    pub fn emit_direct_set_prototype_of(
        &mut self,
        mode: crate::bytecompiler::bytecode_generator::InvalidPrototypeMode,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        prototype: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let set_prototype_direct = self.move_link_time_constant(
            None,
            if mode == crate::bytecompiler::bytecode_generator::InvalidPrototypeMode::Throw {
                crate::bytecode::link_time_constant::LinkTimeConstant::SetPrototypeDirectOrThrow
            } else {
                crate::bytecode::link_time_constant::LinkTimeConstant::SetPrototypeDirect
            },
        );

        let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 1);
        let this_register = args.this_register();
        self.move_register(this_register.as_ref(), base.as_ref().unwrap());
        let argument_register = args.argument_register(0);
        self.move_register(argument_register.as_ref(), prototype.as_ref().unwrap());

        let temporary = self.new_temporary();
        self.emit_call_ignore_result(
            Some(temporary),
            set_prototype_direct,
            crate::bytecompiler::bytecode_generator::NO_EXPECTED_FUNCTION,
            &mut args,
            divot,
            divot_start,
            divot_end,
            crate::bytecompiler::bytecode_generator::DebuggableCall::No,
        );
        base
    }

    // BytecodeGenerator.h:844 a 849 (.cpp)
    // emit_put_by_val(base, property, value); emit_put_by_val_with_this(base, this_value, property, value);
    // emit_put_by_val_with_ecma_mode(base, this_value, property, value, ecma_mode: ECMAMode);
    // emit_enumerator_put_by_val(for_in_context: &mut ForInContext, base, property, value);
    // emit_direct_put_by_val(base, property, value); emit_delete_by_val(dst, base, property)

    // BytecodeGenerator.h:851 a 858 (.cpp)
    // emit_get_internal_field(dst, base, index: u32); emit_put_internal_field(base, index: u32, value);
    // emit_define_private_field(base, property, value); emit_private_field_put(base, property, value);
    // emit_get_private_name(dst, base, property); emit_has_private_name(dst, base, property);
    // emit_has_structure_with_flags(dst, src, flags: u32)

    // BytecodeGenerator.h:860 a 866 (.cpp)
    // emit_create_private_brand(divot, divot_start, divot_end: &JSTextPosition); emit_install_private_brand(target);
    // emit_install_private_class_brand(target)
    // emit_get_private_brand(dst, scope, is_static: bool) -> Option<..>; emit_has_private_brand(dst, base, brand, is_static: bool) -> Option<..>
    // emit_check_private_brand(base, brand, is_static: bool)

    // BytecodeGenerator.h:868 a 872 (.cpp)
    // emit_super_sampler_begin(); emit_super_sampler_end();
    // emit_id_with_profile(src, profile: SpeculatedType) -> Option<..>; emit_unreachable()

    // BytecodeGenerator.h:874 a 878 (.cpp)
    // emit_put_getter_by_id(base, property: &Identifier, property_descriptor_options: u32, getter);
    // emit_put_setter_by_id(base, property: &Identifier, property_descriptor_options: u32, setter);
    // emit_put_getter_setter(base, property: &Identifier, attributes: u32, getter, setter);
    // emit_put_getter_by_val(base, property, property_descriptor_options: u32, getter);
    // emit_put_setter_by_val(base, property, property_descriptor_options: u32, setter)

    // BytecodeGenerator.h:880
    // pub fn emit_get_argument(&mut self, dst: ..., index: i32) -> Option<..>;  // .cpp
    // BytecodeGenerator.h:883 e 885: Inicializa o objeto com os campos de gerador (@generatorThis, @generatorNext, @generatorState, @generatorFrame)
    // pub fn emit_put_generator_fields(&mut self, next_function: Option<..>);  // .cpp
    // pub fn emit_put_async_generator_fields(&mut self, next_function: Option<..>);  // .cpp

    // BytecodeGenerator.h:887 a 892 (.cpp)
    // pub fn expected_function_for_identifier(&mut self, id: &Identifier) -> ExpectedFunction;
    // emit_call, emit_call_in_tail_position: (dst, func, ExpectedFunction, &mut CallArguments, divot, divot_start, divot_end, DebuggableCall) -> Option<..>
    // emit_call_direct_eval: (dst, func, &mut CallArguments, divot, divot_start, divot_end, DebuggableCall) -> Option<..>
    // emit_call_varargs, emit_call_varargs_in_tail_position: (dst, func, this_register, arguments, first_free_register, first_var_arg_offset: i32, divot, divot_start, divot_end, DebuggableCall) -> Option<..>
    // BytecodeGenerator.h:890
    // pub fn emit_call_ignore_result(&mut self, dst, func, ExpectedFunction, &mut CallArguments, divot, divot_start, divot_end, DebuggableCall);  // .cpp

    // BytecodeGenerator.h:892 a 899 (.cpp)
    // pub fn emit_call_define_property(&mut self, new_obj, property_name_register, value_register, getter_register, setter_register, options: u32, position: &JSTextPosition);
    // pub fn emit_try_with_finally_that_does_not_shadow_exception(&mut self, emit_try: &dyn Fn(&mut BytecodeGenerator), emit_finally: &dyn Fn(&mut BytecodeGenerator));
    // pub fn emit_try_with_finally_that_does_not_shadow_exception_with_context(&mut self, finally_context: &mut FinallyContext, emit_try: ..., emit_finally: ...);

    // BytecodeGenerator.h:902
    // Explicit Resource Management: declarações using
    pub fn current_using_scope(&mut self) -> &mut crate::bytecompiler::bytecode_generator::UsingScope {
        self.using_scope_stack
            .last_mut()
            .expect("ASSERT(!m_usingScopeStack.isEmpty())")
    }
    // BytecodeGenerator.h:903 a 905 (.cpp)
    // pub fn emit_prepare_disposable(&mut self, value: &Rc<RefCell<RegisterID>>, divot: &JSTextPosition, is_async: bool);  // is_async tem padrão false: wrapper sem o argumento na parte do .cpp
    // pub fn emit_using_body_scope(&mut self, using_count: u32, has_await_using: bool, emit_body: &dyn Fn(&mut BytecodeGenerator));
    // pub fn emit_body_with_using_if_needed(&mut self, using_count: u32, has_await_using: bool, emit_body: &dyn Fn(&mut BytecodeGenerator));

    // BytecodeGenerator.h:907 a 912 (.cpp)
    // pub fn emit_enumeration(&mut self, enumeration_node: &ThrowableExpressionData, subject_node: &Expression, call_back: &dyn Fn(&mut BytecodeGenerator, &Rc<RefCell<RegisterID>>), for_of_node: Option<NodeRef<ForOfNode>>, for_loop_symbol_table: Option<Rc<RefCell<RegisterID>>>);
    // pub fn emit_get_template_object(&mut self, dst, template: &NodeRef<TaggedTemplateNode>) -> Option<..>;
    // pub fn emit_get_global_private(&mut self, dst, property: &Identifier) -> Option<..>;
    // pub fn emit_return(&mut self, src: Option<Rc<RefCell<RegisterID>>>) -> Option<..>;

    // BytecodeGenerator.h:915 a 920 (.cpp)
    // pub fn emit_construct(&mut self, dst, func, lazy_this, ExpectedFunction, &mut CallArguments, divot, divot_start, divot_end) -> Option<..>;
    // pub fn emit_super_construct(&mut self, dst, func, lazy_this, ExpectedFunction, &mut CallArguments, divot, divot_start, divot_end, is_default_derived_constructor_call: bool) -> Option<..>;
    // pub fn emit_strcat(&mut self, dst, src, count: i32) -> Option<..>;
    // pub fn emit_to_primitive(&mut self, dst, src);
    // pub fn emit_to_property_key(&mut self, dst, src) -> Option<..>;
    // pub fn emit_to_property_key_or_number(&mut self, dst, src) -> Option<..>;

    // BytecodeGenerator.h:922 a 930 (.cpp)
    // pub fn resolve_type(&mut self) -> ResolveType;
    // pub fn emit_resolve_constant_local(&mut self, dst, variable: &Variable) -> Option<..>;
    // pub fn emit_resolve_scope(&mut self, dst, variable: &Variable) -> Option<..>;
    // pub fn emit_get_from_scope(&mut self, dst, scope, variable: &Variable, resolve_mode: ResolveMode) -> Option<..>;
    // pub fn emit_put_to_scope(&mut self, scope, variable: &Variable, value, resolve_mode: ResolveMode, initialization_mode: InitializationMode) -> Option<..>;
    // pub fn emit_put_to_scope_dynamic(&mut self, scope, id: &Identifier, value, resolve_mode: ResolveMode, initialization_mode: InitializationMode) -> Option<..>;
    // pub fn emit_resolve_scope_for_hoisting_func_decl_in_eval(&mut self, dst, id: &Identifier) -> Option<..>;
    // pub fn initialize_variable(&mut self, variable: &Variable, value) -> Option<..>;

    // BytecodeGenerator.h:932 a 944 (.cpp)
    // emit_loop_hint(); emit_jump(target: &mut Label); emit_jump_if_true(cond, target); emit_jump_if_false(cond, target);
    // emit_jump_if_not_function_call(cond, target); emit_jump_if_not_function_apply(cond, target);
    // emit_jump_if_not_eval_function(cond, target); emit_jump_if_empty_property_name_enumerator(cond, target);
    // emit_jump_if_sentinel_string(cond, target)
    // pub fn emit_wide_jump_if_not_function_has_own_property(&mut self, cond, target: &mut Label) -> u32;
    // pub fn record_has_own_property_in_for_in_loop(&mut self, for_in_context: &mut ForInContext, branch_offset: u32, generic_path: &mut Label);

    // BytecodeGenerator.h:946 a 950 (templates, corpo no .cpp)
    // pub fn fuse_compare_and_jump<BinOp, JmpOp>(&mut self, cond: &Rc<RefCell<RegisterID>>, target: &mut Label, swap_operands: bool) -> bool;
    // pub fn fuse_test_and_jmp<UnaryOp, JmpOp>(&mut self, cond: &Rc<RefCell<RegisterID>>, target: &mut Label) -> bool;

    // BytecodeGenerator.h:952 a 953 (.cpp)
    // pub fn emit_enter(&mut self); pub fn emit_check_traps(&mut self);

    // BytecodeGenerator.h:955 a 957 (.cpp)
    // pub fn emit_get_property_enumerator(&mut self, dst, base) -> Option<..>;
    // pub fn emit_enumerator_next(&mut self, property_name, mode, index, base, enumerator);
    // pub fn emit_enumerator_has_own_property(&mut self, dst, base, mode, property_name, index, enumerator) -> Option<..>;

    // BytecodeGenerator.h:959
    // pub fn emit_is_cell_with_type(&mut self, dst, src, js_type: JSType) -> Option<..>;  // .cpp

    // BytecodeGenerator.h:960 a 971, 987 a 989: um forwarder de uma linha no C++, mantido porque cada um fixa o JSType
    pub fn emit_is_generator(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::JSGeneratorType)
    }
    pub fn emit_is_iterator_helper(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::JSIteratorHelperType)
    }
    pub fn emit_is_js_array(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::ArrayType)
    }
    pub fn emit_is_promise(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::JSPromiseType)
    }
    pub fn emit_is_proxy_object(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::ProxyObjectType)
    }
    pub fn emit_is_reg_exp_object(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::RegExpObjectType)
    }
    pub fn emit_is_map(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::JSMapType)
    }
    pub fn emit_is_set(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::JSSetType)
    }
    pub fn emit_is_shadow_realm(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::ShadowRealmType)
    }
    pub fn emit_is_array_iterator(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::JSArrayIteratorType)
    }
    pub fn emit_is_wrap_for_valid_iterator(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::JSWrapForValidIteratorType)
    }
    pub fn emit_is_reg_exp_string_iterator(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::JSRegExpStringIteratorType)
    }
    // BytecodeGenerator.h:972 a 975 (.cpp): emit_is_object, emit_is_callable, emit_is_constructor, emit_is_number (dst, src)
    // BytecodeGenerator.h:976
    pub fn emit_is_null(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let null_value = self.emit_load_js_value(None, crate::runtime::js_value::js_null());
        self.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(dst, Some(src.clone()), null_value)
    }
    // BytecodeGenerator.h:977
    pub fn emit_is_undefined(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let undefined_value = self.emit_load_js_value(None, crate::runtime::js_value::js_undefined());
        self.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(dst, Some(src.clone()), undefined_value)
    }
    // BytecodeGenerator.h:978 e 979 (.cpp): emit_is_undefined_or_null(dst, src), emit_is_empty(dst, src)
    // BytecodeGenerator.h:980
    pub fn emit_is_derived_array(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::DerivedArrayType)
    }
    pub fn emit_is_disposable_stack(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::DisposableStackType)
    }
    pub fn emit_is_async_disposable_stack(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, src: &crate::bytecompiler::bytecode_generator::RegisterRef) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_is_cell_with_type(dst, src, crate::runtime::js_type::JSType::AsyncDisposableStackType)
    }
    // BytecodeGenerator.h:983 e 984 (.cpp)
    // pub fn emit_require_object_coercible(&mut self, value: &Rc<RefCell<RegisterID>>, error: &'static str);
    // pub fn emit_require_object_coercible_for_destructuring(&mut self, value: &Rc<RefCell<RegisterID>>, property_name: Option<&Identifier>);

    // BytecodeGenerator.h:986 a 995 (.cpp)
    // pub fn emit_iterator_open(&mut self, iterator, next_or_index, symbol_iterator, iterable: &mut CallArguments, node: &ThrowableExpressionData);
    // pub fn emit_iterator_next(&mut self, done, value, iterable, next_or_index, iterator: &mut CallArguments, node: &ThrowableExpressionData);
    // pub fn emit_get_generic_iterator(&mut self, iterable: Option<..>, node: &ThrowableExpressionData) -> Option<..>;
    // pub fn emit_iterator_generic_next(&mut self, dst, next_method, iterator, node: &ThrowableExpressionData, emit_await: EmitAwait) -> Option<..>;  // padrão EmitAwait::No: wrapper na parte do .cpp
    // pub fn emit_iterator_generic_next_with_value(&mut self, dst, next_method, iterator, value, node: &ThrowableExpressionData) -> Option<..>;
    // pub fn emit_iterator_generic_close(&mut self, iterator, node: &ThrowableExpressionData, emit_await: EmitAwait);  // padrão EmitAwait::No

    // BytecodeGenerator.h:997
    // pub fn emit_rest_parameter(&mut self, result, num_parameters_to_skip: u32) -> Option<..>;  // .cpp

    // BytecodeGenerator.h:991 (.cpp)
    // pub fn emit_read_only_exception_if_needed(&mut self, variable: &Variable) -> bool;

    // BytecodeGenerator.h:993 a 996 (.cpp)
    // Inicia um bloco try. 'start' precisa já ter sido emitido.
    // pub fn push_try(&mut self, start: &mut Label, handler_label: &mut Label, handler_type: HandlerType) -> Rc<RefCell<TryData>>;
    // Encerra um bloco try. 'end' precisa já ter sido emitido.
    // pub fn pop_try(&mut self, try_data: &Rc<RefCell<TryData>>, end: &mut Label);

    // BytecodeGenerator.h:998 e 999 (.cpp); a última declaração completa antes da linha 1000
    // pub fn emit_out_of_line_catch_handler(&mut self, thrown_value_register: &Rc<RefCell<RegisterID>>, completion_type_register: &Rc<RefCell<RegisterID>>, try_data: &Rc<RefCell<TryData>>);
    // pub fn emit_out_of_line_finally_handler(&mut self, exception_register: &Rc<RefCell<RegisterID>>, completion_type_register: &Rc<RefCell<RegisterID>>, try_data: &Rc<RefCell<TryData>>);
}

// BytecodeGenerator.h:763 e 766: enum class interno do .h (private), usado pelos templates de typeof.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IsNotTypeofUndefined {
    Yes,
    No,
}

// BytecodeGenerator.h:884 a 889: enum PropertyDescriptorOption (membro público da classe)
pub const PROPERTY_CONFIGURABLE: u32 = 1;
pub const PROPERTY_WRITABLE: u32 = 1 << 1;
pub const PROPERTY_ENUMERABLE: u32 = 1 << 2;

// Campos: (esta faixa não declara campos; eles estão fora das linhas 500 a 999)
