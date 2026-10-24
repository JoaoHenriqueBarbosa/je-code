// Parte 3 de bytecompiler/BytecodeGenerator.cpp (linhas 2003 a 3003 do .cpp). Juntada por include!.
// Convenção: `Option<RegisterRef>` é o `RegisterID*` anulável do C++, com
// `RegisterRef = Rc<RefCell<RegisterID>>`. Os overloads do C++ ganham sufixo pelo que os distingue
// (os mesmos nomes que a parte 2 deixou nas declarações).

impl BytecodeGenerator {
    // BytecodeGenerator.cpp:2003
    pub fn emit_profile_type_divots(
        &mut self,
        register_to_profile: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        start_divot: &crate::parser::parser_tokens::JSTextPosition,
        end_divot: &crate::parser::parser_tokens::JSTextPosition,
    ) {
        self.emit_profile_type_flag_divots(
            register_to_profile,
            crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag::ProfileTypeBytecodeDoesNotHaveGlobalID,
            start_divot,
            end_divot,
        );
    }

    // BytecodeGenerator.cpp:2008
    pub fn emit_profile_type_flag_divots(
        &mut self,
        register_to_profile: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        flag: crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag,
        start_divot: &crate::parser::parser_tokens::JSTextPosition,
        end_divot: &crate::parser::parser_tokens::JSTextPosition,
    ) {
        if !self.should_emit_type_profiler_hooks() {
            return;
        }

        let Some(register_to_profile) = register_to_profile else {
            return;
        };

        let resolve_type = self.resolve_type();
        crate::bytecode::bytecode_ops::OpProfileType::emit(
            self,
            &register_to_profile,
            crate::bytecode::bytecode_ops::SymbolTableOrScopeDepth::default(),
            flag,
            0,
            resolve_type,
        );
        self.emit_type_profiler_expression_info(start_divot, end_divot);
    }

    // BytecodeGenerator.cpp:2020
    pub fn emit_profile_type_variable(
        &mut self,
        register_to_profile: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        var: &crate::bytecompiler::bytecode_generator::Variable,
        start_divot: &crate::parser::parser_tokens::JSTextPosition,
        end_divot: &crate::parser::parser_tokens::JSTextPosition,
    ) {
        if !self.should_emit_type_profiler_hooks() {
            return;
        }

        let Some(register_to_profile) = register_to_profile else {
            return;
        };

        let flag;
        let symbol_table_or_scope_depth;
        if var.local().is_some() || var.offset().is_scope() {
            flag = crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag::ProfileTypeBytecodeLocallyResolved;
            symbol_table_or_scope_depth = crate::bytecode::bytecode_ops::SymbolTableOrScopeDepth::symbol_table(
                crate::bytecode::virtual_register::VirtualRegister::new(var.symbol_table_constant_index()),
            );
        } else {
            flag = crate::bytecompiler::profile_type_bytecode_flag::ProfileTypeBytecodeFlag::ProfileTypeBytecodeClosureVar;
            symbol_table_or_scope_depth =
                crate::bytecode::bytecode_ops::SymbolTableOrScopeDepth::scope_depth(self.local_scope_depth());
        }

        let ident_constant = self.add_constant(var.ident());
        let resolve_type = self.resolve_type();
        crate::bytecode::bytecode_ops::OpProfileType::emit(
            self,
            &register_to_profile,
            symbol_table_or_scope_depth,
            flag,
            ident_constant,
            resolve_type,
        );
        self.emit_type_profiler_expression_info(start_divot, end_divot);
    }

    // BytecodeGenerator.cpp:2043
    pub fn emit_profile_control_flow(&mut self, text_offset: i32) {
        if self.should_emit_control_flow_profiler_hooks() {
            assert!(text_offset >= 0);

            crate::bytecode::bytecode_ops::OpProfileControlFlow::emit(self, text_offset);
            let offset = self.last_instruction.offset();
            self.code_block.add_op_profile_control_flow_bytecode_offset(offset);
        }
    }

    // BytecodeGenerator.cpp:2053
    pub fn add_constant_index(&mut self) -> u32 {
        let index = self.next_constant_offset;
        self.constant_pool_registers.push(std::rc::Rc::new(std::cell::RefCell::new(
            crate::bytecompiler::register_id::RegisterID::from_index(
                crate::bytecode::virtual_register::FIRST_CONSTANT_REGISTER_INDEX + self.next_constant_offset,
            ),
        )));
        self.next_constant_offset += 1;
        index as u32
    }

    // BytecodeGenerator.cpp:2061
    pub fn emit_load_bool(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        b: bool,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_load_js_value(dst, crate::runtime::js_value::js_boolean(b))
    }

    // BytecodeGenerator.cpp:2066
    pub fn emit_load_identifier(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        identifier: &crate::runtime::identifier::Identifier,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let key = identifier.impl_();
        let string_in_map = match self.string_map.get(&key) {
            Some(existing) => existing.clone(),
            None => {
                let created = crate::runtime::js_string::js_owned_string(&self.vm, identifier.string().string());
                self.string_map.insert(key, created.clone());
                created
            }
        };

        self.emit_load_js_value(dst, crate::runtime::js_value::JSValue::from_js_string(string_in_map))
    }

    // Wrapper do argumento padrão `SourceCodeRepresentation::Other` das declarações de emitLoad.
    pub fn emit_load_js_value(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        v: crate::runtime::js_value::JSValue,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_load_js_value_with_representation(dst, v, crate::parser::parser_tokens::SourceCodeRepresentation::Other)
    }

    // BytecodeGenerator.cpp:2076
    pub fn emit_load_js_value_with_representation(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        v: crate::runtime::js_value::JSValue,
        source_code_representation: crate::parser::parser_tokens::SourceCodeRepresentation,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let constant_id = self.add_constant_value(v, source_code_representation);
        if dst.is_some() {
            return self.move_register(dst.as_ref(), &constant_id);
        }
        Some(constant_id)
    }

    // BytecodeGenerator.cpp:2084
    pub fn emit_load_excluded_list(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        set: crate::parser::parser::IdentifierSet,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let set_index = self.code_block.add_set_constant(set);
        self.emit_load_js_value(dst, crate::runtime::js_value::JSValue::from_u32(set_index))
    }

