// Parte 4 de bytecompiler/BytecodeGenerator.cpp (linhas 3005 a 4035 do .cpp). Juntada por include!.
// Convenção: `Option<RegisterRef>` é o `RegisterID*` anulável do C++, com
// `RegisterRef = Rc<RefCell<RegisterID>>`. Os overloads do C++ ganham sufixo pelo que os distingue
// (os mesmos nomes que a parte 2 deixou nas declarações). Os templates sobre o opcode de chamada
// (`emitCall<CallOp>`, `emitCallVarargs<VarargsOp>`) recebem o `OpcodeID` e escolhem o `emit` concreto.

impl BytecodeGenerator {
    // BytecodeGenerator.cpp:3005
    pub fn emit_put_by_val_with_this(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpPutByValWithThis::emit(
            self,
            base.as_ref().unwrap(),
            this_value.as_ref().unwrap(),
            property.as_ref().unwrap(),
            value.as_ref().unwrap(),
            self.ecma_mode(),
        );
        value
    }

    // BytecodeGenerator.cpp:3011
    pub fn emit_put_by_val_with_ecma_mode(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        ecma_mode: crate::runtime::ecma_mode::ECMAMode,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpPutByValWithThis::emit(
            self,
            base.as_ref().unwrap(),
            this_value.as_ref().unwrap(),
            property.as_ref().unwrap(),
            value.as_ref().unwrap(),
            ecma_mode,
        );
        value
    }

    // BytecodeGenerator.cpp:3017
    pub fn emit_enumerator_put_by_val(
        &mut self,
        context: &mut crate::bytecompiler::bytecode_generator::ForInContext,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // FIXME: Deveríamos ter um reescritor de bytecode melhor, que redimensione blocos.
        let ecma_mode = self.ecma_mode();
        crate::bytecode::bytecode_ops::OpEnumeratorPutByVal::emit_at_size(
            self,
            crate::bytecode::opcode_size::OpcodeSize::Wide32,
            true,
            base.as_ref().unwrap(),
            context.mode().as_ref().unwrap(),
            property.as_ref().unwrap(),
            context.property_offset().as_ref().unwrap(),
            context.enumerator().as_ref().unwrap(),
            value.as_ref().unwrap(),
            ecma_mode,
        );
        let offset = self.last_instruction.offset();
        context.add_put_inst(offset, property.as_ref().unwrap().borrow().index());
        value
    }

    // BytecodeGenerator.cpp:3025
    pub fn emit_get_private_name(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpGetPrivateName::emit(
            self,
            dst.as_ref().unwrap(),
            base.as_ref().unwrap(),
            property.as_ref().unwrap(),
            value_profile,
        );
        dst
    }

    // BytecodeGenerator.cpp:3031
    pub fn emit_has_private_name(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpHasPrivateName::emit(
            self,
            dst.as_ref().unwrap(),
            base.as_ref().unwrap(),
            property.as_ref().unwrap(),
        );
        dst
    }

    // BytecodeGenerator.cpp:3037
    pub fn emit_has_structure_with_flags(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        flags: u32,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpHasStructureWithFlags::emit(self, dst.as_ref().unwrap(), src.as_ref().unwrap(), flags);
        dst
    }

    // BytecodeGenerator.cpp:3043
    pub fn emit_direct_put_by_val(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpPutByValDirect::emit(
            self,
            base.as_ref().unwrap(),
            property.as_ref().unwrap(),
            value.as_ref().unwrap(),
            self.ecma_mode(),
        );
        value
    }

    // BytecodeGenerator.cpp:3049
    pub fn emit_delete_by_val(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpDelByVal::emit(
            self,
            dst.as_ref().unwrap(),
            base.as_ref().unwrap(),
            property.as_ref().unwrap(),
            self.ecma_mode(),
        );
        dst
    }

    // BytecodeGenerator.cpp:3055
    pub fn emit_get_internal_field(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        index: u32,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpGetInternalField::emit(
            self,
            dst.as_ref().unwrap(),
            base.as_ref().unwrap(),
            index,
            value_profile,
        );
        dst
    }

    // BytecodeGenerator.cpp:3061
    pub fn emit_put_internal_field(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        index: u32,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpPutInternalField::emit(self, base.as_ref().unwrap(), index, value.as_ref().unwrap());
        value
    }

    // BytecodeGenerator.cpp:3067
    pub fn emit_define_private_field(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpPutPrivateName::emit(
            self,
            base.as_ref().unwrap(),
            property.as_ref().unwrap(),
            value.as_ref().unwrap(),
            crate::bytecode::put_kind::PrivateFieldPutKind::define(),
        );
        value
    }

    // BytecodeGenerator.cpp:3073
    pub fn emit_create_private_brand(
        &mut self,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
    ) {
        let create_private_symbol = self.move_link_time_constant(
            None,
            crate::bytecode::link_time_constant::LinkTimeConstant::CreatePrivateSymbol,
        );

        let mut arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 1);
        self.emit_load_js_value(arguments.this_register(), crate::runtime::js_value::js_undefined());
        // O JSC do oráculo (bun) carrega `true` neste argumento, não a string vazia.
        self.emit_load_js_value(arguments.argument_register(0), crate::runtime::js_value::js_boolean(true));
        let destination = self.final_destination(None, create_private_symbol.as_ref());
        // No C++ o destino é o `RegisterID*` cru de `newTemporary()` (contagem 0): o `newTemporary()` do
        // call frame o recupera e reaproveita o slot. Com o `RegisterRef` vivo o frame subia 2 registros.
        let raw_destination = destination.get().clone();
        let new_symbol = self.with_raw_register(Some(&raw_destination), |generator| {
            generator.emit_call(
                Some(destination),
                create_private_symbol.clone(),
                crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                &mut arguments,
                divot,
                divot_start,
                divot_end,
                crate::bytecompiler::bytecode_generator::DebuggableCall::No,
            )
        });

        let private_brand_var = self.variable(
            &self.property_names().builtin_names().private_brand_private_name(),
            ThisResolutionType::Local,
        );

