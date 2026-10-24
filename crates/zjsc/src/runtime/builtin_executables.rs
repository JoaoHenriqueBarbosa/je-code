//! Tradução de `builtins/BuiltinExecutables.h` e `.cpp`: `defaultConstructorSourceCode`,
//! `createDefaultConstructor`, `createBuiltinExecutable`, `computeBuiltinSourceMetadata`, as duas
//! `createExecutable`, o `m_combinedSourceProvider`, os `m_unlinkedExecutables` e as macros
//! `JSC_FOREACH_BUILTIN_CODE` (`xxxSource()`, `xxxExecutable()` e o `xxxCodeGenerator` de
//! `JSCBuiltins.cpp`).
//!
//! Os dados que o gerador de builtins do C++ emite (o `s_JSCCombinedCode`, a tabela de cada
//! `s_xxxCode*` e o `s_JSCBuiltinSourceMetadata`) vêm de `builtins_source.rs`, gerado por
//! `scripts/gen-builtins.py` (não editar à mão). Cada `xxx` do C++ é um `BuiltinCodeIndex`.
//!
//! Divergências de forma:
//! - o `m_vm` (`VM&`) do C++ é um argumento (`vm`), porque o `BuiltinExecutables` mora dentro do
//!   próprio `VM` e um laço de referências não existe em Rust;
//! - o `m_combinedSourceProvider` é criado na primeira necessidade (o C++ usa `createWithoutCopying`
//!   sobre o array estático; aqui o fonte Latin1 é copiado uma vez por VM);
//! - `visitAggregate` não existe (sem coletor); `clear` esvazia os executáveis criados;
//! - o bloco `if (ASSERT_ENABLED || Options::validateBytecode())` de `createExecutable` reparseia o
//!   fonte com `parseRootNode<ProgramNode>` e derruba o processo se o resultado diferir do
//!   `FunctionMetadataNode` montado à mão. É só uma conferência do scanner contra o parser, sem
//!   efeito no executável devolvido, e o `parseRootNode` não está portado; foi omitido.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use crate::bytecode::executable_info::{DerivedContextType, EvalContextType, NeedsClassFieldInitializer};
use crate::bytecode::unlinked_function_executable::{
    UnlinkedFunctionExecutable, UnlinkedFunctionExecutableRef, UnlinkedFunctionKind,
};
use crate::parser::lexer::Lexer;
use crate::parser::nodes::FunctionMetadataNode;
use crate::parser::parser_modes::{
    FunctionMode, JSParserScriptMode, PrivateBrandRequirement, SourceParseMode, SuperBinding,
    NO_LEXICALLY_SCOPED_FEATURES, STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
};
use crate::parser::parser_tokens::{JSTextPosition, JSTokenLocation};
use crate::parser::source_code::{make_source, SourceCode};
use crate::parser::source_provider::{SourceProvider, SourceProviderSourceType, StringSourceProvider};
use crate::runtime::builtins_source::{public_name, BuiltinCodeIndex, BUILTIN_CODES, COMBINED_CODE, NUMBER_OF_BUILTIN_CODES};
use crate::runtime::js_function::FunctionExecutableRef;
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::construct_ability::ConstructAbility;
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::inline_attribute::InlineAttribute;
use crate::runtime::source_origin::SourceOrigin;
use crate::runtime::vm::VM;
use crate::wtf::text::text_position::TextPosition;
use crate::wtf::text::wtf_string::String as WtfString;

/// `struct BuiltinSourceMetadata`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuiltinSourceMetadata {
    pub source_length: u32,
    pub parameters_start: u32,
    pub parameter_count: u32,
    pub line_count: u32,
    pub end_column: u32,
    pub offset_of_last_newline: u32,
    pub position_before_last_newline_line_start_offset: u32,
    pub close_brace_offset_from_end: i32,
    pub is_async_function: bool,
    pub is_in_strict_context: bool,
}