    // BytecodeGenerator.cpp:2091
    pub fn instantiate_lexical_variables<LookUpVarKindFunctor>(
        &mut self,
        lexical_variables: &crate::parser::variable_environment::VariableEnvironment,
        scope_type: ScopeType,
        symbol_table: &crate::runtime::symbol_table::SymbolTableRef,
        scope_register_type: ScopeRegisterType,
        mut look_up_var_kind: LookUpVarKindFunctor,
    ) -> bool
    where
        LookUpVarKindFunctor: FnMut(
            &crate::wtf::text::string_impl::UniquedKey,
            &crate::parser::variable_environment::VariableEnvironmentEntry,
        ) -> crate::runtime::var_offset::VarKind,
    {
        let mut has_captured_variables = false;
        {
            let has_private_names = lexical_variables.private_names_size() != 0;
            // Precisamos garantir que os offsets de @privateClassBrand e @privateBrand sejam 0 e 1.
            // Para isso, primeiro os definimos e depois os filtramos de lexicalVariables.
            if scope_type == ScopeType::ClassScope && has_private_names {
                has_captured_variables = true;
                let private_class_brand_offset = crate::runtime::var_offset::VarOffset::from_scope_offset(
                    symbol_table
                        .borrow_mut()
                        .take_next_scope_offset_locked(crate::runtime::symbol_table::NO_LOCKING_NECESSARY),
                );
                assert!(
                    private_class_brand_offset.raw_offset()
                        == crate::parser::variable_environment::PrivateNameEntry::PRIVATE_CLASS_BRAND_OFFSET
                );
                symbol_table.borrow_mut().add_locked(
                    crate::runtime::symbol_table::NO_LOCKING_NECESSARY,
                    self.property_names().builtin_names().private_class_brand_private_name().impl_().unwrap(),
                    crate::runtime::symbol_table::SymbolTableEntry::new(
                        private_class_brand_offset,
                        crate::runtime::property_attribute::PropertyAttribute::ReadOnly as u32,
                    ),
                );

                let private_brand_offset = crate::runtime::var_offset::VarOffset::from_scope_offset(
                    symbol_table
                        .borrow_mut()
                        .take_next_scope_offset_locked(crate::runtime::symbol_table::NO_LOCKING_NECESSARY),
                );
                assert!(
                    private_brand_offset.raw_offset()
                        == crate::parser::variable_environment::PrivateNameEntry::PRIVATE_BRAND_OFFSET
                );
                symbol_table.borrow_mut().add_locked(
                    crate::runtime::symbol_table::NO_LOCKING_NECESSARY,
                    self.property_names().builtin_names().private_brand_private_name().impl_().unwrap(),
                    crate::runtime::symbol_table::SymbolTableEntry::new(
                        private_brand_offset,
                        crate::runtime::property_attribute::PropertyAttribute::ReadOnly as u32,
                    ),
                );
            }

            for entry in lexical_variables.iter() {
                let key = &entry.0;
                if scope_type == ScopeType::ClassScope && has_private_names {
                    if Some(key) == self.property_names().builtin_names().private_class_brand_private_name().impl_().as_ref() {
                        continue;
                    }
                    if Some(key) == self.property_names().builtin_names().private_brand_private_name().impl_().as_ref() {
                        continue;
                    }
                }

                // Bindings importados que não são o namespace não são alocados no ambiente do módulo
                // como as variáveis comuns. Esses tipos de variável só aparecem no ambiente do módulo,
                // então os outros ambientes léxicos não precisam cuidar disso.
                if entry.1.is_imported() && !entry.1.is_imported_namespace() {
                    continue;
                }

                let var_kind = look_up_var_kind(key, &entry.1);
                let var_offset;
                if var_kind == crate::runtime::var_offset::VarKind::Scope {
                    var_offset = crate::runtime::var_offset::VarOffset::from_scope_offset(
                        symbol_table
                            .borrow_mut()
                            .take_next_scope_offset_locked(crate::runtime::symbol_table::NO_LOCKING_NECESSARY),
                    );
                    has_captured_variables = true;
                } else {
                    let local_register: crate::bytecode::virtual_register::VirtualRegister;
                    if scope_register_type == ScopeRegisterType::Block {
                        let local = self.new_block_scope_variable();
                        local.borrow_mut().ref_();
                        local_register = local.borrow().virtual_register();
                    } else {
                        local_register = self.add_var().borrow().virtual_register();
                    }
                    var_offset = crate::runtime::var_offset::VarOffset::from_virtual_register(local_register);
                }

                let new_entry = crate::runtime::symbol_table::SymbolTableEntry::new(
                    var_offset,
                    if entry.1.is_const() {
                        crate::runtime::property_attribute::PropertyAttribute::ReadOnly as u32
                    } else {
                        crate::runtime::property_attribute::PropertyAttribute::None as u32
                    },
                );
                symbol_table
                    .borrow_mut()
                    .add_locked(crate::runtime::symbol_table::NO_LOCKING_NECESSARY, key.clone(), new_entry);

                // FIXME: só fazer isso se houver um eval() dentro de um escopo aninhado, senão não é
                // necessário. https://bugs.webkit.org/show_bug.cgi?id=206663

                let Some(private_environment) = lexical_variables.private_name_environment() else {
                    continue;
                };

                let Some(find_result) = private_environment.find(key) else {
                    continue;
                };

                symbol_table.borrow_mut().add_private_name(key.clone(), *find_result);
            }
        }
        has_captured_variables
    }

    // BytecodeGenerator.cpp:2181
    pub fn emit_prefill_stack_tdz_variables(
        &mut self,
        lexical_variables: &crate::parser::variable_environment::VariableEnvironment,
        symbol_table: &crate::runtime::symbol_table::SymbolTableRef,
    ) {
        // Preenche as variáveis de pilha com o valor vazio do TDZ. As variáveis de escopo são
        // inicializadas com o valor vazio do TDZ quando o JSLexicalEnvironment é alocado.
        for entry in lexical_variables.iter() {
            // Bindings importados que não são o namespace não são alocados no ambiente do módulo
            // como as variáveis comuns (ver instantiate_lexical_variables).
            if entry.1.is_imported() && !entry.1.is_imported_namespace() {
                continue;
            }

            if entry.1.is_function() {
                continue;
            }

            let symbol_table_entry = symbol_table.borrow().get(&entry.0);
            assert!(!symbol_table_entry.is_null());
            let offset = symbol_table_entry.var_offset();
            if offset.is_scope() {
                continue;
            }

            let register = self.register_for(offset.stack_offset());
            self.move_empty_value(Some(register));
        }
    }

    // BytecodeGenerator.cpp:2207
    pub fn push_lexical_scope(
        &mut self,
        node: &crate::parser::nodes::VariableEnvironmentNode,
        scope_type: ScopeType,
        tdz_check_optimization: TDZCheckOptimization,
        nested_scope_type: NestedScopeType,
        constant_symbol_table_result: Option<&mut Option<crate::bytecompiler::bytecode_generator::RegisterRef>>,
        should_initialize_block_scoped_functions: bool,
    ) {
        // O nó é compartilhado e imutável aqui, e o C++ marca todas as variáveis como capturadas no
        // próprio ambiente do nó; a marcação é idempotente e refeita no pop, então trabalhamos numa cópia.
        let mut environment = node.lexical_variables.clone();
        let mut constant_symbol_table_result_temp: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
        self.push_lexical_scope_internal(
            &mut environment,
            tdz_check_optimization,
            nested_scope_type,
            Some(&mut constant_symbol_table_result_temp),
            TDZRequirement::UnderTDZ,
            scope_type,
            ScopeRegisterType::Block,
        );

        if should_initialize_block_scoped_functions {
            self.initialize_block_scoped_functions(
                &mut environment,
                &node.function_stack,
                constant_symbol_table_result_temp.clone(),
            );
        }

        if let Some(result) = constant_symbol_table_result {
            if constant_symbol_table_result_temp.is_some() {
                *result = constant_symbol_table_result_temp;
            }
        }
    }

    // BytecodeGenerator.cpp:2220
    pub fn push_class_head_lexical_scope(&mut self, environment: &mut crate::parser::variable_environment::VariableEnvironment) {
        self.push_lexical_scope_internal(
            environment,
            TDZCheckOptimization::Optimize,
            NestedScopeType::IsNested,
            None,
            TDZRequirement::UnderTDZ,
            ScopeType::LetConstScope,
            ScopeRegisterType::Block,
        );
    }

