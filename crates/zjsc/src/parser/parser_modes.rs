//! Tradução de `parser/ParserModes.h`.

use crate::runtime::construct_ability::ConstructAbility;
use crate::runtime::identifier::Identifier;
use crate::wtf::option_set::{OptionSet, OptionSetFlag};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JSParserBuiltinMode {
    NotBuiltin = 0,
    Builtin = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JSParserScriptMode {
    Classic = 0,
    Module = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuperBinding {
    Needed = 0,
    NotNeeded = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateBrandRequirement {
    None = 0,
    Needed = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeGenerationMode {
    Debugger = 1 << 0,
    TypeProfiler = 1 << 1,
    ControlFlowProfiler = 1 << 2,
}

impl OptionSetFlag for CodeGenerationMode {
    type Mask = u8;
    const NONE: u8 = 0;
    const ALL: &'static [CodeGenerationMode] =
        &[CodeGenerationMode::Debugger, CodeGenerationMode::TypeProfiler, CodeGenerationMode::ControlFlowProfiler];

    fn bit(self) -> u8 {
        self as u8
    }
}

/// `OptionSet<CodeGenerationMode>`.
pub type CodeGenerationModeSet = OptionSet<CodeGenerationMode>;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionMode {
    None = 0,
    FunctionExpression = 1,
    FunctionDeclaration = 2,
    MethodDefinition = 3,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionConstructionMode {
    Function = 0,
    Generator = 1,
    Async = 2,
    AsyncGenerator = 3,
}

// Keep it less than 32, it means this should be within 5 bits.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceParseMode {
    NormalFunctionMode = 0,
    GeneratorBodyMode = 1,
    GeneratorWrapperFunctionMode = 2,
    GetterMode = 3,
    SetterMode = 4,
    MethodMode = 5,
    ArrowFunctionMode = 6,
    AsyncFunctionBodyMode = 7,
    AsyncArrowFunctionBodyMode = 8,
    AsyncFunctionMode = 9,
    AsyncMethodMode = 10,
    AsyncArrowFunctionMode = 11,
    ProgramMode = 12,
    ModuleAnalyzeMode = 13,
    ModuleEvaluateMode = 14,
    AsyncGeneratorBodyMode = 15,
    AsyncGeneratorWrapperFunctionMode = 16,
    AsyncGeneratorWrapperMethodMode = 17,
    GeneratorWrapperMethodMode = 18,
    ClassFieldInitializerMode = 19,
    ClassStaticBlockMode = 20,
}

/// `SourceParseModeSet`: o enumerador é a posição do bit (`1u32 << mode`).
impl OptionSetFlag for SourceParseMode {
    type Mask = u32;
    const NONE: u32 = 0;
    const ALL: &'static [SourceParseMode] = &[
        SourceParseMode::NormalFunctionMode,
        SourceParseMode::GeneratorBodyMode,
        SourceParseMode::GeneratorWrapperFunctionMode,
        SourceParseMode::GetterMode,
        SourceParseMode::SetterMode,
        SourceParseMode::MethodMode,
        SourceParseMode::ArrowFunctionMode,
        SourceParseMode::AsyncFunctionBodyMode,
        SourceParseMode::AsyncArrowFunctionBodyMode,
        SourceParseMode::AsyncFunctionMode,
        SourceParseMode::AsyncMethodMode,
        SourceParseMode::AsyncArrowFunctionMode,
        SourceParseMode::ProgramMode,
        SourceParseMode::ModuleAnalyzeMode,
        SourceParseMode::ModuleEvaluateMode,
        SourceParseMode::AsyncGeneratorBodyMode,
        SourceParseMode::AsyncGeneratorWrapperFunctionMode,
        SourceParseMode::AsyncGeneratorWrapperMethodMode,
        SourceParseMode::GeneratorWrapperMethodMode,
        SourceParseMode::ClassFieldInitializerMode,
        SourceParseMode::ClassStaticBlockMode,
    ];

    fn bit(self) -> u32 {
        1u32 << (self as u32)
    }
}

pub type SourceParseModeSet = OptionSet<SourceParseMode>;

#[inline(always)]
pub fn is_function_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[
        NormalFunctionMode,
        GeneratorBodyMode,
        GeneratorWrapperFunctionMode,
        GeneratorWrapperMethodMode,
        GetterMode,
        SetterMode,
        MethodMode,
        ArrowFunctionMode,
        AsyncFunctionBodyMode,
        AsyncFunctionMode,
        AsyncMethodMode,
        AsyncArrowFunctionMode,
        AsyncArrowFunctionBodyMode,
        AsyncGeneratorBodyMode,
        AsyncGeneratorWrapperFunctionMode,
        AsyncGeneratorWrapperMethodMode,
        ClassFieldInitializerMode,
        ClassStaticBlockMode,
    ])
    .contains(parse_mode)
}

#[inline(always)]
pub fn is_async_function_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[
        AsyncGeneratorWrapperFunctionMode,
        AsyncGeneratorBodyMode,
        AsyncGeneratorWrapperMethodMode,
        AsyncFunctionBodyMode,
        AsyncFunctionMode,
        AsyncMethodMode,
        AsyncArrowFunctionMode,
        AsyncArrowFunctionBodyMode,
    ])
    .contains(parse_mode)
}