/// `class BuiltinExecutables`.
pub struct BuiltinExecutables {
    /// `m_combinedSourceProvider`, criado na primeira necessidade.
    combined_source_provider: OnceCell<Rc<dyn SourceProvider>>,
    /// `m_unlinkedExecutables`, indexado por `BuiltinCodeIndex`.
    unlinked_executables: RefCell<Vec<Option<UnlinkedFunctionExecutableRef>>>,
}

impl std::fmt::Debug for BuiltinExecutables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let created = self.unlinked_executables.borrow().iter().filter(|executable| executable.is_some()).count();
        f.debug_struct("BuiltinExecutables").field("created_executables", &created).finish()
    }
}

impl Default for BuiltinExecutables {
    fn default() -> BuiltinExecutables {
        BuiltinExecutables::new()
    }
}

impl BuiltinExecutables {
    /// `BuiltinExecutables(VM&)`.
    pub fn new() -> BuiltinExecutables {
        BuiltinExecutables {
            combined_source_provider: OnceCell::new(),
            unlinked_executables: RefCell::new(vec![None; NUMBER_OF_BUILTIN_CODES]),
        }
    }

    /// `m_combinedSourceProvider`: `StringSourceProvider::create(s_JSCCombinedCode, { }, String(), Untainted)`.
    fn combined_source_provider(&self) -> Rc<dyn SourceProvider> {
        Rc::clone(self.combined_source_provider.get_or_init(|| {
            let provider: Rc<dyn SourceProvider> = StringSourceProvider::create(
                &WtfString::from_latin1(COMBINED_CODE.as_bytes()),
                &SourceOrigin::default(),
                WtfString::default(),
                SourceTaintedOrigin::Untainted,
                TextPosition::default(),
                SourceProviderSourceType::Program,
            );
            provider
        }))
    }

    /// `xxxSource()`: `SourceCode { m_combinedSourceProvider, offset, offset + length, 1, 1 }`.
    pub fn source(&self, index: BuiltinCodeIndex) -> SourceCode {
        let code = &BUILTIN_CODES[index as usize];
        SourceCode::with_offsets(
            Some(self.combined_source_provider()),
            code.offset as i32,
            (code.offset + code.length) as i32,
            1,
            1,
        )
    }

    /// `xxxExecutable()`: cria o `UnlinkedFunctionExecutable` do builtin na primeira chamada.
    pub fn executable(&self, vm: &VM, index: BuiltinCodeIndex) -> UnlinkedFunctionExecutableRef {
        let cached = self.unlinked_executables.borrow()[index as usize].clone();
        if let Some(executable) = cached {
            return executable;
        }

        let code = &BUILTIN_CODES[index as usize];
        let executable_name = match code.overridden_name {
            Some(overridden) => Identifier::from_span(vm, overridden.as_bytes()),
            None => public_name(vm.property_names.builtin_names(), index).clone(),
        };
        let executable = self.create_builtin_executable(
            vm,
            &self.source(index),
            &code.metadata,
            &executable_name,
            code.implementation_visibility,
            code.constructor_kind,
            code.construct_ability,
            code.inline_attribute,
        );
        self.unlinked_executables.borrow_mut()[index as usize] = Some(Rc::clone(&executable));
        executable
    }

    /// `xxxCodeGenerator(VM&)` de `JSCBuiltins.cpp`: `xxxExecutable()->link(vm, nullptr, xxxSource(),
    /// std::nullopt, s_xxxCodeIntrinsic)`.
    pub fn code_generator(&self, vm: &VM, index: BuiltinCodeIndex) -> FunctionExecutableRef {
        UnlinkedFunctionExecutable::link(
            &self.executable(vm, index),
            vm,
            None,
            &self.source(index),
            None,
            BUILTIN_CODES[index as usize].intrinsic,
            false,
        )
    }

    /// `clear()`.
    pub fn clear(&self) {
        self.unlinked_executables.borrow_mut().fill(None);
    }