    // BytecodeGenerator.cpp:2230
    pub fn push_lexical_scope_internal(
        &mut self,
        environment: &mut crate::parser::variable_environment::VariableEnvironment,
        tdz_check_optimization: TDZCheckOptimization,
        nested_scope_type: NestedScopeType,
        constant_symbol_table_result: Option<&mut Option<crate::bytecompiler::bytecode_generator::RegisterRef>>,
        tdz_requirement: TDZRequirement,
        scope_type: ScopeType,
        scope_register_type: ScopeRegisterType,
    ) {
        if environment.size() == 0 {
            return;
        }

        if self.should_emit_debug_hooks() {
            environment.mark_all_variables_as_captured();
        }

        let symbol_table = crate::runtime::symbol_table::SymbolTable::create(&self.vm);
        match scope_type {
            ScopeType::CatchScope => {
                symbol_table.borrow_mut().set_scope_type(crate::runtime::symbol_table::SymbolTableScopeType::CatchScope)
            }
            ScopeType::CatchScopeWithSimpleParameter => symbol_table
                .borrow_mut()
                .set_scope_type(crate::runtime::symbol_table::SymbolTableScopeType::CatchScopeWithSimpleParameter),
            ScopeType::LetConstScope | ScopeType::ClassScope => {
                symbol_table.borrow_mut().set_scope_type(crate::runtime::symbol_table::SymbolTableScopeType::LexicalScope)
            }
            ScopeType::FunctionNameScope => symbol_table
                .borrow_mut()
                .set_scope_type(crate::runtime::symbol_table::SymbolTableScopeType::FunctionNameScope),
        }

        if nested_scope_type == NestedScopeType::IsNested {
            symbol_table.borrow_mut().mark_is_nested_lexical_scope();
        }

        let look_up_var_kind = |_: &crate::wtf::text::string_impl::UniquedKey,
                                entry: &crate::parser::variable_environment::VariableEnvironmentEntry|
         -> crate::runtime::var_offset::VarKind {
            if entry.is_captured() {
                crate::runtime::var_offset::VarKind::Scope
            } else {
                crate::runtime::var_offset::VarKind::Stack
            }
        };

        let has_captured_variables =
            self.instantiate_lexical_variables(environment, scope_type, &symbol_table, scope_register_type, look_up_var_kind);

        let mut new_scope: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
        let mut constant_symbol_table: Option<crate::bytecompiler::bytecode_generator::RegisterRef> = None;
        let mut symbol_table_constant_index: i32 = 0;
        if self.should_emit_type_profiler_hooks() {
            let constant = self.add_constant_value(
                crate::runtime::js_value::JSValue::from_cell(symbol_table.borrow().cell_id()),
                crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
            );
            symbol_table_constant_index = constant.borrow().index();
            constant_symbol_table = Some(constant);
        }
        if has_captured_variables {
            let scope = if scope_register_type == ScopeRegisterType::Block {
                let scope = self.new_block_scope_variable();
                scope.borrow_mut().ref_();
                scope
            } else {
                let var = self.add_var();
                var
            };
            new_scope = Some(scope.clone());
            if constant_symbol_table.is_none() {
                let cloned = symbol_table.borrow().clone_scope_part(
                    &self.vm,
                    crate::runtime::symbol_table::PropagateCloneInvalidationToOriginal::No,
                );
                let constant = self.add_constant_value(
                    crate::runtime::js_value::JSValue::from_cell(cloned.borrow().cell_id()),
                    crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
                );
                symbol_table_constant_index = constant.borrow().index();
                constant_symbol_table = Some(constant);
            }
            if let Some(result) = constant_symbol_table_result {
                *result = constant_symbol_table.clone();
            }

            let tdz_or_undefined = if tdz_requirement == TDZRequirement::UnderTDZ {
                crate::runtime::js_value::js_tdz_value()
            } else {
                crate::runtime::js_value::js_undefined()
            };
            let tdz_constant = self.add_constant_value(tdz_or_undefined, crate::parser::parser_tokens::SourceCodeRepresentation::Other);
            let scope_register = self.scope_register();
            crate::bytecode::bytecode_ops::OpCreateLexicalEnvironment::emit(
                self,
                &scope,
                scope_register.as_ref(),
                crate::bytecode::virtual_register::VirtualRegister::new(symbol_table_constant_index),
                &tdz_constant,
            );

            self.move_register(scope_register.as_ref(), &scope);

            self.push_local_control_flow_scope();
        }

        let is_with_scope = false;
        self.lexical_scope_stack.push(LexicalScopeStackEntry {
            symbol_table: Some(symbol_table.clone()),
            scope: new_scope,
            is_with_scope,
            symbol_table_constant_index,
        });
        self.push_tdz_variables(environment, tdz_check_optimization, tdz_requirement);

        if tdz_requirement == TDZRequirement::UnderTDZ {
            self.emit_prefill_stack_tdz_variables(environment, &symbol_table);
        }
    }

    // BytecodeGenerator.cpp:2301
    pub fn initialize_block_scoped_functions(
        &mut self,
        environment: &mut crate::parser::variable_environment::VariableEnvironment,
        function_stack: &crate::parser::nodes::FunctionStack,
        constant_symbol_table: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        // Precisamos transformar declarações de função em bloco no modo estrito assim:
        //
        // function foo() {
        //     if (c) {
        //           function foo() { ... }
        //           if (bar) { ... }
        //           else { ... }
        //           function baz() { ... }
        //     }
        // }
        //
        // para:
        //
        // function foo() {
        //     if (c) {
        //         let foo = function foo() { ... }
        //         let baz = function baz() { ... }
        //         if (bar) { ... }
        //         else { ... }
        //     }
        // }
        //
        // Mas sem as checagens de TDZ.

        if environment.size() == 0 {
            assert!(function_stack.is_empty());
            return;
        }

        if function_stack.is_empty() {
            return;
        }

        let (symbol_table, scope) = {
            let last = self.lexical_scope_stack.last().unwrap();
            (last.symbol_table.clone().unwrap(), last.scope.clone())
        };
        let temp = self.new_temporary();
        let symbol_table_index = match &constant_symbol_table {
            Some(constant) => constant.borrow().index(),
            None => 0,
        };
        for function in function_stack.iter() {
            let name = function.ident.borrow().clone();
            let iter = environment.find(&name.impl_().unwrap());
            let iter = iter.unwrap();
            assert!(iter.is_function());
            // Propositalmente não seguramos o lock da tabela de símbolos neste laço, porque
            // emit_new_function_expression_common pode disparar o GC.
            let entry = symbol_table.borrow().get(&name.impl_().unwrap());
            assert!(!entry.is_null());
            self.emit_new_function_expression_common(Some(temp.clone()), function);
            let is_lexically_scoped = true;
            let variable = self.variable_for_local_entry(&name, &entry, symbol_table_index, is_lexically_scoped);
            self.emit_put_to_scope(
                scope.clone(),
                &variable,
                Some(temp.clone()),
                crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                crate::runtime::get_put_info::InitializationMode::Initialization,
            );
        }
    }

