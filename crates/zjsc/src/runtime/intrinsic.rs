//! Porte de `runtime/Intrinsic.h` e `runtime/Intrinsic.cpp`: o `enum Intrinsic` (a lista do
//! `JSC_FOR_EACH_INTRINSIC`, na mesma ordem, que dá os valores numéricos), `intrinsicName` e
//! `iterationKindForIntrinsic`.
//!
//! `USE(BUN_JSC_ADDITIONS)` é 1 (`derived/cmakeconfig.h`), logo `BufferAccessorIntrinsic` vale e
//! entra onde o `JSC_FOR_EACH_BUN_JSC_INTRINSIC` o expande: depois dos `DataView*` e antes de
//! `WasmFunctionIntrinsic`.
//!
//! Divergência: `printInternal(PrintStream&, Intrinsic)` é o `Display`, que imprime o `intrinsicName`.

use std::fmt;

use crate::runtime::iteration_kind::IterationKind;

/// Gera o `enum Intrinsic : uint8_t` e o `intrinsicName` a partir de uma lista só, como o
/// `JSC_FOR_EACH_INTRINSIC` faz com o `JSC_DEFINE_INTRINSIC` e o `JSC_INTRINSIC_STRING`.
macro_rules! for_each_intrinsic {
    ($($name:ident),* $(,)?) => {
        /// `enum Intrinsic : uint8_t`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum Intrinsic {
            $($name),*
        }

        /// `intrinsicName`.
        pub fn intrinsic_name(intrinsic: Intrinsic) -> &'static str {
            match intrinsic {
                $(Intrinsic::$name => stringify!($name)),*
            }
        }
    };
}

