// Parte 2 de bytecompiler/BytecodeGenerator.cpp (linhas 1060 a 2002). Juntada por include!.
// Convenção: `Option<RegisterRef>` é o `RegisterID*` anulável; `RegisterRef` é o `RefPtr<RegisterID>`.
// Suposições sobre a struct (parte 3): `BytecodeGenerator::with_defaults(vm, base_code_block,
// code_generation_mode, scope_node, code_type)` devolve a struct com os campos nos valores padrão do C++
// (o que o inicializador de membros do .h dá) e os campos são os `m_*` em snake_case. `last_instruction` guarda a última instrução
// escrita, com `is_op::<Op>()` e `as_op::<Op>()`; `Op*::emit(gen, ...)` escreve a instrução.

impl BytecodeGenerator {
    // BytecodeGenerator.cpp:1060
    pub fn new_for_module_program(
        vm: &std::rc::Rc<crate::runtime::vm::VM>,
        module_program_node: crate::parser::nodes::NodeRef<crate::parser::nodes::ModuleProgramNode>,
        code_block: &std::rc::Rc<std::cell::RefCell<crate::bytecode::unlinked_code_block::UnlinkedModuleProgramCodeBlock>>,
        code_generation_mode: crate::parser::parser_modes::CodeGenerationModeSet,
        parent_scope_tdz_variables: &Option<std::rc::Rc<crate::bytecode::tdz_environment::TDZEnvironmentLink>>,
    ) -> BytecodeGenerator {
        use crate::bytecompiler::bytecode_generator::{Variable, VariableKind};
        let base = code_block.borrow().base_ref();
        let mut this = BytecodeGenerator::with_defaults(
            vm,
            base,
            code_generation_mode,
            crate::parser::nodes::ScopeNodeRef::ModuleProgram(module_program_node.clone()),
            crate::bytecode::code_type::CodeType::ModuleCode,
        );
        this.default_allow_call_ignore_result_optimization = !crate::runtime::options::Options::eval_mode();
        this.uses_exceptions = false;
        this.expression_too_deep = false;
        this.is_builtin_function = false;
        this.uses_sloppy_eval = false;
        this.allow_tail_call_optimization = false;
        this.allow_call_ignore_result_optimization = this.default_allow_call_ignore_result_optimization;
        this.needs_to_update_arrow_function_context =
            module_program_node.borrow().uses_arrow_function() || module_program_node.borrow().uses_eval();
        this.ecma_mode = crate::runtime::ecma_mode::ECMAMode::strict();

        debug_assert!(parent_scope_tdz_variables.is_none());
        code_block.borrow_mut().set_variable_declarations(module_program_node.borrow().var_declarations.clone());

        let module_environment_symbol_table = crate::runtime::symbol_table::SymbolTable::create(&this.vm);
        module_environment_symbol_table.borrow_mut().set_uses_sloppy_eval(this.uses_sloppy_eval);
        module_environment_symbol_table
            .borrow_mut()
            .set_scope_type(crate::runtime::symbol_table::ScopeType::LexicalScope);

        let should_capture_all_of_the_things = this.should_emit_debug_hooks() || this.uses_eval();
        if should_capture_all_of_the_things {
            module_program_node.borrow_mut().var_declarations.mark_all_variables_as_captured();
        }

        let captures_node = module_program_node.clone();
        let captures = move |uid: &crate::wtf::text::string_impl::UniquedKey| -> bool {
            captures_node.borrow().var_declarations.captures(uid)
        };
        let look_up_var_kind = |uid: &crate::wtf::text::string_impl::UniquedKey,
                                entry: &crate::parser::variable_environment::VariableEnvironmentEntry|
         -> crate::runtime::var_offset::VarKind {
            use crate::runtime::var_offset::VarKind;
            // Aloca as variáveis exportadas no ambiente do módulo.
            if entry.is_exported() {
                return VarKind::Scope;
            }

            // Aloca as variáveis de namespace no ambiente do módulo, para instanciá-lo de fora do
            // código do módulo.
            if entry.is_imported_namespace() {
                return VarKind::Scope;
            }

            if entry.is_captured() {
                return VarKind::Scope;
            }
            if captures(uid) { VarKind::Scope } else { VarKind::Stack }
        };

        if module_program_node.borrow().uses_await {
            this.needs_generatorification = true;
            this.initialize_next_parameter(); // |this|
            for _ in 0..crate::runtime::js_generator::Argument::NUMBER_OF_ARGUMENTS {
                this.initialize_next_parameter();
            }
            this.generator_register = Some(
                RegisterRef::new(&this.parameters[crate::runtime::abstract_module_record::Argument::Generator as usize]),
            );
        }

        this.emit_enter();
        this.allocate_scope();
        this.top_level_scope_register = Some(this.add_var());
        this.top_level_scope_register.as_ref().unwrap().borrow_mut().ref_();
        let scope_register = this.scope_register();
        let top_level_scope_register = this.top_level_scope_register.clone();
        this.move_register(top_level_scope_register.as_ref(), scope_register.as_ref().unwrap());

        this.callee_register.set_index(VirtualRegister::new(
            crate::interpreter::call_frame::CallFrameSlot::Callee as i32,
        ));

        this.code_block.set_num_parameters(
            crate::runtime::abstract_module_record::Argument::NUMBER_OF_ARGUMENTS as u32 + 1,
        ); // Aloca espaço para "this" + os argumentos do módulo async.

        // Agora declara todas as variáveis.

        let star_namespace_private_name = this.vm.property_names.star_namespace_private_name.clone();
        this.create_variable(
            &star_namespace_private_name,
            crate::runtime::var_offset::VarKind::Scope,
            &module_environment_symbol_table,
            crate::bytecompiler::existing_variable_mode::ExistingVariableMode::VerifyExisting,
        );
        if module_program_node.borrow().features & crate::parser::parser_modes::IMPORT_META_FEATURE != 0 {
            let meta_private_name = this.vm.property_names.builtin_names().meta_private_name();
            this.create_variable(
                &meta_private_name,
                crate::runtime::var_offset::VarKind::Scope,
                &module_environment_symbol_table,
                crate::bytecompiler::existing_variable_mode::ExistingVariableMode::VerifyExisting,
            );
        }

        let var_entries: Vec<_> = module_program_node.borrow().var_declarations.iter().cloned().collect();
        for (key, value) in var_entries.iter() {
            debug_assert!(!value.is_let() && !value.is_const());
            if !value.is_var() {
                // Este é um parâmetro ou o callee.
                continue;
            }
            // As ligações importadas não são alocadas no ambiente do módulo como as variáveis comuns.
            // Essas referências continuam "Dynamic" no code block deslinkado. Depois, ao linkar o code
            // block, a referência é resolvida para "ModuleVar".
            if value.is_imported() && !value.is_imported_namespace() {
                continue;
            }
            let ident = crate::runtime::identifier::Identifier::from_uid(&this.vm, Some(key));
            let var_kind = look_up_var_kind(key, value);
            this.create_variable(
                &ident,
                var_kind,
                &module_environment_symbol_table,
                crate::bytecompiler::existing_variable_mode::ExistingVariableMode::IgnoreExisting,
            );
        }

        let lexical_variables = module_program_node.borrow().variable_environment.lexical_variables.clone();
        this.instantiate_lexical_variables(
            &lexical_variables,
            crate::bytecompiler::bytecode_generator::ScopeType::LetConstScope,
            &module_environment_symbol_table,
            crate::bytecompiler::bytecode_generator::ScopeRegisterType::Block,
            &look_up_var_kind,
        );

        // Mantemos a tabela de símbolos no pool de constantes.
        let constant_symbol_table: Option<RegisterRef>;
        if this.should_emit_type_profiler_hooks() || module_program_node.borrow().uses_await {
            constant_symbol_table = Some(this.add_constant_value(
                crate::runtime::js_value::JSValue::from_cell(module_environment_symbol_table.borrow().cell_id()),
                crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
            ));
        } else {
            let cloned = module_environment_symbol_table.borrow().clone_scope_part(
                &this.vm,
                crate::runtime::symbol_table::PropagateCloneInvalidationToOriginal::No,
            );
            constant_symbol_table = Some(this.add_constant_value(
                crate::runtime::js_value::JSValue::from_cell(cloned.borrow().cell_id()),
                crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
            ));
        }
        let constant_symbol_table = constant_symbol_table.unwrap();
        let constant_symbol_table_index = constant_symbol_table.borrow().index();

        if module_program_node.borrow().uses_await {
            this.generator_frame_symbol_table = Some(module_environment_symbol_table.clone());
            this.generator_frame_symbol_table_index = constant_symbol_table_index;
            let generator_register = this.generator_register();
            let generator_frame_register = Some(this.generator_frame_register());
            this.emit_put_internal_field(
                generator_register,
                crate::runtime::abstract_module_record::Field::Frame as u32,
                generator_frame_register,
            );
        }

        this.push_tdz_variables(
            &lexical_variables,
            crate::bytecompiler::bytecode_generator::TDZCheckOptimization::Optimize,
            crate::bytecompiler::bytecode_generator::TDZRequirement::UnderTDZ,
        );
        let is_with_scope = false;

        this.lexical_scope_stack.push(crate::bytecompiler::bytecode_generator::LexicalScopeStackEntry {
            symbol_table: Some(module_environment_symbol_table.clone()),
            scope: this.top_level_scope_register.clone(),
            is_with_scope,
            symbol_table_constant_index: constant_symbol_table_index,
        });
        this.emit_prefill_stack_tdz_variables(&lexical_variables, &module_environment_symbol_table);

        // makeFunction supõe que há entradas de TDZ corretas na pilha. Por isso deve ser chamado depois de
        // pôr nosso ambiente léxico na pilha de TDZ corretamente.

        let function_stack: Vec<_> = module_program_node.borrow().variable_environment.function_stack.to_vec();
        for function in function_stack.iter() {
            let function_key = function.ident.borrow().impl_().expect("RELEASE_ASSERT(iterator != end)");
            let found_value = module_program_node
                .borrow()
                .variable_environment
                .lexical_variables
                .find(&function_key)
                .cloned()
                .expect("RELEASE_ASSERT(iterator != end)");
            assert!(!found_value.is_imported());

            let var_kind = look_up_var_kind(&function_key, &found_value);
            if var_kind == crate::runtime::var_offset::VarKind::Scope {
                // http://www.ecma-international.org/ecma-262/6.0/#sec-moduledeclarationinstantiation
                // Seção 15.2.1.16.4, passo 16-a-iv-1.
                // Todas as declarações de função alocadas no heap devem ser instanciadas quando o ambiente
                // do módulo é criado. Isso inclui as declarações exportadas e as não exportadas que são
                // alocadas no heap. É preciso porque a função exportada deve ser instanciada antes de
                // executar qualquer módulo do grafo de dependências. Assim os módulos podem linkar as
                // ligações importadas antes de executar o código de qualquer módulo.
                //
                // E como as declarações de função são instanciadas antes de executar o corpo do módulo,
                // a especificação permite que as funções do módulo sejam executadas antes do corpo, sob
                // dependências circulares. Exemplo:
                //
                // Módulo A (executado primeiro):
                //    import { b } from "B";
                //    // Aqui o módulo "B" ainda não executou, mas a declaração de função já está
                //    // instanciada. Então podemos chamar a função exportada de "B".
                //    b();
                //
                //    export function a() {
                //    }
                //
                // Módulo B (executado em segundo):
                //    import { a } from "A";
                //
                //    export function b() {
                //        c();
                //    }
                //
                //    // c não é exportada, mas como b a referencia, devemos instanciá-la antes de
                //    // executar o código do módulo "B".
                //    function c() {
                //        a();
                //    }
                //
                // Módulo de entrada (executado por último):
                //    import "B";
                //    import "A";
                //
                let made = this.make_function(function);
                this.code_block.add_function_decl(made);
            } else {
                // As funções alocadas na pilha podem ser alocadas ao executar o corpo do módulo.
                this.functions_to_initialize.push((
                    function.clone(),
                    crate::bytecompiler::bytecode_generator::FunctionVariableType::NormalFunctionVariable,
                ));
            }
        }

        // Lembra o offset do registro constante da tabela de símbolos mais externa. Essa tabela será
        // clonada ao linkar o code block. Depois, para criar o ambiente do módulo, pegamos a tabela
        // clonada do code block linkado por esse offset.
        code_block.borrow_mut().set_module_environment_symbol_table_constant_register_offset(constant_symbol_table_index);
        this
    }

