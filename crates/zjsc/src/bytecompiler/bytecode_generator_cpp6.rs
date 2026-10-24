// Parte 6 de bytecompiler/BytecodeGenerator.cpp (linhas 5097 a 6096 do .cpp). Juntada por include!.
// Convenção das partes anteriores: `Option<RegisterRef>` é o `RegisterID*` anulável do C++ e `RefPtr<RegisterID>`
// vira `RegisterRef`; `Ref<Label>` vira `crate::bytecompiler::label::LabelRef`.
// A função anterior (emitEnumeration, .cpp:4964) começa antes da linha 5000 e pertence à parte 5.

impl BytecodeGenerator {
    // BytecodeGenerator.cpp:5097
    pub fn emit_get_template_object(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        tagged_template: &crate::parser::nodes::NodeRef<crate::parser::nodes::TaggedTemplateNode>,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let mut raw_strings = Vec::new();
        let mut cooked_strings = Vec::new();

        let mut template_string = tagged_template.borrow().template_literal.borrow().template_strings.clone();
        while let Some(current) = template_string {
            let string_node = current.borrow().node.clone();
            let string = string_node.borrow();
            assert!(string.raw.is_some());
            raw_strings.push(string.raw.as_ref().unwrap().string().string().clone());
            match &string.cooked {
                None => cooked_strings.push(None),
                Some(cooked) => cooked_strings.push(Some(cooked.string().string().clone())),
            }
            let next = current.borrow().next.clone();
            template_string = next;
        }
        let descriptor = crate::runtime::template_object_descriptor::TemplateObjectDescriptor::create(raw_strings, cooked_strings);
        let constant = self.add_template_object_constant(descriptor, tagged_template.borrow().end_offset());
        if dst.is_none() {
            return constant;
        }
        self.move_register(dst.as_ref(), constant.as_ref().unwrap())
    }

    // BytecodeGenerator.cpp:5118
    pub fn emit_get_global_private(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property: &crate::runtime::identifier::Identifier,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let dst = self.temp_destination(dst.as_ref());
        let var = self.variable(property, crate::bytecompiler::bytecode_generator::ThisResolutionType::Local);
        if let Some(local) = var.local() {
            return self.move_register(Some(&dst), &local);
        }

        let scope = self.new_temporary();
        let resolved = self.emit_resolve_scope(Some(scope.clone()), &var);
        self.move_register(Some(&scope), resolved.as_ref().unwrap());
        self.emit_get_from_scope(
            Some(dst),
            Some(scope),
            &var,
            crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
        )
    }

    // BytecodeGenerator.cpp:5130
    pub fn emit_enumerator_next(
        &mut self,
        property_name: &crate::bytecompiler::bytecode_generator::RegisterRef,
        mode: &crate::bytecompiler::bytecode_generator::RegisterRef,
        index: &crate::bytecompiler::bytecode_generator::RegisterRef,
        base: &crate::bytecompiler::bytecode_generator::RegisterRef,
        enumerator: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) {
        crate::bytecode::bytecode_ops::OpEnumeratorNext::emit(self, property_name, mode, index, base, enumerator);
    }

    // BytecodeGenerator.cpp:5135
    pub fn emit_enumerator_has_own_property(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: &crate::bytecompiler::bytecode_generator::RegisterRef,
        mode: &crate::bytecompiler::bytecode_generator::RegisterRef,
        property_name: &crate::bytecompiler::bytecode_generator::RegisterRef,
        index: &crate::bytecompiler::bytecode_generator::RegisterRef,
        enumerator: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpEnumeratorHasOwnProperty::emit(
            self,
            dst.as_ref().unwrap(),
            base,
            mode,
            property_name,
            index,
            enumerator,
        );
        dst
    }

    // BytecodeGenerator.cpp:5141
    pub fn emit_get_property_enumerator(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpGetPropertyEnumerator::emit(self, dst.as_ref().unwrap(), base);
        dst
    }

    // BytecodeGenerator.cpp:5147
    pub fn emit_is_cell_with_type(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: &crate::bytecompiler::bytecode_generator::RegisterRef,
        type_: crate::runtime::js_type::JSType,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpIsCellWithType::emit(self, dst.as_ref().unwrap(), src, type_);
        dst
    }

    // BytecodeGenerator.cpp:5153
    pub fn emit_is_object(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpIsObject::emit(self, dst.as_ref().unwrap(), src);
        dst
    }

    // BytecodeGenerator.cpp:5159
    pub fn emit_is_callable(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpIsCallable::emit(self, dst.as_ref().unwrap(), src);
        dst
    }

    // BytecodeGenerator.cpp:5165
    pub fn emit_is_constructor(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpIsConstructor::emit(self, dst.as_ref().unwrap(), src);
        dst
    }

    // BytecodeGenerator.cpp:5171
    pub fn emit_is_number(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpIsNumber::emit(self, dst.as_ref().unwrap(), src);
        dst
    }

    // BytecodeGenerator.cpp:5177
    pub fn emit_is_undefined_or_null(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpIsUndefinedOrNull::emit(self, dst.as_ref().unwrap(), src);
        dst
    }

    // BytecodeGenerator.cpp:5183
    pub fn emit_is_empty(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpIsEmpty::emit(self, dst.as_ref().unwrap(), src);
        dst
    }

    // BytecodeGenerator.cpp:5189
    pub fn emit_load_arrow_function_lexical_environment(
        &mut self,
        identifier: &crate::runtime::identifier::Identifier,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        debug_assert!(
            self.code_block.is_arrow_function()
                || self.code_block.is_arrow_function_context()
                || self.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends
                || self.code_type == crate::bytecode::code_type::CodeType::EvalCode
                || self.code_block.parse_mode() == crate::parser::parser_modes::SourceParseMode::ClassFieldInitializerMode
        );

        let variable = self.variable(identifier, crate::bytecompiler::bytecode_generator::ThisResolutionType::Scoped);
        self.emit_resolve_scope(None, &variable)
    }

    // BytecodeGenerator.cpp:5196
    pub fn emit_load_this_from_arrow_function_lexical_environment(&mut self) {
        let this_private_name = self.property_names().builtin_names().this_private_name().clone();
        let scope = self.emit_load_arrow_function_lexical_environment(&this_private_name);
        let variable = self.variable(&this_private_name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Scoped);
        let this_register = self.this_register();
        self.emit_get_from_scope(
            Some(this_register),
            scope,
            &variable,
            crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
        );
    }

    // BytecodeGenerator.cpp:5201
    pub fn emit_load_new_target_from_arrow_function_lexical_environment(
        &mut self,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let new_target_local_private_name = self.property_names().builtin_names().new_target_local_private_name().clone();
        let new_target_var = self.variable(
            &new_target_local_private_name,
            crate::bytecompiler::bytecode_generator::ThisResolutionType::Local,
        );

        let scope = self.emit_load_arrow_function_lexical_environment(&new_target_local_private_name);
        let new_target_register = self.new_target_register.clone();
        self.emit_get_from_scope(
            new_target_register,
            scope,
            &new_target_var,
            crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
        )
    }

    // BytecodeGenerator.cpp:5209
    pub fn emit_load_derived_constructor_from_arrow_function_lexical_environment(
        &mut self,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let derived_constructor_private_name =
            self.property_names().builtin_names().derived_constructor_private_name().clone();
        let proto_scope_var = self.variable(
            &derived_constructor_private_name,
            crate::bytecompiler::bytecode_generator::ThisResolutionType::Local,
        );
        let destination = self.new_temporary();
        let scope = self.emit_load_arrow_function_lexical_environment(&derived_constructor_private_name);
        self.emit_get_from_scope(
            Some(destination),
            scope,
            &proto_scope_var,
            crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
        )
    }

