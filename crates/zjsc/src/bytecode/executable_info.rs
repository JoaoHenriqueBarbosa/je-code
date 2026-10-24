//! Tradução de `bytecode/ExecutableInfo.h`.
//!
//! O C++ empacota os campos em bitfields (`unsigned m_isConstructor : 1` e assim por diante) e
//! confere com `ASSERT` que a largura comporta o enum; aqui cada campo guarda o seu tipo, que tem o
//! mesmo conjunto de valores.

use crate::parser::parser_modes::{JSParserScriptMode, PrivateBrandRequirement, SourceParseMode, SuperBinding};
use crate::runtime::constructor_kind::ConstructorKind;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerivedContextType {
    None = 0,
    DerivedConstructorContext = 1,
    DerivedMethodContext = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalContextType {
    None = 0,
    FunctionEvalContext = 1,
    InstanceFieldEvalContext = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedsClassFieldInitializer {
    No = 0,
    Yes = 1,
}

// FIXME: These flags, ParserModes and propagation to XXXCodeBlocks should be reorganized.
// https://bugs.webkit.org/show_bug.cgi?id=151547
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutableInfo {
    is_constructor: bool,
    private_brand_requirement: PrivateBrandRequirement,
    is_builtin_function: bool,
    constructor_kind: ConstructorKind,
    super_binding: SuperBinding,
    script_mode: JSParserScriptMode,
    parse_mode: SourceParseMode,
    derived_context_type: DerivedContextType,
    needs_class_field_initializer: NeedsClassFieldInitializer,
    is_arrow_function_context: bool,
    is_class_context: bool,
    eval_context_type: EvalContextType,
    is_builtin_default_class_constructor: bool,
}

impl ExecutableInfo {
    /// O último argumento (`isBuiltinDefaultClassConstructor`) tem `false` como padrão no C++.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        is_constructor: bool,
        private_brand_requirement: PrivateBrandRequirement,
        is_builtin_function: bool,
        constructor_kind: ConstructorKind,
        script_mode: JSParserScriptMode,
        super_binding: SuperBinding,
        parse_mode: SourceParseMode,
        derived_context_type: DerivedContextType,
        needs_class_field_initializer: NeedsClassFieldInitializer,
        is_arrow_function_context: bool,
        is_class_context: bool,
        eval_context_type: EvalContextType,
        is_builtin_default_class_constructor: bool,
    ) -> Self {
        Self {
            is_constructor,
            private_brand_requirement,
            is_builtin_function,
            constructor_kind,
            super_binding,
            script_mode,
            parse_mode,
            derived_context_type,
            needs_class_field_initializer,
            is_arrow_function_context,
            is_class_context,
            eval_context_type,
            is_builtin_default_class_constructor,
        }
    }

    pub fn is_constructor(&self) -> bool {
        self.is_constructor
    }
    pub fn private_brand_requirement(&self) -> PrivateBrandRequirement {
        self.private_brand_requirement
    }
    pub fn is_builtin_function(&self) -> bool {
        self.is_builtin_function
    }
    pub fn constructor_kind(&self) -> ConstructorKind {
        self.constructor_kind
    }
    pub fn super_binding(&self) -> SuperBinding {
        self.super_binding
    }
    pub fn script_mode(&self) -> JSParserScriptMode {
        self.script_mode
    }
    pub fn parse_mode(&self) -> SourceParseMode {
        self.parse_mode
    }
    pub fn derived_context_type(&self) -> DerivedContextType {
        self.derived_context_type
    }
    pub fn eval_context_type(&self) -> EvalContextType {
        self.eval_context_type
    }
    pub fn is_arrow_function_context(&self) -> bool {
        self.is_arrow_function_context
    }
    pub fn is_class_context(&self) -> bool {
        self.is_class_context
    }
    pub fn needs_class_field_initializer(&self) -> NeedsClassFieldInitializer {
        self.needs_class_field_initializer
    }
    pub fn is_builtin_default_class_constructor(&self) -> bool {
        self.is_builtin_default_class_constructor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let info = ExecutableInfo::new(
            true,
            PrivateBrandRequirement::Needed,
            false,
            ConstructorKind::Extends,
            JSParserScriptMode::Module,
            SuperBinding::NotNeeded,
            SourceParseMode::ClassStaticBlockMode,
            DerivedContextType::DerivedMethodContext,
            NeedsClassFieldInitializer::Yes,
            true,
            false,
            EvalContextType::InstanceFieldEvalContext,
            true,
        );
        assert!(info.is_constructor());
        assert_eq!(info.private_brand_requirement(), PrivateBrandRequirement::Needed);
        assert!(!info.is_builtin_function());
        assert_eq!(info.constructor_kind(), ConstructorKind::Extends);
        assert_eq!(info.super_binding(), SuperBinding::NotNeeded);
        assert_eq!(info.script_mode(), JSParserScriptMode::Module);
        assert_eq!(info.parse_mode(), SourceParseMode::ClassStaticBlockMode);
        assert_eq!(info.derived_context_type(), DerivedContextType::DerivedMethodContext);
        assert_eq!(info.needs_class_field_initializer(), NeedsClassFieldInitializer::Yes);
        assert!(info.is_arrow_function_context());
        assert!(!info.is_class_context());
        assert_eq!(info.eval_context_type(), EvalContextType::InstanceFieldEvalContext);
        assert!(info.is_builtin_default_class_constructor());
    }
}
