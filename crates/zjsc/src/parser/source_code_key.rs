//! Tradução de `parser/SourceCodeKey.h`.
//!
//! - `SourceCodeKey(WTF::HashTableDeletedValueType)`, `isHashTableDeletedValue()` e `HashTraits`
//!   (`SimpleClassHashTraits`, `isEmptyValue`) não existem: o `HashMap` do Rust dispensa o marcador de
//!   valor deletado. O papel de chave de tabela fica com `Hash` (o `m_hash` calculado) e `Eq`
//!   (o `operator==`).
//! - `OptionSet<CodeGenerationMode>` chega como os bits crus (`toRaw()`), `u8`, com os valores do
//!   `enum CodeGenerationMode`.
//! - `operator==` segue o ramo `USE(BUN_JSC_ADDITIONS)` ligado pelo `cmakeconfig.h`: não compara o
//!   texto do código-fonte, só o hash, o comprimento, as flags, o nome e o host.
//! - Depende de `DerivedContextType` e `EvalContextType` de `bytecode/ExecutableInfo.h`.

use std::hash::{Hash, Hasher};

use crate::bytecode::executable_info::{DerivedContextType, EvalContextType};
use crate::parser::parser_modes::{JSParserScriptMode, LexicallyScopedFeatures, STRICT_MODE_LEXICALLY_SCOPED_FEATURE};
use crate::parser::unlinked_source_code::UnlinkedSourceCode;
use crate::wtf::text::wtf_string::String as WtfString;

/// `enum class SourceCodeType`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCodeType {
    EvalType = 0,
    ProgramType = 1,
    FunctionType = 2,
    ModuleType = 3,
}

/// `class SourceCodeFlags`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceCodeFlags {
    flags: u32,
}

impl SourceCodeFlags {
    /// `SourceCodeFlags(codeType, lexicallyScopedFeatures, scriptMode, derivedContextType,
    /// evalContextType, isArrowFunctionContext, codeGenerationMode)`.
    pub fn new(
        code_type: SourceCodeType,
        lexically_scoped_features: LexicallyScopedFeatures,
        script_mode: JSParserScriptMode,
        derived_context_type: DerivedContextType,
        eval_context_type: EvalContextType,
        is_arrow_function_context: bool,
        code_generation_mode: u8,
    ) -> SourceCodeFlags {
        SourceCodeFlags {
            flags: ((code_generation_mode as u32) << 6)
                | ((script_mode as u32) << 5)
                | ((is_arrow_function_context as u32) << 4)
                | ((eval_context_type as u32) << 3)
                | ((derived_context_type as u32) << 2)
                | ((code_type as u32) << 1)
                | ((lexically_scoped_features & STRICT_MODE_LEXICALLY_SCOPED_FEATURE) as u32),
        }
    }

    /// `bits()`.
    pub fn bits(&self) -> u32 {
        self.flags
    }
}

/// `class SourceCodeKey`.
#[derive(Clone, Default)]
pub struct SourceCodeKey {
    source_code: UnlinkedSourceCode,
    name: WtfString,
    flags: SourceCodeFlags,
    function_constructor_parameters_end_position: i32,
    hash: u32,
}

impl SourceCodeKey {
    /// `SourceCodeKey()`. O C++ deixa `m_functionConstructorParametersEndPosition` e `m_hash` sem
    /// inicializar; aqui ficam em zero.
    pub fn new() -> SourceCodeKey {
        SourceCodeKey::default()
    }

    /// `SourceCodeKey(sourceCode, name, codeType, lexicallyScopedFeatures, scriptMode,
    /// derivedContextType, evalContextType, isArrowFunctionContext, codeGenerationMode,
    /// functionConstructorParametersEndPosition)`.
    #[allow(clippy::too_many_arguments)]
    pub fn with(
        source_code: &UnlinkedSourceCode,
        name: &WtfString,
        code_type: SourceCodeType,
        lexically_scoped_features: LexicallyScopedFeatures,
        script_mode: JSParserScriptMode,
        derived_context_type: DerivedContextType,
        eval_context_type: EvalContextType,
        is_arrow_function_context: bool,
        code_generation_mode: u8,
        function_constructor_parameters_end_position: Option<i32>,
    ) -> SourceCodeKey {
        let flags = SourceCodeFlags::new(
            code_type,
            lexically_scoped_features,
            script_mode,
            derived_context_type,
            eval_context_type,
            is_arrow_function_context,
            code_generation_mode,
        );
        SourceCodeKey {
            source_code: source_code.clone(),
            name: name.clone(),
            flags,
            function_constructor_parameters_end_position: function_constructor_parameters_end_position.unwrap_or(-1),
            hash: source_code.hash() ^ flags.bits(),
        }
    }