    // BytecodeGenerator.cpp:5215
    pub fn emit_load_derived_constructor(&mut self) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        debug_assert!(
            self.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends
                || self.is_derived_constructor_context()
        );
        if self.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends {
            return Some(self.callee_register());
        }
        self.emit_load_derived_constructor_from_arrow_function_lexical_environment()
    }

    // BytecodeGenerator.cpp:5223
    pub fn ensure_this(&mut self) -> crate::bytecompiler::bytecode_generator::RegisterRef {
        if self.constructor_kind() == crate::runtime::constructor_kind::ConstructorKind::Extends
            || self.is_derived_constructor_context()
        {
            if (self.needs_to_update_arrow_function_context() && self.is_super_call_used_in_inner_arrow_function())
                || self.code_block.parse_mode() == crate::parser::parser_modes::SourceParseMode::AsyncArrowFunctionBodyMode
            {
                self.emit_load_this_from_arrow_function_lexical_environment();
            }

            let this_register = self.this_register();
            self.emit_tdz_check(&this_register);
        }

        self.this_register()
    }

    // BytecodeGenerator.cpp:5235
    pub fn is_this_used_in_inner_arrow_function(&mut self) -> bool {
        let scope_node = self.scope_node.borrow();
        let used = scope_node.do_any_inner_arrow_functions_use_this()
            || scope_node.do_any_inner_arrow_functions_use_super_property()
            || scope_node.do_any_inner_arrow_functions_use_super_call()
            || scope_node.do_any_inner_arrow_functions_use_eval();
        drop(scope_node);
        used || self.uses_eval()
    }

    // BytecodeGenerator.cpp:5240
    pub fn is_arguments_used_in_inner_arrow_function(&mut self) -> bool {
        let scope_node = self.scope_node.borrow();
        scope_node.do_any_inner_arrow_functions_use_arguments() || scope_node.do_any_inner_arrow_functions_use_eval()
    }

    // BytecodeGenerator.cpp:5245
    pub fn is_new_target_used_in_inner_arrow_function(&mut self) -> bool {
        let scope_node = self.scope_node.borrow();
        let used = scope_node.do_any_inner_arrow_functions_use_new_target()
            || scope_node.do_any_inner_arrow_functions_use_super_call()
            || scope_node.do_any_inner_arrow_functions_use_eval();
        drop(scope_node);
        used || self.uses_eval()
    }

    // BytecodeGenerator.cpp:5250
    pub fn is_super_used_in_inner_arrow_function(&mut self) -> bool {
        let scope_node = self.scope_node.borrow();
        let used = scope_node.do_any_inner_arrow_functions_use_super_call()
            || scope_node.do_any_inner_arrow_functions_use_super_property()
            || scope_node.do_any_inner_arrow_functions_use_eval();
        drop(scope_node);
        used || self.uses_eval()
    }

    // BytecodeGenerator.cpp:5255
    pub fn is_super_call_used_in_inner_arrow_function(&mut self) -> bool {
        let scope_node = self.scope_node.borrow();
        let used = scope_node.do_any_inner_arrow_functions_use_super_call()
            || scope_node.do_any_inner_arrow_functions_use_eval();
        drop(scope_node);
        used || self.uses_eval()
    }

    // BytecodeGenerator.cpp:5260
    pub fn emit_put_new_target_to_arrow_function_context_scope(&mut self) {
        if self.is_new_target_used_in_inner_arrow_function() {
            assert!(self.arrow_function_context_lexical_environment_register.is_some());

            let new_target_local_private_name =
                self.property_names().builtin_names().new_target_local_private_name().clone();
            let new_target_var = self.variable(
                &new_target_local_private_name,
                crate::bytecompiler::bytecode_generator::ThisResolutionType::Local,
            );
            let scope = self.arrow_function_context_lexical_environment_register.clone();
            let new_target = self.new_target();
            self.emit_put_to_scope(
                scope,
                &new_target_var,
                Some(new_target),
                crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                crate::runtime::get_put_info::InitializationMode::Initialization,
            );
        }
    }

    // BytecodeGenerator.cpp:5270
    pub fn emit_put_derived_constructor_to_arrow_function_context_scope(&mut self) {
        if self.needs_derived_constructor_in_arrow_function_lexical_environment() {
            assert!(self.arrow_function_context_lexical_environment_register.is_some());

            let derived_constructor_private_name =
                self.property_names().builtin_names().derived_constructor_private_name().clone();
            let proto_scope = self.variable(
                &derived_constructor_private_name,
                crate::bytecompiler::bytecode_generator::ThisResolutionType::Local,
            );
            let scope = self.arrow_function_context_lexical_environment_register.clone();
            let callee_register = self.callee_register();
            self.emit_put_to_scope(
                scope,
                &proto_scope,
                Some(callee_register),
                crate::runtime::get_put_info::ResolveMode::DoNotThrowIfNotFound,
                crate::runtime::get_put_info::InitializationMode::Initialization,
            );
        }
    }

    // BytecodeGenerator.cpp:5280
    pub fn emit_put_this_to_arrow_function_context_scope(&mut self) {
        if self.is_this_used_in_inner_arrow_function()
            || (self.scope_node.borrow().uses_super_call() && self.code_type == crate::bytecode::code_type::CodeType::EvalCode)
        {
            assert!(
                self.is_derived_constructor_context() || self.arrow_function_context_lexical_environment_register.is_some()
            );

            let this_private_name = self.property_names().builtin_names().this_private_name().clone();
            let this_var = self.variable(&this_private_name, crate::bytecompiler::bytecode_generator::ThisResolutionType::Scoped);
            let scope = if self.is_derived_constructor_context() {
                self.emit_load_arrow_function_lexical_environment(&this_private_name)
            } else {
                self.arrow_function_context_lexical_environment_register.clone()
            };

            let this_register = self.this_register();
            self.emit_put_to_scope(
                scope,
                &this_var,
                Some(this_register),
                crate::runtime::get_put_info::ResolveMode::ThrowIfNotFound,
                crate::runtime::get_put_info::InitializationMode::NotInitialization,
            );
        }
    }

    // BytecodeGenerator.cpp:5292
    pub fn push_for_in_scope(
        &mut self,
        local_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property_name_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        property_offset_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        enumerator_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        mode_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        base_variable: Option<crate::bytecompiler::bytecode_generator::Variable>,
    ) {
        if local_register.is_none() {
            return;
        }
        let body_bytecode_start_offset = self.instructions().size_in_bytes() as u32;
        self.for_in_context_stack.push(std::rc::Rc::new(std::cell::RefCell::new(
            crate::bytecompiler::bytecode_generator::ForInContext::new(
                local_register,
                property_name_register,
                property_offset_register,
                enumerator_register,
                mode_register,
                base_variable,
                body_bytecode_start_offset,
            ),
        )));
        self.disable_peephole_optimization();
    }

    // BytecodeGenerator.cpp:5301
    pub fn pop_for_in_scope(&mut self, local_register: Option<crate::bytecompiler::bytecode_generator::RegisterRef>) {
        if local_register.is_none() {
            return;
        }
        let body_bytecode_end_offset = self.instructions().size_in_bytes() as u32;
        let context = self.for_in_context_stack.last().unwrap().clone();
        context.borrow_mut().finalize(self, body_bytecode_end_offset);
        self.for_in_context_stack.pop();
    }

    // BytecodeGenerator.cpp:5310
    pub fn emit_rest_parameter(
        &mut self,
        result: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        num_parameters_to_skip: u32,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        crate::bytecode::bytecode_ops::OpCreateRest::emit(self, result.as_ref().unwrap(), num_parameters_to_skip);

        result
    }

    // BytecodeGenerator.cpp:5317
    pub fn emit_require_object_coercible(&mut self, value: &crate::bytecompiler::bytecode_generator::RegisterRef, error: &str) {
        let target = self.new_label();
        let bound = target.bind_generator();
        crate::bytecode::bytecode_ops::OpJnundefinedOrNull::emit(self, value, bound);
        self.emit_throw_type_error(error);
        self.emit_label(&target);
    }

    // BytecodeGenerator.cpp:5325
    pub fn emit_require_object_coercible_for_destructuring(
        &mut self,
        value: &crate::bytecompiler::bytecode_generator::RegisterRef,
        property_name: Option<&crate::runtime::identifier::Identifier>,
    ) {
        let target = self.new_label();
        let bound = target.bind_generator();
        crate::bytecode::bytecode_ops::OpJnundefinedOrNull::emit(self, value, bound);

        match property_name {
            Some(property_name) if !property_name.is_null() => {
                let error_message = crate::wtf::text::try_make_string_dyn(&[
                    &"Cannot destructure property '",
                    &property_name.string().string(),
                    &"' from null or undefined value",
                ])
                .unwrap_or_default();
                let identifier = crate::runtime::identifier::Identifier::from_string(&self.vm, &error_message);
                self.emit_throw_type_error_identifier(&identifier);
            }
            _ => self.emit_throw_type_error("Cannot destructure null or undefined value"),
        }

        self.emit_label(&target);
    }

    // BytecodeGenerator.cpp:5342
    pub fn emit_yield_point(
        &mut self,
        argument: &crate::bytecompiler::bytecode_generator::RegisterRef,
        result: crate::runtime::js_async_generator::AsyncGeneratorSuspendReason,
    ) {
        let merge_point = self.new_label();
        let yield_point_index = self.yield_points;
        self.yield_points += 1;
        // `Checked<int32_t>`: estouro derruba o processo.
        let mut state = (yield_point_index as i32).checked_add(1).expect("CheckedArithmetic: estouro em state");
        if self.parse_mode() == crate::parser::parser_modes::SourceParseMode::AsyncGeneratorBodyMode {
            state = state
                .checked_mul(1i32 << crate::runtime::js_async_generator::REASON_SHIFT)
                .expect("CheckedArithmetic: estouro em state")
                | result as i32;
        }

        self.emit_generator_state_change(state);

        // Split the try range here.
        let save_point = self.new_emitted_label();
        for i in (0..self.try_context_stack.len()).rev() {
            let start = self.try_context_stack[i].start.clone();
            let try_data = self.try_context_stack[i].try_data.clone();
            self.try_ranges.push(crate::bytecompiler::bytecode_generator::TryRange {
                start,
                end: save_point.clone(),
                try_data,
            });
            // Try range will be restared at the merge point.
            self.try_context_stack[i].start = merge_point.clone();
        }
        let mut saved_try_context_stack = Vec::new();
        std::mem::swap(&mut self.try_context_stack, &mut saved_try_context_stack);

        crate::bytecode::bytecode_ops::OpYield::emit(self, yield_point_index, argument);

        // Restore the try contexts, which start offset is updated to the merge point.
        std::mem::swap(&mut self.try_context_stack, &mut saved_try_context_stack);
        self.emit_label(&merge_point);
    }

    // BytecodeGenerator.cpp:5375
    pub fn emit_yield(
        &mut self,
        argument: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        // For async generators the operand is Awaited by the driver (reason Yield), not here, so there is no
        // extra yield point. `yield*` instead suspends with YieldNoAwait (see emitDelegateYield) so the driver
        // skips that await.
        self.emit_yield_point(argument, crate::runtime::js_async_generator::AsyncGeneratorSuspendReason::Yield);

        let normal_label = self.new_label();
        self.emit_jump_if_resume_mode(crate::runtime::js_generator::ResumeMode::NormalMode, &normal_label);

        let throw_label = self.new_label();
        self.emit_jump_if_resume_mode(crate::runtime::js_generator::ResumeMode::ThrowMode, &throw_label);
        // Return.
        {
            let return_register = self.generator_value_register();
            let has_finally = self.emit_return_via_finally_if_needed(&return_register);
            if !has_finally {
                self.emit_return(Some(return_register.clone()));
            }
        }

        // Throw.
        self.emit_label(&throw_label);
        let generator_value_register = self.generator_value_register();
        self.emit_throw(Some(generator_value_register));

        // Normal.
        self.emit_label(&normal_label);
        Some(self.generator_value_register())
    }

    // Os trechos `emitJumpIfTrue(emitEqualityOp<OpStricteq>(newTemporary(), generatorResumeModeRegister(),
    // emitLoad(nullptr, mode)), label)` do C++ aparecem seis vezes; aqui têm nome para não se repetirem.
    // (Não existe no C++: é a extração do trecho repetido.)
    pub(crate) fn emit_jump_if_resume_mode(
        &mut self,
        mode: crate::runtime::js_generator::ResumeMode,
        label: &crate::bytecompiler::label::LabelRef,
    ) {
        let temporary = self.new_temporary();
        let resume_mode_register = self.generator_resume_mode_register();
        let mode_register = self.emit_load_resume_mode(None, mode);
        let condition = self
            .emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(Some(temporary), Some(resume_mode_register), mode_register)
            .unwrap();
        self.emit_jump_if_true_raw(&condition, label);
    }

    // `emitJumpIfTrue(emitIsObject(newTemporary(), reg), label)`, que o C++ repete cinco vezes.
    // (Não existe no C++: é a extração do trecho repetido.)
    fn emit_jump_if_object(
        &mut self,
        register: &crate::bytecompiler::bytecode_generator::RegisterRef,
        label: &crate::bytecompiler::label::LabelRef,
    ) {
        let temporary = self.new_temporary();
        let condition = self.emit_is_object(Some(temporary), register).unwrap();
        self.emit_jump_if_true_raw(&condition, label);
    }

    // `emitJumpIfTrue/False(emitIsUndefinedOrNull(newTemporary(), reg), label)`.
    // (Não existe no C++: é a extração do trecho repetido.)
    fn emit_jump_if_undefined_or_null(
        &mut self,
        register: &crate::bytecompiler::bytecode_generator::RegisterRef,
        label: &crate::bytecompiler::label::LabelRef,
        jump_when: bool,
    ) {
        let temporary = self.new_temporary();
        let condition = self.emit_is_undefined_or_null(Some(temporary), register).unwrap();
        if jump_when {
            self.emit_jump_if_true_raw(&condition, label);
        } else {
            self.emit_jump_if_false_raw(&condition, label);
        }
    }

    // BytecodeGenerator.cpp:5404
    pub fn emit_await(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        src: &crate::bytecompiler::bytecode_generator::RegisterRef,
        position: &crate::parser::parser_tokens::JSTextPosition,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let generator_register = self.generator_register();
        self.emit_debug_hook(crate::interpreter::interpreter::DebugHookType::WillAwait, position, generator_register);

        // Não solta a referência de `dst`: a maioria dos chamadores passa `RefPtr` com contagem maior que zero.
        // Os dois que passam o temporário cru (`ReturnNode` de async generator e `AwaitExprNode`) usam
        // `with_raw_register` no próprio site.
        self.emit_yield_point(src, crate::runtime::js_async_generator::AsyncGeneratorSuspendReason::Await);

        let normal_label = self.new_label();
        self.emit_jump_if_resume_mode(crate::runtime::js_generator::ResumeMode::NormalMode, &normal_label);

        let generator_value_register = self.generator_value_register();
        self.emit_throw(Some(generator_value_register));

        self.emit_label(&normal_label);
        let generator_value_register = self.generator_value_register();
        let result = self.move_register(dst.as_ref(), &generator_value_register);

        let generator_register = self.generator_register();
        self.emit_debug_hook(crate::interpreter::interpreter::DebugHookType::DidAwait, position, generator_register);

        result
    }

    // BytecodeGenerator.cpp:5423
    pub fn emit_call_iterator(
        &mut self,
        iterator: &crate::bytecompiler::bytecode_generator::RegisterRef,
        argument: &crate::bytecompiler::bytecode_generator::RegisterRef,
        node: &crate::parser::nodes::ThrowableExpressionData,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let mut args = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 0);
        let this_register = args.this_register();
        self.move_register(this_register.as_ref(), argument);
        self.emit_call(
            Some(iterator.clone()),
            Some(iterator.clone()),
            crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
            &mut args,
            &node.divot,
            &node.divot_start,
            &node.divot_end,
            crate::bytecompiler::bytecode_generator::DebuggableCall::No,
        );

        Some(iterator.clone())
    }

    // BytecodeGenerator.cpp:5432
    pub fn emit_iterator_open(
        &mut self,
        iterator: &crate::bytecompiler::bytecode_generator::RegisterRef,
        next_or_index: &crate::bytecompiler::bytecode_generator::RegisterRef,
        symbol_iterator: &crate::bytecompiler::bytecode_generator::RegisterRef,
        iterable: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        node: &crate::parser::nodes::ThrowableExpressionData,
    ) {
        // Reserve space for call frame.
        let mut call_frame = Vec::new();
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
        let this_register = iterable.this_register();
        crate::bytecode::bytecode_ops::OpIteratorOpen::emit(
            self,
            iterator,
            next_or_index,
            symbol_iterator,
            this_register.as_ref().unwrap(),
            iterable.stack_offset(),
            iterable_value_profile,
            iterator_value_profile,
            next_value_profile,
        );
    }

    // BytecodeGenerator.cpp:5449
    pub fn emit_iterator_next(
        &mut self,
        done: &crate::bytecompiler::bytecode_generator::RegisterRef,
        value: &crate::bytecompiler::bytecode_generator::RegisterRef,
        iterable: &crate::bytecompiler::bytecode_generator::RegisterRef,
        next_or_index: &crate::bytecompiler::bytecode_generator::RegisterRef,
        iterator: &mut crate::bytecompiler::bytecode_generator::CallArguments,
        node: &crate::parser::nodes::ThrowableExpressionData,
    ) {
        // Reserve space for call frame.
        let mut call_frame = Vec::new();
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
        let next_result_value_profile = self.next_value_profile_index();
        let done_value_profile = self.next_value_profile_index();
        let value_value_profile = self.next_value_profile_index();
        let this_register = iterator.this_register();
        crate::bytecode::bytecode_ops::OpIteratorNext::emit(
            self,
            done,
            value,
            iterable,
            next_or_index,
            this_register.as_ref().unwrap(),
            iterator.stack_offset(),
            next_result_value_profile,
            done_value_profile,
            value_value_profile,
        );
    }

    // BytecodeGenerator.cpp:5466
    pub fn emit_get_generic_iterator(
        &mut self,
        argument: &crate::bytecompiler::bytecode_generator::RegisterRef,
        node: &crate::parser::nodes::ThrowableExpressionData,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        self.emit_expression_info(&node.divot, &node.divot_start, &node.divot_end);
        let temporary = self.new_temporary();
        let iterator_symbol = self.property_names().iterator_symbol.clone();
        let iterator = self.emit_get_by_id(Some(temporary), Some(argument.clone()), &iterator_symbol).unwrap();
        self.emit_call_iterator(&iterator, argument, node);

        Some(iterator)
    }

    // BytecodeGenerator.cpp:5475
    pub fn emit_iterator_generic_next(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        next_method: &crate::bytecompiler::bytecode_generator::RegisterRef,
        iterator: &crate::bytecompiler::bytecode_generator::RegisterRef,
        node: &crate::parser::nodes::ThrowableExpressionData,
        do_emit_await: crate::bytecompiler::bytecode_generator::EmitAwait,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        {
            let mut next_arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 0);
            let this_register = next_arguments.this_register();
            self.move_register(this_register.as_ref(), iterator);
            self.emit_call(
                dst.clone(),
                Some(next_method.clone()),
                crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                &mut next_arguments,
                &node.divot,
                &node.divot_start,
                &node.divot_end,
                crate::bytecompiler::bytecode_generator::DebuggableCall::No,
            );

            if do_emit_await == crate::bytecompiler::bytecode_generator::EmitAwait::Yes {
                self.emit_await(dst.clone(), dst.as_ref().unwrap(), &node.divot);
            }
        }
        {
            let type_is_object = self.new_label();
            self.emit_jump_if_object(dst.as_ref().unwrap(), &type_is_object);
            self.emit_throw_type_error("Iterator result interface is not an object.");
            self.emit_label(&type_is_object);
        }
        dst
    }

    // BytecodeGenerator.cpp:5494
    pub fn emit_iterator_generic_next_with_value(
        &mut self,
        dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>,
        next_method: &crate::bytecompiler::bytecode_generator::RegisterRef,
        iterator: &crate::bytecompiler::bytecode_generator::RegisterRef,
        value: &crate::bytecompiler::bytecode_generator::RegisterRef,
        node: &crate::parser::nodes::ThrowableExpressionData,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        {
            let mut next_arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 1);
            let this_register = next_arguments.this_register();
            self.move_register(this_register.as_ref(), iterator);
            let argument_register = next_arguments.argument_register(0);
            self.move_register(argument_register.as_ref(), value);
            self.emit_call(
                dst.clone(),
                Some(next_method.clone()),
                crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
                &mut next_arguments,
                &node.divot,
                &node.divot_start,
                &node.divot_end,
                crate::bytecompiler::bytecode_generator::DebuggableCall::No,
            );
        }

        dst
    }

    // BytecodeGenerator.cpp:5506
    pub fn emit_iterator_generic_close(
        &mut self,
        iterator: &crate::bytecompiler::bytecode_generator::RegisterRef,
        node: &crate::parser::nodes::ThrowableExpressionData,
        do_emit_await: crate::bytecompiler::bytecode_generator::EmitAwait,
    ) {
        let done = self.new_label();
        let temporary = self.new_temporary();
        let return_keyword = self.property_names().return_keyword.clone();
        let return_method = self.emit_get_by_id(Some(temporary), Some(iterator.clone()), &return_keyword).unwrap();
        self.emit_jump_if_undefined_or_null(&return_method, &done, true);

        let value = self.new_temporary();
        let mut return_arguments = crate::bytecompiler::bytecode_generator::CallArguments::new(self, None, 0);
        let this_register = return_arguments.this_register();
        self.move_register(this_register.as_ref(), iterator);
        self.emit_call(
            Some(value.clone()),
            Some(return_method.clone()),
            crate::bytecompiler::bytecode_generator::ExpectedFunction::NoExpectedFunction,
            &mut return_arguments,
            &node.divot,
            &node.divot_start,
            &node.divot_end,
            crate::bytecompiler::bytecode_generator::DebuggableCall::No,
        );

        if do_emit_await == crate::bytecompiler::bytecode_generator::EmitAwait::Yes {
            self.emit_await(Some(value.clone()), &value, &node.divot);
        }

        self.emit_jump_if_object(&value, &done);
        self.emit_throw_type_error("Iterator result interface is not an object.");
        self.emit_label(&done);
    }

    // BytecodeGenerator.cpp:5526
    pub fn emit_delegate_yield(
        &mut self,
        argument: &crate::bytecompiler::bytecode_generator::RegisterRef,
        node: &crate::parser::nodes::ThrowableExpressionData,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        use crate::bytecompiler::bytecode_generator::{CallArguments, DebuggableCall, EmitAwait, ExpectedFunction};
        use crate::runtime::js_async_generator::AsyncGeneratorSuspendReason;
        use crate::runtime::js_generator::ResumeMode;

        let is_async = self.parse_mode() == crate::parser::parser_modes::SourceParseMode::AsyncGeneratorBodyMode;
        let emit_await_in_close = if is_async { EmitAwait::Yes } else { EmitAwait::No };

        let value = self.new_temporary();
        {
            let iterator;
            let next_method;
            if is_async {
                iterator = self.new_temporary();
                next_method = self.new_temporary();
                self.emit_get_generic_async_iterator(Some(iterator.clone()), Some(next_method.clone()), Some(argument.clone()), node);
            } else {
                iterator = self.emit_get_generic_iterator(argument, node).unwrap();
                let temporary = self.new_temporary();
                let next = self.property_names().next.clone();
                next_method = self.emit_get_by_id(Some(temporary), Some(iterator.clone()), &next).unwrap();
            }

            let loop_done = self.new_label();
            {
                let next_element = self.new_label();
                self.emit_load_js_value(Some(value.clone()), crate::runtime::js_value::JSValue::Undefined);

                self.emit_jump(&next_element);

                let loop_start = self.new_label();
                self.emit_label(&loop_start);
                self.emit_loop_hint();

                let branch_on_result = self.new_label();
                {
                    // `yield*` delegates the inner iterator's value without an enclosing Await (for the async
                    // case, the iterator result is already Awaited below). YieldNoAwait tells the async driver
                    // not to await it again.
                    self.emit_yield_point(&value, AsyncGeneratorSuspendReason::YieldNoAwait);
                    let generator_value_register = self.generator_value_register();
                    self.move_register(Some(&value), &generator_value_register);

                    let normal_label = self.new_label();
                    self.emit_jump_if_resume_mode(ResumeMode::NormalMode, &normal_label);

                    let return_label = self.new_label();
                    self.emit_jump_if_resume_mode(ResumeMode::ReturnMode, &return_label);

                    // Throw. throw()/return() have no dedicated opcode, so call them generically (the fast
                    // async driver path only applies to the normal-mode next() below).
                    {
                        let throw_method_found = self.new_label();
                        let temporary = self.new_temporary();
                        let throw_keyword = self.property_names().throw_keyword.clone();
                        let throw_method =
                            self.emit_get_by_id(Some(temporary), Some(iterator.clone()), &throw_keyword).unwrap();
                        self.emit_jump_if_undefined_or_null(&throw_method, &throw_method_found, false);

                        self.emit_iterator_generic_close(&iterator, node, emit_await_in_close);

                        self.emit_throw_type_error(
                            "The iterator, to which yield* delegated iteration, does not have a 'throw' method.",
                        );

                        self.emit_label(&throw_method_found);
                        let mut throw_arguments = CallArguments::new(self, None, 1);
                        let this_register = throw_arguments.this_register();
                        self.move_register(this_register.as_ref(), &iterator);
                        let argument_register = throw_arguments.argument_register(0);
                        self.move_register(argument_register.as_ref(), &value);
                        self.emit_call(
                            Some(value.clone()),
                            Some(throw_method.clone()),
                            ExpectedFunction::NoExpectedFunction,
                            &mut throw_arguments,
                            &node.divot,
                            &node.divot_start,
                            &node.divot_end,
                            DebuggableCall::No,
                        );

                        self.emit_jump(&branch_on_result);
                    }

                    // Return.
                    self.emit_label(&return_label);
                    {
                        let return_method_found = self.new_label();
                        let temporary = self.new_temporary();
                        let return_keyword = self.property_names().return_keyword.clone();
                        let return_method =
                            self.emit_get_by_id(Some(temporary), Some(iterator.clone()), &return_keyword).unwrap();
                        self.emit_jump_if_undefined_or_null(&return_method, &return_method_found, false);

                        if is_async {
                            self.emit_await(Some(value.clone()), &value, &node.divot);
                        }

                        let return_sequence = self.new_label();
                        self.emit_jump(&return_sequence);

                        self.emit_label(&return_method_found);
                        let mut return_arguments = CallArguments::new(self, None, 1);
                        let this_register = return_arguments.this_register();
                        self.move_register(this_register.as_ref(), &iterator);
                        let argument_register = return_arguments.argument_register(0);
                        self.move_register(argument_register.as_ref(), &value);
                        self.emit_call(
                            Some(value.clone()),
                            Some(return_method.clone()),
                            ExpectedFunction::NoExpectedFunction,
                            &mut return_arguments,
                            &node.divot,
                            &node.divot_start,
                            &node.divot_end,
                            DebuggableCall::No,
                        );

                        if is_async {
                            self.emit_await(Some(value.clone()), &value, &node.divot);
                        }

                        let return_iterator_result_is_object = self.new_label();
                        self.emit_jump_if_object(&value, &return_iterator_result_is_object);
                        self.emit_throw_type_error("Iterator result interface is not an object.");

                        self.emit_label(&return_iterator_result_is_object);

                        let return_from_generator = self.new_label();
                        let temporary = self.new_temporary();
                        let done_identifier = self.property_names().done.clone();
                        let done_register =
                            self.emit_get_by_id(Some(temporary), Some(value.clone()), &done_identifier).unwrap();
                        self.emit_jump_if_true_raw(&done_register, &return_from_generator);

                        let value_identifier = self.property_names().value.clone();
                        self.emit_get_by_id(Some(value.clone()), Some(value.clone()), &value_identifier);
                        self.emit_jump(&loop_start);

                        self.emit_label(&return_from_generator);
                        self.emit_get_by_id(Some(value.clone()), Some(value.clone()), &value_identifier);

                        self.emit_label(&return_sequence);
                        let has_finally = self.emit_return_via_finally_if_needed(&value);
                        if !has_finally {
                            self.emit_return(Some(value.clone()));
                        }
                    }

                    // Normal.
                    self.emit_label(&normal_label);
                }

                self.emit_label(&next_element);
                if is_async {
                    self.emit_async_iterator_next(Some(value.clone()), Some(next_method.clone()), Some(iterator.clone()), Some(value.clone()), node);
                } else {
                    self.emit_iterator_generic_next_with_value(Some(value.clone()), &next_method, &iterator, &value, node);
                }

                self.emit_label(&branch_on_result);

                if is_async {
                    self.emit_await(Some(value.clone()), &value, &node.divot);
                }

                let iterator_value_is_object = self.new_label();
                self.emit_jump_if_object(&value, &iterator_value_is_object);
                self.emit_throw_type_error("Iterator result interface is not an object.");
                self.emit_label(&iterator_value_is_object);

                let temporary = self.new_temporary();
                let done_identifier = self.property_names().done.clone();
                let done_register = self.emit_get_by_id(Some(temporary), Some(value.clone()), &done_identifier).unwrap();
                self.emit_jump_if_true_raw(&done_register, &loop_done);
                let value_identifier = self.property_names().value.clone();
                self.emit_get_by_id(Some(value.clone()), Some(value.clone()), &value_identifier);

                self.emit_jump(&loop_start);
            }
            self.emit_label(&loop_done);
        }

        let value_identifier = self.property_names().value.clone();
        self.emit_get_by_id(Some(value.clone()), Some(value.clone()), &value_identifier);
        Some(value)
    }

    // BytecodeGenerator.cpp:5664
    pub fn emit_generator_state_change(&mut self, state: i32) {
        // FIXME: It seems like this will create a lot of constants if there are many yield points. Maybe we should op_inc the old state. https://bugs.webkit.org/show_bug.cgi?id=222254
        let completed_state = self.emit_load_js_value(None, crate::runtime::js_value::js_number_i32(state));
        const _: () = assert!(
            crate::runtime::js_generator::Field::State as u32 == crate::runtime::js_async_generator::Field::State as u32
        );
        let field = if crate::parser::parser_modes::is_module_parse_mode(self.parse_mode()) {
            crate::runtime::abstract_module_record::Field::State as u32
        } else {
            crate::runtime::js_generator::Field::State as u32
        };
        let generator_register = self.generator_register();
        self.emit_put_internal_field(generator_register, field, completed_state);
    }

    // BytecodeGenerator.cpp:5672
    pub fn emit_jump_via_finally_if_needed(
        &mut self,
        target_label_scope_depth: i32,
        jump_target: &crate::bytecompiler::label::LabelRef,
    ) -> bool {
        debug_assert!(self.label_scope_depth() - target_label_scope_depth >= 0);
        let mut number_of_scopes_to_check_for_finally = (self.label_scope_depth() - target_label_scope_depth) as usize;
        debug_assert!(number_of_scopes_to_check_for_finally <= self.control_flow_scope_stack.len());
        if number_of_scopes_to_check_for_finally == 0 {
            return false;
        }

        let mut innermost_finally_context: Option<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::FinallyContext>>> =
            None;
        let mut outermost_finally_context: Option<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::FinallyContext>>> =
            None;
        let mut scope_index = self.control_flow_scope_stack.len() as isize - 1;
        while number_of_scopes_to_check_for_finally > 0 {
            number_of_scopes_to_check_for_finally -= 1;
            let scope = self.control_flow_scope_stack[scope_index as usize].clone();
            scope_index -= 1;
            if scope.is_finally_scope() {
                let finally_context = scope.finally_context.clone().unwrap();
                if innermost_finally_context.is_none() {
                    innermost_finally_context = Some(finally_context.clone());
                }
                outermost_finally_context = Some(finally_context.clone());
                finally_context.borrow_mut().inc_number_of_breaks_or_continues();
            }
        }
        let Some(outermost_finally_context) = outermost_finally_context else {
            return false; // No finallys to thread through.
        };
        let innermost_finally_context = innermost_finally_context.unwrap();

        let jump_id = crate::bytecompiler::bytecode_generator::bytecode_offset_to_jump_id(self.instructions().size_in_bytes() as u32);
        let lexical_scope_index = self.label_scope_depth_to_lexical_scope_index(target_label_scope_depth);
        outermost_finally_context
            .borrow_mut()
            .register_jump(jump_id, lexical_scope_index, jump_target.clone());

        let completion_type_register = innermost_finally_context.borrow().completion_type_register();
        self.emit_load_completion_type(completion_type_register, jump_id);
        let finally_label = innermost_finally_context.borrow().finally_label().unwrap();
        self.emit_jump(&finally_label);
        true // We'll be jumping to a finally block.
    }

    // BytecodeGenerator.cpp:5705
    pub fn emit_return_via_finally_if_needed(&mut self, return_register: &crate::bytecompiler::bytecode_generator::RegisterRef) -> bool {
        let mut number_of_scopes_to_check_for_finally = self.control_flow_scope_stack.len();
        if number_of_scopes_to_check_for_finally == 0 {
            return false;
        }

        let mut innermost_finally_context: Option<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::FinallyContext>>> =
            None;
        while number_of_scopes_to_check_for_finally > 0 {
            number_of_scopes_to_check_for_finally -= 1;
            let scope_index = number_of_scopes_to_check_for_finally;
            let scope = self.control_flow_scope_stack[scope_index].clone();
            if scope.is_finally_scope() {
                let finally_context = scope.finally_context.clone().unwrap();
                if innermost_finally_context.is_none() {
                    innermost_finally_context = Some(finally_context.clone());
                }
                finally_context.borrow_mut().set_handles_returns();
            }
        }
        let Some(innermost_finally_context) = innermost_finally_context else {
            return false; // No finallys to thread through.
        };

        let completion_type_register = innermost_finally_context.borrow().completion_type_register();
        self.emit_load_completion_type(completion_type_register, crate::bytecompiler::bytecode_generator::CompletionType::RETURN);
        let completion_value_register = innermost_finally_context.borrow().completion_value_register();
        self.move_register(completion_value_register.as_ref(), return_register);
        let finally_label = innermost_finally_context.borrow().finally_label().unwrap();
        self.emit_jump(&finally_label);
        true // We'll be jumping to a finally block.
    }

    // BytecodeGenerator.cpp:5731
    pub fn emit_finally_completion(
        &mut self,
        context: &mut crate::bytecompiler::bytecode_generator::FinallyContext,
        normal_completion_label: &crate::bytecompiler::label::LabelRef,
    ) {
        use crate::bytecompiler::bytecode_generator::CompletionType;

        let completion_type_register = context.completion_type_register().unwrap();
        if context.number_of_breaks_or_continues() != 0 || context.handles_returns() {
            let completion_type_normal = self.emit_load_completion_type(None, CompletionType::NORMAL);
            // O temporário do C++ morre no fim da expressão: escopo próprio para liberar o registrador.
            {
                let temporary = self.new_temporary();
                let is_normal = self
                    .emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                        Some(temporary),
                        Some(completion_type_register.clone()),
                        completion_type_normal,
                    )
                    .unwrap();
                self.emit_jump_if_true_raw(&is_normal, normal_completion_label);
            }

            let outer_context = context.outer_context();

            let number_of_jumps = context.number_of_jumps();
            debug_assert!(outer_context.is_some() || number_of_jumps as u32 == context.number_of_breaks_or_continues());

            // Handle Break or Continue completions that jumps into this FinallyContext.
            for i in 0..number_of_jumps {
                let next_label = self.new_label();
                let (jump_id, target_lexical_scope_index, target_label) = {
                    let jump = context.jumps(i);
                    (jump.jump_id, jump.target_lexical_scope_index, jump.target_label.clone())
                };
                let jump_id_register = self.emit_load_completion_type(None, jump_id);
                let temporary = self.new_temporary();
                let is_jump = self
                    .emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                        Some(temporary),
                        Some(completion_type_register.clone()),
                        jump_id_register,
                    )
                    .unwrap();
                self.emit_jump_if_false_raw(&is_jump, &next_label);

                // This case is for Break / Continue completions from an inner finally context
                // with a jump target that is not beyond the next outer finally context:
                //
                //     try {
                //         for (... stuff ...) {
                //             try {
                //                 continue; // Sets completionType to jumpID of top of the for loop.
                //             } finally {
                //             } // Jump to top of the for loop on completion.
                //         }
                //     } finally {
                //     }
                //
                // Since the jumpID is targetting a label that is inside the outer finally context,
                // we can jump to it directly on completion of this finally context: there is no intermediate
                // finally blocks to run. After the Break / Continue, we will contnue execution as normal.
                // So, we'll set the completionType to Normal (on behalf of the target) before we jump.
                // We can also set the completion value to undefined, but it will never be used for normal
                // completion anyway. So, we'll skip setting it.

                self.restore_scope_register_at(target_lexical_scope_index);
                self.emit_load_completion_type(Some(completion_type_register.clone()), CompletionType::NORMAL);
                self.emit_jump(&target_label);

                self.emit_label(&next_label);
            }

            // Handle completions that take us out of this FinallyContext.
            if let Some(outer_context) = outer_context {
                let outer_type_register = outer_context.borrow().completion_type_register().unwrap();
                let outer_finally_label = outer_context.borrow().finally_label().unwrap();
                if context.handles_returns() {
                    let is_not_return_label = self.new_label();
                    let completion_type_return = self.emit_load_completion_type(None, CompletionType::RETURN);
                    let temporary = self.new_temporary();
                    let is_return = self
                        .emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                            Some(temporary),
                            Some(completion_type_register.clone()),
                            completion_type_return,
                        )
                        .unwrap();
                    self.emit_jump_if_false_raw(&is_return, &is_not_return_label);

                    // This case is for Return completion from an inner finally context:
                    //
                    //     try {
                    //         try {
                    //             return result; // Sets completionType to Return, and completionValue to result.
                    //         } finally {
                    //         } // Jump to outer finally on completion.
                    //     } finally {
                    //     }
                    //
                    // Since we know there's at least one outer finally context (beyond the current context),
                    // we cannot actually return from here. Instead, we pass the completionType and completionValue
                    // on to the next outer finally, and let it decide what to do next on its completion. The
                    // outer finally may or may not actual return depending on whether it encounters an abrupt
                    // completion in its body that overrrides this Return completion.

                    self.move_register(Some(&outer_type_register), &completion_type_register);
                    let outer_value_register = outer_context.borrow().completion_value_register().unwrap();
                    let completion_value_register = context.completion_value_register().unwrap();
                    self.move_register(Some(&outer_value_register), &completion_value_register);
                    self.emit_jump(&outer_finally_label);

                    self.emit_label(&is_not_return_label);
                }

                let has_breaks_or_continues_that_escape_current_finally =
                    context.number_of_breaks_or_continues() as usize > number_of_jumps;
                if has_breaks_or_continues_that_escape_current_finally {
                    let is_throw_or_normal_label = self.new_label();
                    let completion_type_throw = self.emit_load_completion_type(None, CompletionType::THROW);
                    let temporary = self.new_temporary();
                    let is_throw_or_normal = self
                        .emit_binary_op::<crate::bytecode::bytecode_ops::OpBeloweq>(
                            Some(temporary),
                            Some(completion_type_register.clone()),
                            completion_type_throw,
                            crate::parser::result_type::OperandTypes::default(),
                        )
                        .unwrap();
                    self.emit_jump_if_true_raw(&is_throw_or_normal, &is_throw_or_normal_label);

                    // A completionType above Throw means we have a Break or Continue encoded as a jumpID.
                    // We already ruled out Return above.
                    const _: () = assert!(CompletionType::THROW.0 < CompletionType::RETURN.0, "jumpIDs are above CompletionType::Return");

                    // This case is for Break / Continue completions in an inner finally context:
                    //
                    // 10: label:
                    // 11: try {
                    // 12:     try {
                    // 13:         for (... stuff ...)
                    // 14:             break label; // Sets completionType to jumpID of label.
                    // 15:     } finally {
                    // 16:     } // Jumps to outer finally on completion.
                    // 17:  } finally {
                    // 18:  }
                    //
                    // The break (line 14) says to continue execution at the label at line 10. Before we can
                    // goto line 10, the inner context's finally (line 15) needs to be run, followed by the
                    // outer context's finally (line 17). 'outerContext' being non-null above tells us that
                    // there is at least one outer finally context that we need to run after we complete the
                    // current finally. Note that unless the body of the outer finally abruptly completes in a
                    // different way, that outer finally also needs to complete with a Break / Continue to
                    // the same target label. Hence, we need to pass the jumpID in this finally's completionTypeRegister
                    // to the outer finally. The completion value for Break and Continue according to the spec
                    // is undefined, but it won't ever be used. So, we'll skip setting it.
                    //
                    // Note that all we're doing here is passing the Break / Continue completion to the next
                    // outer finally context. We don't worry about finally contexts beyond that. It is the
                    // responsibility of the next outer finally to determine what to do next at its completion,
                    // and pass on to the next outer context if present and needed.

                    self.move_register(Some(&outer_type_register), &completion_type_register);
                    self.emit_jump(&outer_finally_label);

                    self.emit_label(&is_throw_or_normal_label);
                }
            } else {
                // We are the outermost finally.
                if context.handles_returns() {
                    let not_return_label = self.new_label();
                    let completion_type_return = self.emit_load_completion_type(None, CompletionType::RETURN);
                    let temporary = self.new_temporary();
                    let is_return = self
                        .emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                            Some(temporary),
                            Some(completion_type_register.clone()),
                            completion_type_return,
                        )
                        .unwrap();
                    self.emit_jump_if_false_raw(&is_return, &not_return_label);

                    // This case is for Return completion from the outermost finally context:
                    //
                    //     try {
                    //         return result; // Sets completionType to Return, and completionValue to result.
                    //     } finally {
                    //     } // Executes the return of the completionValue.
                    //
                    // Since we know there's no outer finally context (beyond the current context) to run,
                    // we can actually execute a return for this Return completion. The value to return
                    // is whatever is in the completionValueRegister.

                    self.emit_will_leave_call_frame_debug_hook();
                    let completion_value_register = context.completion_value_register().unwrap();
                    self.emit_return(Some(completion_value_register.clone()));

                    self.emit_label(&not_return_label);
                }
            }
        }

        // By now, we've rule out all Break / Continue / Return completions above. The only remaining
        // possibilities are Normal or Throw.

        let completion_type_throw = self.emit_load_completion_type(None, CompletionType::THROW);
        let temporary = self.new_temporary();
        let is_throw = self
            .emit_equality_op::<crate::bytecode::bytecode_ops::OpStricteq>(
                Some(temporary),
                Some(completion_type_register),
                completion_type_throw,
            )
            .unwrap();
        self.emit_jump_if_false_raw(&is_throw, normal_completion_label);

        // We get here because we entered this finally context with Throw completionType (i.e. we have
        // an exception that we need to rethrow), and we didn't encounter a different abrupt completion
        // that overrides that incoming completionType. All we have to do here is re-throw the exception
        // captured in the completionValue.
        //
        // Note that unlike for Break / Continue / Return, we don't need to worry about outer finally
        // contexts. This is because any outer finally context (if present) will have its own exception
        // handler, which will take care of receiving the Throw completion, and re-capturing the exception
        // in its completionValue.

        let completion_value_register = context.completion_value_register().unwrap();
        self.emit_throw(Some(completion_value_register.clone()));
    }

    // BytecodeGenerator.cpp:5888
    pub fn push_optional_chain_target(&mut self) {
        let label = self.new_label();
        self.optional_chain_target_stack.push(label);
    }

    // BytecodeGenerator.cpp:5893
    pub fn push_optional_chain_target_existing(&mut self, existing_target: &crate::bytecompiler::label::LabelRef) {
        self.optional_chain_target_stack.push(existing_target.clone());
    }

    // BytecodeGenerator.cpp:5898
    pub fn pop_optional_chain_target(&mut self) {
        assert!(!self.optional_chain_target_stack.is_empty());
        let label = self.optional_chain_target_stack.pop().unwrap();
        self.emit_label(&label);
    }

    // BytecodeGenerator.cpp:5904
    pub fn pop_optional_chain_target_dst(&mut self, dst: Option<crate::bytecompiler::bytecode_generator::RegisterRef>, is_delete: bool) {
        let end_label = self.new_label();
        self.emit_jump(&end_label);

        self.pop_optional_chain_target();
        let value = if is_delete {
            crate::runtime::js_value::js_boolean(true)
        } else {
            crate::runtime::js_value::JSValue::Undefined
        };
        self.emit_load_js_value(dst, value);

        self.emit_label(&end_label);
    }

    // BytecodeGenerator.cpp:5915
    pub fn discard_optional_chain_target(&mut self) {
        assert!(!self.optional_chain_target_stack.is_empty());
        self.optional_chain_target_stack.pop();
    }

    // BytecodeGenerator.cpp:5921
    pub fn emit_optional_check(&mut self, src: &crate::bytecompiler::bytecode_generator::RegisterRef) {
        assert!(!self.optional_chain_target_stack.is_empty());
        let target = self.optional_chain_target_stack.last().unwrap().clone();
        self.emit_jump_if_undefined_or_null(src, &target, true);
    }

    // BytecodeGenerator.cpp:6058
    pub fn emit_to_this(
        &mut self,
        src_dst: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<crate::bytecompiler::bytecode_generator::RegisterRef> {
        let killed = self.kill(src_dst);
        let ecma_mode = self.ecma_mode();
        let value_profile = self.next_value_profile_index();
        crate::bytecode::bytecode_ops::OpToThis::emit(self, &killed, ecma_mode, value_profile);
        Some(src_dst.clone())
    }

    // BytecodeGenerator.cpp:6064
    pub fn find_for_in_context(
        &mut self,
        property: &crate::bytecompiler::bytecode_generator::RegisterRef,
    ) -> Option<std::rc::Rc<std::cell::RefCell<crate::bytecompiler::bytecode_generator::ForInContext>>> {
        for i in (0..self.for_in_context_stack.len()).rev() {
            let context = self.for_in_context_stack[i].clone();
            let is_property = match context.borrow().local() {
                Some(local) => local.is_same_register(property),
                None => false,
            };
            if !is_property {
                continue;
            }

            return Some(context);
        }

        None
    }
}