    // BytecodeGenerator.cpp:2355
    pub fn hoist_sloppy_mode_function_if_necessary(&mut self, metadata: &crate::parser::nodes::FunctionMetadataNode) {
        if metadata.is_sloppy_mode_hoisted_function.get() {
            let function_name = metadata.ident.borrow().clone();

            if let Some(names) = &self.generator_or_async_wrapper_function_parameter_names {
                if names.contains(&function_name) {
                    return;
                }
            }

            let current_function_variable = self.variable(&function_name, ThisResolutionType::Local);
            let current_value;
            if let Some(local) = current_function_variable.local() {
                current_value = local;
            } else {
                let scope = self.emit_resolve_scope(None, &current_function_variable);
                let temporary = self.new_temporary();
                current_value = self
                    .emit_get_from_scope(
                        Some(temporary),
                        scope,
                        &current_function_variable,
                        crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                    )
                    .unwrap();
            }

            match self.code_type() {
                crate::bytecode::code_type::CodeType::FunctionCode => {
                    let var_scope_index = self.var_scope_lexical_scope_stack_index.unwrap();
                    assert!(var_scope_index < self.lexical_scope_stack.len());
                    let mut var_scope = self.lexical_scope_stack[var_scope_index].clone();
                    let var_symbol_table = var_scope.symbol_table.clone().unwrap();
                    let mut entry = var_symbol_table.borrow().get(&function_name.impl_().unwrap());
                    if function_name == self.property_names().arguments && entry.is_null() {
                        // "arguments" pode estar no escopo de parâmetros quando a lista de parâmetros
                        // não é simples, já que "arguments" é visível às expressões dentro da lista.
                        // ex.: function foo(x = arguments) { { function arguments() { } } }
                        assert!(var_scope_index > 0);
                        var_scope = self.lexical_scope_stack[var_scope_index - 1].clone();
                        let parameter_symbol_table = var_scope.symbol_table.clone().unwrap();
                        entry = parameter_symbol_table.borrow().get(&function_name.impl_().unwrap());
                    }
                    assert!(!entry.is_null());
                    let is_lexically_scoped = false;
                    let variable = self.variable_for_local_entry(
                        &function_name,
                        &entry,
                        var_scope.symbol_table_constant_index,
                        is_lexically_scoped,
                    );
                    self.emit_put_to_scope(
                        var_scope.scope.clone(),
                        &variable,
                        Some(current_value),
                        crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                        crate::runtime::get_put_info::InitializationMode::NotInitialization,
                    );
                }
                crate::bytecode::code_type::CodeType::GlobalCode | crate::bytecode::code_type::CodeType::EvalCode => {
                    let scope_id = self.emit_resolve_scope_for_hoisting_func_decl_in_eval(None, &function_name);
                    let is_not_var_scope_label = self.new_label();
                    let temporary = self.new_temporary();
                    let is_undefined = self.emit_is_undefined(Some(temporary), scope_id.as_ref().unwrap());
                    self.emit_jump_if_true_raw(is_undefined.as_ref().unwrap(), &is_not_var_scope_label);
                    // Escreve no escopo externo
                    self.emit_put_to_scope_dynamic(
                        scope_id,
                        &function_name,
                        Some(current_value),
                        crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                        crate::runtime::get_put_info::InitializationMode::NotInitialization,
                    );
                    self.emit_label(&is_not_var_scope_label);
                }
                crate::bytecode::code_type::CodeType::ModuleCode => {
                    unreachable!("RELEASE_ASSERT_NOT_REACHED");
                }
            }
        }
    }

    // BytecodeGenerator.cpp:2412
    pub fn emit_resolve_scope_for_hoisting_func_decl_in_eval(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let temp_dst = self.temp_destination(dst.as_ref());
        let result = self.final_destination(dst.as_ref(), Some(&temp_dst));
        let scope = self.new_temporary();
        if let Some(top_level) = self.top_level_scope_register.clone() {
            self.move_register(Some(&scope), &top_level);
        } else {
            crate::bytecode::bytecode_ops::OpGetScope::emit(self, &scope);
        }
        let property_constant = self.add_constant(property);
        let killed = self.kill(&result);
        crate::bytecode::bytecode_ops::OpResolveScopeForHoistingFuncDeclInEval::emit(self, &killed, &scope, property_constant);
        Some(result)
    }

    // BytecodeGenerator.cpp:2424
    pub fn pop_lexical_scope(&mut self, node: &crate::parser::nodes::VariableEnvironmentNode) {
        let mut environment = node.lexical_variables.clone();
        self.pop_lexical_scope_internal(&mut environment);
    }

    // BytecodeGenerator.cpp:2430
    pub fn pop_lexical_scope_internal(&mut self, environment: &mut crate::parser::variable_environment::VariableEnvironment) {
        // NOTA: esta função só faz sentido para escopos que não são ScopeRegisterType::Var (hoje só o
        // escopo do nome da função é Var). Não vale para Var porque aqui damos deref nos RegisterIDs.
        if environment.size() == 0 {
            return;
        }

        if self.should_emit_debug_hooks() {
            environment.mark_all_variables_as_captured();
        }

        let stack_entry = self.lexical_scope_stack.pop().unwrap();
        let symbol_table = stack_entry.symbol_table.clone().unwrap();
        let mut has_captured_variables = false;
        for entry in environment.iter() {
            if entry.1.is_captured() {
                has_captured_variables = true;
                continue;
            }
            let symbol_table_entry = symbol_table.borrow().get(&entry.0);
            assert!(!symbol_table_entry.is_null());
            let offset = symbol_table_entry.var_offset();
            assert!(offset.is_stack());
            let local = self.register_for(offset.stack_offset());
            local.borrow_mut().deref();
        }

        if has_captured_variables {
            let scope = stack_entry.scope.clone().unwrap();
            let scope_register = self.scope_register();
            self.emit_get_parent_scope(scope_register, Some(scope.clone()));
            self.pop_local_control_flow_scope();
            scope.borrow_mut().deref();
        }

        self.tdz_stack.pop();
    }

    // BytecodeGenerator.cpp:2466
    pub fn prepare_lexical_scope_for_next_for_loop_iteration(
        &mut self,
        node: &crate::parser::nodes::VariableEnvironmentNode,
        loop_symbol_table: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        let mut environment = node.lexical_variables.clone();
        if environment.size() == 0 {
            return;
        }
        if self.should_emit_debug_hooks() {
            environment.mark_all_variables_as_captured();
        }
        if !environment.has_captured_variables() {
            return;
        }

        let loop_symbol_table = loop_symbol_table.unwrap();

        // Esta função prepara a ativação de um laço for se alguma das variáveis declaradas
        // lexicalmente no cabeçalho do laço (não no corpo) for capturada. Ela precisa fazer uma
        // cópia da ativação atual e copiar os valores da ativação anterior para a nova, porque cada
        // iteração de um laço for ganha uma ativação nova.

        let stack_entry = self.lexical_scope_stack.last().unwrap().clone();
        let symbol_table = stack_entry.symbol_table.clone().unwrap();
        let loop_scope = stack_entry.scope.clone().unwrap();
        assert!(symbol_table.borrow().scope_size() != 0);

        let mut activation_values_to_copy_over: Vec<(
            crate::bytecompiler::bytecode_generator::RegisterRef,
            crate::runtime::identifier::Identifier,
        )> = Vec::new();
        for (key, symbol_table_entry) in symbol_table.borrow().iter() {
            if !symbol_table_entry.var_offset().is_scope() {
                continue;
            }

            let identifier = crate::runtime::identifier::Identifier::from_uid(&self.vm, Some(key));

            let transition_value = self.new_block_scope_variable();
            transition_value.borrow_mut().ref_();
            let variable = self.variable_for_local_entry(
                &identifier,
                &symbol_table_entry.get_fast(),
                loop_symbol_table.borrow().index(),
                true,
            );
            self.emit_get_from_scope(
                Some(transition_value.clone()),
                Some(loop_scope.clone()),
                &variable,
                crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
            );
            activation_values_to_copy_over.push((transition_value, identifier));
        }

        // Precisamos deste comportamento dinâmico do código em execução para garantir que cada
        // iteração do laço tenha um objeto de ativação novo. (É bem feio.) Além disso, a nova
        // ativação precisa ser atribuída ao mesmo registrador do escopo anterior, porque o corpo do
        // laço é compilado supondo que o índice do registrador do escopo é constante, embora o valor
        // nele mude a cada iteração.
        let scope_register = self.scope_register();
        self.emit_get_parent_scope(scope_register.clone(), Some(loop_scope.clone()));

        let tdz_constant = self.add_constant_value(
            crate::runtime::js_value::js_tdz_value(),
            crate::parser::parser_tokens::SourceCodeRepresentation::Other,
        );
        crate::bytecode::bytecode_ops::OpCreateLexicalEnvironment::emit(
            self,
            &loop_scope,
            scope_register.as_ref(),
            crate::bytecode::virtual_register::VirtualRegister::new(loop_symbol_table.borrow().index()),
            &tdz_constant,
        );

        self.move_register(scope_register.as_ref(), &loop_scope);

        {
            for pair in &activation_values_to_copy_over {
                let identifier = &pair.1;
                let entry = symbol_table.borrow().get(&identifier.impl_().unwrap());
                assert!(!entry.is_null());
                let transition_value = pair.0.clone();
                let variable = self.variable_for_local_entry(identifier, &entry, loop_symbol_table.borrow().index(), true);
                self.emit_put_to_scope(
                    Some(loop_scope.clone()),
                    &variable,
                    Some(transition_value.clone()),
                    crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                    crate::runtime::get_put_info::InitializationMode::NotInitialization,
                );
                transition_value.borrow_mut().deref();
            }
        }
    }