    /// `defaultConstructorSourceCode(ConstructorKind)`.
    pub fn default_constructor_source_code(constructor_kind: ConstructorKind) -> SourceCode {
        let code: &[u8] = match constructor_kind {
            ConstructorKind::Base => b"(function () { })",
            ConstructorKind::Extends => b"(function (...args) { super(...args); })",
            _ => panic!("RELEASE_ASSERT_NOT_REACHED"),
        };
        make_source(
            &WtfString::from_latin1(code),
            &SourceOrigin::default(),
            SourceTaintedOrigin::Untainted,
            WtfString::default(),
            TextPosition::default(),
            SourceProviderSourceType::Program,
        )
    }

    /// `createDefaultConstructor(ConstructorKind, const Identifier&, NeedsClassFieldInitializer, PrivateBrandRequirement)`.
    pub fn create_default_constructor(
        &self,
        vm: &VM,
        constructor_kind: ConstructorKind,
        name: &Identifier,
        needs_class_field_initializer: NeedsClassFieldInitializer,
        private_brand_requirement: PrivateBrandRequirement,
    ) -> Option<UnlinkedFunctionExecutableRef> {
        match constructor_kind {
            ConstructorKind::None | ConstructorKind::Naked => {
                debug_assert!(false, "ASSERT_NOT_REACHED");
                None
            }
            ConstructorKind::Base | ConstructorKind::Extends => Some(Self::create_executable(
                vm,
                &Self::default_constructor_source_code(constructor_kind),
                name,
                ImplementationVisibility::Public,
                constructor_kind,
                ConstructAbility::CanConstruct,
                InlineAttribute::Always,
                needs_class_field_initializer,
                private_brand_requirement,
            )),
        }
    }

    /// `createBuiltinExecutable(const SourceCode&, const BuiltinSourceMetadata&, ...)`.
    pub fn create_builtin_executable(
        &self,
        vm: &VM,
        code: &SourceCode,
        metadata: &BuiltinSourceMetadata,
        name: &Identifier,
        implementation_visibility: ImplementationVisibility,
        constructor_kind: ConstructorKind,
        construct_ability: ConstructAbility,
        inline_attribute: InlineAttribute,
    ) -> UnlinkedFunctionExecutableRef {
        Self::create_executable_with_metadata(
            vm,
            code,
            metadata,
            name,
            implementation_visibility,
            constructor_kind,
            construct_ability,
            inline_attribute,
            NeedsClassFieldInitializer::No,
            PrivateBrandRequirement::None,
        )
    }