// BytecodeGenerator.cpp:5927: `template<OldOpType, NewOpType, TupleType> rewriteOp`. O corpo comum às três
// reescritas (get, in, put) é este; o que muda de uma para outra é a emissão do op novo, passada em `emit_new`.
fn rewrite_op<EmitNew>(
    generator: &mut BytecodeGenerator,
    inst_tuple: &(u32, i32),
    emit_new: EmitNew,
) where
    EmitNew: FnOnce(&mut BytecodeGenerator, &crate::bytecode::instruction_stream::MutableRef, crate::bytecode::virtual_register::VirtualRegister),
{
    let inst_index = inst_tuple.0;
    let property_reg_index = inst_tuple.1;
    let instruction = generator.writer.ref_at(inst_index);
    let end = inst_index + instruction.size() as u32;
    debug_assert!(instruction.with_instruction(|inst| inst.is_wide32()));

    generator.writer.seek(inst_index);

    generator.disable_peephole_optimization();

    // Change the opcode to get_by_val.
    // 1. dst stays the same.
    // 2. base stays the same.
    // 3. property gets switched to the original property.
    emit_new(generator, &instruction, crate::bytecode::virtual_register::VirtualRegister::new(property_reg_index));

    // 4. nop out the remaining bytes
    while generator.writer.position() < end {
        crate::bytecode::bytecode_ops::OpNop::emit(generator);
    }
}