    // BytecodeGenerator.cpp:2535
    pub fn variable(
        &mut self,
        property: &crate::runtime::identifier::Identifier,
        this_resolution_type: ThisResolutionType,
    ) -> crate::bytecompiler::bytecode_generator::Variable {
        if *property == self.property_names().builtin_names().this_private_name()
            && this_resolution_type == ThisResolutionType::Local
        {
            let this_register = self.this_register();
            let this_virtual_register = this_register.borrow().virtual_register();
            return crate::bytecompiler::bytecode_generator::Variable::new(
                property,
                crate::runtime::var_offset::VarOffset::from_virtual_register(this_virtual_register),
                Some(this_register),
                crate::runtime::property_attribute::PropertyAttribute::ReadOnly as u32,
                crate::bytecompiler::bytecode_generator::VariableKind::SpecialVariable,
                0,
                false,
            );
        }

        // Podemos otimizar as buscas se a variável léxica for encontrada antes de um escopo "with"
        // ou "catch", porque a resolução estática está garantida. Se precisarmos passar por um
        // escopo "with" ou "catch", perdemos essa garantia.
        // Não podemos otimizar casos como este:
        // {
        //     let x = ...;
        //     with (o) {
        //         doSomethingWith(x);
        //     }
        // }
        // Porque não podemos garantir a resolução estática de x.
        // Mas, neste caso, a resolução estática está garantida:
        // {
        //     let x = ...;
        //     with (o) {
        //         let x = ...;
        //         doSomethingWith(x);
        //     }
        // }
        for i in (0..self.lexical_scope_stack.len()).rev() {
            let stack_entry = self.lexical_scope_stack[i].clone();
            if stack_entry.is_with_scope {
                return crate::bytecompiler::bytecode_generator::Variable::from_ident(property);
            }
            let symbol_table = stack_entry.symbol_table.clone().unwrap();
            let symbol_table_entry = symbol_table.borrow().get(&property.impl_().unwrap());
            if symbol_table_entry.is_null() {
                continue;
            }
            let mut result_is_callee = false;
            if symbol_table.borrow().scope_type() == crate::runtime::symbol_table::SymbolTableScopeType::FunctionNameScope {
                if self.uses_sloppy_eval {
                    // Não sabemos se um eval introduziu um "var" com o mesmo nome da variável do escopo
                    // do nome da função. Recorremos à busca dinâmica para responder isso.
                    return crate::bytecompiler::bytecode_generator::Variable::from_ident(property);
                }
                result_is_callee = true;
            }
            let mut result = self.variable_for_local_entry(
                property,
                &symbol_table_entry,
                stack_entry.symbol_table_constant_index,
                symbol_table.borrow().scope_type() == crate::runtime::symbol_table::SymbolTableScopeType::LexicalScope,
            );
            if result_is_callee {
                result.set_is_read_only();
            }
            return result;
        }

        crate::bytecompiler::bytecode_generator::Variable::from_ident(property)
    }

    // BytecodeGenerator.cpp:2586
    pub fn variable_for_local_entry(
        &mut self,
        property: &crate::runtime::identifier::Identifier,
        entry: &crate::runtime::symbol_table::SymbolTableEntryFast,
        symbol_table_constant_index: i32,
        is_lexically_scoped: bool,
    ) -> crate::bytecompiler::bytecode_generator::Variable {
        let offset = entry.var_offset();

        let local = if offset.is_stack() {
            Some(self.register_for(offset.stack_offset()))
        } else {
            None
        };

        crate::bytecompiler::bytecode_generator::Variable::new(
            property,
            offset,
            local,
            entry.get_attributes(),
            crate::bytecompiler::bytecode_generator::VariableKind::NormalVariable,
            symbol_table_constant_index,
            is_lexically_scoped,
        )
    }

    // BytecodeGenerator.cpp:2600
    pub fn create_variable(
        &mut self,
        property: &crate::runtime::identifier::Identifier,
        var_kind: crate::runtime::var_offset::VarKind,
        symbol_table: &crate::runtime::symbol_table::SymbolTableRef,
        existing_variable_mode: ExistingVariableMode,
    ) {
        let entry = symbol_table.borrow().get(&property.impl_().unwrap());

        if !entry.is_null() {
            if existing_variable_mode == ExistingVariableMode::IgnoreExisting {
                return;
            }

            // Faz algumas checagens para garantir que a variável pedida é suficientemente
            // compatível com a que já criamos.

            let offset = entry.var_offset();

            // Não podemos mudar de ideia sobre ela ser capturada.
            if offset.kind() != var_kind {
                panic!(
                    "Trying to add variable called {} as {:?} but it was already added as {:?}.",
                    String::from_utf8_lossy(&property.utf8()),
                    var_kind,
                    offset
                );
            }

            return;
        }

        let var_offset;
        if var_kind == crate::runtime::var_offset::VarKind::Scope {
            var_offset = crate::runtime::var_offset::VarOffset::from_scope_offset(
                symbol_table
                    .borrow_mut()
                    .take_next_scope_offset_locked(crate::runtime::symbol_table::NO_LOCKING_NECESSARY),
            );
        } else {
            assert!(var_kind == crate::runtime::var_offset::VarKind::Stack);
            var_offset = crate::runtime::var_offset::VarOffset::from_virtual_register(
                crate::bytecode::virtual_register::virtual_register_for_local(self.callee_locals.len() as i32),
            );
        }
        let new_entry = crate::runtime::symbol_table::SymbolTableEntry::new(var_offset, 0);
        symbol_table.borrow_mut().add_locked(
            crate::runtime::symbol_table::NO_LOCKING_NECESSARY,
            property.impl_().unwrap(),
            new_entry,
        );

        if var_kind == crate::runtime::var_offset::VarKind::Stack {
            let local = self.add_var();
            assert!(local.borrow().index() == var_offset.stack_offset().offset());
        }
    }

    // BytecodeGenerator.cpp:2642
    pub fn try_resolve_variable(
        &mut self,
        expr: &crate::parser::nodes::Expression,
    ) -> Option<crate::bytecompiler::bytecode_generator::Variable> {
        if expr.is_resolve_node() {
            let identifier = expr.as_resolve_node().unwrap().borrow().ident.clone();
            return Some(self.variable(&identifier, ThisResolutionType::Local));
        }

        if expr.is_assign_resolve_node() {
            let identifier = expr.as_assign_resolve_node().unwrap().borrow().ident.clone();
            return Some(self.variable(&identifier, ThisResolutionType::Local));
        }

        if expr.is_this_node() {
            // Depois de generator.ensureThis (que deve ser invocado na materialização de |base|),
            // podemos garantir que |this| está no registrador local de this.
            let this_private_name = self.property_names().builtin_names().this_private_name().clone();
            return Some(self.variable(&this_private_name, ThisResolutionType::Local));
        }

        if let crate::parser::nodes::Expression::Comma(comma_node) = expr {
            let mut node = comma_node.clone();
            loop {
                let next = node.borrow().next.clone();
                match next {
                    Some(next) => node = next,
                    None => break,
                }
            }
            let last_expr = node.borrow().expr.clone();
            return self.try_resolve_variable(&last_expr);
        }

        None
    }