#[inline(always)]
pub fn is_async_arrow_function_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[AsyncArrowFunctionMode, AsyncArrowFunctionBodyMode]).contains(parse_mode)
}

#[inline(always)]
pub fn is_async_generator_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[
        AsyncGeneratorWrapperFunctionMode,
        AsyncGeneratorWrapperMethodMode,
        AsyncGeneratorBodyMode,
    ])
    .contains(parse_mode)
}

#[inline(always)]
pub fn is_async_generator_wrapper_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[AsyncGeneratorWrapperFunctionMode, AsyncGeneratorWrapperMethodMode])
        .contains(parse_mode)
}

#[inline(always)]
pub fn is_async_function_or_async_generator_wrapper_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[
        AsyncArrowFunctionMode,
        AsyncFunctionMode,
        AsyncGeneratorWrapperFunctionMode,
        AsyncGeneratorWrapperMethodMode,
        AsyncMethodMode,
    ])
    .contains(parse_mode)
}

#[inline(always)]
pub fn is_async_function_wrapper_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[AsyncArrowFunctionMode, AsyncFunctionMode, AsyncMethodMode]).contains(parse_mode)
}

#[inline(always)]
pub fn is_async_function_body_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[AsyncFunctionBodyMode, AsyncGeneratorBodyMode, AsyncArrowFunctionBodyMode])
        .contains(parse_mode)
}

#[inline(always)]
pub fn is_generator_method_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[GeneratorWrapperMethodMode]).contains(parse_mode)
}

#[inline(always)]
pub fn is_async_method_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[AsyncMethodMode]).contains(parse_mode)
}

#[inline(always)]
pub fn is_async_generator_method_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[AsyncGeneratorWrapperMethodMode]).contains(parse_mode)
}

#[inline(always)]
pub fn is_method_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[
        GeneratorWrapperMethodMode,
        GetterMode,
        SetterMode,
        MethodMode,
        AsyncMethodMode,
        AsyncGeneratorWrapperMethodMode,
        ClassStaticBlockMode,
    ])
    .contains(parse_mode)
}

#[inline(always)]
pub fn is_generator_or_async_function_body_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[
        GeneratorBodyMode,
        AsyncFunctionBodyMode,
        AsyncGeneratorBodyMode,
        AsyncArrowFunctionBodyMode,
    ])
    .contains(parse_mode)
}

#[inline(always)]
pub fn is_generator_or_async_function_wrapper_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[
        GeneratorWrapperFunctionMode,
        GeneratorWrapperMethodMode,
        AsyncFunctionMode,
        AsyncArrowFunctionMode,
        AsyncGeneratorWrapperFunctionMode,
        AsyncMethodMode,
        AsyncGeneratorWrapperMethodMode,
    ])
    .contains(parse_mode)
}

#[inline(always)]
pub fn is_generator_or_async_generator_wrapper_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[
        GeneratorWrapperFunctionMode,
        GeneratorWrapperMethodMode,
        AsyncGeneratorWrapperFunctionMode,
        AsyncGeneratorWrapperMethodMode,
    ])
    .contains(parse_mode)
}