    /// `computeBuiltinSourceMetadata(std::span<const Latin1Character>)`.
    pub fn compute_builtin_source_metadata(characters: &[u8]) -> BuiltinSourceMetadata {
        let regular_function_begin: &[u8] = b"(function (";
        let async_function_begin: &[u8] = b"(async function (";
        assert!(characters.len() >= "(function (){})".len());
        let is_async_function =
            characters.len() >= "(async function (){})".len() && characters.starts_with(async_function_begin);
        assert!(is_async_function || characters.starts_with(regular_function_begin));

        let async_offset = if is_async_function { "async ".len() } else { 0 };
        let parameters_start = "function (".len() + async_offset;
        let mut is_in_strict_context = false;

        let parameter_count;
        {
            let mut i = parameters_start + 1;
            let mut commas = 0u32;
            let mut inside_curly_brackets = false;
            let mut saw_one_param = false;
            let mut has_rest_param = false;
            loop {
                debug_assert!(i < characters.len());
                if characters[i] == b')' {
                    break;
                }

                if characters[i] == b'}' {
                    inside_curly_brackets = false;
                } else if characters[i] == b'{' || inside_curly_brackets {
                    inside_curly_brackets = true;
                    i += 1;
                    continue;
                } else if characters[i] == b',' {
                    commas += 1;
                } else if !Lexer::<u8>::is_white_space(characters[i]) {
                    saw_one_param = true;
                }

                if i + 2 < characters.len() && characters[i] == b'.' && characters[i + 1] == b'.' && characters[i + 2] == b'.' {
                    has_rest_param = true;
                    i += 2;
                }

                i += 1;
            }

            let mut count = if commas != 0 {
                commas + 1
            } else if saw_one_param {
                1
            } else {
                0
            };

            if has_rest_param {
                assert!(count != 0);
                count -= 1;
            }
            parameter_count = count;
        }

        let use_strict: &[u8] = b"use strict";
        let mut line_count = 0u32;
        let mut end_column = 0u32;
        let mut offset_of_last_newline = 0u32;
        let mut offset_of_second_to_last_newline: Option<u32> = None;
        let mut i = 0usize;
        while i < characters.len() {
            if characters[i] == b'\n' {
                if line_count != 0 {
                    offset_of_second_to_last_newline = Some(offset_of_last_newline);
                }
                line_count += 1;
                end_column = 0;
                offset_of_last_newline = i as u32;
            } else {
                end_column += 1;
            }

            if !is_in_strict_context && (characters[i] == b'"' || characters[i] == b'\'') && i + 1 + use_strict.len() < characters.len() && characters[i + 1..].starts_with(use_strict) {
                is_in_strict_context = true;
                i += 1 + use_strict.len();
            }
            i += 1;
        }

        let position_before_last_newline_line_start_offset = match offset_of_second_to_last_newline {
            Some(offset) => offset + 1,
            None => 0,
        };

        let mut close_brace_offset_from_end = 1usize;
        while characters[characters.len() - close_brace_offset_from_end] != b'}' {
            close_brace_offset_from_end += 1;
        }

        BuiltinSourceMetadata {
            source_length: characters.len() as u32,
            parameters_start: parameters_start as u32,
            parameter_count,
            line_count,
            end_column,
            offset_of_last_newline,
            position_before_last_newline_line_start_offset,
            close_brace_offset_from_end: close_brace_offset_from_end as i32,
            is_async_function,
            is_in_strict_context,
        }
    }