    // Indica o limite superior mínimo do tipo de resolução com base no escopo local. O linker do
    // bytecode começa com este ResolveType e calcula o limite superior incluindo os escopos
    // interceptadores.
    // BytecodeGenerator.cpp:2666
    pub fn resolve_type(&mut self) -> crate::runtime::get_put_info::ResolveType {
        for i in (0..self.lexical_scope_stack.len()).rev() {
            if self.lexical_scope_stack[i].is_with_scope {
                return crate::runtime::get_put_info::ResolveType::Dynamic;
            }
            if self.uses_sloppy_eval
                && self.lexical_scope_stack[i].symbol_table.as_ref().unwrap().borrow().scope_type()
                    == crate::runtime::symbol_table::SymbolTableScopeType::FunctionNameScope
            {
                // Nunca queremos escrever num FunctionNameScope. Devolver Dynamic aqui atinge isso.
                // Se não estamos em eval não estrito, o NodesCodeGen precisa cuidar de não emitir um
                // put_to_scope com destino na variável do escopo do nome da função.
                return crate::runtime::get_put_info::ResolveType::Dynamic;
            }
        }

        if self.uses_sloppy_eval {
            return crate::runtime::get_put_info::ResolveType::GlobalPropertyWithVarInjectionChecks;
        }
        crate::runtime::get_put_info::ResolveType::GlobalProperty
    }

    // BytecodeGenerator.cpp:2684
    pub fn emit_resolve_scope(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        variable: &crate::bytecompiler::bytecode_generator::Variable,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        match variable.offset().kind() {
            crate::runtime::var_offset::VarKind::Stack => None,

            crate::runtime::var_offset::VarKind::DirectArgument => self.arguments_register(),

            crate::runtime::var_offset::VarKind::Scope => {
                // Isto sempre se refere à ativação que *nós* alocamos, e não ao escopo atual em que o
                // código vive. Isto mudará quando houver suporte adequado a escopo de bloco; então
                // será correto devolver scopeRegister(). O único motivo de não fazermos isso já é que
                // m_lexicalEnvironment é exigido pelo ConstDeclNode, que exige coisas estranhas porque
                // é uma pilha de absurdos vergonhosa, mas o escopo de bloco tornaria aquele código
                // sensato e eliminaria a necessidade de fazer coisas ruins.
                for i in (0..self.lexical_scope_stack.len()).rev() {
                    let stack_entry = &self.lexical_scope_stack[i];
                    // Não devemos resolver uma variável para VarKind::Scope se um escopo "with" estiver
                    // entre o escopo atual e o escopo resolvido.
                    assert!(!stack_entry.is_with_scope);

                    if stack_entry
                        .symbol_table
                        .as_ref()
                        .unwrap()
                        .borrow()
                        .get(&variable.ident().impl_().unwrap())
                        .is_null()
                    {
                        continue;
                    }

                    let scope = stack_entry.scope.clone();
                    assert!(scope.is_some());
                    return scope;
                }

                unreachable!("RELEASE_ASSERT_NOT_REACHED");
            }
            crate::runtime::var_offset::VarKind::Invalid => {
                // Indica resolução não local.

                let dst = Some(self.temp_destination(dst.as_ref()));
                let killed = self.kill(dst.as_ref().unwrap());
                let scope_register = self.scope_register();
                let ident_constant = self.add_constant(variable.ident());
                let resolve_type = self.resolve_type();
                let local_scope_depth = self.local_scope_depth();
                crate::bytecode::bytecode_ops::OpResolveScope::emit(
                    self,
                    &killed,
                    scope_register.as_ref(),
                    ident_constant,
                    resolve_type,
                    local_scope_depth,
                );
                dst
            }
        }
    }

    // BytecodeGenerator.cpp:2730
    pub fn emit_get_from_scope(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        scope: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        variable: &crate::bytecompiler::bytecode_generator::Variable,
        resolve_mode: crate::runtime::get_put_info::ResolveMode,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        match variable.offset().kind() {
            crate::runtime::var_offset::VarKind::Stack => {
                let local = variable.local().unwrap();
                self.move_register(dst.as_ref(), &local)
            }

            crate::runtime::var_offset::VarKind::DirectArgument => {
                let killed = self.kill(dst.as_ref().unwrap());
                let value_profile = self.next_value_profile_index();
                crate::bytecode::bytecode_ops::OpGetFromArguments::emit(
                    self,
                    &killed,
                    scope.as_ref().unwrap(),
                    variable.offset().captured_arguments_offset().offset(),
                    value_profile,
                );
                dst
            }

            crate::runtime::var_offset::VarKind::Scope | crate::runtime::var_offset::VarKind::Invalid => {
                let killed = self.kill(dst.as_ref().unwrap());
                let ident_constant = self.add_constant(variable.ident());
                let resolve_type = if variable.offset().is_scope() {
                    crate::runtime::get_put_info::ResolveType::ResolvedClosureVar
                } else {
                    self.resolve_type()
                };
                let get_put_info = crate::runtime::get_put_info::GetPutInfo::new(
                    resolve_mode,
                    resolve_type,
                    crate::runtime::get_put_info::InitializationMode::NotInitialization,
                    self.ecma_mode(),
                );
                let local_scope_depth = self.local_scope_depth();
                let scope_offset = if variable.offset().is_scope() {
                    variable.offset().scope_offset().offset()
                } else {
                    0
                };
                let value_profile = self.next_value_profile_index();
                crate::bytecode::bytecode_ops::OpGetFromScope::emit(
                    self,
                    &killed,
                    scope.as_ref().unwrap(),
                    ident_constant,
                    get_put_info,
                    local_scope_depth,
                    scope_offset,
                    value_profile,
                );
                dst
            }
        }
    }

    // BytecodeGenerator.cpp:2758
    pub fn emit_put_to_scope(
        &mut self,
        scope: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        variable: &crate::bytecompiler::bytecode_generator::Variable,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        resolve_mode: crate::runtime::get_put_info::ResolveMode,
        initialization_mode: crate::runtime::get_put_info::InitializationMode,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        match variable.offset().kind() {
            crate::runtime::var_offset::VarKind::Stack => {
                let local = variable.local();
                self.move_register(local.as_ref(), value.as_ref().unwrap());
                value
            }

            crate::runtime::var_offset::VarKind::DirectArgument => {
                crate::bytecode::bytecode_ops::OpPutToArguments::emit(
                    self,
                    scope.as_ref().unwrap(),
                    variable.offset().captured_arguments_offset().offset(),
                    value.as_ref().unwrap(),
                );
                value
            }

            crate::runtime::var_offset::VarKind::Scope | crate::runtime::var_offset::VarKind::Invalid => {
                let get_put_info;
                let symbol_table_or_scope_depth;
                let mut offset: Option<crate::runtime::var_offset::ScopeOffset> = None;
                if variable.offset().is_scope() {
                    offset = Some(variable.offset().scope_offset());
                    get_put_info = crate::runtime::get_put_info::GetPutInfo::new(
                        resolve_mode,
                        crate::runtime::get_put_info::ResolveType::ResolvedClosureVar,
                        initialization_mode,
                        self.ecma_mode(),
                    );
                    symbol_table_or_scope_depth = crate::bytecode::bytecode_ops::SymbolTableOrScopeDepth::symbol_table(
                        crate::bytecode::virtual_register::VirtualRegister::new(variable.symbol_table_constant_index()),
                    );
                } else {
                    let resolve_type = self.resolve_type();
                    assert!(resolve_type != crate::runtime::get_put_info::ResolveType::ResolvedClosureVar);
                    get_put_info =
                        crate::runtime::get_put_info::GetPutInfo::new(resolve_mode, resolve_type, initialization_mode, self.ecma_mode());
                    symbol_table_or_scope_depth =
                        crate::bytecode::bytecode_ops::SymbolTableOrScopeDepth::scope_depth(self.local_scope_depth());
                }
                let ident_constant = self.add_constant(variable.ident());
                crate::bytecode::bytecode_ops::OpPutToScope::emit(
                    self,
                    scope.as_ref().unwrap(),
                    ident_constant,
                    value.as_ref().unwrap(),
                    get_put_info,
                    symbol_table_or_scope_depth,
                    match offset {
                        Some(offset) => offset.offset(),
                        None => 0,
                    },
                );
                value
            }
        }
    }