for_each_intrinsic! {
    // Call intrinsics.
    NoIntrinsic, AbsIntrinsic, ACosIntrinsic, ASinIntrinsic, ATanIntrinsic, ACoshIntrinsic,
    ASinhIntrinsic, ATanhIntrinsic, MinIntrinsic, MaxIntrinsic, SqrtIntrinsic, SinIntrinsic,
    CbrtIntrinsic, Clz32Intrinsic, CosIntrinsic, TanIntrinsic, CoshIntrinsic, SinhIntrinsic,
    TanhIntrinsic, ArrayPushIntrinsic, ArrayPopIntrinsic, ArrayShiftIntrinsic,
    ArrayUnshiftIntrinsic, ArrayConcatIntrinsic, ArraySliceIntrinsic, ArraySpliceIntrinsic,
    ArrayIncludesIntrinsic, ArrayIndexOfIntrinsic, ArrayJoinIntrinsic, ArraySortIntrinsic,
    ArrayValuesIntrinsic, ArrayKeysIntrinsic, ArrayEntriesIntrinsic, ArrayConstructorOfIntrinsic,
    ArrayIsArrayIntrinsic, AsyncIteratorIntrinsic, BooleanConstructorIntrinsic,
    CharCodeAtIntrinsic, CharAtIntrinsic, DateNowIntrinsic, DatePrototypeGetTimeIntrinsic,
    DatePrototypeGetFullYearIntrinsic, DatePrototypeGetUTCFullYearIntrinsic,
    DatePrototypeGetMonthIntrinsic, DatePrototypeGetUTCMonthIntrinsic,
    DatePrototypeGetDateIntrinsic, DatePrototypeGetUTCDateIntrinsic, DatePrototypeGetDayIntrinsic,
    DatePrototypeGetUTCDayIntrinsic, DatePrototypeGetHoursIntrinsic,
    DatePrototypeGetUTCHoursIntrinsic, DatePrototypeGetMinutesIntrinsic,
    DatePrototypeGetUTCMinutesIntrinsic, DatePrototypeGetSecondsIntrinsic,
    DatePrototypeGetUTCSecondsIntrinsic, DatePrototypeGetMillisecondsIntrinsic,
    DatePrototypeGetUTCMillisecondsIntrinsic, DatePrototypeGetTimezoneOffsetIntrinsic,
    DatePrototypeGetYearIntrinsic, DatePrototypeSetTimeIntrinsic, ErrorIsErrorIntrinsic,
    FromCharCodeIntrinsic, FromCodePointIntrinsic, GlobalIsFiniteIntrinsic, GlobalIsNaNIntrinsic,
    PowIntrinsic, FloorIntrinsic, CeilIntrinsic, RoundIntrinsic, ExpIntrinsic, Expm1Intrinsic,
    LogIntrinsic, Log10Intrinsic, Log1pIntrinsic, Log2Intrinsic, RegExpExecIntrinsic,
    RegExpTestIntrinsic, RegExpMatchIntrinsic, RegExpSearchIntrinsic, RegExpSplitIntrinsic,
    ObjectAssignIntrinsic, ObjectCreateIntrinsic, ObjectDefinePropertyIntrinsic,
    ObjectGetOwnPropertyNamesIntrinsic, ObjectGetOwnPropertySymbolsIntrinsic,
    ObjectGetPrototypeOfIntrinsic, ObjectHasOwnIntrinsic, ObjectIsIntrinsic, ObjectKeysIntrinsic,
    ObjectToStringIntrinsic, ReflectGetPrototypeOfIntrinsic, ReflectOwnKeysIntrinsic,
    StringConstructorIntrinsic, StringPrototypeConcatIntrinsic, StringPrototypeAtIntrinsic,
    StringPrototypeCodePointAtIntrinsic, StringPrototypeIndexOfIntrinsic,
    StringPrototypeLastIndexOfIntrinsic, StringPrototypeIncludesIntrinsic,
    StringPrototypeStartsWithIntrinsic, StringPrototypeEndsWithIntrinsic,
    StringPrototypeLocaleCompareIntrinsic, StringPrototypeValueOfIntrinsic,
    StringPrototypeMatchIntrinsic, StringPrototypeSearchIntrinsic, StringPrototypeReplaceIntrinsic,
    StringPrototypeReplaceAllIntrinsic, StringPrototypeSplitIntrinsic,
    StringPrototypeSliceIntrinsic, StringPrototypeSubstringIntrinsic,
    StringPrototypeSubstrIntrinsic, StringPrototypeToLowerCaseIntrinsic,
    StringPrototypeToUpperCaseIntrinsic, StringPrototypeTrimIntrinsic,
    StringPrototypeTrimStartIntrinsic, StringPrototypeTrimEndIntrinsic,
    SymbolPrototypeToStringIntrinsic, NumberPrototypeToStringIntrinsic, NumberIsFiniteIntrinsic,
    NumberIsNaNIntrinsic, NumberIsSafeIntegerIntrinsic, NumberIsIntegerIntrinsic,
    NumberConstructorIntrinsic, IMulIntrinsic, RandomIntrinsic, FRoundIntrinsic, F16RoundIntrinsic,
    ToIntegerOrInfinityIntrinsic, ToLengthIntrinsic, TruncIntrinsic, TypedArrayValuesIntrinsic,
    TypedArrayKeysIntrinsic, TypedArrayEntriesIntrinsic, IsTypedArrayViewIntrinsic,
    ArrayBufferIsViewIntrinsic, BoundFunctionCallIntrinsic, RemoteFunctionCallIntrinsic,
    IteratorIntrinsic, JSMapGetIntrinsic, JSMapHasIntrinsic, JSMapSetIntrinsic,
    JSMapDeleteIntrinsic, JSMapValuesIntrinsic, JSMapKeysIntrinsic, JSMapEntriesIntrinsic,
    JSMapStorageIntrinsic, JSMapIterationNextIntrinsic, JSMapIterationEntryIntrinsic,
    JSMapIterationEntryKeyIntrinsic, JSMapIterationEntryValueIntrinsic, JSSetStorageIntrinsic,
    JSSetIterationNextIntrinsic, JSSetIterationEntryIntrinsic, JSSetIterationEntryKeyIntrinsic,
    JSMapIteratorNextIntrinsic, JSSetIteratorNextIntrinsic, JSSetHasIntrinsic, JSSetAddIntrinsic,
    JSSetDeleteIntrinsic, JSSetValuesIntrinsic, JSSetEntriesIntrinsic, JSStringIteratorIntrinsic,
    JSStringIteratorNextIntrinsic, JSWeakMapGetIntrinsic, JSWeakMapHasIntrinsic,
    JSWeakMapSetIntrinsic, JSWeakSetHasIntrinsic, JSWeakSetAddIntrinsic, HasOwnPropertyIntrinsic,
    AtomicsAddIntrinsic, AtomicsAndIntrinsic, AtomicsCompareExchangeIntrinsic,
    AtomicsExchangeIntrinsic, AtomicsIsLockFreeIntrinsic, AtomicsLoadIntrinsic,
    AtomicsNotifyIntrinsic, AtomicsOrIntrinsic, AtomicsPauseIntrinsic, AtomicsStoreIntrinsic,
    AtomicsSubIntrinsic, AtomicsWaitIntrinsic, AtomicsWaitAsyncIntrinsic, AtomicsXorIntrinsic,
    ParseIntIntrinsic, FunctionToStringIntrinsic, FunctionBindIntrinsic,
    IteratorHelperCreateIntrinsic, WrapForValidIteratorCreateIntrinsic,
    RegExpStringIteratorCreateIntrinsic, RegExpStringIteratorNextIntrinsic,
    ResolvePromiseWithFirstResolvingFunctionCallCheckIntrinsic,
    RejectPromiseWithFirstResolvingFunctionCallCheckIntrinsic,
    FulfillPromiseWithFirstResolvingFunctionCallCheckIntrinsic, NewResolvedPromiseIntrinsic,
    NewRejectedPromiseIntrinsic, PromiseConstructorResolveIntrinsic, PromiseResolveIntrinsic,
    PromiseConstructorRejectIntrinsic, PromiseRejectIntrinsic, PromisePrototypeThenIntrinsic,
    PromisePrototypeCatchIntrinsic,

    // Getter intrinsics.
    TypedArrayLengthIntrinsic, TypedArrayByteLengthIntrinsic, DataViewByteLengthIntrinsic,
    TypedArrayByteOffsetIntrinsic, UnderscoreProtoIntrinsic, SpeciesGetterIntrinsic,
    WebAssemblyInstanceExportsIntrinsic, JSSetSizeIntrinsic, JSMapSizeIntrinsic,
    RegExpHasIndicesIntrinsic, RegExpGlobalIntrinsic, RegExpIgnoreCaseIntrinsic,
    RegExpMultilineIntrinsic, RegExpDotAllIntrinsic, RegExpUnicodeIntrinsic,
    RegExpUnicodeSetsIntrinsic, RegExpStickyIntrinsic,

    // Debugging intrinsics (testing hacks do jsc.cpp, nunca expostos a usuários).
    DFGTrueIntrinsic, FTLTrueIntrinsic, OSRExitIntrinsic, IsFinalTierIntrinsic,
    SetInt32HeapPredictionIntrinsic, CheckInt32Intrinsic, FiatInt52Intrinsic,

    // Usados pelos recursos de depuração de desempenho do `$vm`.
    CPUMfenceIntrinsic, CPURdtscIntrinsic, CPUCpuidIntrinsic, CPUPauseIntrinsic,

    DataViewGetInt8, DataViewGetUint8, DataViewGetInt16, DataViewGetUint16, DataViewGetInt32,
    DataViewGetUint32, DataViewGetFloat16, DataViewGetFloat32, DataViewGetFloat64,
    DataViewGetBigInt64, DataViewGetBigUint64, DataViewSetInt8, DataViewSetUint8,
    DataViewSetInt16, DataViewSetUint16, DataViewSetInt32, DataViewSetUint32, DataViewSetFloat16,
    DataViewSetFloat32, DataViewSetFloat64, DataViewSetBigInt64, DataViewSetBigUint64,

    // `JSC_FOR_EACH_BUN_JSC_INTRINSIC`.
    BufferAccessorIntrinsic,

    WasmFunctionIntrinsic,
}