    // BytecodeGenerator.cpp:1227
    pub fn initialize_default_parameter_values_and_setup_function_scope_stack(
        &mut self,
        parameters: &crate::parser::nodes::FunctionParameters,
        is_simple_parameter_list: bool,
        function_node: &crate::parser::nodes::NodeRef<crate::parser::nodes::FunctionNode>,
        function_symbol_table: &crate::runtime::symbol_table::SymbolTableRef,
        symbol_table_constant_index: i32,
        captures: &dyn Fn(&crate::wtf::text::string_impl::UniquedKey) -> bool,
        should_create_arguments_variable_in_parameter_scope: bool,
    ) {
        use crate::bytecompiler::bytecode_generator::{NestedScopeType, TDZCheckOptimization, TDZRequirement};
        let mut values_to_move_into_vars: Vec<(crate::runtime::identifier::Identifier, RegisterRef)> = Vec::new();
        debug_assert!(!(is_simple_parameter_list && should_create_arguments_variable_in_parameter_scope));
        if !is_simple_parameter_list {
            // Veja a seção 9.2.12 da ES6: http://www.ecma-international.org/ecma-262/6.0/index.html#sec-functiondeclarationinstantiation
            // Isto implementa o passo 21.
            let mut environment = crate::parser::variable_environment::VariableEnvironment::default();
            let mut all_parameter_names: Vec<crate::runtime::identifier::Identifier> = Vec::new();
            for parameter in parameters.patterns.iter() {
                parameter.0.collect_bound_identifiers(&mut all_parameter_names);
            }
            if should_create_arguments_variable_in_parameter_scope {
                all_parameter_names.push(self.property_names().arguments.clone());
            }
            let mut parameter_set = crate::parser::parser::IdentifierSet::default();
            for ident in all_parameter_names.iter() {
                let ident_key = ident.impl_().expect("identificador de parâmetro sem impl");
                parameter_set.add(&ident_key, ());
                let entry = environment.add(&ident_key);
                entry.value.set_is_let(); // Com expressões de parâmetro default, os parâmetros agem como "let".
                if captures(&ident_key) {
                    entry.value.set_is_captured();
                }
            }
            // Isto implementa o passo 25 da seção 9.2.12.
            self.push_lexical_scope_internal(
                &mut environment,
                TDZCheckOptimization::Optimize,
                NestedScopeType::IsNotNested,
                None,
                TDZRequirement::UnderTDZ,
                crate::bytecompiler::bytecode_generator::ScopeType::LetConstScope,
                crate::bytecompiler::bytecode_generator::ScopeRegisterType::Block,
            );

            if should_create_arguments_variable_in_parameter_scope {
                let arguments_variable = self.variable(&self.property_names().arguments.clone(), crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                let arguments_register = self.arguments_register.clone();
                self.initialize_variable(&arguments_variable, arguments_register);
                self.lift_tdz_check_if_possible(&arguments_variable);
            }

            let temp = self.new_temporary();
            for i in 0..parameters.patterns.len() {
                let parameter = &parameters.patterns[i];
                if parameter.0.is_rest_parameter() {
                    continue;
                }
                if (i + 1) < self.parameters.len() {
                    let source = RegisterRef::new(&self.parameters[i + 1]);
                    self.move_register(Some(&temp), &source);
                } else {
                    self.emit_get_argument(Some(temp.clone()), i as i32);
                }
                if let Some(default_value) = &parameter.1 {
                    let skip_default_parameter_because_not_undefined = self.new_label();
                    let undefined_temp = self.new_temporary();
                    let is_undefined = self.emit_is_undefined(Some(undefined_temp), &temp);
                    self.emit_jump_if_false_raw(is_undefined.as_ref().unwrap(), &skip_default_parameter_because_not_undefined);
                    self.emit_node_expression(Some(temp.clone()), default_value);
                    self.emit_label(&skip_default_parameter_because_not_undefined);
                }

                parameter.0.bind_value(self, Some(temp.clone()));
            }

            if let Some(rest_parameter) = self.rest_parameter.clone() {
                rest_parameter.borrow().emit(self);
            }

            // Último ato de estranheza dos parâmetros default. Se um "var" tem o mesmo nome de um
            // parâmetro, ele deve começar com o valor do parâmetro. Note que serão ligações distintas.
            // Este é o passo 28 da seção 9.2.12.
            let var_entries: Vec<_> = function_node.borrow().var_declarations.iter().cloned().collect();
            for (key, value) in var_entries.iter() {
                if !value.is_var() {
                    // Este é um parâmetro ou o callee.
                    continue;
                }

                if parameter_set.contains(key) {
                    let ident = crate::runtime::identifier::Identifier::from_uid(&self.vm, Some(key));
                    let var = self.variable(&ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
                    let scope = self.emit_resolve_scope(None, &var);
                    let value_temp = self.new_temporary();
                    let value = self.emit_get_from_scope(
                        Some(value_temp),
                        scope,
                        &var,
                        crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                    );
                    values_to_move_into_vars.push((ident, value.expect("emitGetFromScope devolve dst")));
                }
            }

            // As funções com expressões de parâmetro default precisam de um registro de ambiente separado
            // para parâmetros e "var"s. O registro de ambiente "var" deve ter o de parâmetros como pai.
            // Veja o passo 28 da seção 9.2.12.
            let has_captured_variables = self.lexical_environment_register.is_some();
            self.initialize_var_lexical_environment(
                symbol_table_constant_index,
                function_symbol_table,
                has_captured_variables,
            );
        }

        // Isto completa o passo 28 da seção 9.2.12.
        for (ident, value) in values_to_move_into_vars.iter() {
            debug_assert!(!is_simple_parameter_list);
            let var = self.variable(ident, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
            let scope = self.emit_resolve_scope(None, &var);
            self.emit_put_to_scope(
                scope,
                &var,
                Some(value.clone()),
                crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                crate::runtime::get_put_info::InitializationMode::NotInitialization,
            );
        }
    }

    // BytecodeGenerator.cpp:1316
    pub fn needs_derived_constructor_in_arrow_function_lexical_environment(&mut self) -> bool {
        debug_assert!(
            self.code_block.is_class_context()
                || !(self.is_constructor() && self.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends)
        );
        self.code_block.is_class_context() && self.is_super_used_in_inner_arrow_function()
    }

    // BytecodeGenerator.cpp:1322
    pub fn initialize_arrow_function_context_scope_if_needed(
        &mut self,
        function_symbol_table: Option<&crate::runtime::symbol_table::SymbolTableRef>,
        can_reuse_lexical_environment: bool,
    ) {
        use crate::runtime::symbol_table::{SymbolTableEntry, NO_LOCKING_NECESSARY};
        use crate::runtime::var_offset::VarOffset;
        debug_assert!(self.arrow_function_context_lexical_environment_register.is_none());

        if can_reuse_lexical_environment && self.lexical_environment_register.is_some() {
            assert!(!self.code_block.is_arrow_function());
            let function_symbol_table = function_symbol_table.expect("RELEASE_ASSERT(functionSymbolTable)");

            self.arrow_function_context_lexical_environment_register = self.lexical_environment_register.clone();

            if self.is_this_used_in_inner_arrow_function() {
                let offset = function_symbol_table.borrow_mut().take_next_scope_offset_locked(NO_LOCKING_NECESSARY);
                function_symbol_table.borrow_mut().add_locked(
                    NO_LOCKING_NECESSARY,
                    self.property_names().builtin_names().this_private_name().impl_().unwrap(),
                    SymbolTableEntry::from_var_offset(VarOffset::from_scope_offset(offset)),
                );
            }

            if self.code_type == crate::bytecode::code_type::CodeType::FunctionCode
                && self.is_new_target_used_in_inner_arrow_function()
            {
                let offset = function_symbol_table.borrow_mut().take_next_scope_offset_locked(NO_LOCKING_NECESSARY);
                function_symbol_table.borrow_mut().add_locked(
                    NO_LOCKING_NECESSARY,
                    self.property_names().builtin_names().new_target_local_private_name().impl_().unwrap(),
                    SymbolTableEntry::from_var_offset(VarOffset::from_scope_offset(offset)),
                );
            }

            if self.needs_derived_constructor_in_arrow_function_lexical_environment() {
                let offset = function_symbol_table.borrow_mut().take_next_scope_offset_locked(NO_LOCKING_NECESSARY);
                function_symbol_table.borrow_mut().add_locked(
                    NO_LOCKING_NECESSARY,
                    self.property_names().builtin_names().derived_constructor_private_name().impl_().unwrap(),
                    SymbolTableEntry::from_var_offset(VarOffset::from_scope_offset(offset)),
                );
            }

            return;
        }

        let mut environment = crate::parser::variable_environment::VariableEnvironment::default();

        if self.is_this_used_in_inner_arrow_function() {
            let entry = environment.add_identifier(&self.property_names().builtin_names().this_private_name());
            entry.value.set_is_captured();
            entry.value.set_is_let();
        }

        if self.code_type == crate::bytecode::code_type::CodeType::FunctionCode
            && self.is_new_target_used_in_inner_arrow_function()
        {
            let entry = environment.add_identifier(&self.property_names().builtin_names().new_target_local_private_name());
            entry.value.set_is_captured();
            entry.value.set_is_let();
        }

        if self.needs_derived_constructor_in_arrow_function_lexical_environment() {
            let entry = environment.add_identifier(&self.property_names().builtin_names().derived_constructor_private_name());
            entry.value.set_is_captured();
            entry.value.set_is_let();
        }

        if environment.size() > 0 {
            let size = self.lexical_scope_stack.len();
            self.push_lexical_scope_internal(
                &mut environment,
                crate::bytecompiler::bytecode_generator::TDZCheckOptimization::Optimize,
                crate::bytecompiler::bytecode_generator::NestedScopeType::IsNotNested,
                None,
                crate::bytecompiler::bytecode_generator::TDZRequirement::UnderTDZ,
                crate::bytecompiler::bytecode_generator::ScopeType::LetConstScope,
                crate::bytecompiler::bytecode_generator::ScopeRegisterType::Block,
            );

            debug_assert!(self.lexical_scope_stack.len() == size + 1);

            self.arrow_function_context_lexical_environment_register =
                self.lexical_scope_stack.last().and_then(|entry| entry.scope.clone());
        }
    }

    // BytecodeGenerator.cpp:1382
    pub fn initialize_next_parameter(&mut self) -> RegisterRef {
        let reg = crate::bytecode::virtual_register::virtual_register_for_argument_including_this(
            self.code_block.num_parameters() as i32,
            0,
        );
        // `m_parameters.grow(m_parameters.size() + 1)`: um `RegisterID` novo, de construção padrão.
        self.parameters.push(std::rc::Rc::new(std::cell::RefCell::new(
            crate::bytecompiler::register_id::RegisterID::default(),
        )));
        let parameter = self.register_for(reg);
        parameter.borrow_mut().set_index(reg);
        let num_parameters = self.code_block.num_parameters();
        self.code_block.set_num_parameters(num_parameters + 1);
        parameter
    }

    // BytecodeGenerator.cpp:1392
    pub fn initialize_parameters(&mut self, parameters: &crate::parser::nodes::FunctionParameters) {
        // Garante que o code block conhece todos os nossos parâmetros, e que os parâmetros que precisam
        // de desestruturação foram anotados.
        let this_parameter = self.initialize_next_parameter();
        let this_index = this_parameter.borrow().index();
        self.this_register
            .set_index(crate::bytecode::virtual_register::VirtualRegister::new(this_index)); // this

        let mut non_simple_arguments = false;
        for i in 0..parameters.patterns.len() {
            let parameter = &parameters.patterns[i];
            let pattern = parameter.0.clone();
            if let crate::parser::nodes::DestructuringPatternNode::RestParameter(rest) = &pattern {
                assert!(self.rest_parameter.is_none());
                self.rest_parameter = Some(rest.clone());
                non_simple_arguments = true;
                continue;
            }
            if parameter.1.is_some() {
                non_simple_arguments = true;
                continue;
            }
            if !non_simple_arguments {
                self.initialize_next_parameter();
            }
        }
    }

    // BytecodeGenerator.cpp:1417
    pub fn initialize_var_lexical_environment(
        &mut self,
        symbol_table_constant_index: i32,
        function_symbol_table: &crate::runtime::symbol_table::SymbolTableRef,
        has_captured_variables: bool,
    ) {
        if has_captured_variables {
            assert!(self.lexical_environment_register.is_some());
            let undefined_constant = self.add_constant_value(
                crate::runtime::js_value::JSValue::undefined(),
                crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
            );
            let scope_register = self.scope_register();
            let lexical_environment_register = self.lexical_environment_register.clone();
            crate::bytecode::bytecode_ops::OpCreateLexicalEnvironment::emit(
                self,
                lexical_environment_register.clone(),
                scope_register,
                crate::bytecode::virtual_register::VirtualRegister::new(symbol_table_constant_index),
                Some(undefined_constant),
            );

            let scope_register = self.scope_register();
            self.move_register(scope_register.as_ref(), lexical_environment_register.as_ref().unwrap());

            self.push_local_control_flow_scope();
        }
        let is_with_scope = false;
        self.lexical_scope_stack.push(crate::bytecompiler::bytecode_generator::LexicalScopeStackEntry {
            symbol_table: Some(function_symbol_table.clone()),
            scope: self.lexical_environment_register.clone(),
            is_with_scope,
            symbol_table_constant_index,
        });
        self.var_scope_lexical_scope_stack_index = Some(self.lexical_scope_stack.len() - 1);
    }

    // BytecodeGenerator.cpp:1432
    pub fn visible_name_for_parameter(
        &mut self,
        pattern: &crate::parser::nodes::DestructuringPatternNode,
    ) -> Option<crate::wtf::text::string_impl::UniquedKey> {
        if let crate::parser::nodes::DestructuringPatternNode::Binding(binding) = pattern {
            let ident = binding.borrow().bound_property.clone();
            // `m_functions.contains(ident.impl())`: `impl()` nulo nunca está no conjunto.
            let uid = ident.impl_();
            let in_functions = match &uid {
                Some(key) => self.functions.contains(key),
                None => false,
            };
            if !in_functions {
                return uid;
            }
        }
        None
    }

    // BytecodeGenerator.cpp:1442
    pub fn new_block_scope_variable(&mut self) -> RegisterRef {
        self.reclaim_free_registers();

        self.new_register()
    }

    // BytecodeGenerator.cpp:1449
    pub fn new_label_scope(&mut self, type_: LabelScopeType, name: Option<&Identifier>) -> LabelScopeRef {
        crate::wtf::vector::shrink_to_fit(&mut self.label_scopes);

        // Aloca um novo escopo de rótulo.
        let break_target = self.new_label();
        // Só os laços têm alvos de continue.
        let continue_target = if type_ == LabelScopeType::Loop { Some(self.new_label()) } else { None };
        let depth = self.label_scope_depth();
        self.label_scopes
            .push(Rc::new(LabelScope::new(type_, name.cloned(), depth, break_target, continue_target)));
        // `return m_labelScopes.last()`: o `Ref<LabelScope>` incrementa a contagem do escopo guardado.
        LabelScopeRef::new(self.label_scopes.last().unwrap())
    }

    // BytecodeGenerator.cpp:1458
    pub fn emit_enter(&mut self) {
        crate::bytecode::bytecode_ops::OpEnter::emit(self);

        if crate::runtime::options::Options::optimize_recursive_tail_calls() {
            // Devemos adicionar o fim do op_enter como possível alvo de salto, porque o parser de bytecode
            // pode decidir dividir seu basic block para ter para onde saltar caso haja uma tail-call
            // recursiva apontando para esta função.
            self.disable_peephole_optimization();
        }
    }

    // BytecodeGenerator.cpp:1470
    pub fn emit_loop_hint(&mut self) {
        crate::bytecode::bytecode_ops::OpLoopHint::emit(self);
        crate::bytecode::bytecode_ops::OpCheckTraps::emit(self);
    }

    // BytecodeGenerator.cpp:1476
    pub fn emit_jump(&mut self, target: &crate::bytecompiler::label::LabelRef) {
        let bound = target.bind_generator();
        crate::bytecode::bytecode_ops::OpJmp::emit(self, bound);
    }

    // BytecodeGenerator.cpp:1486
    pub fn rewind(&mut self) {
        debug_assert!(self.last_instruction.is_valid());
        self.disable_peephole_optimization();
        let last_instruction = self.last_instruction.clone();
        self.writer.rewind(&last_instruction);
    }

    /// Emite `emit` tratando `register` como o `RegisterID*` cru do C++ (contagem 0): solta a referência que o
    /// próprio `RegisterRef` segura e a devolve depois, antes de o `RegisterRef` cair. Sem isso o slot não é
    /// reaproveitado pelo próximo `newTemporary()` e a fusão `!cond->refCount()` nunca acontece.
    pub fn with_raw_register<R>(&mut self, register: Option<&crate::bytecompiler::register_id::RegisterIDRef>, emit: impl FnOnce(&mut Self) -> R) -> R {
        let released = register.map(std::rc::Rc::clone);
        if let Some(register) = &released {
            register.borrow_mut().enter_raw();
        }
        let result = emit(self);
        if let Some(register) = &released {
            register.borrow_mut().leave_raw();
        }
        result
    }

    /// `emitJumpIfTrue(emitXxx(newTemporary(), ...), label)`: o argumento é o `RegisterID*` cru (contagem 0),
    /// então a fusão do salto pode ocorrer. Acrescenta a soltura da referência que o `RegisterRef` segura.
    pub fn emit_jump_if_true_raw(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) {
        self.with_raw_register(Some(cond.get()), |generator| generator.emit_jump_if_true(cond, target))
    }

    /// Versão de `emit_jump_if_true_raw` para `emitJumpIfFalse`.
    pub fn emit_jump_if_false_raw(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) {
        self.with_raw_register(Some(cond.get()), |generator| generator.emit_jump_if_false(cond, target))
    }

    // BytecodeGenerator.cpp:1494
    pub fn fuse_compare_and_jump<BinOp: crate::bytecode::bytecode_ops_accessors::CompareOp, JmpOp: crate::bytecode::bytecode_ops_jumps::CompareJumpOp>(
        &mut self,
        cond: &RegisterRef,
        target: &crate::bytecompiler::label::LabelRef,
        swap_operands: bool,
    ) -> bool {
        debug_assert!(self.can_do_peephole_optimization());
        let mut binop = self.last_instruction.as_op::<BinOp>();
        let matches = {
            // `!cond->refCount()`: quem passa o `RegisterID*` cru do C++ solta a referência do próprio
            // `RegisterRef` antes (`with_raw_register`); um `RefPtr` vivo (o `temp` do `LogicalOpNode`) impede a fusão.
            let cond = cond.borrow();
            cond.index() == binop.dst().offset() && cond.is_temporary() && crate::wtf::ref_counted::RefCounted::ref_count(&*cond) == 0
        };
        if matches {
            self.rewind();

            if swap_operands {
                binop.swap_operands();
            }

            let bound = target.bind_generator();
            JmpOp::emit(self, binop.lhs(), binop.rhs(), bound);
            return true;
        }
        false
    }

    // BytecodeGenerator.cpp:1511
    pub fn fuse_test_and_jmp<UnaryOp: crate::bytecode::bytecode_ops_accessors::TestOp, JmpOp: crate::bytecode::bytecode_ops_jumps::TestJumpOp>(
        &mut self,
        cond: &RegisterRef,
        target: &crate::bytecompiler::label::LabelRef,
    ) -> bool {
        debug_assert!(self.can_do_peephole_optimization());
        let unop = self.last_instruction.as_op::<UnaryOp>();
        let matches = {
            // `!cond->refCount()`, como em `fuse_compare_and_jump`.
            let cond = cond.borrow();
            cond.index() == unop.dst().offset() && cond.is_temporary() && crate::wtf::ref_counted::RefCounted::ref_count(&*cond) == 0
        };
        if matches {
            self.rewind();

            let bound = target.bind_generator();
            JmpOp::emit(self, unop.operand(), bound);
            return true;
        }
        false
    }

    // BytecodeGenerator.cpp:1524
    pub fn emit_jump_if_true(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) {
        use crate::bytecode::bytecode_ops::*;
        use crate::bytecode::opcode::OpcodeID;
        if self.can_do_peephole_optimization() {
            let last = self.last_opcode_id;
            if last == OpcodeID::op_less {
                if self.fuse_compare_and_jump::<OpLess, OpJless>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_lesseq {
                if self.fuse_compare_and_jump::<OpLesseq, OpJlesseq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_greater {
                if self.fuse_compare_and_jump::<OpGreater, OpJgreater>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_greatereq {
                if self.fuse_compare_and_jump::<OpGreatereq, OpJgreatereq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_eq {
                if self.fuse_compare_and_jump::<OpEq, OpJeq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_stricteq {
                if self.fuse_compare_and_jump::<OpStricteq, OpJstricteq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_neq {
                if self.fuse_compare_and_jump::<OpNeq, OpJneq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_nstricteq {
                if self.fuse_compare_and_jump::<OpNstricteq, OpJnstricteq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_below {
                if self.fuse_compare_and_jump::<OpBelow, OpJbelow>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_beloweq {
                if self.fuse_compare_and_jump::<OpBeloweq, OpJbeloweq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_eq_null && target.is_forward() {
                if self.fuse_test_and_jmp::<OpEqNull, OpJeqNull>(cond, target) {
                    return;
                }
            } else if last == OpcodeID::op_neq_null && target.is_forward() {
                if self.fuse_test_and_jmp::<OpNeqNull, OpJneqNull>(cond, target) {
                    return;
                }
            } else if last == OpcodeID::op_is_undefined_or_null && target.is_forward() {
                if self.fuse_test_and_jmp::<OpIsUndefinedOrNull, OpJundefinedOrNull>(cond, target) {
                    return;
                }
            }
        }

        let bound = target.bind_generator();
        OpJtrue::emit(self, Some(cond.clone()), bound);
    }

    // BytecodeGenerator.cpp:1572
    pub fn emit_jump_if_false(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) {
        use crate::bytecode::bytecode_ops::*;
        use crate::bytecode::opcode::OpcodeID;
        if self.can_do_peephole_optimization() {
            let last = self.last_opcode_id;
            if last == OpcodeID::op_less && target.is_forward() {
                if self.fuse_compare_and_jump::<OpLess, OpJnless>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_lesseq && target.is_forward() {
                if self.fuse_compare_and_jump::<OpLesseq, OpJnlesseq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_greater && target.is_forward() {
                if self.fuse_compare_and_jump::<OpGreater, OpJngreater>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_greatereq && target.is_forward() {
                if self.fuse_compare_and_jump::<OpGreatereq, OpJngreatereq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_eq && target.is_forward() {
                if self.fuse_compare_and_jump::<OpEq, OpJneq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_stricteq && target.is_forward() {
                if self.fuse_compare_and_jump::<OpStricteq, OpJnstricteq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_neq && target.is_forward() {
                if self.fuse_compare_and_jump::<OpNeq, OpJeq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_nstricteq && target.is_forward() {
                if self.fuse_compare_and_jump::<OpNstricteq, OpJstricteq>(cond, target, false) {
                    return;
                }
            } else if last == OpcodeID::op_below && target.is_forward() {
                if self.fuse_compare_and_jump::<OpBelow, OpJbeloweq>(cond, target, true) {
                    return;
                }
            } else if last == OpcodeID::op_beloweq && target.is_forward() {
                if self.fuse_compare_and_jump::<OpBeloweq, OpJbelow>(cond, target, true) {
                    return;
                }
            } else if last == OpcodeID::op_not {
                if self.fuse_test_and_jmp::<OpNot, OpJtrue>(cond, target) {
                    return;
                }
            } else if last == OpcodeID::op_eq_null && target.is_forward() {
                if self.fuse_test_and_jmp::<OpEqNull, OpJneqNull>(cond, target) {
                    return;
                }
            } else if last == OpcodeID::op_neq_null && target.is_forward() {
                if self.fuse_test_and_jmp::<OpNeqNull, OpJeqNull>(cond, target) {
                    return;
                }
            } else if last == OpcodeID::op_is_undefined_or_null && target.is_forward() {
                if self.fuse_test_and_jmp::<OpIsUndefinedOrNull, OpJnundefinedOrNull>(cond, target) {
                    return;
                }
            }
        }

        let bound = target.bind_generator();
        OpJfalse::emit(self, Some(cond.clone()), bound);
    }

    // BytecodeGenerator.cpp:1623
    pub fn emit_jump_if_not_function_call(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) {
        let constant = self.move_link_time_constant(None, crate::bytecode::link_time_constant::LinkTimeConstant::CallFunction);
        let bound = target.bind_generator();
        crate::bytecode::bytecode_ops::OpJneqPtr::emit(self, Some(cond.clone()), constant, bound);
    }

    // BytecodeGenerator.cpp:1628
    pub fn emit_jump_if_not_function_apply(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) {
        let constant = self.move_link_time_constant(None, crate::bytecode::link_time_constant::LinkTimeConstant::ApplyFunction);
        let bound = target.bind_generator();
        crate::bytecode::bytecode_ops::OpJneqPtr::emit(self, Some(cond.clone()), constant, bound);
    }

    // BytecodeGenerator.cpp:1633
    pub fn emit_jump_if_not_eval_function(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) {
        let constant = self.move_link_time_constant(None, crate::bytecode::link_time_constant::LinkTimeConstant::EvalFunction);
        let bound = target.bind_generator();
        crate::bytecode::bytecode_ops::OpJneqPtr::emit(self, Some(cond.clone()), constant, bound);
    }

    // BytecodeGenerator.cpp:1638
    pub fn emit_jump_if_empty_property_name_enumerator(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) {
        let constant = self.move_link_time_constant(
            None,
            crate::bytecode::link_time_constant::LinkTimeConstant::EmptyPropertyNameEnumerator,
        );
        let bound = target.bind_generator();
        crate::bytecode::bytecode_ops::OpJeqPtr::emit(self, Some(cond.clone()), constant, bound);
    }

    // BytecodeGenerator.cpp:1643
    pub fn emit_jump_if_sentinel_string(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) {
        let constant = self.move_link_time_constant(None, crate::bytecode::link_time_constant::LinkTimeConstant::SentinelString);
        let bound = target.bind_generator();
        crate::bytecode::bytecode_ops::OpJeqPtr::emit(self, Some(cond.clone()), constant, bound);
    }

    // BytecodeGenerator.cpp:1648
    pub fn emit_wide_jump_if_not_function_has_own_property(&mut self, cond: &RegisterRef, target: &crate::bytecompiler::label::LabelRef) -> u32 {
        let constant = self.move_link_time_constant(
            None,
            crate::bytecode::link_time_constant::LinkTimeConstant::HasOwnPropertyFunction,
        );
        let bound = target.bind_generator();
        crate::bytecode::bytecode_ops::OpJneqPtr::emit_with_smallest_size_requirement(
            self,
            crate::bytecode::opcode_size::OpcodeSize::Wide32,
            Some(cond.clone()),
            constant,
            bound,
        );
        self.last_instruction.offset()
    }

    // BytecodeGenerator.cpp:1654
    pub fn record_has_own_property_in_for_in_loop(
        &mut self,
        context: &mut crate::bytecompiler::bytecode_generator::ForInContext,
        branch_offset: u32,
        generic_path: &crate::bytecompiler::label::LabelRef,
    ) {
        assert!(generic_path.borrow().is_bound());
        assert!(!generic_path.is_forward());
        context.add_has_own_property_jump(branch_offset, generic_path.borrow().location());
    }

    // BytecodeGenerator.cpp:1661
    pub fn has_constant(&self, ident: &Identifier) -> bool {
        self.identifier_map.contains_key(&ident.impl_())
    }

    // BytecodeGenerator.cpp:1667
    pub fn add_constant(&mut self, ident: &Identifier) -> u32 {
        let rep = ident.impl_();
        let next_index = self.code_block.number_of_identifiers();
        let (index, is_new_entry) = match self.identifier_map.get(&rep) {
            Some(index) => (*index, false),
            None => {
                self.identifier_map.insert(rep, next_index);
                (next_index, true)
            }
        };
        if is_new_entry {
            self.code_block.add_identifier(ident.clone());
        }

        index
    }

    // BytecodeGenerator.cpp:1678
    // Não podemos fazer hash de JSValue(), então um membro dedicado guarda o cache dele.
    pub fn add_constant_empty_value(&mut self) -> RegisterRef {
        if self.empty_value_register.is_none() {
            let index = self.add_constant_index();
            self.code_block.add_constant(crate::runtime::js_value::JSValue::empty());
            self.empty_value_register = Some(RegisterRef::new(&self.constant_pool_registers[index as usize]));
        }

        self.empty_value_register.clone().unwrap()
    }

    // BytecodeGenerator.cpp:1689
    pub fn add_constant_value(
        &mut self,
        v: crate::runtime::js_value::JSValue,
        source_code_representation: crate::runtime::js_cjs_value_types::SourceCodeRepresentation,
    ) -> RegisterRef {
        use crate::runtime::js_cjs_value_types::SourceCodeRepresentation;
        let mut v = v;
        if v.is_empty() {
            return self.add_constant_empty_value();
        }

        let mut index = self.next_constant_offset as u32;

        if source_code_representation == SourceCodeRepresentation::Double && v.is_int32() {
            v = crate::runtime::js_value::JSValue::double_number(v.as_number());
        }
        // Um NaN que o parser dobrou (0 / 0) tem os bits que a aritmética desta CPU produz (o sinal difere
        // entre x86 e ARM); a constante, e portanto o bytecode, não deve depender disso.
        if v.is_double() && v.as_double().is_nan() {
            v = crate::runtime::js_value::JSValue::nan();
        }
        let value_map_key = (v.encode(), source_code_representation);
        match self.js_value_map.get(&value_map_key) {
            Some(existing) => index = *existing,
            None => {
                self.js_value_map.insert(value_map_key, self.next_constant_offset as u32);
                self.add_constant_index();
                self.code_block.add_constant_with_representation(v, source_code_representation);
            }
        }
        RegisterRef::new(&self.constant_pool_registers[index as usize])
    }

    // BytecodeGenerator.cpp:1712
    pub fn move_link_time_constant(
        &mut self,
        dst: Option<RegisterRef>,
        type_: crate::bytecode::link_time_constant::LinkTimeConstant,
    ) -> Option<RegisterRef> {
        let constant = match self.link_time_constant_registers.get(&type_) {
            Some(existing) => existing.clone(),
            None => {
                let index = self.add_constant_index();
                self.code_block.add_link_time_constant(type_);
                let register = RegisterRef::new(&self.constant_pool_registers[index as usize]);
                self.link_time_constant_registers.insert(type_, register.clone());
                register
            }
        };
        let Some(dst) = dst else {
            return Some(constant);
        };

        self.move_register(Some(&dst), &constant)
    }

    // BytecodeGenerator.cpp:1725
    pub fn move_empty_value(&mut self, dst: Option<RegisterRef>) -> Option<RegisterRef> {
        let empty_value = self.add_constant_empty_value();

        self.move_register(dst.as_ref(), &empty_value)
    }

    // BytecodeGenerator.cpp:1732
    pub fn emit_move(&mut self, dst: &RegisterRef, src: &RegisterRef) -> Option<RegisterRef> {
        self.static_property_analyzer.mov(&dst.borrow(), &src.borrow());
        if self.can_do_peephole_optimization() && self.last_instruction.is_op::<crate::bytecode::bytecode_ops::OpMov>() {
            let op = self.last_instruction.as_op::<crate::bytecode::bytecode_ops::OpMov>();
            if op.dst() == dst.borrow().virtual_register() {
                self.rewind();
            }
        }
        crate::bytecode::bytecode_ops::OpMov::emit(self, Some(dst.clone()), Some(src.clone()));

        Some(dst.clone())
    }

    // BytecodeGenerator.cpp:1746
    // Tratamos `typeof x > "u"` como `typeof x === "undefined"`.
    pub fn try_emit_typeof_is_undefined_for_string_comparison<const IS_NOT_TYPEOF_UNDEFINED: bool>(
        &mut self,
        dst: Option<RegisterRef>,
        src1: &RegisterRef,
        src2: &RegisterRef,
    ) -> bool {
        use crate::bytecode::bytecode_ops::{OpNot, OpTypeof, OpTypeofIsUndefined};
        if !self.can_do_peephole_optimization() || !self.last_instruction.is_op::<OpTypeof>() {
            return false;
        }

        let op = self.last_instruction.as_op::<OpTypeof>();
        if src1.borrow().virtual_register() != op.dst()
            || !src1.borrow().is_temporary()
            || !src2.borrow().virtual_register().is_constant()
            || !self.code_block.constant_register(src2.borrow().virtual_register()).is_string()
        {
            return false;
        }

        let value = self
            .code_block
            .constant_register(src2.borrow().virtual_register())
            .as_js_string()
            .try_get_value();
        if value != crate::wtf::text::wtf_string::String::from_latin1(b"u") {
            return false;
        }

        self.rewind();
        if IS_NOT_TYPEOF_UNDEFINED {
            let temp = self.new_temporary();
            OpTypeofIsUndefined::emit(self, Some(temp.clone()), op.value());
            self.emit_unary_op::<OpNot>(dst, Some(temp));
        } else {
            OpTypeofIsUndefined::emit(self, dst, op.value());
        }
        true
    }

    // BytecodeGenerator.cpp:1773
    pub fn emit_unary_op_dynamic(
        &mut self,
        opcode_id: crate::bytecode::opcode::OpcodeID,
        dst: Option<RegisterRef>,
        src: Option<RegisterRef>,
        type_: crate::parser::result_type::ResultType,
    ) -> Option<RegisterRef> {
        use crate::bytecode::bytecode_ops::*;
        use crate::bytecode::opcode::OpcodeID;
        match opcode_id {
            OpcodeID::op_not => {
                self.emit_unary_op::<OpNot>(dst.clone(), src);
            }
            OpcodeID::op_negate => {
                let profile = self.code_block.add_unary_arith_profile();
                OpNegate::emit(self, dst.clone(), src, profile, type_);
            }
            OpcodeID::op_bitnot => {
                let profile = self.code_block.add_unary_arith_profile();
                OpBitnot::emit(self, dst.clone(), src, profile);
            }
            OpcodeID::op_to_number => {
                let profile = self.code_block.add_unary_arith_profile();
                OpToNumber::emit(self, dst.clone(), src, profile);
            }
            OpcodeID::op_to_numeric => {
                let profile = self.code_block.add_unary_arith_profile();
                OpToNumeric::emit(self, dst.clone(), src, profile);
            }
            _ => unreachable!("ASSERT_NOT_REACHED"),
        }
        dst
    }

    // BytecodeGenerator.cpp:1797
    pub fn emit_binary_op_dynamic(
        &mut self,
        opcode_id: crate::bytecode::opcode::OpcodeID,
        dst: Option<RegisterRef>,
        src1: Option<RegisterRef>,
        src2: Option<RegisterRef>,
        types: crate::parser::result_type::OperandTypes,
    ) -> Option<RegisterRef> {
        use crate::bytecode::bytecode_ops::*;
        use crate::bytecode::opcode::OpcodeID;
        match opcode_id {
            OpcodeID::op_eq => self.emit_binary_op::<OpEq>(dst, src1, src2, types),
            OpcodeID::op_neq => self.emit_binary_op::<OpNeq>(dst, src1, src2, types),
            OpcodeID::op_stricteq => self.emit_binary_op::<OpStricteq>(dst, src1, src2, types),
            OpcodeID::op_nstricteq => self.emit_binary_op::<OpNstricteq>(dst, src1, src2, types),
            OpcodeID::op_less => {
                if self.try_emit_typeof_is_undefined_for_string_comparison::<true>(
                    dst.clone(),
                    src1.as_ref().unwrap(),
                    src2.as_ref().unwrap(),
                ) {
                    return dst;
                }
                self.emit_binary_op::<OpLess>(dst, src1, src2, types)
            }
            OpcodeID::op_lesseq => self.emit_binary_op::<OpLesseq>(dst, src1, src2, types),
            OpcodeID::op_greater => {
                if self.try_emit_typeof_is_undefined_for_string_comparison::<false>(
                    dst.clone(),
                    src1.as_ref().unwrap(),
                    src2.as_ref().unwrap(),
                ) {
                    return dst;
                }
                self.emit_binary_op::<OpGreater>(dst, src1, src2, types)
            }
            OpcodeID::op_greatereq => self.emit_binary_op::<OpGreatereq>(dst, src1, src2, types),
            OpcodeID::op_below => self.emit_binary_op::<OpBelow>(dst, src1, src2, types),
            OpcodeID::op_beloweq => self.emit_binary_op::<OpBeloweq>(dst, src1, src2, types),
            OpcodeID::op_mod => self.emit_binary_op::<OpMod>(dst, src1, src2, types),
            OpcodeID::op_pow => self.emit_binary_op::<OpPow>(dst, src1, src2, types),
            OpcodeID::op_lshift => self.emit_binary_op::<OpLshift>(dst, src1, src2, types),
            OpcodeID::op_rshift => self.emit_binary_op::<OpRshift>(dst, src1, src2, types),
            OpcodeID::op_urshift => self.emit_binary_op::<OpUrshift>(dst, src1, src2, types),
            OpcodeID::op_add => {
                let is_constant_empty_string = |generator: &BytecodeGenerator, src: &RegisterRef| -> bool {
                    let virtual_register = src.borrow().virtual_register();
                    if !virtual_register.is_constant() {
                        return false;
                    }
                    if !generator.code_block.constant_register(virtual_register).is_string() {
                        return false;
                    }
                    let value = generator.code_block.constant_register(virtual_register).as_js_string().try_get_value();
                    value.is_empty()
                };

                if is_constant_empty_string(self, src1.as_ref().unwrap()) {
                    self.emit_to_primitive(dst.clone(), src2);
                    return self.emit_to_string(dst.clone(), dst);
                }

                if is_constant_empty_string(self, src2.as_ref().unwrap()) {
                    self.emit_to_primitive(dst.clone(), src1);
                    return self.emit_to_string(dst.clone(), dst);
                }

                self.emit_binary_op::<OpAdd>(dst, src1, src2, types)
            }
            OpcodeID::op_mul => self.emit_binary_op::<OpMul>(dst, src1, src2, types),
            OpcodeID::op_div => self.emit_binary_op::<OpDiv>(dst, src1, src2, types),
            OpcodeID::op_sub => self.emit_binary_op::<OpSub>(dst, src1, src2, types),
            OpcodeID::op_bitand => self.emit_binary_op::<OpBitand>(dst, src1, src2, types),
            OpcodeID::op_bitxor => self.emit_binary_op::<OpBitxor>(dst, src1, src2, types),
            OpcodeID::op_bitor => self.emit_binary_op::<OpBitor>(dst, src1, src2, types),
            _ => unreachable!("ASSERT_NOT_REACHED"),
        }
    }

    // BytecodeGenerator.cpp:1876
    pub fn emit_to_object(&mut self, dst: Option<RegisterRef>, src: Option<RegisterRef>, message: &Identifier) -> Option<RegisterRef> {
        let message_index = self.add_constant(message);
        let value_profile_index = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpToObject::emit(self, dst.clone(), src, message_index, value_profile_index);
        dst
    }

    // BytecodeGenerator.cpp:1882
    pub fn emit_to_number(&mut self, dst: Option<RegisterRef>, src: Option<RegisterRef>) -> Option<RegisterRef> {
        let profile = self.code_block.add_unary_arith_profile();
        crate::bytecode::bytecode_ops::OpToNumber::emit(self, dst.clone(), src, profile);
        dst
    }

    // BytecodeGenerator.cpp:1888
    pub fn emit_to_numeric(&mut self, dst: Option<RegisterRef>, src: Option<RegisterRef>) -> Option<RegisterRef> {
        let profile = self.code_block.add_unary_arith_profile();
        crate::bytecode::bytecode_ops::OpToNumeric::emit(self, dst.clone(), src, profile);
        dst
    }

    // BytecodeGenerator.cpp:1894
    pub fn emit_to_string(&mut self, dst: Option<RegisterRef>, src: Option<RegisterRef>) -> Option<RegisterRef> {
        self.emit_unary_op::<crate::bytecode::bytecode_ops::OpToString>(dst, src)
    }

    // BytecodeGenerator.cpp:1899
    pub fn emit_type_of(&mut self, dst: Option<RegisterRef>, src: Option<RegisterRef>) -> Option<RegisterRef> {
        self.emit_unary_op::<crate::bytecode::bytecode_ops::OpTypeof>(dst, src)
    }

    // BytecodeGenerator.cpp:1904
    pub fn emit_inc(&mut self, src_dst: &RegisterRef) -> Option<RegisterRef> {
        let profile = self.code_block.add_unary_arith_profile();
        crate::bytecode::bytecode_ops::OpInc::emit(self, Some(src_dst.clone()), profile);
        Some(src_dst.clone())
    }

    // BytecodeGenerator.cpp:1910
    pub fn emit_dec(&mut self, src_dst: &RegisterRef) -> Option<RegisterRef> {
        let profile = self.code_block.add_unary_arith_profile();
        crate::bytecode::bytecode_ops::OpDec::emit(self, Some(src_dst.clone()), profile);
        Some(src_dst.clone())
    }

    // BytecodeGenerator.cpp:1916
    pub fn emit_equality_op_impl(&mut self, dst: Option<RegisterRef>, src1: Option<RegisterRef>, src2: Option<RegisterRef>) -> bool {
        use crate::bytecode::bytecode_ops::*;
        use crate::runtime::js_type::JSType;
        if !self.can_do_peephole_optimization() {
            return false;
        }

        if self.last_instruction.is_op::<OpTypeof>() {
            let op = self.last_instruction.as_op::<OpTypeof>();
            let (src1, src2) = (src1.unwrap(), src2.unwrap());
            if src1.borrow().virtual_register() == op.dst()
                && src1.borrow().is_temporary()
                && src2.borrow().virtual_register().is_constant()
                && self.code_block.constant_register(src2.borrow().virtual_register()).is_string()
            {
                let value = self
                    .code_block
                    .constant_register(src2.borrow().virtual_register())
                    .as_js_string()
                    .try_get_value();
                let is = |text: &str| value == crate::wtf::text::wtf_string::String::from_latin1(text.as_bytes());
                if is("undefined") {
                    self.rewind();
                    OpTypeofIsUndefined::emit(self, dst, op.value());
                    return true;
                }
                if is("boolean") {
                    self.rewind();
                    OpIsBoolean::emit(self, dst, op.value());
                    return true;
                }
                if is("number") {
                    self.rewind();
                    OpIsNumber::emit(self, dst, op.value());
                    return true;
                }
                if is("string") {
                    self.rewind();
                    OpIsCellWithType::emit(self, dst, op.value(), JSType::StringType);
                    return true;
                }
                if is("symbol") {
                    self.rewind();
                    OpIsCellWithType::emit(self, dst, op.value(), JSType::SymbolType);
                    return true;
                }
                if is("bigint") {
                    self.rewind();
                    // USE(BIGINT32) vale em x86_64 Linux.
                    OpIsBigInt::emit(self, dst, op.value());
                    return true;
                }
                if is("object") {
                    self.rewind();
                    OpTypeofIsObject::emit(self, dst, op.value());
                    return true;
                }
                if is("function") {
                    self.rewind();
                    OpTypeofIsFunction::emit(self, dst, op.value());
                    return true;
                }
            }
        }

        false
    }

    // BytecodeGenerator.cpp:1978
    pub fn emit_type_profiler_expression_info(
        &mut self,
        start_divot: &crate::parser::parser_tokens::JSTextPosition,
        end_divot: &crate::parser::parser_tokens::JSTextPosition,
    ) {
        debug_assert!(self.should_emit_type_profiler_hooks());

        let start = start_divot.offset as u32; // Os intervalos incluem os extremos e começam em 0.
        let end = end_divot.offset as u32 - 1; // O fim já passa um do intervalo inclusivo, então subtrai 1.
        let instruction_offset = self.instructions().position() - 1;
        self.code_block.add_type_profiler_expression_info(instruction_offset, start, end);
    }

    // BytecodeGenerator.cpp:1988
    pub fn emit_profile_type_flag(&mut self, register_to_profile: Option<RegisterRef>, flag: crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag) {
        if !self.should_emit_type_profiler_hooks() {
            return;
        }

        let Some(register_to_profile) = register_to_profile else {
            return;
        };

        let resolve_type = self.resolve_type();
        crate::bytecode::bytecode_ops::OpProfileType::emit(
            self,
            Some(register_to_profile),
            crate::bytecode::bytecode_ops::SymbolTableOrScopeDepth::default(),
            flag,
            0,
            resolve_type,
        );

        // Não emite expression info nesta versão de profile type. Em geral isso significa que estamos
        // perfilando algo que não está no texto do programa JavaScript. Por exemplo, o return undefined
        // implícito de uma chamada de função.
    }
}