#[inline(always)]
pub fn is_generator_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[GeneratorBodyMode, GeneratorWrapperFunctionMode, GeneratorWrapperMethodMode])
        .contains(parse_mode)
}

#[inline(always)]
pub fn is_generator_wrapper_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[GeneratorWrapperFunctionMode, GeneratorWrapperMethodMode]).contains(parse_mode)
}

#[inline(always)]
pub fn is_arrow_function_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[ArrowFunctionMode, AsyncArrowFunctionMode, AsyncArrowFunctionBodyMode])
        .contains(parse_mode)
}

#[inline(always)]
pub fn is_module_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[ModuleAnalyzeMode, ModuleEvaluateMode]).contains(parse_mode)
}

#[inline(always)]
pub fn is_program_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[ProgramMode]).contains(parse_mode)
}

#[inline(always)]
pub fn is_program_or_module_parse_mode(parse_mode: SourceParseMode) -> bool {
    use SourceParseMode::*;
    SourceParseModeSet::new(&[ProgramMode, ModuleAnalyzeMode, ModuleEvaluateMode]).contains(parse_mode)
}

#[inline(always)]
pub fn construct_ability_for_parse_mode(parse_mode: SourceParseMode) -> ConstructAbility {
    if parse_mode == SourceParseMode::NormalFunctionMode {
        return ConstructAbility::CanConstruct;
    }

    ConstructAbility::CannotConstruct
}

pub fn function_name_is_in_scope(name: &Identifier, function_mode: FunctionMode) -> bool {
    if name.is_null() {
        return false;
    }

    if function_mode != FunctionMode::FunctionExpression {
        return false;
    }

    true
}

pub fn function_name_scope_is_dynamic(uses_eval: bool, is_strict_mode: bool) -> bool {
    // If non-strict eval is in play, a function gets a separate object in the scope chain for its name.
    // This enables eval to declare and then delete a name that shadows the function's name.

    if !uses_eval {
        return false;
    }

    if is_strict_mode {
        return false;
    }

    true
}

pub type LexicallyScopedFeatures = u8;

pub const NO_LEXICALLY_SCOPED_FEATURES: LexicallyScopedFeatures = 0;
pub const STRICT_MODE_LEXICALLY_SCOPED_FEATURE: LexicallyScopedFeatures = 1 << 0;
pub const TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE: LexicallyScopedFeatures = 1 << 1;

pub const ALL_LEXICALLY_SCOPED_FEATURES: LexicallyScopedFeatures = NO_LEXICALLY_SCOPED_FEATURES
    | STRICT_MODE_LEXICALLY_SCOPED_FEATURE
    | TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE;
pub const BIT_WIDTH_OF_LEXICALLY_SCOPED_FEATURES: u32 = 2;
const _: () = assert!(
    (ALL_LEXICALLY_SCOPED_FEATURES as u32) <= (1 << BIT_WIDTH_OF_LEXICALLY_SCOPED_FEATURES) - 1,
    "LexicallyScopedFeatures must be 2bits"
);

pub type CodeFeatures = u16;

pub const NO_FEATURES: CodeFeatures = 0;
pub const EVAL_FEATURE: CodeFeatures = 1 << 0;
pub const ARGUMENTS_FEATURE: CodeFeatures = 1 << 1;
pub const WITH_FEATURE: CodeFeatures = 1 << 2;
pub const THIS_FEATURE: CodeFeatures = 1 << 3;
pub const NON_SIMPLE_PARAMETER_LIST_FEATURE: CodeFeatures = 1 << 4;
pub const SHADOWS_ARGUMENTS_FEATURE: CodeFeatures = 1 << 5;
pub const ARROW_FUNCTION_FEATURE: CodeFeatures = 1 << 6;
pub const AWAIT_FEATURE: CodeFeatures = 1 << 7;
pub const SUPER_CALL_FEATURE: CodeFeatures = 1 << 8;
pub const SUPER_PROPERTY_FEATURE: CodeFeatures = 1 << 9;
pub const NEW_TARGET_FEATURE: CodeFeatures = 1 << 10;
pub const NO_EVAL_CACHE_FEATURE: CodeFeatures = 1 << 11;
pub const IMPORT_META_FEATURE: CodeFeatures = 1 << 12;
pub const ASYNC_FUNCTION_WITHOUT_AWAIT_FEATURE: CodeFeatures = 1 << 13;