    // BytecodeGenerator.cpp:2790
    pub fn emit_put_to_scope_dynamic(
        &mut self,
        scope: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        ident: &crate::runtime::identifier::Identifier,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        resolve_mode: crate::runtime::get_put_info::ResolveMode,
        initialization_mode: crate::runtime::get_put_info::InitializationMode,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let get_put_info = crate::runtime::get_put_info::GetPutInfo::new(
            resolve_mode,
            crate::runtime::get_put_info::ResolveType::Dynamic,
            initialization_mode,
            self.ecma_mode(),
        );
        let scope_offset: u32 = 0;
        let ident_constant = self.add_constant(ident);
        let local_scope_depth = self.local_scope_depth();
        crate::bytecode::bytecode_ops::OpPutToScope::emit(
            self,
            scope.as_ref().unwrap(),
            ident_constant,
            value.as_ref().unwrap(),
            get_put_info,
            crate::bytecode::bytecode_ops::SymbolTableOrScopeDepth::scope_depth(local_scope_depth),
            scope_offset,
        );
        value
    }

    // BytecodeGenerator.cpp:2798
    pub fn initialize_variable(
        &mut self,
        variable: &crate::bytecompiler::bytecode_generator::Variable,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        assert!(variable.offset().kind() != crate::runtime::var_offset::VarKind::Invalid);
        let scope = self.emit_resolve_scope(None, variable);
        self.emit_put_to_scope(
            scope,
            variable,
            value,
            crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
            crate::runtime::get_put_info::InitializationMode::NotInitialization,
        )
    }

    // BytecodeGenerator.cpp:2805
    pub fn emit_instanceof(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        constructor: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        has_instance_or_prototype: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let has_instance_value_profile = self.next_value_profile_index();
        let prototype_value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpInstanceof::emit(
            self,
            dst.as_ref().unwrap(),
            value.as_ref().unwrap(),
            constructor.as_ref().unwrap(),
            has_instance_or_prototype.as_ref().unwrap(),
            has_instance_value_profile,
            prototype_value_profile,
        );
        dst
    }

