//! Tradução de `parser/SourceProviderCacheItem.h`.
//!
//! O `SourceProviderCacheItem` do C++ é um `TrailingArray` com campos em bitfield; a largura dos
//! bitfields não é observável, então os campos guardam o tipo natural (os `ASSERT` de estreitamento
//! do construtor somem junto). As variáveis usadas (`PackedRefPtr<UniquedStringImpl>`) viram
//! `Box<[UniquedKey]>`.

use std::rc::Rc;

use crate::parser::parser_modes::{
    InnerArrowFunctionCodeFeatures, LexicallyScopedFeatures, SuperBinding,
    NO_LEXICALLY_SCOPED_FEATURES, STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
    TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE,
};
use crate::parser::parser_tokens::{JSToken, JSTokenType, CLOSEBRACE};
use crate::runtime::constructor_kind::ConstructorKind;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::wtf::text::string_impl::UniquedKey;

/// `struct SourceProviderCacheItemCreationParameters`.
#[derive(Clone, Debug)]
pub struct SourceProviderCacheItemCreationParameters {
    pub last_token_line: u32,
    pub last_token_start_offset: u32,
    pub last_token_end_offset: u32,
    pub last_token_line_start_offset: u32,
    pub end_function_offset: u32,
    pub parameter_count: u32,
    pub free_variable_count: u32,
    pub lexically_scoped_features: LexicallyScopedFeatures,
    pub inner_arrow_function_features: InnerArrowFunctionCodeFeatures,
    pub implementation_visibility: ImplementationVisibility,
    /// Scope's own free variables followed by the captures from its parameter expressions.
    pub used_variables: Vec<UniquedKey>,
    pub token_type: JSTokenType,
    pub constructor_kind: ConstructorKind,
    pub expected_super_binding: SuperBinding,
    pub needs_full_activation: bool,
    pub uses_eval: bool,
    pub uses_import_meta: bool,
    pub needs_super_binding: bool,
    pub is_body_arrow_expression: bool,
    pub contains_tagged_template: bool,
    /// Where the lexer stood after the function's last token, which for a token spanning lines is not lastTokenLine.
    pub last_token_end_line: u32,
    pub last_token_end_line_start_offset: u32,
}

impl Default for SourceProviderCacheItemCreationParameters {
    fn default() -> Self {
        // `constructorKind` e `expectedSuperBinding` não têm inicializador no C++ (o chamador sempre os
        // preenche antes de criar o item); os valores zero dos enums são o estado inicial neutro.
        SourceProviderCacheItemCreationParameters {
            last_token_line: 0,
            last_token_start_offset: 0,
            last_token_end_offset: 0,
            last_token_line_start_offset: 0,
            end_function_offset: 0,
            parameter_count: 0,
            free_variable_count: 0,
            lexically_scoped_features: 0,
            inner_arrow_function_features: 0,
            implementation_visibility: ImplementationVisibility::Public,
            used_variables: Vec::new(),
            token_type: CLOSEBRACE,
            constructor_kind: ConstructorKind::None,
            expected_super_binding: SuperBinding::Needed,
            needs_full_activation: false,
            uses_eval: false,
            uses_import_meta: false,
            needs_super_binding: false,
            is_body_arrow_expression: false,
            contains_tagged_template: false,
            last_token_end_line: 0,
            last_token_end_line_start_offset: 0,
        }
    }
}

impl SourceProviderCacheItemCreationParameters {
    /// `freeVariables()`: as primeiras `freeVariableCount` de `usedVariables`.
    pub fn free_variables(&self) -> &[UniquedKey] {
        debug_assert!(self.free_variable_count as usize <= self.used_variables.len());
        &self.used_variables[..self.free_variable_count as usize]
    }
}

