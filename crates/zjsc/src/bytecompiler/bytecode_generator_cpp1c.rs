// Parte 1c de bytecompiler/BytecodeGenerator.cpp (linhas 993 a 1059): o construtor de EvalNode.
// Juntada por include!, entre a parte 1 e a parte 2. Mesmas convenções da parte 1.

impl BytecodeGenerator {
    /// `BytecodeGenerator::BytecodeGenerator(VM&, EvalNode*, UnlinkedEvalCodeBlock*, ...)`.
    pub fn new_eval(
        vm: &std::rc::Rc<crate::runtime::vm::VM>,
        eval_node: crate::parser::nodes::NodeRef<crate::parser::nodes::EvalNode>,
        code_block: &std::rc::Rc<std::cell::RefCell<crate::bytecode::unlinked_code_block::UnlinkedEvalCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<std::rc::Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
        _generator_or_async_wrapper_function_parameter_names: Option<&Vec<crate::runtime::identifier::Identifier>>,
        parent_private_name_environment: Option<&crate::parser::variable_environment::PrivateNameEnvironment>,
    ) -> BytecodeGenerator {
        let base = code_block.borrow().base_ref();
        let mut this = BytecodeGenerator::with_defaults(
            vm,
            base.clone(),
            code_generation_mode,
            crate::parser::nodes::ScopeNodeRef::Eval(eval_node.clone()),
            crate::bytecode::code_type::CodeType::EvalCode,
        );
        {
            let node = eval_node.borrow();
            this.uses_exceptions = false;
            this.expression_too_deep = false;
            this.is_builtin_function = false;
            this.uses_sloppy_eval = node.uses_eval() && !node.is_strict_mode();
            this.allow_tail_call_optimization = false;
            this.allow_call_ignore_result_optimization = this.default_allow_call_ignore_result_optimization;
            this.needs_to_update_arrow_function_context = node.uses_arrow_function() || node.uses_eval();
            this.ecma_mode = ECMAMode::from_bool(node.is_strict_mode());
        }
        this.derived_context_type = base.borrow().derived_context_type();

        this.code_block.set_num_parameters(1);

        this.push_private_access_names(parent_private_name_environment);

        this.cached_parent_tdz = parent_scope_tdz_variables.clone();

        this.emit_enter();
        this.allocate_scope();
        this.top_level_scope_register = Some(this.add_var());
        this.top_level_scope_register.as_ref().unwrap().borrow_mut().ref_();
        let top_level_scope_register = this.top_level_scope_register.clone();
        let scope_register = this.scope_register();
        this.move_register(top_level_scope_register.as_ref(), scope_register.as_ref().unwrap());

        let function_stack = eval_node.borrow().base.variable_environment.function_stack.clone();
        for function in function_stack.iter() {
            let made = this.make_function(function);
            this.code_block.add_function_decl(made);
            this.functions_to_initialize
                .push((function.clone(), FunctionVariableType::TopLevelFunctionVariable));
        }

        let mut variables: Vec<crate::runtime::identifier::Identifier> = Vec::new();
        let mut hoisted_functions: Vec<crate::runtime::identifier::Identifier> = Vec::new();
        for entry in eval_node.borrow().base.var_declarations.iter() {
            debug_assert!(entry.1.is_var());
            debug_assert!(entry.0 .0.is_atom() || entry.0 .0.is_symbol());
            if entry.1.is_sloppy_mode_hoisted_function() {
                hoisted_functions.push(crate::runtime::identifier::Identifier::from_uid(&this.vm, Some(&entry.0)));
            } else if !entry.1.is_function() {
                variables.push(crate::runtime::identifier::Identifier::from_uid(&this.vm, Some(&entry.0)));
            }
        }
        code_block.borrow_mut().adopt_variables(variables);
        code_block.borrow_mut().adopt_function_hoisting_candidates(hoisted_functions);

        if eval_node.borrow().needs_new_target_register_for_this_scope() {
            this.new_target_register = Some(this.add_var());
        }

        if base.borrow().is_arrow_function_context()
            && (eval_node.borrow().uses_this() || eval_node.borrow().uses_super_property())
        {
            this.emit_load_this_from_arrow_function_lexical_environment();
        }

        if eval_node.borrow().needs_new_target_register_for_this_scope() {
            this.emit_load_new_target_from_arrow_function_lexical_environment();
        }

        if this.needs_to_update_arrow_function_context()
            && !base.borrow().is_arrow_function_context()
            && !this.is_derived_constructor_context()
        {
            this.initialize_arrow_function_context_scope_if_needed(None, false);
            this.emit_put_this_to_arrow_function_context_scope();
        }

        let should_initialize_block_scoped_functions = false; // We generate top-level function declarations in ::generate().
        let scope_node = this.scope_node.clone();
        this.push_lexical_scope(
            &scope_node.borrow().variable_environment,
            ScopeType::LetConstScope,
            TDZCheckOptimization::Optimize,
            NestedScopeType::IsNotNested,
            None,
            should_initialize_block_scoped_functions,
        );
        this
    }
}