pub const ALL_FEATURES: CodeFeatures = EVAL_FEATURE
    | ARGUMENTS_FEATURE
    | WITH_FEATURE
    | THIS_FEATURE
    | NON_SIMPLE_PARAMETER_LIST_FEATURE
    | SHADOWS_ARGUMENTS_FEATURE
    | ARROW_FUNCTION_FEATURE
    | AWAIT_FEATURE
    | SUPER_CALL_FEATURE
    | SUPER_PROPERTY_FEATURE
    | NEW_TARGET_FEATURE
    | NO_EVAL_CACHE_FEATURE
    | IMPORT_META_FEATURE
    | ASYNC_FUNCTION_WITHOUT_AWAIT_FEATURE;
pub const BIT_WIDTH_OF_CODE_FEATURES: u32 = 14;
const _: () = assert!(
    (ALL_FEATURES as u32) <= (1 << BIT_WIDTH_OF_CODE_FEATURES) - 1,
    "CodeFeatures must fit within 14 bits"
);

pub type InnerArrowFunctionCodeFeatures = u8;

pub const NO_INNER_ARROW_FUNCTION_FEATURES: InnerArrowFunctionCodeFeatures = 0;
pub const EVAL_INNER_ARROW_FUNCTION_FEATURE: InnerArrowFunctionCodeFeatures = 1 << 0;
pub const ARGUMENTS_INNER_ARROW_FUNCTION_FEATURE: InnerArrowFunctionCodeFeatures = 1 << 1;
pub const THIS_INNER_ARROW_FUNCTION_FEATURE: InnerArrowFunctionCodeFeatures = 1 << 2;
pub const SUPER_CALL_INNER_ARROW_FUNCTION_FEATURE: InnerArrowFunctionCodeFeatures = 1 << 3;
pub const SUPER_PROPERTY_INNER_ARROW_FUNCTION_FEATURE: InnerArrowFunctionCodeFeatures = 1 << 4;
pub const NEW_TARGET_INNER_ARROW_FUNCTION_FEATURE: InnerArrowFunctionCodeFeatures = 1 << 5;

pub const ALL_INNER_ARROW_FUNCTION_CODE_FEATURES: InnerArrowFunctionCodeFeatures =
    EVAL_INNER_ARROW_FUNCTION_FEATURE
        | ARGUMENTS_INNER_ARROW_FUNCTION_FEATURE
        | THIS_INNER_ARROW_FUNCTION_FEATURE
        | SUPER_CALL_INNER_ARROW_FUNCTION_FEATURE
        | SUPER_PROPERTY_INNER_ARROW_FUNCTION_FEATURE
        | NEW_TARGET_INNER_ARROW_FUNCTION_FEATURE;
const _: () = assert!(
    ALL_INNER_ARROW_FUNCTION_CODE_FEATURES <= 0b111111,
    "InnerArrowFunctionCodeFeatures must be 6bits"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mode_predicates() {
        assert!(is_function_parse_mode(SourceParseMode::ClassStaticBlockMode));
        assert!(!is_function_parse_mode(SourceParseMode::ProgramMode));
        assert!(is_program_or_module_parse_mode(SourceParseMode::ModuleEvaluateMode));
        assert!(is_async_function_parse_mode(SourceParseMode::AsyncGeneratorBodyMode));
        assert!(!is_async_function_parse_mode(SourceParseMode::GeneratorBodyMode));
        assert!(is_method_parse_mode(SourceParseMode::ClassStaticBlockMode));
        assert_eq!(
            construct_ability_for_parse_mode(SourceParseMode::NormalFunctionMode),
            ConstructAbility::CanConstruct
        );
        assert_eq!(
            construct_ability_for_parse_mode(SourceParseMode::ArrowFunctionMode),
            ConstructAbility::CannotConstruct
        );
    }

    #[test]
    fn feature_bits() {
        assert_eq!(ALL_FEATURES, 0x3fff);
        assert_eq!(ALL_INNER_ARROW_FUNCTION_CODE_FEATURES, 0b111111);
        assert_eq!(CodeGenerationMode::ControlFlowProfiler as u8, 4);
        assert_eq!(SourceParseMode::ClassStaticBlockMode as u8, 20);
    }
}