        let scope_register = self.scope_register();
        self.emit_put_to_scope(
            scope_register,
            &private_brand_var,
            new_symbol,
            crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
            crate::runtime::get_put_info::InitializationMode::ConstInitialization,
        );
    }

    // BytecodeGenerator.cpp:3087
    pub fn emit_install_private_brand(&mut self, target: Option<crate::bytecompiler::bytecode_generator::RegisterRef>) {
        let private_brand_var = self.variable(
            &self.property_names().builtin_names().private_brand_private_name(),
            ThisResolutionType::Local,
        );
        let private_brand_var_scope = self.emit_resolve_scope(None, &private_brand_var);
        let is_static = false;
        let temporary = self.new_temporary();
        let private_brand_symbol = self.emit_get_private_brand(Some(temporary), private_brand_var_scope, is_static);
        crate::bytecode::bytecode_ops::OpSetPrivateBrand::emit(
            self,
            target.as_ref().unwrap(),
            private_brand_symbol.as_ref().unwrap(),
        );
    }

    // BytecodeGenerator.cpp:3096
    pub fn emit_install_private_class_brand(&mut self, target: Option<crate::bytecompiler::bytecode_generator::RegisterRef>) {
        let private_brand_var = self.variable(
            &self.property_names().builtin_names().private_class_brand_private_name(),
            ThisResolutionType::Local,
        );
        let scope_register = self.scope_register();
        self.emit_put_to_scope(
            scope_register,
            &private_brand_var,
            target,
            crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
            crate::runtime::get_put_info::InitializationMode::ConstInitialization,
        );
    }

    // BytecodeGenerator.cpp:3102
    pub fn emit_get_private_brand(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        scope: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        is_static: bool,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        assert!(scope.is_some());
        let killed = self.kill(dst.as_ref().unwrap());
        let brand_name = if is_static {
            self.property_names().builtin_names().private_class_brand_private_name()
        } else {
            self.property_names().builtin_names().private_brand_private_name()
        };
        let brand_constant = self.add_constant(&brand_name);
        let get_put_info = crate::runtime::get_put_info::GetPutInfo::new(
            crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
            crate::runtime::get_put_info::ResolveType::ResolvedClosureVar,
            crate::runtime::get_put_info::InitializationMode::NotInitialization,
            self.ecma_mode(),
        );
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpGetFromScope::emit(
            self,
            &killed,
            scope.as_ref().unwrap(),
            brand_constant,
            get_put_info,
            0,
            if is_static {
                crate::parser::variable_environment::PrivateNameEntry::PRIVATE_CLASS_BRAND_OFFSET
            } else {
                crate::parser::variable_environment::PrivateNameEntry::PRIVATE_BRAND_OFFSET
            },
            value_profile,
        );
        dst
    }

    // BytecodeGenerator.cpp:3117
    pub fn emit_private_field_put(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpPutPrivateName::emit(
            self,
            base.as_ref().unwrap(),
            property.as_ref().unwrap(),
            value.as_ref().unwrap(),
            crate::bytecode::put_kind::PrivateFieldPutKind::set(),
        );
        value
    }

    // BytecodeGenerator.cpp:3123
    pub fn emit_has_private_brand(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        brand: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        is_static: bool,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if is_static {
            let is_object_label = self.new_label();
            let temporary = self.new_temporary();
            let is_object = self.emit_is_object(Some(temporary), base.as_ref().unwrap());
            self.emit_jump_if_true_raw(is_object.as_ref().unwrap(), &is_object_label);
            self.emit_throw_type_error("Cannot access static private method or accessor of a non-Object");
            self.emit_label(&is_object_label);
            self.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(dst.clone(), base, brand);
        } else {
            crate::bytecode::bytecode_ops::OpHasPrivateBrand::emit(
                self,
                dst.as_ref().unwrap(),
                base.as_ref().unwrap(),
                brand.as_ref().unwrap(),
            );
        }
        dst
    }

    // BytecodeGenerator.cpp:3136
    pub fn emit_check_private_brand(
        &mut self,
        base: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        brand: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        is_static: bool,
    ) {
        if is_static {
            let brand_check_ok_label = self.new_label();
            self.emit_tdz_check(brand.as_ref().unwrap());
            let temporary = self.new_temporary();
            let equal = self.emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(Some(temporary), base, brand);
            self.emit_jump_if_true_raw(equal.as_ref().unwrap(), &brand_check_ok_label);
            self.emit_throw_type_error("Cannot access static private method or accessor");
            self.emit_label(&brand_check_ok_label);
            return;
        }
        crate::bytecode::bytecode_ops::OpCheckPrivateBrand::emit(self, base.as_ref().unwrap(), brand.as_ref().unwrap());
    }

    // BytecodeGenerator.cpp:3160
    pub fn emit_id_with_profile(
        &mut self,
        src: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        profile: crate::bytecode::speculated_type::SpeculatedType,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpIdentityWithProfile::emit(
            self,
            src.as_ref().unwrap(),
            (profile >> 32) as u32,
            profile as u32,
        );
        src
    }

    // BytecodeGenerator.cpp:3171
    pub fn emit_get_argument(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        index: i32,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let value_profile = self.next_value_profile_index();
        // O +1 inclui |this|.
        crate::bytecode::bytecode_ops::OpGetArgument::emit(self, dst.as_ref().unwrap(), index + 1, value_profile);
        dst
    }

    // BytecodeGenerator.cpp:3177
    pub fn emit_create_this(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpCreateThis::emit(self, dst.as_ref().unwrap(), dst.as_ref().unwrap(), 0);
        let last_instruction = self.last_instruction.clone();
        self.static_property_analyzer.create_this(&dst.as_ref().unwrap().borrow(), last_instruction);
        dst
    }

    // BytecodeGenerator.cpp:3184
    pub fn emit_create_promise(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        new_target: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpCreatePromise::emit(self, dst.as_ref().unwrap(), new_target.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:3190
    pub fn emit_new_promise(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpNewPromise::emit(self, dst.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:3196
    pub fn emit_create_generator(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        new_target: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpCreateGenerator::emit(self, dst.as_ref().unwrap(), new_target.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:3202
    pub fn emit_new_generator(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpNewGenerator::emit(self, dst.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:3208
    pub fn emit_new_async_function_generator(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpNewAsyncFunctionGenerator::emit(self, dst.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:3214
    pub fn emit_create_async_generator(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        new_target: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpCreateAsyncGenerator::emit(self, dst.as_ref().unwrap(), new_target.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:3220
    pub fn emit_instance_field_initialization_if_needed(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        constructor: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if !(self.is_constructor() || self.is_derived_constructor_context())
            || self.needs_class_field_initializer() == crate::bytecode::executable_info::NeedsClassFieldInitializer::No
        {
            return dst;
        }

        let temporary = self.new_temporary();
        let initializer_name = self.property_names().builtin_names().instance_field_initializer_private_name();
        let initializer = self.emit_direct_get_by_id(Some(temporary), constructor, &initializer_name);
        let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 0);
        let this_register = args.this_register();
        self.emit_move(this_register.as_ref().unwrap(), dst.as_ref().unwrap());
        let temporary = self.new_temporary();
        self.emit_call_ignore_result(
            Some(temporary),
            initializer,
            crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
            &mut args,
            divot,
            divot_start,
            divot_end,
            crate::bytecompiler::bytecode_generator::DebuggableCall::No,
        );

        dst
    }

    // BytecodeGenerator.cpp:3233
    pub fn emit_tdz_check_variable(
        &mut self,
        target: &crate::bytecompiler::bytecode_generator::RegisterRef,
        variable: &crate::bytecompiler::bytecode_generator::Variable,
    ) {
        let string_constant = self.add_string_constant(variable.ident());
        let constant = self.add_constant_value(
            crate::runtime::js_value::JSValue::from_cell(string_constant.cell_id()),
            crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
        );
        crate::bytecode::bytecode_ops::OpCheckTdz::emit(self, target, &constant);
    }

    // BytecodeGenerator.cpp:3238
    pub fn emit_tdz_check(&mut self, target: &crate::bytecompiler::bytecode_generator::RegisterRef) {
        let constant = self.add_constant_value(
            crate::runtime::js_value::js_undefined(),
            crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
        );
        crate::bytecode::bytecode_ops::OpCheckTdz::emit(self, target, &constant);
    }

    // BytecodeGenerator.cpp:3243
    pub fn needs_tdz_check(&mut self, variable: &crate::bytecompiler::bytecode_generator::Variable) -> bool {
        let identifier = variable.ident().impl_().expect("variável sem identificador");
        for i in (0..self.tdz_stack.len()).rev() {
            let Some(level) = self.tdz_stack[i].0.get(&identifier) else {
                continue;
            };
            return *level != TDZNecessityLevel::NotNeeded;
        }

        {
            let mut environment = self.cached_parent_tdz.as_deref();
            while let Some(link) = environment {
                if link.contains(&identifier) {
                    return true;
                }
                environment = link.parent();
            }
        }
        false
    }

    // BytecodeGenerator.cpp:3263
    pub fn emit_tdz_check_if_necessary(
        &mut self,
        variable: &crate::bytecompiler::bytecode_generator::Variable,
        target: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        scope: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        if self.needs_tdz_check(variable) {
            if let Some(target) = target {
                self.emit_tdz_check_variable(&target, variable);
            } else {
                assert!(!variable.is_local() && scope.is_some());
                let temporary = self.new_temporary();
                let result = self.emit_get_from_scope(
                    Some(temporary),
                    scope,
                    variable,
                    crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                );
                self.emit_tdz_check_variable(result.as_ref().unwrap(), variable);
            }
        }
    }

    // BytecodeGenerator.cpp:3276
    pub fn lift_tdz_check_if_possible(&mut self, variable: &crate::bytecompiler::bytecode_generator::Variable) {
        let identifier = variable.ident().impl_().expect("variável sem identificador");
        for i in (0..self.tdz_stack.len()).rev() {
            if let Some(level) = self.tdz_stack[i].0.get_mut(&identifier) {
                if *level == TDZNecessityLevel::Optimize {
                    *level = TDZNecessityLevel::NotNeeded;
                }
                break;
            }
        }
    }

    // BytecodeGenerator.cpp:3290
    // Deve ser chamado só com PrivateNames disponíveis.
    pub fn get_private_traits(
        &mut self,
        ident: &crate::runtime::identifier::Identifier,
    ) -> crate::parser::variable_environment::PrivateNameEntry {
        let key = ident.impl_().expect("identificador privado sem impl");
        for i in (0..self.private_names_stack.len()).rev() {
            if let Some(entry) = self.private_names_stack[i].find(&key) {
                return entry.clone();
            }
        }

        unreachable!("RELEASE_ASSERT_NOT_REACHED");
    }

    // BytecodeGenerator.cpp:3303
    pub fn push_private_access_names(&mut self, environment: Option<&crate::parser::variable_environment::PrivateNameEnvironment>) {
        let Some(environment) = environment else {
            return;
        };
        if environment.is_empty() {
            return;
        }

        self.private_names_stack.push(environment.clone());
    }

    // BytecodeGenerator.cpp:3311
    pub fn pop_private_access_names(&mut self) {
        debug_assert!(!self.private_names_stack.is_empty());
        self.private_names_stack.pop();
    }

    // BytecodeGenerator.cpp:3317
    pub fn push_tdz_variables(
        &mut self,
        environment: &crate::parser::variable_environment::VariableEnvironment,
        optimization: TDZCheckOptimization,
        requirement: TDZRequirement,
    ) {
        if environment.is_empty() {
            return;
        }

        let level = if requirement == TDZRequirement::UnderTDZ {
            if optimization == TDZCheckOptimization::Optimize {
                TDZNecessityLevel::Optimize
            } else {
                TDZNecessityLevel::DoNotOptimize
            }
        } else {
            TDZNecessityLevel::NotNeeded
        };

        let mut map = TDZMap::new();
        for (key, value) in environment.iter() {
            map.insert(
                key.clone(),
                if value.is_function() {
                    TDZNecessityLevel::NotNeeded
                } else {
                    level
                },
            );
        }

        self.tdz_stack.push((map, None));
    }

    // BytecodeGenerator.cpp:3338
    pub fn get_parameter_names(&self) -> Vec<crate::runtime::identifier::Identifier> {
        let crate::parser::nodes::ScopeNodeRef::Function(function_node) = &self.scope_node else {
            panic!("ASSERT(m_scopeNode->isFunctionNode())");
        };
        let parameters = function_node.borrow().parameters.clone().expect("FunctionNode sem parâmetros");
        let mut parameter_names = Vec::new();
        for (pattern, _) in parameters.borrow().patterns.iter() {
            pattern.collect_bound_identifiers(&mut parameter_names);
        }
        parameter_names
    }

    // BytecodeGenerator.cpp:3348
    pub fn get_available_private_access_names(&mut self) -> Option<crate::parser::variable_environment::PrivateNameEnvironment> {
        let mut result = crate::parser::variable_environment::PrivateNameEnvironment::default();
        let mut excluded_names = std::collections::HashSet::new();
        for i in (0..self.private_names_stack.len()).rev() {
            let map = &self.private_names_stack[i];
            for (key, value) in map.iter() {
                if excluded_names.insert(key.clone()) {
                    result.add(key, value.clone());
                }
            }
        }

        if result.is_empty() {
            return None;
        }
        Some(result)
    }

    // BytecodeGenerator.cpp:3366
    pub fn get_variables_under_tdz(
        &mut self,
    ) -> Option<std::rc::Rc<crate::parser::variable_environment::TDZEnvironmentLink>> {
        let mut parent = self.cached_parent_tdz.clone();
        if self.tdz_stack.is_empty() {
            return parent;
        }

        if let Some(last) = &self.tdz_stack.last().unwrap().1 {
            return Some(last.clone());
        }

        for i in 0..self.tdz_stack.len() {
            if self.tdz_stack[i].1.is_none() {
                let mut environment = crate::parser::variable_environment::TDZEnvironment::default();
                for (key, level) in self.tdz_stack[i].0.iter() {
                    if *level != TDZNecessityLevel::NotNeeded {
                        environment.add(key, ());
                    }
                }
                let compact = self.vm.compact_variable_map().get(&environment);
                self.tdz_stack[i].1 = Some(crate::parser::variable_environment::TDZEnvironmentLink::create(compact, parent.clone()));
            }
            parent = self.tdz_stack[i].1.clone();
        }

        parent
    }

    // BytecodeGenerator.cpp:3406
    pub fn preserve_tdz_stack(&mut self, preserved_stack: &mut PreservedTDZStack) {
        preserved_stack.preserved_tdz_stack = self.tdz_stack.clone();
    }

    // BytecodeGenerator.cpp:3411
    pub fn restore_tdz_stack(&mut self, preserved_stack: &PreservedTDZStack) {
        self.tdz_stack = preserved_stack.preserved_tdz_stack.clone();
    }

    // BytecodeGenerator.cpp:3416
    pub fn emit_new_object(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpNewObject::emit(self, dst.as_ref().unwrap(), 0);
        let last_instruction = self.last_instruction.clone();
        self.static_property_analyzer.new_object(&dst.as_ref().unwrap().borrow(), last_instruction);

        dst
    }

    // BytecodeGenerator.cpp:3424
    pub fn add_big_int_constant(
        &mut self,
        identifier: &crate::runtime::identifier::Identifier,
        radix: u8,
        sign: bool,
    ) -> crate::runtime::js_value::JSValue {
        let key: BigIntMapEntry = (identifier.impl_(), radix, sign);
        if let Some(existing) = self.big_int_map.get(&key) {
            return *existing;
        }

        let vm = self.vm.clone();
        let _defer_scope = crate::runtime::vm::DeferTermination::new(&vm);
        let parse_int_sign = if sign {
            crate::runtime::js_big_int::ParseIntSign::Signed
        } else {
            crate::runtime::js_big_int::ParseIntSign::Unsigned
        };
        let big_int_in_map = crate::runtime::js_big_int::JSBigInt::parse_int(
            None,
            &vm,
            identifier.string(),
            radix,
            crate::runtime::js_big_int::ErrorParseMode::ThrowExceptions,
            parse_int_sign,
        )
        .into_js_value(&vm);
        self.add_constant_value(
            big_int_in_map,
            crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
        );

        self.big_int_map.insert(key, big_int_in_map);
        big_int_in_map
    }

    // BytecodeGenerator.cpp:3439
    pub fn add_string_constant(
        &mut self,
        identifier: &crate::runtime::identifier::Identifier,
    ) -> crate::runtime::js_string::JSStringRef {
        let key = identifier.impl_();
        if let Some(existing) = self.string_map.get(&key) {
            return existing.clone();
        }
        let string_in_map = crate::runtime::js_string::js_string(&self.vm, identifier.string().string());
        self.add_constant_value(
            crate::runtime::js_value::JSValue::from_cell(string_in_map.cell_id()),
            crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
        );
        self.string_map.insert(key, string_in_map.clone());
        string_in_map
    }

    // BytecodeGenerator.cpp:3449
    pub fn add_template_object_constant(
        &mut self,
        descriptor: std::rc::Rc<crate::runtime::template_object_descriptor::TemplateObjectDescriptor>,
        end_offset: i32,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.template_object_descriptor_set.insert(descriptor.clone());
        let stored_descriptor = self.template_object_descriptor_set.get(&descriptor).unwrap().clone();
        let key = end_offset as u64;
        let descriptor_value = match self.template_descriptor_map.get(&key) {
            Some(existing) => existing.clone(),
            None => {
                let created = crate::runtime::js_template_object_descriptor::JSTemplateObjectDescriptor::create(
                    &self.vm,
                    stored_descriptor,
                    end_offset,
                );
                self.template_descriptor_map.insert(key, created.clone());
                created
            }
        };
        let index = self.add_constant_index();
        self.code_block.add_constant(crate::runtime::js_value::JSValue::from_cell(descriptor_value.cell_id()));
        Some(crate::bytecompiler::bytecode_generator::RegisterRef::new(&self.constant_pool_registers[index as usize]))
    }

    // BytecodeGenerator.cpp:3460
    pub fn emit_new_array_buffer(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        array: crate::runtime::js_cell_butterfly::JSCellButterflyRef,
        recommended_indexing_type: crate::runtime::indexing_type::IndexingType,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let array_constant = self.add_constant_value(
            crate::runtime::js_value::JSValue::from_cell(array.cell_id()),
            crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
        );
        crate::bytecode::bytecode_ops::OpNewArrayBuffer::emit(
            self,
            dst.as_ref().unwrap(),
            &array_constant,
            recommended_indexing_type,
        );
        dst
    }

    // BytecodeGenerator.cpp:3466
    pub fn emit_new_array(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        elements: Option<crate::parser::nodes::NodeRef<crate::parser::nodes::ElementNode>>,
        length: u32,
        recommended_indexing_type: crate::runtime::indexing_type::IndexingType,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let mut length = length;
        // Não solta a referência de `dst`: só o `ArrayNode` (`finalDestination`/`tempDestination`) e o
        // `RestElement` passam o cru, e aplicam `with_raw_register` no próprio site.
        let mut argv: Vec<crate::bytecompiler::bytecode_generator::RegisterRef> = Vec::new();
        let mut n = elements;
        while let Some(node) = n {
            if length == 0 {
                break;
            }
            length -= 1;
            let value = node.borrow().node.clone();
            assert!(!value.is_spread_expression());
            let temporary = self.new_temporary();
            argv.push(temporary.clone());
            // op_new_array exige que os valores iniciais sejam uma faixa sequencial de registradores.
            debug_assert!(argv.len() == 1 || argv[argv.len() - 1].borrow().index() == argv[argv.len() - 2].borrow().index() - 1);
            self.emit_node_expression(Some(temporary), &value);
            n = node.borrow().next.clone();
        }
        assert!(length == 0);
        let argv_count = argv.len() as u32;
        match argv.first() {
            Some(first) => crate::bytecode::bytecode_ops::OpNewArray::emit(
                self,
                dst.as_ref().unwrap(),
                first.borrow().virtual_register(),
                argv_count,
                recommended_indexing_type,
            ),
            None => crate::bytecode::bytecode_ops::OpNewArray::emit(
                self,
                dst.as_ref().unwrap(),
                crate::bytecode::virtual_register::VirtualRegister::new(0),
                argv_count,
                recommended_indexing_type,
            ),
        }
        dst
    }

    // BytecodeGenerator.cpp:3484
    pub fn emit_new_array_with_spread(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        elements: Option<crate::parser::nodes::NodeRef<crate::parser::nodes::ElementNode>>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let mut bit_vector = crate::wtf::bit_vector::BitVector::new();
        // Só o `ArrayNode` (`finalDestination`) passa `dst` cru, e aplica `with_raw_register` no próprio site.
        let mut argv: Vec<crate::bytecompiler::bytecode_generator::RegisterRef> = Vec::new();
        let mut node = elements.clone();
        while let Some(current) = node {
            bit_vector.set(argv.len(), current.borrow().node.is_spread_expression());

            argv.push(self.new_temporary());
            // op_new_array_with_spread exige que os valores iniciais sejam uma faixa sequencial de registradores.
            assert!(argv.len() == 1 || argv[argv.len() - 1].borrow().index() == argv[argv.len() - 2].borrow().index() - 1);
            node = current.borrow().next.clone();
        }

        assert!(!argv.is_empty());

        {
            let mut i = 0usize;
            let mut node = elements;
            while let Some(current) = node {
                let value = current.borrow().node.clone();
                if let crate::parser::nodes::Expression::SpreadExpression(spread) = &value {
                    let expression = spread.borrow().expression.clone();
                    let tmp = self.new_temporary();
                    self.emit_node_expression(Some(tmp.clone()), &expression);

                    let (divot, divot_start, divot_end) = {
                        let spread = spread.borrow();
                        (
                            spread.throwable.divot.clone(),
                            spread.throwable.divot_start.clone(),
                            spread.throwable.divot_end.clone(),
                        )
                    };
                    self.emit_expression_info(&divot, &divot_start, &divot_end);
                    crate::bytecode::bytecode_ops::OpSpread::emit(self, &argv[i], &tmp);
                } else {
                    self.emit_node_expression(Some(argv[i].clone()), &value);
                }
                i += 1;
                node = current.borrow().next.clone();
            }
        }

        let bit_vector_index = self.code_block.add_bit_vector(bit_vector);
        let argv_count = argv.len() as u32;
        crate::bytecode::bytecode_ops::OpNewArrayWithSpread::emit(
            self,
            dst.as_ref().unwrap(),
            argv[0].borrow().virtual_register(),
            argv_count,
            bit_vector_index,
        );
        dst
    }

    // BytecodeGenerator.cpp:3522
    pub fn emit_new_array_with_size(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        length: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpNewArrayWithSize::emit(self, dst.as_ref().unwrap(), length.as_ref().unwrap());
        dst
    }

    // BytecodeGenerator.cpp:3528
    pub fn emit_new_array_with_species(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        length: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        array: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpNewArrayWithSpecies::emit(
            self,
            dst.as_ref().unwrap(),
            length.as_ref().unwrap(),
            array.as_ref().unwrap(),
            value_profile,
        );
        dst
    }

    // BytecodeGenerator.cpp:3534
    pub fn emit_new_reg_exp(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        reg_exp: std::rc::Rc<crate::runtime::reg_exp::RegExp>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let reg_exp_constant = self.add_constant_value(
            crate::runtime::js_value::JSValue::from_cell(reg_exp.cell_id()),
            crate::runtime::js_cjs_value_types::SourceCodeRepresentation::Other,
        );
        crate::bytecode::bytecode_ops::OpNewRegExp::emit(self, dst.as_ref().unwrap(), &reg_exp_constant);
        dst
    }

    // BytecodeGenerator.cpp:3540
    pub fn emit_new_function_expression_common(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        function: &crate::parser::nodes::FunctionMetadataNodeRef,
    ) {
        use crate::parser::parser_modes::SourceParseMode;

        let executable = self.make_function(function);
        let index = self.code_block.add_function_expr(executable);
        let scope_register = self.scope_register();

        match function.parse_mode {
            SourceParseMode::GeneratorWrapperFunctionMode | SourceParseMode::GeneratorWrapperMethodMode => {
                crate::bytecode::bytecode_ops::OpNewGeneratorFuncExp::emit(
                    self,
                    dst.as_ref().unwrap(),
                    scope_register.as_ref().unwrap(),
                    index,
                );
            }
            SourceParseMode::AsyncFunctionMode | SourceParseMode::AsyncMethodMode | SourceParseMode::AsyncArrowFunctionMode => {
                crate::bytecode::bytecode_ops::OpNewAsyncFuncExp::emit(
                    self,
                    dst.as_ref().unwrap(),
                    scope_register.as_ref().unwrap(),
                    index,
                );
            }
            SourceParseMode::AsyncGeneratorWrapperFunctionMode | SourceParseMode::AsyncGeneratorWrapperMethodMode => {
                crate::bytecode::bytecode_ops::OpNewAsyncGeneratorFuncExp::emit(
                    self,
                    dst.as_ref().unwrap(),
                    scope_register.as_ref().unwrap(),
                    index,
                );
            }
            _ => {
                crate::bytecode::bytecode_ops::OpNewFuncExp::emit(
                    self,
                    dst.as_ref().unwrap(),
                    scope_register.as_ref().unwrap(),
                    index,
                );
            }
        }
    }

    // BytecodeGenerator.cpp:3564
    pub fn emit_new_function_expression(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: &crate::parser::nodes::NodeRef<crate::parser::nodes::FuncExprNode>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let metadata = func.borrow().metadata.clone();
        self.emit_new_function_expression_common(dst.clone(), &metadata);
        dst
    }

    // BytecodeGenerator.cpp:3570
    pub fn emit_new_arrow_function_expression(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: &crate::parser::nodes::NodeRef<crate::parser::nodes::ArrowFuncExprNode>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let metadata = func.borrow().metadata.clone();
        debug_assert!(matches!(
            metadata.parse_mode,
            crate::parser::parser_modes::SourceParseMode::ArrowFunctionMode
                | crate::parser::parser_modes::SourceParseMode::AsyncArrowFunctionMode
        ));
        self.emit_new_function_expression_common(dst.clone(), &metadata);
        dst
    }

    // BytecodeGenerator.cpp:3577
    pub fn emit_new_method_definition(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: &crate::parser::nodes::NodeRef<crate::parser::nodes::MethodDefinitionNode>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let metadata = func.borrow().metadata.clone();
        debug_assert!(crate::parser::parser_modes::is_method_parse_mode(metadata.parse_mode));
        self.emit_new_function_expression_common(dst.clone(), &metadata);
        dst
    }

    // BytecodeGenerator.cpp:3584
    pub fn emit_new_default_constructor(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        constructor_kind: crate::runtime::constructor_kind::ConstructorKind,
        name: &crate::runtime::identifier::Identifier,
        ecma_name: &crate::runtime::identifier::Identifier,
        class_source: &crate::parser::source_code::SourceCode,
        needs_class_field_initializer: crate::bytecode::executable_info::NeedsClassFieldInitializer,
        private_brand_requirement: crate::parser::parser_modes::PrivateBrandRequirement,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let executable = self.vm.builtin_executables().create_default_constructor(
            &self.vm,
            constructor_kind,
            name,
            needs_class_field_initializer,
            private_brand_requirement,
        ).unwrap();
        executable.borrow_mut().set_ecma_name(ecma_name.clone());
        executable.borrow_mut().set_class_source(class_source.clone());

        let index = self.code_block.add_function_expr(executable);

        let scope_register = self.scope_register();
        crate::bytecode::bytecode_ops::OpNewFuncExp::emit(self, dst.as_ref().unwrap(), scope_register.as_ref().unwrap(), index);
        dst
    }

    // BytecodeGenerator.cpp:3597
    pub fn emit_new_class_field_initializer_function(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        class_element_definitions: Vec<crate::bytecode::unlinked_function_executable::ClassElementDefinition>,
        is_derived: bool,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        use crate::bytecode::executable_info::DerivedContextType;
        use crate::parser::parser_modes::SuperBinding;

        let (new_derived_context_type, super_binding) = if !is_derived {
            (DerivedContextType::None, SuperBinding::NotNeeded)
        } else {
            (DerivedContextType::DerivedMethodContext, SuperBinding::Needed)
        };

        let variables_under_tdz = self.get_variables_under_tdz();
        let parent_private_name_environment = self.get_available_private_access_names();
        let parse_mode = crate::parser::parser_modes::SourceParseMode::ClassFieldInitializerMode;
        let construct_ability = crate::runtime::construct_ability::ConstructAbility::CannotConstruct;

        let token_location = crate::parser::parser_tokens::JSTokenLocation::default();
        let metadata = crate::parser::nodes::FunctionMetadataNode::new(
            &token_location,
            &token_location,
            0,
            0,
            0,
            0,
            0,
            crate::runtime::implementation_visibility::ImplementationVisibility::Private,
            crate::parser::parser_modes::STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
            crate::runtime::constructor_kind::ConstructorKind::None,
            super_binding,
            0,
            parse_mode,
            false,
        );
        let source = self.scope_node.borrow().source.clone();
        metadata.finish_parsing(
            &source,
            &crate::runtime::identifier::Identifier::default(),
            crate::parser::parser_modes::FunctionMode::MethodDefinition,
        );
        let initializer = crate::runtime::unlinked_function_executable::UnlinkedFunctionExecutable::create(
            &self.vm,
            &source,
            &metadata,
            if self.is_builtin_function() {
                crate::runtime::unlinked_function_executable::UnlinkedFunctionKind::UnlinkedBuiltinFunction
            } else {
                crate::runtime::unlinked_function_executable::UnlinkedFunctionKind::UnlinkedNormalFunction
            },
            construct_ability,
            crate::runtime::inline_attribute::InlineAttribute::Always,
            self.script_mode(),
            variables_under_tdz,
            Vec::new(),
            parent_private_name_environment,
            new_derived_context_type,
            crate::bytecode::executable_info::EvalContextType::InstanceFieldEvalContext,
            crate::bytecode::executable_info::NeedsClassFieldInitializer::No,
            crate::parser::parser_modes::PrivateBrandRequirement::None,
        );
        initializer.borrow_mut().set_class_element_definitions(class_element_definitions);

        let index = self.code_block.add_function_expr(initializer);
        let scope_register = self.scope_register();
        crate::bytecode::bytecode_ops::OpNewFuncExp::emit(self, dst.as_ref().unwrap(), scope_register.as_ref().unwrap(), index);
        dst
    }

    // BytecodeGenerator.cpp:3624
    pub fn emit_new_function(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        function: &crate::parser::nodes::FunctionMetadataNodeRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let executable = self.make_function(function);
        let index = self.code_block.add_function_decl(executable);
        let scope_register = self.scope_register();
        let parse_mode = function.parse_mode;
        if crate::parser::parser_modes::is_generator_wrapper_parse_mode(parse_mode) {
            crate::bytecode::bytecode_ops::OpNewGeneratorFunc::emit(
                self,
                dst.as_ref().unwrap(),
                scope_register.as_ref().unwrap(),
                index,
            );
        } else if parse_mode == crate::parser::parser_modes::SourceParseMode::AsyncFunctionMode {
            crate::bytecode::bytecode_ops::OpNewAsyncFunc::emit(
                self,
                dst.as_ref().unwrap(),
                scope_register.as_ref().unwrap(),
                index,
            );
        } else if crate::parser::parser_modes::is_async_generator_wrapper_parse_mode(parse_mode) {
            crate::bytecode::bytecode_ops::OpNewAsyncGeneratorFunc::emit(
                self,
                dst.as_ref().unwrap(),
                scope_register.as_ref().unwrap(),
                index,
            );
        } else {
            crate::bytecode::bytecode_ops::OpNewFunc::emit(
                self,
                dst.as_ref().unwrap(),
                scope_register.as_ref().unwrap(),
                index,
            );
        }
        dst
    }

    // BytecodeGenerator.cpp:3638
    pub fn should_set_function_name(&mut self, node: &crate::parser::nodes::Expression) -> bool {
        use crate::parser::nodes::Expression;
        let base_func_metadata = match node {
            Expression::FuncExpr(func) => Some(func.borrow().metadata.clone()),
            Expression::ArrowFuncExpr(func) => Some(func.borrow().metadata.clone()),
            Expression::MethodDefinition(func) => Some(func.borrow().metadata.clone()),
            _ => None,
        };
        if let Some(metadata) = base_func_metadata {
            if !metadata.ecma_name().is_null() {
                return false;
            }
        } else if let Expression::ClassExpr(class_expr_node) = node {
            if !class_expr_node.borrow().ecma_name().is_null() {
                return false;
            }
            if class_expr_node.borrow().has_static_property(&self.vm.property_names.name) {
                return false;
            }
        } else {
            return false;
        }

        true
    }

    // BytecodeGenerator.cpp:3656
    pub fn emit_set_function_name_identifier(
        &mut self,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        ident: &crate::runtime::identifier::Identifier,
    ) {
        let temporary = self.new_temporary();
        let name = self.emit_load_identifier(Some(temporary), ident);

        // FIXME: Deveríamos usar um op_call para uma função interna aqui.
        // https://bugs.webkit.org/show_bug.cgi?id=155547
        crate::bytecode::bytecode_ops::OpSetFunctionName::emit(self, value.as_ref().unwrap(), name.as_ref().unwrap());
    }

    // BytecodeGenerator.cpp:3665
    pub fn emit_set_function_name(
        &mut self,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        name: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) {
        // FIXME: Deveríamos usar um op_call para uma função interna aqui.
        // https://bugs.webkit.org/show_bug.cgi?id=155547
        crate::bytecode::bytecode_ops::OpSetFunctionName::emit(self, value.as_ref().unwrap(), name.as_ref().unwrap());
    }

    // BytecodeGenerator.cpp:3672
    pub fn emit_async_iterator_open(
        &mut self,
        iterator: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        next: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        symbol_iterator: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        iterable: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        node: &crate::parser::nodes::ThrowableExpressionData,
    ) {
        // Reserva espaço para o call frame. Espelha emitIteratorOpen.
        let mut call_frame: Vec<crate::bytecompiler::bytecode_generator::RegisterRef> = Vec::new();
        for _ in 0..crate::interpreter::call_frame::HEADER_SIZE_IN_REGISTERS {
            call_frame.push(self.new_temporary());
        }

        if self.should_emit_debug_hooks() {
            self.emit_debug_hook(
                crate::interpreter::interpreter::DebugHookType::WillExecuteExpression,
                &node.divot_start,
                None,
            );
        }

        self.emit_expression_info(&node.divot, &node.divot_start, &node.divot_end);
        let iterable_value_profile = self.next_value_profile_index();
        let iterator_value_profile = self.next_value_profile_index();
        let next_value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpAsyncIteratorOpen::emit(
            self,
            iterator.as_ref().unwrap(),
            next.as_ref().unwrap(),
            symbol_iterator.as_ref().unwrap(),
            iterable.this_register().as_ref().unwrap(),
            iterable.stack_offset(),
            iterable_value_profile,
            iterator_value_profile,
            next_value_profile,
        );
    }

    // BytecodeGenerator.cpp:3689
    pub fn emit_get_generic_async_iterator(
        &mut self,
        iterator: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        next: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        subject: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        node: &crate::parser::nodes::ThrowableExpressionData,
    ) {
        self.emit_expression_info(&node.divot, &node.divot_start, &node.divot_end);
        let temporary = self.new_temporary();
        let async_iterator_symbol = self.property_names().async_iterator_symbol.clone();
        let symbol_async_iterator = self.emit_get_by_id(Some(temporary), subject.clone(), &async_iterator_symbol);
        let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 0);
        let this_register = args.this_register();
        self.move_register(this_register.as_ref(), subject.as_ref().unwrap());
        self.emit_async_iterator_open(iterator, next, symbol_async_iterator, &mut args, node);
    }

    // BytecodeGenerator.cpp:3698
    pub fn emit_async_iterator_next(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        next: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        iterator: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        node: &crate::parser::nodes::ThrowableExpressionData,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // dst pode ser alias de value (o caminho async de emitDelegateYield reusa um temporário para os
        // dois): value é lido para o registrador de argumento da chamada abaixo antes de dst ser escrito
        // por OpAsyncIteratorNext::emit.
        let mut next_arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(
            self,
            None,
            if value.is_some() { 1 } else { 0 },
        );
        let this_register = next_arguments.this_register();
        self.move_register(this_register.as_ref(), iterator.as_ref().unwrap());
        if let Some(value) = &value {
            let argument_register = next_arguments.argument_register(0);
            self.move_register(argument_register.as_ref(), value);
        }

        // Reserva espaço para o call frame. Espelha emitIteratorNext / emitAsyncIteratorOpen; o ramo
        // genérico de op_async_iterator_next faz um next.call(iterator) de verdade, então
        // numCalleeLocals precisa cobrir o cabeçalho do frame do callee abaixo de argv.
        let mut call_frame: Vec<crate::bytecompiler::bytecode_generator::RegisterRef> = Vec::new();
        for _ in 0..crate::interpreter::call_frame::HEADER_SIZE_IN_REGISTERS {
            call_frame.push(self.new_temporary());
        }

        self.emit_expression_info(&node.divot, &node.divot_start, &node.divot_end);
        let killed = self.kill(dst.as_ref().unwrap());
        let generator_register = self.generator_register();
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpAsyncIteratorNext::emit(
            self,
            &killed,
            next.as_ref().unwrap(),
            next_arguments.this_register().as_ref().unwrap(),
            generator_register.as_ref().unwrap(),
            value.is_some(),
            next_arguments.stack_offset(),
            value_profile,
        );
        dst
    }

    // BytecodeGenerator.cpp:3719
    pub fn emit_call(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        expected_function: crate::bytecompiler::bytecode_generator::ExpectedFunction,
        call_arguments: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_call_op(
            crate::bytecode::opcode::OpcodeID::op_call,
            dst,
            func,
            expected_function,
            call_arguments,
            divot,
            divot_start,
            divot_end,
            debuggable_call,
        )
    }

    // BytecodeGenerator.cpp:3724
    pub fn emit_call_ignore_result(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        expected_function: crate::bytecompiler::bytecode_generator::ExpectedFunction,
        call_arguments: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) {
        // No C++ o destino é o `RegisterID*` cru de `newTemporary()` (contagem 0): o primeiro
        // `newTemporary()` dentro de `emitCallOp` o recupera e o call frame reaproveita o slot.
        let raw = dst.as_ref().map(|register| std::rc::Rc::clone(register.get()));
        let result = self.with_raw_register(raw.as_ref(), |generator| {
            generator.emit_call_op(
                crate::bytecode::opcode::OpcodeID::op_call_ignore_result,
                dst,
                func,
                expected_function,
                call_arguments,
                divot,
                divot_start,
                divot_end,
                debuggable_call,
            )
        });
        drop(result);
    }

    // BytecodeGenerator.cpp:3729
    pub fn emit_call_in_tail_position(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        expected_function: crate::bytecompiler::bytecode_generator::ExpectedFunction,
        call_arguments: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        if self.allow_tail_call_optimization {
            self.code_block.set_has_tail_calls();
            return self.emit_call_op(
                crate::bytecode::opcode::OpcodeID::op_tail_call,
                dst,
                func,
                expected_function,
                call_arguments,
                divot,
                divot_start,
                divot_end,
                debuggable_call,
            );
        }
        if self.allow_call_ignore_result_optimization {
            return self.emit_call_op(
                crate::bytecode::opcode::OpcodeID::op_call_ignore_result,
                dst,
                func,
                expected_function,
                call_arguments,
                divot,
                divot_start,
                divot_end,
                debuggable_call,
            );
        }
        self.emit_call_op(
            crate::bytecode::opcode::OpcodeID::op_call,
            dst,
            func,
            expected_function,
            call_arguments,
            divot,
            divot_start,
            divot_end,
            debuggable_call,
        )
    }

    // BytecodeGenerator.cpp:3740
    pub fn emit_call_direct_eval(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        call_arguments: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_call_op(
            crate::bytecode::opcode::OpcodeID::op_call_direct_eval,
            dst,
            func,
            crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
            call_arguments,
            divot,
            divot_start,
            divot_end,
            debuggable_call,
        )
    }

    // BytecodeGenerator.cpp:3745
    pub fn expected_function_for_identifier(
        &mut self,
        identifier: &crate::runtime::identifier::Identifier,
    ) -> crate::bytecompiler::bytecode_generator::ExpectedFunction {
        use crate::bytecompiler::bytecode_generator::ExpectedFunction;
        if self.should_emit_debug_hooks() {
            return ExpectedFunction::NoExpectedFunction;
        }
        if *identifier == self.property_names().object
            || *identifier == self.property_names().builtin_names().object_private_name()
        {
            return ExpectedFunction::ExpectObjectConstructor;
        }
        if *identifier == self.property_names().array
            || *identifier == self.property_names().builtin_names().array_private_name()
        {
            return ExpectedFunction::ExpectArrayConstructor;
        }
        ExpectedFunction::NoExpectedFunction
    }

    // BytecodeGenerator.cpp:3756
    pub fn emit_expected_function_snippet(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        expected_function: crate::bytecompiler::bytecode_generator::ExpectedFunction,
        call_arguments: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        done: &crate::bytecompiler::label::LabelRef,
    ) -> crate::bytecompiler::bytecode_generator::ExpectedFunction {
        use crate::bytecompiler::bytecode_generator::ExpectedFunction;
        let real_call = self.new_label();
        match expected_function {
            ExpectedFunction::ExpectObjectConstructor => {
                // Se o número de argumentos não é zero, não há nada interessante a fazer.
                if call_arguments.argument_count_including_this() >= 2 {
                    return ExpectedFunction::NoExpectedFunction;
                }

                let object_constant = self.move_link_time_constant(
                    None,
                    crate::bytecode::link_time_constant::LinkTimeConstant::Object,
                );
                let bound = real_call.bind_generator();
                crate::bytecode::bytecode_ops::OpJneqPtr::emit(
                    self,
                    func.as_ref().unwrap(),
                    object_constant.as_ref().unwrap(),
                    bound,
                );

                if !self.is_ignored_dst(dst.as_ref()) {
                    self.emit_new_object(dst);
                }
            }

            ExpectedFunction::ExpectArrayConstructor => {
                // Se você faz qualquer coisa além de "new Array()" ou "new Array(foo)", por ora não
                // inlinamos. O único motivo é que os argumentos da chamada estão na ordem oposta à que
                // op_new_array espera, então teríamos de mudar o op_new_array ou criar um
                // op_new_array_reverse. Nenhum dos dois parece valer a pena.
                if call_arguments.argument_count_including_this() > 2 {
                    return ExpectedFunction::NoExpectedFunction;
                }

                let array_constant = self.move_link_time_constant(
                    None,
                    crate::bytecode::link_time_constant::LinkTimeConstant::Array,
                );
                let bound = real_call.bind_generator();
                crate::bytecode::bytecode_ops::OpJneqPtr::emit(
                    self,
                    func.as_ref().unwrap(),
                    array_constant.as_ref().unwrap(),
                    bound,
                );

                if !self.is_ignored_dst(dst.as_ref()) {
                    if call_arguments.argument_count_including_this() == 2 {
                        let argument_register = call_arguments.argument_register(0);
                        self.emit_new_array_with_size(dst, argument_register);
                    } else {
                        assert!(call_arguments.argument_count_including_this() == 1);
                        crate::bytecode::bytecode_ops::OpNewArray::emit(
                            self,
                            dst.as_ref().unwrap(),
                            crate::bytecode::virtual_register::VirtualRegister::new(0),
                            0,
                            crate::runtime::indexing_type::ARRAY_WITH_UNDECIDED,
                        );
                    }
                }
            }

            _ => {
                assert!(expected_function == ExpectedFunction::NoExpectedFunction);
                return ExpectedFunction::NoExpectedFunction;
            }
        }

        let bound = done.bind_generator();
        crate::bytecode::bytecode_ops::OpJmp::emit(self, bound);
        self.emit_label(&real_call);

        expected_function
    }

    // BytecodeGenerator.cpp:3805
    pub fn compute_features_for_call_direct_eval(&mut self) -> crate::parser::parser_modes::LexicallyScopedFeatures {
        let mut features = self.lexically_scoped_features();

        for i in (0..self.lexical_scope_stack.len()).rev() {
            if self.lexical_scope_stack[i].is_with_scope {
                features |= crate::parser::parser_modes::TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE;
                break;
            }
        }

        features
    }

    // BytecodeGenerator.cpp:3820
    // `template<typename CallOp> emitCall`: `call_opcode` é `CallOp::opcodeID` (OpCall, OpCallDirectEval,
    // OpTailCall ou OpCallIgnoreResult) e escolhe o `emit` concreto.
    pub fn emit_call_op(
        &mut self,
        call_opcode: crate::bytecode::opcode::OpcodeID,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        expected_function: crate::bytecompiler::bytecode_generator::ExpectedFunction,
        call_arguments: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        use crate::bytecode::opcode::OpcodeID;
        let mut expected_function = expected_function;
        assert!(
            call_opcode == OpcodeID::op_call
                || call_opcode == OpcodeID::op_call_direct_eval
                || call_opcode == OpcodeID::op_tail_call
                || call_opcode == OpcodeID::op_call_ignore_result
        );
        assert!(func.is_some());

        // Gera o código dos argumentos.
        let mut argument = 0usize;
        if let Some(arguments_node) = call_arguments.arguments_node() {
            let mut n = arguments_node.borrow().list_node.clone();
            if let Some(first) = n.clone() {
                let first_expr = first.borrow().expr.clone();
                if first_expr.is_spread_expression() {
                    assert!(first.borrow().next.is_none());
                    assert!(call_opcode != OpcodeID::op_call_direct_eval);
                    let crate::parser::nodes::Expression::SpreadExpression(spread_node) = &first_expr else {
                        unreachable!("isSpreadExpression");
                    };
                    let expression = spread_node.borrow().expression.clone();
                    if expression.is_array_literal() {
                        let crate::parser::nodes::Expression::Array(array_node) = &expression else {
                            unreachable!("isArrayLiteral");
                        };
                        let elements = array_node.borrow().element.clone();
                        if let Some(elements) = elements {
                            let elements_value = elements.borrow().node.clone();
                            if elements.borrow().next.is_none() && elements_value.is_spread_expression() {
                                let crate::parser::nodes::Expression::SpreadExpression(spread) = &elements_value else {
                                    unreachable!("isSpreadExpression");
                                };
                                let expression = spread.borrow().expression.clone();
                                let argument_destination = call_arguments.argument_register(0);
                                let emitted = self.emit_node_expression(argument_destination, &expression);
                                let argument_register = self.temp_destination(emitted.as_ref());
                                let (spread_divot, spread_divot_start, spread_divot_end) = {
                                    let spread = spread.borrow();
                                    (
                                        spread.throwable.divot.clone(),
                                        spread.throwable.divot_start.clone(),
                                        spread.throwable.divot_end.clone(),
                                    )
                                };
                                self.emit_expression_info(&spread_divot, &spread_divot_start, &spread_divot_end);
                                crate::bytecode::bytecode_ops::OpSpread::emit(self, &argument_register, &argument_register);

                                let first_free_register = self.new_temporary();
                                return self.emit_call_varargs_op(
                                    Self::varargs_opcode_for(call_opcode),
                                    dst,
                                    func,
                                    call_arguments.this_register(),
                                    Some(argument_register),
                                    Some(first_free_register),
                                    0,
                                    divot,
                                    divot_start,
                                    divot_end,
                                    debuggable_call,
                                );
                            }
                        }
                    }
                    let argument_destination = call_arguments.argument_register(0);
                    let argument_register = expression.emit_bytecode(self, argument_destination);
                    let first_free_register = self.new_temporary();
                    return self.emit_call_varargs_op(
                        Self::varargs_opcode_for(call_opcode),
                        dst,
                        func,
                        call_arguments.this_register(),
                        argument_register,
                        Some(first_free_register),
                        0,
                        divot,
                        divot_start,
                        divot_end,
                        debuggable_call,
                    );
                }
            }
            while let Some(node) = n {
                let expression = node.borrow().expr.clone();
                let argument_destination = call_arguments.argument_register(argument);
                argument += 1;
                self.emit_node_expression(argument_destination, &expression);
                n = node.borrow().next.clone();
            }
        }

        // Reserva espaço para o call frame.
        let mut call_frame: Vec<crate::bytecompiler::bytecode_generator::RegisterRef> = Vec::new();
        for _ in 0..crate::interpreter::call_frame::HEADER_SIZE_IN_REGISTERS {
            call_frame.push(self.new_temporary());
        }

        if self.should_emit_debug_hooks()
            && debuggable_call == crate::bytecompiler::bytecode_generator::DebuggableCall::Yes
        {
            self.emit_debug_hook(
                crate::interpreter::interpreter::DebugHookType::WillExecuteExpression,
                divot_start,
                None,
            );
        }

        self.emit_expression_info(divot, divot_start, divot_end);

        let done = self.new_label();
        expected_function = self.emit_expected_function_snippet(dst.clone(), func.clone(), expected_function, call_arguments, &done);

        if call_opcode == OpcodeID::op_tail_call {
            self.emit_log_shadow_chicken_tail_if_necessary();
        }

        // Emite a chamada.
        assert!(dst.is_some());
        assert!(!self.is_ignored_dst(dst.as_ref()));
        let argument_count_including_this = call_arguments.argument_count_including_this();
        let stack_offset = call_arguments.stack_offset();
        if call_opcode == OpcodeID::op_call_direct_eval {
            let this_register = self.this_register();
            let scope_register = self.scope_register();
            let features = self.compute_features_for_call_direct_eval();
            let value_profile = self.next_value_profile_index();
            crate::bytecode::bytecode_ops::OpCallDirectEval::emit(
                self,
                dst.as_ref().unwrap(),
                func.as_ref().unwrap(),
                argument_count_including_this,
                stack_offset,
                &this_register,
                scope_register.as_ref().unwrap(),
                features as u32,
                value_profile,
            );
        } else if call_opcode == OpcodeID::op_call_ignore_result {
            crate::bytecode::bytecode_ops::OpCallIgnoreResult::emit(
                self,
                func.as_ref().unwrap(),
                argument_count_including_this,
                stack_offset,
            );
            if self.should_emit_type_profiler_hooks() {
                self.emit_load_js_value(dst.clone(), crate::runtime::js_value::js_undefined());
            }
        } else if call_opcode == OpcodeID::op_tail_call {
            crate::bytecode::bytecode_ops::OpTailCall::emit(
                self,
                dst.as_ref().unwrap(),
                func.as_ref().unwrap(),
                argument_count_including_this,
                stack_offset,
            );
        } else {
            let value_profile = self.next_value_profile_index();
            crate::bytecode::bytecode_ops::OpCall::emit(
                self,
                dst.as_ref().unwrap(),
                func.as_ref().unwrap(),
                argument_count_including_this,
                stack_offset,
                value_profile,
            );
        }

        if expected_function != crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction {
            self.emit_label(&done);
        }

        dst
    }

    // `VarArgsOp<CallOp>::Type` (definido no início da parte 1) em forma de `OpcodeID`.
    fn varargs_opcode_for(call_opcode: crate::bytecode::opcode::OpcodeID) -> crate::bytecode::opcode::OpcodeID {
        use crate::bytecode::opcode::OpcodeID;
        match call_opcode {
            OpcodeID::op_tail_call => OpcodeID::op_tail_call_varargs,
            OpcodeID::op_construct => OpcodeID::op_construct_varargs,
            OpcodeID::op_super_construct => OpcodeID::op_super_construct_varargs,
            _ => OpcodeID::op_call_varargs,
        }
    }

    // BytecodeGenerator.cpp:3890
    pub fn emit_call_varargs(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        arguments: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_free_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_var_arg_offset: i32,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_call_varargs_op(
            crate::bytecode::opcode::OpcodeID::op_call_varargs,
            dst,
            func,
            this_register,
            arguments,
            first_free_register,
            first_var_arg_offset,
            divot,
            divot_start,
            divot_end,
            debuggable_call,
        )
    }

    // BytecodeGenerator.cpp:3895
    pub fn emit_call_varargs_in_tail_position(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        arguments: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_free_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_var_arg_offset: i32,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let opcode = if self.allow_tail_call_optimization {
            crate::bytecode::opcode::OpcodeID::op_tail_call_varargs
        } else {
            crate::bytecode::opcode::OpcodeID::op_call_varargs
        };
        self.emit_call_varargs_op(
            opcode,
            dst,
            func,
            this_register,
            arguments,
            first_free_register,
            first_var_arg_offset,
            divot,
            divot_start,
            divot_end,
            debuggable_call,
        )
    }

    // BytecodeGenerator.cpp:3902
    pub fn emit_construct_varargs(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        arguments: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_free_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_var_arg_offset: i32,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_call_varargs_op(
            crate::bytecode::opcode::OpcodeID::op_construct_varargs,
            dst,
            func,
            this_register,
            arguments,
            first_free_register,
            first_var_arg_offset,
            divot,
            divot_start,
            divot_end,
            debuggable_call,
        )
    }

    // BytecodeGenerator.cpp:3907
    pub fn emit_super_construct_varargs(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        arguments: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_free_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_var_arg_offset: i32,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_call_varargs_op(
            crate::bytecode::opcode::OpcodeID::op_super_construct_varargs,
            dst,
            func,
            this_register,
            arguments,
            first_free_register,
            first_var_arg_offset,
            divot,
            divot_start,
            divot_end,
            debuggable_call,
        )
    }

    // BytecodeGenerator.cpp:3913
    // `template<typename VarargsOp> emitCallVarargs`: `varargs_opcode` é `VarargsOp::opcodeID`.
    pub fn emit_call_varargs_op(
        &mut self,
        varargs_opcode: crate::bytecode::opcode::OpcodeID,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        func: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        this_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        arguments: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_free_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        first_var_arg_offset: i32,
        divot: &crate::parser::parser_tokens::JSTextPosition,
        divot_start: &crate::parser::parser_tokens::JSTextPosition,
        divot_end: &crate::parser::parser_tokens::JSTextPosition,
        debuggable_call: crate::bytecompiler::bytecode_generator::DebuggableCall,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        use crate::bytecode::opcode::OpcodeID;
        if self.should_emit_debug_hooks()
            && debuggable_call == crate::bytecompiler::bytecode_generator::DebuggableCall::Yes
        {
            self.emit_debug_hook(
                crate::interpreter::interpreter::DebugHookType::WillExecuteExpression,
                divot_start,
                None,
            );
        }

        self.emit_expression_info(divot, divot_start, divot_end);

        if varargs_opcode == OpcodeID::op_tail_call_varargs {
            self.emit_log_shadow_chicken_tail_if_necessary();
        }

        // Emite a chamada.
        assert!(!self.is_ignored_dst(dst.as_ref()));
        // `arguments ? arguments : VirtualRegister(0)`.
        let arguments_register = match &arguments {
            Some(arguments) => arguments.borrow().virtual_register(),
            None => crate::bytecode::virtual_register::VirtualRegister::new(0),
        };
        match varargs_opcode {
            OpcodeID::op_tail_call_varargs => {
                crate::bytecode::bytecode_ops::OpTailCallVarargs::emit(
                    self,
                    dst.as_ref().unwrap(),
                    func.as_ref().unwrap(),
                    this_register.as_ref().unwrap(),
                    arguments_register,
                    first_free_register.as_ref().unwrap(),
                    first_var_arg_offset,
                );
            }
            OpcodeID::op_construct_varargs => {
                let value_profile = self.next_value_profile_index();
                crate::bytecode::bytecode_ops::OpConstructVarargs::emit(
                    self,
                    dst.as_ref().unwrap(),
                    func.as_ref().unwrap(),
                    this_register.as_ref().unwrap(),
                    arguments_register,
                    first_free_register.as_ref().unwrap(),
                    first_var_arg_offset,
                    value_profile,
                );
            }
            OpcodeID::op_super_construct_varargs => {
                let value_profile = self.next_value_profile_index();
                crate::bytecode::bytecode_ops::OpSuperConstructVarargs::emit(
                    self,
                    dst.as_ref().unwrap(),
                    func.as_ref().unwrap(),
                    this_register.as_ref().unwrap(),
                    arguments_register,
                    first_free_register.as_ref().unwrap(),
                    first_var_arg_offset,
                    value_profile,
                );
            }
            _ => {
                let value_profile = self.next_value_profile_index();
                crate::bytecode::bytecode_ops::OpCallVarargs::emit(
                    self,
                    dst.as_ref().unwrap(),
                    func.as_ref().unwrap(),
                    this_register.as_ref().unwrap(),
                    arguments_register,
                    first_free_register.as_ref().unwrap(),
                    first_var_arg_offset,
                    value_profile,
                );
            }
        }
        assert!(self.code_block.has_checkpoints());
        dst
    }

    // BytecodeGenerator.cpp:3933
    pub fn emit_log_shadow_chicken_prologue_if_necessary(&mut self) {
        if !self.should_emit_debug_hooks() && !crate::runtime::options::Options::always_use_shadow_chicken() {
            return;
        }
        let scope_register = self.scope_register();
        crate::bytecode::bytecode_ops::OpLogShadowChickenPrologue::emit(self, scope_register.as_ref().unwrap());
    }

    // BytecodeGenerator.cpp:3940
    pub fn emit_log_shadow_chicken_tail_if_necessary(&mut self) {
        if !self.should_emit_debug_hooks() && !crate::runtime::options::Options::always_use_shadow_chicken() {
            return;
        }
        let this_register = self.this_register();
        let scope_register = self.scope_register();
        crate::bytecode::bytecode_ops::OpLogShadowChickenTail::emit(self, &this_register, scope_register.as_ref().unwrap());
    }

    // BytecodeGenerator.cpp:3947
    pub fn emit_call_define_property(
        &mut self,
        new_obj: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property_name_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        value_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        getter_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        setter_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        options: u32,
        position: &crate::parser::parser_tokens::JSTextPosition,
    ) {
        let mut attributes = crate::runtime::define_property_attributes::DefinePropertyAttributes::default();
        if options & PROPERTY_CONFIGURABLE != 0 {
            attributes.set_configurable(true);
        }

        if options & PROPERTY_WRITABLE != 0 {
            attributes.set_writable(true);
        } else if value_register.is_some() {
            attributes.set_writable(false);
        }

        if options & PROPERTY_ENUMERABLE != 0 {
            attributes.set_enumerable(true);
        }

        if value_register.is_some() {
            attributes.set_value();
        }
        if getter_register.is_some() {
            attributes.set_get();
        }
        if setter_register.is_some() {
            attributes.set_set();
        }

        assert!(value_register.is_none() || (getter_register.is_none() && setter_register.is_none()));

        self.emit_expression_info(position, position, position);

        if attributes.has_get() || attributes.has_set() {
            let mut throw_type_error_function = None;
            if !attributes.has_get() || !attributes.has_set() {
                throw_type_error_function = self.move_link_time_constant(
                    None,
                    crate::bytecode::link_time_constant::LinkTimeConstant::ThrowTypeErrorFunction,
                );
            }

            let getter = if attributes.has_get() {
                getter_register
            } else {
                throw_type_error_function.clone()
            };

            let setter = if attributes.has_set() {
                setter_register
            } else {
                throw_type_error_function
            };

            let attributes_value = self.emit_load_js_value(
                None,
                crate::runtime::js_value::JSValue::from_u32(attributes.raw_representation()),
            );
            crate::bytecode::bytecode_ops::OpDefineAccessorProperty::emit(
                self,
                new_obj.as_ref().unwrap(),
                property_name_register.as_ref().unwrap(),
                getter.as_ref().unwrap(),
                setter.as_ref().unwrap(),
                attributes_value.as_ref().unwrap(),
            );
        } else {
            let attributes_value = self.emit_load_js_value(
                None,
                crate::runtime::js_value::JSValue::from_u32(attributes.raw_representation()),
            );
            crate::bytecode::bytecode_ops::OpDefineDataProperty::emit(
                self,
                new_obj.as_ref().unwrap(),
                property_name_register.as_ref().unwrap(),
                value_register.as_ref().unwrap(),
                attributes_value.as_ref().unwrap(),
            );
        }
    }

    // BytecodeGenerator.cpp:3996
    pub fn emit_return(
        &mut self,
        src: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // Funções normais e construtores naked não tratam `return` de forma especial.
        if self.is_constructor() && self.constructor_kind() != crate::runtime::constructor_kind::ConstructorKind::Naked {
            let is_derived = self.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends;
            let src_is_this = src.as_ref().unwrap().borrow().index() == self.this_register.index();

            if !src_is_this {
                let is_object_label = self.new_label();
                let temporary = self.new_temporary();
                let is_object = self.emit_is_object(Some(temporary), src.as_ref().unwrap());
                self.emit_jump_if_true_raw(is_object.as_ref().unwrap(), &is_object_label);

                if is_derived {
                    let is_undefined_label = self.new_label();
                    let temporary = self.new_temporary();
                    let is_undefined = self.emit_is_undefined(Some(temporary), src.as_ref().unwrap());
                    self.emit_jump_if_true_raw(is_undefined.as_ref().unwrap(), &is_undefined_label);

                    let crate::parser::nodes::ScopeNodeRef::Function(function_node) = &self.scope_node else {
                        panic!("ASSERT(m_scopeNode->isFunctionNode())");
                    };
                    let class_name = function_node.borrow().ident.string().string().clone();
                    if class_name.is_null() || class_name.is_empty() {
                        self.emit_throw_type_error("Cannot return a non-object type in the constructor of a derived class.");
                    } else {
                        let error_message = crate::wtf::text::try_make_string_dyn(&[
                            &"Cannot return a non-object type in the constructor of a derived class ",
                            &&class_name,
                            &".",
                        ])
                        .unwrap_or_default();
                        let identifier = crate::runtime::identifier::Identifier::from_string(&self.vm, &error_message);
                        self.emit_throw_type_error_identifier(&identifier);
                    }

                    self.emit_label(&is_undefined_label);
                }

                let this_value = self.ensure_this();
                crate::bytecode::bytecode_ops::OpRet::emit(self, &this_value);
                self.emit_label(&is_object_label);
            }
        }

        crate::bytecode::bytecode_ops::OpRet::emit(self, src.as_ref().unwrap());
        src
    }

    /// `dst == ignoredResult()` com `dst` nulo possível (nulo nunca é o `ignoredResult()`); o nome
    /// difere de `is_ignored_result` porque este recebe `Option`.
    pub fn is_ignored_dst(&self, dst: Option<&crate::bytecompiler::bytecode_generator::RegisterRef>) -> bool {
        dst.map_or(false, |register| self.is_ignored_result(register))
    }
}