    /// `createExecutable(VM&, const SourceCode&, const Identifier&, ...)`: escaneia o fonte e delega.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_executable(
        vm: &VM,
        source: &SourceCode,
        name: &Identifier,
        implementation_visibility: ImplementationVisibility,
        constructor_kind: ConstructorKind,
        construct_ability: ConstructAbility,
        inline_attribute: InlineAttribute,
        needs_class_field_initializer: NeedsClassFieldInitializer,
        private_brand_requirement: PrivateBrandRequirement,
    ) -> UnlinkedFunctionExecutableRef {
        let view = source.view();
        assert!(!view.is_null());
        assert!(view.is_8bit());
        Self::create_executable_with_metadata(
            vm,
            source,
            &Self::compute_builtin_source_metadata(view.span8()),
            name,
            implementation_visibility,
            constructor_kind,
            construct_ability,
            inline_attribute,
            needs_class_field_initializer,
            private_brand_requirement,
        )
    }

    /// `createExecutable(VM&, const SourceCode&, const BuiltinSourceMetadata&, const Identifier&, ...)`.
    #[allow(clippy::too_many_arguments)]
    fn create_executable_with_metadata(
        vm: &VM,
        source: &SourceCode,
        scanned: &BuiltinSourceMetadata,
        name: &Identifier,
        mut implementation_visibility: ImplementationVisibility,
        constructor_kind: ConstructorKind,
        construct_ability: ConstructAbility,
        inline_attribute: InlineAttribute,
        needs_class_field_initializer: NeedsClassFieldInitializer,
        private_brand_requirement: PrivateBrandRequirement,
    ) -> UnlinkedFunctionExecutableRef {
        let view = source.view();
        assert!(scanned.source_length == view.length());

        let parameters_start = scanned.parameters_start;
        let start_column = parameters_start;
        let function_keyword_start = "(".len() as u32;
        let function_name_start = parameters_start as i32;
        let is_arrow_function_body_expression = false;

        let position_before_last_newline = JSTextPosition::new(
            scanned.line_count as i32,
            source.start_offset() + scanned.offset_of_last_newline as i32,
            source.start_offset() + scanned.position_before_last_newline_line_start_offset as i32,
        );

        let new_source = source.sub_expression(
            (source.start_offset() + parameters_start as i32) as u32,
            (source.start_offset() + (view.length() as i32 - scanned.close_brace_offset_from_end)) as u32,
            0,
            parameters_start as i32,
        );
        let is_builtin_default_class_constructor = constructor_kind != ConstructorKind::None && constructor_kind != ConstructorKind::Naked;
        let kind = if is_builtin_default_class_constructor {
            UnlinkedFunctionKind::UnlinkedNormalFunction
        } else {
            UnlinkedFunctionKind::UnlinkedBuiltinFunction
        };

        let parse_mode = if scanned.is_async_function {
            SourceParseMode::AsyncFunctionMode
        } else {
            SourceParseMode::NormalFunctionMode
        };

        // Async functions should have Private visibility for correct stack traces.
        // See https://bugs.webkit.org/show_bug.cgi?id=304740
        if scanned.is_async_function {
            implementation_visibility = std::cmp::max_by_key(implementation_visibility, ImplementationVisibility::Private, |visibility| *visibility as u8);
        }

        let start = JSTokenLocation {
            line: -1,
            line_start_offset: u32::MAX,
            start_offset: (source.start_offset() + parameters_start as i32) as u32,
            end_offset: u32::MAX,
        };

        let end = JSTokenLocation {
            line: 1,
            line_start_offset: source.start_offset() as u32,
            start_offset: (source.start_offset() + "(".len() as i32) as u32,
            end_offset: u32::MAX,
        };

        let metadata = FunctionMetadataNode::new(
            &start,
            &end,
            start_column,
            scanned.end_column,
            (source.start_offset() + function_keyword_start as i32) as u32,
            source.start_offset() + function_name_start,
            source.start_offset() + parameters_start as i32,
            implementation_visibility,
            if scanned.is_in_strict_context { STRICT_MODE_LEXICALLY_SCOPED_FEATURE } else { NO_LEXICALLY_SCOPED_FEATURES },
            constructor_kind,
            if constructor_kind == ConstructorKind::Extends { SuperBinding::Needed } else { SuperBinding::NotNeeded },
            scanned.parameter_count,
            parse_mode,
            is_arrow_function_body_expression,
        );

        metadata.finish_parsing(&new_source, &Identifier::default(), FunctionMode::FunctionExpression);
        metadata.override_name(name);
        metadata.set_end_position(position_before_last_newline);

        UnlinkedFunctionExecutable::create_with_builtin_default_class_constructor(
            vm,
            source,
            &metadata,
            kind,
            construct_ability,
            inline_attribute,
            JSParserScriptMode::Classic,
            None,
            Vec::new(),
            None,
            DerivedContextType::None,
            EvalContextType::FunctionEvalContext,
            needs_class_field_initializer,
            private_brand_requirement,
            is_builtin_default_class_constructor,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_of_the_base_default_constructor() {
        let metadata = BuiltinExecutables::compute_builtin_source_metadata(b"(function () { })");
        assert_eq!(metadata.source_length, 17);
        assert_eq!(metadata.parameters_start, 10);
        assert_eq!(metadata.parameter_count, 0);
        assert_eq!(metadata.line_count, 0);
        assert_eq!(metadata.end_column, 17);
        assert_eq!(metadata.close_brace_offset_from_end, 2);
        assert!(!metadata.is_async_function);
        assert!(!metadata.is_in_strict_context);
    }

    #[test]
    fn metadata_of_the_derived_default_constructor_discounts_the_rest_parameter() {
        let metadata = BuiltinExecutables::compute_builtin_source_metadata(b"(function (...args) { super(...args); })");
        assert_eq!(metadata.parameter_count, 0);
        assert_eq!(metadata.parameters_start, 10);
    }
}