/// `class SourceProviderCacheItem`.
#[derive(Debug)]
pub struct SourceProviderCacheItem {
    pub needs_full_activation: bool,
    pub end_function_offset: u32,
    pub uses_eval: bool,
    pub last_token_line: u32,
    pub strict_mode: bool,
    pub last_token_start_offset: u32,
    pub expected_super_binding: SuperBinding,
    pub last_token_end_offset: u32,
    pub needs_super_binding: bool,
    pub parameter_count: u32,
    pub tainted_by_with_scope: bool,
    pub last_token_line_start_offset: u32,
    pub is_body_arrow_expression: bool,
    pub token_type: JSTokenType,
    pub inner_arrow_function_features: InnerArrowFunctionCodeFeatures,
    pub constructor_kind: ConstructorKind,
    pub implementation_visibility: ImplementationVisibility,
    pub uses_import_meta: bool,
    pub contains_tagged_template: bool,
    pub last_token_end_line: u32,
    pub last_token_end_line_start_offset: u32,
    used_variables: Box<[UniquedKey]>,
}

impl SourceProviderCacheItem {
    /// `static std::unique_ptr<SourceProviderCacheItem> create(const SourceProviderCacheItemCreationParameters&)`.
    /// O cache entrega o item por `Rc` (o parser o segura enquanto reaproveita), então o dono único do C++
    /// vira `Rc` aqui.
    pub fn create(parameters: &SourceProviderCacheItemCreationParameters) -> Rc<SourceProviderCacheItem> {
        Rc::new(SourceProviderCacheItem {
            needs_full_activation: parameters.needs_full_activation,
            end_function_offset: parameters.end_function_offset,
            uses_eval: parameters.uses_eval,
            last_token_line: parameters.last_token_line,
            strict_mode: parameters.lexically_scoped_features & STRICT_MODE_LEXICALLY_SCOPED_FEATURE != 0,
            last_token_start_offset: parameters.last_token_start_offset,
            expected_super_binding: parameters.expected_super_binding,
            last_token_end_offset: parameters.last_token_end_offset,
            needs_super_binding: parameters.needs_super_binding,
            parameter_count: parameters.parameter_count,
            tainted_by_with_scope: parameters.lexically_scoped_features & TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE != 0,
            last_token_line_start_offset: parameters.last_token_line_start_offset,
            is_body_arrow_expression: parameters.is_body_arrow_expression,
            token_type: parameters.token_type,
            inner_arrow_function_features: parameters.inner_arrow_function_features,
            constructor_kind: parameters.constructor_kind,
            implementation_visibility: parameters.implementation_visibility,
            uses_import_meta: parameters.uses_import_meta,
            contains_tagged_template: parameters.contains_tagged_template,
            last_token_end_line: parameters.last_token_end_line,
            last_token_end_line_start_offset: parameters.last_token_end_line_start_offset,
            used_variables: parameters.used_variables.clone().into_boxed_slice(),
        })
    }

    pub fn end_function_token(&self) -> JSToken {
        let mut token = JSToken::default();
        token.type_ = if self.is_body_arrow_expression { self.token_type } else { CLOSEBRACE };
        token.data.offset = self.last_token_start_offset;
        token.start_position.offset = self.last_token_start_offset as i32;
        token.start_position.line = self.last_token_line as i32;
        token.start_position.line_start_offset = self.last_token_line_start_offset as i32;
        token.end_position.offset = self.last_token_end_offset as i32;
        token.end_position.line = self.last_token_end_line as i32;
        token.end_position.line_start_offset = self.last_token_end_line_start_offset as i32;
        // token.m_location.sourceOffset is initialized once by the client. So,
        // we do not need to set it here.
        token
    }

    pub fn lexically_scoped_features(&self) -> LexicallyScopedFeatures {
        let mut features = NO_LEXICALLY_SCOPED_FEATURES;
        if self.strict_mode {
            features |= STRICT_MODE_LEXICALLY_SCOPED_FEATURE;
        }
        if self.tainted_by_with_scope {
            features |= TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE;
        }
        features
    }

    pub fn used_variables(&self) -> &[UniquedKey] {
        &self.used_variables
    }
}