/// `iterationKindForIntrinsic`.
pub fn iteration_kind_for_intrinsic(intrinsic: Intrinsic) -> Option<IterationKind> {
    match intrinsic {
        Intrinsic::ArrayValuesIntrinsic | Intrinsic::TypedArrayValuesIntrinsic => {
            Some(IterationKind::Values)
        }
        Intrinsic::ArrayKeysIntrinsic | Intrinsic::TypedArrayKeysIntrinsic => Some(IterationKind::Keys),
        Intrinsic::ArrayEntriesIntrinsic | Intrinsic::TypedArrayEntriesIntrinsic => {
            Some(IterationKind::Entries)
        }
        _ => None,
    }
}

impl fmt::Display for Intrinsic {
    /// `printInternal(PrintStream&, Intrinsic)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(intrinsic_name(*self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_order() {
        assert_eq!(Intrinsic::NoIntrinsic as u8, 0);
        assert_eq!(intrinsic_name(Intrinsic::ACosIntrinsic), "ACosIntrinsic");
        assert_eq!(
            iteration_kind_for_intrinsic(Intrinsic::TypedArrayKeysIntrinsic),
            Some(IterationKind::Keys)
        );
        assert_eq!(iteration_kind_for_intrinsic(Intrinsic::AbsIntrinsic), None);
        assert_eq!(Intrinsic::WasmFunctionIntrinsic as u8 as usize, 252);
    }
}