    // BytecodeGenerator.cpp:2813
    pub fn emit_in_by_val(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        for i in (0..self.for_in_context_stack.len()).rev() {
            let context = self.for_in_context_stack[i].clone();
            if !Self::same_register(&context.borrow().local(), &property) {
                continue;
            }

            let (mode, property_offset, enumerator) = {
                let context = context.borrow();
                (context.mode(), context.property_offset(), context.enumerator())
            };
            crate::bytecode::bytecode_ops::OpEnumeratorInByVal::emit_with_smallest_size_requirement(
                self,
                crate::bytecode::opcode_size::OpcodeSize::Wide32,
                dst.as_ref().unwrap(),
                base.as_ref().unwrap(),
                mode.as_ref().unwrap(),
                property.as_ref().unwrap(),
                property_offset.as_ref().unwrap(),
                enumerator.as_ref().unwrap(),
            );
            let offset = self.last_instruction.offset();
            context.borrow_mut().add_in_inst(offset, property.as_ref().unwrap().borrow().index());
            return dst;
        }

        crate::bytecode::bytecode_ops::OpInByVal::emit(self, dst.as_ref().unwrap(), base.as_ref().unwrap(), property.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:2829
    pub fn emit_in_by_id(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let property_constant = self.add_constant(property);
        crate::bytecode::bytecode_ops::OpInById::emit(self, dst.as_ref().unwrap(), base.as_ref().unwrap(), property_constant);
        dst
    }

    // BytecodeGenerator.cpp:2835
    pub fn emit_get_length(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let killed = self.kill(dst.as_ref().unwrap());
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpGetLength::emit(self, &killed, base.as_ref().unwrap(), value_profile);
        dst
    }

    // BytecodeGenerator.cpp:2841
    pub fn emit_get_by_id(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // Propriedades indexadas devem ser tratadas com get_by_val.
        assert!(crate::runtime::identifier::parse_index_identifier(property).is_none());

        let killed = self.kill(dst.as_ref().unwrap());
        if *property == self.vm.property_names.length {
            let value_profile = self.next_value_profile_index();
            crate::bytecode::bytecode_ops::OpGetLength::emit(self, &killed, base.as_ref().unwrap(), value_profile);
        } else {
            let property_constant = self.add_constant(property);
            let value_profile = self.next_value_profile_index();
            crate::bytecode::bytecode_ops::OpGetById::emit(self, &killed, base.as_ref().unwrap(), property_constant, value_profile);
        }
        dst
    }

    // BytecodeGenerator.cpp:2852
    pub fn emit_get_by_id_with_this(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_val: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // Propriedades indexadas devem ser tratadas com get_by_val.
        assert!(crate::runtime::identifier::parse_index_identifier(property).is_none());

        let killed = self.kill(dst.as_ref().unwrap());
        let property_constant = self.add_constant(property);
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpGetByIdWithThis::emit(
            self,
            &killed,
            base.as_ref().unwrap(),
            this_val.as_ref().unwrap(),
            property_constant,
            value_profile,
        );
        dst
    }

    // BytecodeGenerator.cpp:2860
    pub fn emit_direct_get_by_id(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // Propriedades indexadas devem ser tratadas com get_by_val_direct.
        assert!(crate::runtime::identifier::parse_index_identifier(property).is_none());

        let killed = self.kill(dst.as_ref().unwrap());
        let property_constant = self.add_constant(property);
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpGetByIdDirect::emit(self, &killed, base.as_ref().unwrap(), property_constant, value_profile);
        dst
    }

    // BytecodeGenerator.cpp:2868
    pub fn emit_put_by_id(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // Propriedades indexadas devem ser tratadas com put_by_val.
        assert!(crate::runtime::identifier::parse_index_identifier(property).is_none());

        let property_index = self.add_constant(property);

        self.static_property_analyzer.put_by_id(&base.as_ref().unwrap().borrow(), property_index);

        // não é direto
        crate::bytecode::bytecode_ops::OpPutById::emit(
            self,
            base.as_ref().unwrap(),
            property_index,
            value.as_ref().unwrap(),
            crate::bytecode::put_by_id_flags::PutByIdFlags::create(self.ecma_mode()),
        );
        value
    }

    // BytecodeGenerator.cpp:2880
    pub fn emit_put_by_id_with_this(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // Propriedades indexadas devem ser tratadas com put_by_val.
        assert!(crate::runtime::identifier::parse_index_identifier(property).is_none());

        let property_index = self.add_constant(property);

        crate::bytecode::bytecode_ops::OpPutByIdWithThis::emit(
            self,
            base.as_ref().unwrap(),
            this_value.as_ref().unwrap(),
            property_index,
            value.as_ref().unwrap(),
            self.ecma_mode(),
        );

        value
    }

    // BytecodeGenerator.cpp:2891
    pub fn emit_direct_put_by_id(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // Propriedades indexadas devem ser tratadas com put_by_val(direct).
        assert!(crate::runtime::identifier::parse_index_identifier(property).is_none());

        let property_index = self.add_constant(property);

        self.static_property_analyzer.put_by_id(&base.as_ref().unwrap().borrow(), property_index);

        let type_ = crate::bytecode::put_by_id_flags::PutByIdFlags::create_direct(self.ecma_mode());
        crate::bytecode::bytecode_ops::OpPutById::emit(self, base.as_ref().unwrap(), property_index, value.as_ref().unwrap(), type_);
        value
    }

    // BytecodeGenerator.cpp:2904
    pub fn emit_put_getter_by_id(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
        attributes: u32,
        getter: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        let property_index = self.add_constant(property);
        self.static_property_analyzer.put_by_id(&base.as_ref().unwrap().borrow(), property_index);

        crate::bytecode::bytecode_ops::OpPutGetterById::emit(
            self,
            base.as_ref().unwrap(),
            property_index,
            attributes,
            getter.as_ref().unwrap(),
        );
    }

    // BytecodeGenerator.cpp:2912
    pub fn emit_put_setter_by_id(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
        attributes: u32,
        setter: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        let property_index = self.add_constant(property);
        self.static_property_analyzer.put_by_id(&base.as_ref().unwrap().borrow(), property_index);

        crate::bytecode::bytecode_ops::OpPutSetterById::emit(
            self,
            base.as_ref().unwrap(),
            property_index,
            attributes,
            setter.as_ref().unwrap(),
        );
    }

    // BytecodeGenerator.cpp:2920
    pub fn emit_put_getter_setter(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
        attributes: u32,
        getter: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        setter: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        let property_index = self.add_constant(property);

        self.static_property_analyzer.put_by_id(&base.as_ref().unwrap().borrow(), property_index);

        crate::bytecode::bytecode_ops::OpPutGetterSetterById::emit(
            self,
            base.as_ref().unwrap(),
            property_index,
            attributes,
            getter.as_ref().unwrap(),
            setter.as_ref().unwrap(),
        );
    }

    // BytecodeGenerator.cpp:2929
    pub fn emit_put_getter_by_val(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        attributes: u32,
        getter: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        crate::bytecode::bytecode_ops::OpPutGetterByVal::emit(
            self,
            base.as_ref().unwrap(),
            property.as_ref().unwrap(),
            attributes,
            getter.as_ref().unwrap(),
        );
    }

    // BytecodeGenerator.cpp:2934
    pub fn emit_put_setter_by_val(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        attributes: u32,
        setter: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        crate::bytecode::bytecode_ops::OpPutSetterByVal::emit(
            self,
            base.as_ref().unwrap(),
            property.as_ref().unwrap(),
            attributes,
            setter.as_ref().unwrap(),
        );
    }

    // BytecodeGenerator.cpp:2939
    pub fn emit_put_generator_fields(&mut self, next_function: Option<crate::bytecompiler::bytecode_generator::RegisterRef>) {
        let generator_register = self.generator_register();
        self.emit_put_internal_field(
            generator_register.clone(),
            crate::runtime::js_generator::Field::Next as u32,
            next_function,
        );

        // Não guardamos 'this' em arrow function dentro de construtor, porque ele pode não estar
        // inicializado, se super for chamado depois.
        if !(self.is_derived_constructor_context()
            && self.code_block.parse_mode() == crate::parser::parser_modes::SourceParseMode::AsyncArrowFunctionMode)
        {
            let this_register = self.this_register();
            self.emit_put_internal_field(
                generator_register,
                crate::runtime::js_generator::Field::This as u32,
                Some(this_register),
            );
        }
    }

    // BytecodeGenerator.cpp:2949
    pub fn emit_put_async_generator_fields(&mut self, next_function: Option<crate::bytecompiler::bytecode_generator::RegisterRef>) {
        assert!(crate::parser::parser_modes::is_async_generator_wrapper_parse_mode(self.parse_mode()));

        let generator_register = self.generator_register();
        self.emit_put_internal_field(
            generator_register.clone(),
            crate::runtime::js_async_generator::Field::Next as u32,
            next_function,
        );
        let this_register = self.this_register();
        self.emit_put_internal_field(
            generator_register,
            crate::runtime::js_async_generator::Field::This as u32,
            Some(this_register),
        );
    }

    // BytecodeGenerator.cpp:2957
    pub fn emit_delete_by_id(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let property_constant = self.add_constant(property);
        crate::bytecode::bytecode_ops::OpDelById::emit(
            self,
            dst.as_ref().unwrap(),
            base.as_ref().unwrap(),
            property_constant,
            self.ecma_mode(),
        );
        dst
    }

    // BytecodeGenerator.cpp:2963
    pub fn emit_get_by_val(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        for i in (0..self.for_in_context_stack.len()).rev() {
            let context = self.for_in_context_stack[i].clone();
            if !Self::same_register(&context.borrow().local(), &property) {
                continue;
            }

            // FIXME: Deveríamos ter um reescritor de bytecode melhor, que redimensione blocos.
            let (mode, property_offset, enumerator) = {
                let context = context.borrow();
                (context.mode(), context.property_offset(), context.enumerator())
            };
            let killed = self.kill(dst.as_ref().unwrap());
            let value_profile = self.next_value_profile_index();
            crate::bytecode::bytecode_ops::OpEnumeratorGetByVal::emit_with_smallest_size_requirement(
                self,
                crate::bytecode::opcode_size::OpcodeSize::Wide32,
                &killed,
                base.as_ref().unwrap(),
                mode.as_ref().unwrap(),
                property.as_ref().unwrap(),
                property_offset.as_ref().unwrap(),
                enumerator.as_ref().unwrap(),
                value_profile,
            );
            let offset = self.last_instruction.offset();
            context.borrow_mut().add_get_inst(offset, property.as_ref().unwrap().borrow().index());
            return dst;
        }

        let killed = self.kill(dst.as_ref().unwrap());
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpGetByVal::emit(self, &killed, base.as_ref().unwrap(), property.as_ref().unwrap(), value_profile);
        dst
    }

    // BytecodeGenerator.cpp:2980
    pub fn emit_get_by_val_with_this(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let killed = self.kill(dst.as_ref().unwrap());
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpGetByValWithThis::emit(
            self,
            &killed,
            base.as_ref().unwrap(),
            this_value.as_ref().unwrap(),
            property.as_ref().unwrap(),
            value_profile,
        );
        dst
    }

    // BytecodeGenerator.cpp:2986
    pub fn emit_get_prototype_of(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpGetPrototypeOf::emit(self, dst.as_ref().unwrap(), value.as_ref().unwrap(), value_profile);
        dst
    }

    // BytecodeGenerator.cpp:2992
    pub fn emit_put_by_val(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        for i in (0..self.for_in_context_stack.len()).rev() {
            let context = self.for_in_context_stack[i].clone();
            if !Self::same_register(&context.borrow().local(), &property) {
                continue;
            }
            return self.emit_enumerator_put_by_val(&mut context.borrow_mut(), base, property, value);
        }

        crate::bytecode::bytecode_ops::OpPutByVal::emit(
            self,
            base.as_ref().unwrap(),
            property.as_ref().unwrap(),
            value.as_ref().unwrap(),
            self.ecma_mode(),
        );
        value
    }

    // Igualdade de ponteiro de `RegisterID*` (o `!=` do C++ entre os dois ponteiros).
    fn same_register(
        a: &Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        b: &Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => a.is_same_register(b),
            (None, None) => true,
            _ => false,
        }
    }
}