    /// `hash()`.
    pub fn hash(&self) -> u32 {
        self.hash
    }

    /// `source()`.
    pub fn source(&self) -> &UnlinkedSourceCode {
        &self.source_code
    }

    /// `length()`.
    pub fn length(&self) -> usize {
        self.source_code.length() as usize
    }

    /// `isNull()`.
    pub fn is_null(&self) -> bool {
        self.source_code.is_null()
    }

    /// `string()`: o `StringView` vira `String`. To save memory, we compute our string on demand.
    pub fn string(&self) -> WtfString {
        self.source_code.view()
    }

    /// `host()`: o host da URL da origem do provedor (vazio sem provedor, onde o C++ desreferenciaria
    /// um ponteiro nulo).
    pub fn host(&self) -> WtfString {
        match &self.source_code.provider {
            None => WtfString::default(),
            Some(provider) => provider.source_origin().url().host(),
        }
    }

    /// `functionConstructorParametersEndPosition()`.
    pub fn function_constructor_parameters_end_position(&self) -> i32 {
        self.function_constructor_parameters_end_position
    }
}

impl PartialEq for SourceCodeKey {
    /// `operator==(const SourceCodeKey&) const`.
    fn eq(&self, other: &SourceCodeKey) -> bool {
        self.hash == other.hash
            && self.length() == other.length()
            && self.flags == other.flags
            && self.function_constructor_parameters_end_position == other.function_constructor_parameters_end_position
            && self.name == other.name
            && self.host() == other.host()
    }
}

impl Eq for SourceCodeKey {}

impl Hash for SourceCodeKey {
    /// O `DefaultHash` do C++ devolve `hash()`.
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::source_provider::{SourceProvider, SourceProviderSourceType, StringSourceProvider};
    use crate::parser::source_tainted_origin::SourceTaintedOrigin;
    use crate::runtime::source_origin::SourceOrigin;
    use crate::wtf::text::text_position::TextPosition;
    use std::rc::Rc;

    fn key(source: &str, code_type: SourceCodeType) -> SourceCodeKey {
        let provider: Rc<dyn SourceProvider> = StringSourceProvider::create(
            &WtfString::from_latin1(source.as_bytes()),
            &SourceOrigin::default(),
            WtfString::default(),
            SourceTaintedOrigin::Untainted,
            TextPosition::default(),
            SourceProviderSourceType::Program,
        );
        SourceCodeKey::with(
            &UnlinkedSourceCode::from_provider(provider),
            &WtfString::default(),
            code_type,
            STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
            JSParserScriptMode::Classic,
            DerivedContextType::None,
            EvalContextType::None,
            false,
            0,
            None,
        )
    }

    #[test]
    fn flags_bits() {
        let flags = SourceCodeFlags::new(
            SourceCodeType::ModuleType,
            STRICT_MODE_LEXICALLY_SCOPED_FEATURE,
            JSParserScriptMode::Module,
            DerivedContextType::None,
            EvalContextType::None,
            true,
            0b101,
        );
        assert_eq!(flags.bits(), (0b101 << 6) | (1 << 5) | (1 << 4) | (3 << 1) | 1);
    }

    #[test]
    fn equality_and_hash() {
        let a = key("1 + 1", SourceCodeType::ProgramType);
        let b = key("1 + 1", SourceCodeType::ProgramType);
        assert!(a == b);
        assert_eq!(a.hash(), b.hash());
        assert!(a != key("1 + 1", SourceCodeType::EvalType));
        assert_eq!(a.function_constructor_parameters_end_position(), -1);
        assert_eq!(a.string(), WtfString::from_latin1(b"1 + 1"));
        assert!(SourceCodeKey::new().is_null());
    }
}