impl crate::bytecompiler::bytecode_generator::ForInContext {
    // BytecodeGenerator.cpp:5958
    pub fn finalize(
        &mut self,
        generator: &mut BytecodeGenerator,
        body_bytecode_end_offset: u32,
    ) {
        // Lexically invalidating ForInContexts is kind of weak sauce, but it only occurs if
        // either of the following conditions is true:
        //
        // (1) The loop iteration variable is re-assigned within the body of the loop.
        // (2) The loop iteration variable is captured in the lexical scope of the function.
        //
        // These two situations occur sufficiently rarely that it's okay to use this style of
        // "analysis" to make iteration faster. If we didn't want to do this, we would either have
        // to perform some flow-sensitive analysis to see if/when the loop iteration variable was
        // reassigned, or we'd have to resort to runtime checks to see if the variable had been
        // reassigned from its original value.

        let mut escaped = false;
        let mut offset = self.body_bytecode_start_offset();
        while !escaped && offset < body_bytecode_end_offset {
            let instruction = generator.instructions().at(offset);
            debug_assert!(!instruction.is_op::<crate::bytecode::bytecode_ops::OpEnter>());
            let mut checkpoint = instruction.number_of_checkpoints();
            while checkpoint > 0 {
                checkpoint -= 1;
                let local_virtual_register = self.local().unwrap().borrow().virtual_register();
                crate::bytecode::bytecode_use_def::compute_defs_for_bytecode_index(&generator.code_block, &instruction, checkpoint, |operand| {
                    if local_virtual_register == operand {
                        escaped = true;
                    }
                });
            }
            offset += instruction.size() as u32;
        }

        if !escaped {
            return;
        }

        for inst_tuple in &self.get_insts {
            rewrite_op(generator, inst_tuple, |generator, instruction, property| {
                let bytecode = instruction.as_op::<crate::bytecode::bytecode_ops::OpEnumeratorGetByVal>();
                let value_profile = generator.next_value_profile_index();
                crate::bytecode::bytecode_ops::OpGetByVal::emit(generator, bytecode.dst(), bytecode.base(), property, value_profile);
            });
        }

        for inst_tuple in &self.in_insts {
            rewrite_op(generator, inst_tuple, |generator, instruction, property| {
                let bytecode = instruction.as_op::<crate::bytecode::bytecode_ops::OpEnumeratorInByVal>();
                crate::bytecode::bytecode_ops::OpInByVal::emit(generator, bytecode.dst(), bytecode.base(), property);
            });
        }

        for inst_tuple in &self.put_insts {
            rewrite_op(generator, inst_tuple, |generator, instruction, property| {
                let bytecode = instruction.as_op::<crate::bytecode::bytecode_ops::OpEnumeratorPutByVal>();
                crate::bytecode::bytecode_ops::OpPutByVal::emit(generator, bytecode.base(), property, bytecode.value(), bytecode.ecma_mode());
            });
        }

        for has_own_property_tuple in &self.has_own_property_jump_insts {
            debug_assert!(
                crate::bytecode::opcode::OPCODE_LENGTHS
                    [<crate::bytecode::bytecode_ops::OpJmp as crate::bytecode::bytecode_ops::BytecodeOp>::OPCODE_ID as usize]
                    <= crate::bytecode::opcode::OPCODE_LENGTHS[<crate::bytecode::bytecode_ops::OpJneqPtr as crate::bytecode::bytecode_ops::BytecodeOp>::OPCODE_ID
                        as usize]
            );
            let branch_inst_index = has_own_property_tuple.0;
            let new_branch_target = has_own_property_tuple.1;

            let instruction = generator.writer.ref_at(branch_inst_index);
            assert!(instruction.with_instruction(|inst| inst.is::<crate::bytecode::bytecode_ops::OpJneqPtr>()));
            assert!(instruction.with_instruction(|inst| inst.is_wide32()));
            let end = branch_inst_index + instruction.size() as u32;

            generator.writer.seek(branch_inst_index);
            generator.disable_peephole_optimization();

            crate::bytecode::bytecode_ops::OpJmp::emit(
                generator,
                crate::bytecompiler::label::BoundLabel::from_offset(new_branch_target as i32 - branch_inst_index as i32),
            );

            while generator.writer.position() < end {
                crate::bytecode::bytecode_ops::OpNop::emit(generator);
            }
        }

        let size = generator.writer.size_in_bytes() as u32;
        generator.writer.seek(size);
        generator.disable_peephole_optimization(); // We might've just changed the last bytecode that was emitted.
    }
}

// BytecodeGenerator.cpp:6079: `WTF::printInternal(PrintStream&, Variable::VariableKind)`.
impl std::fmt::Display for crate::bytecompiler::bytecode_generator::VariableKind {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            crate::bytecompiler::bytecode_generator::VariableKind::NormalVariable => out.write_str("Normal"),
            crate::bytecompiler::bytecode_generator::VariableKind::SpecialVariable => out.write_str("Special"),
        }
    }
}

// BytecodeGenerator.cpp:6038 (`StaticPropertyAnalysis::record`) já está portado em static_property_analysis.rs.
// BytecodeGenerator.cpp:6096 (`WTF_ALLOW_UNSAFE_BUFFER_USAGE_END`) é macro de diagnóstico do compilador, sem equivalente.
