//! Tradução de `wasm/WasmExceptionType.h`: os tipos de trap e as mensagens que o
//! `WebAssembly.RuntimeError` leva.

macro_rules! exception_types {
    ($(($name:ident, $message:expr)),* $(,)?) => {
        /// `enum class ExceptionType`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum ExceptionType {
            $($name),*
        }

        /// `errorMessageForExceptionType`.
        pub fn error_message_for_exception_type(ty: ExceptionType) -> &'static str {
            match ty {
                $(ExceptionType::$name => $message),*
            }
        }
    };
}

exception_types! {
    (OutOfBoundsMemoryAccess, "Out of bounds memory access"),
    (UnalignedMemoryAccess, "Unaligned memory access"),
    (OutOfBoundsTableAccess, "Out of bounds table access"),
    (OutOfBoundsCallIndirect, "Out of bounds call_indirect"),
    (NullTableEntry, "call_indirect to a null table entry"),
    (NullReference, "call_ref to a null reference"),
    (NullExnrefReference, "throw_ref on a null reference"),
    (NullI31Get, "i31.get_<sx> to a null reference"),
    (BadSignature, "call_indirect to a signature that does not match"),
    (OutOfBoundsTrunc, "Out of bounds Trunc operation"),
    (Unreachable, "Unreachable code should not be executed"),
    (DivisionByZero, "Division by zero"),
    (IntegerOverflow, "Integer overflow"),
    (StackOverflow, "Stack overflow"),
    (InvalidGCTypeUse, "Unsupported use of struct or array type"),
    (OutOfBoundsArrayGet, "Out of bounds array.get"),
    (OutOfBoundsArraySet, "Out of bounds array.set"),
    (OutOfBoundsArrayFill, "Out of bounds array.fill"),
    (OutOfBoundsArrayCopy, "Out of bounds array.copy"),
    (OutOfBoundsArrayInitElem, "Out of bounds array.init_elem"),
    (OutOfBoundsArrayInitData, "Out of bounds array.init_data"),
    (BadStructNew, "Failed to allocate new struct"),
    (BadArrayNew, "Failed to allocate new array"),
    (BadArrayNewInitElem, "Out of bounds or failed to allocate in array.new_elem"),
    (BadArrayNewInitData, "Out of bounds or failed to allocate in array.new_data"),
    (NullAccess, "access to a null reference"),
    (NullArrayInitElem, "array.init_elem to a null reference"),
    (NullArrayInitData, "array.init_data to a null reference"),
    (TypeErrorInvalidValueUse, "an exported wasm function cannot contain an invalid parameter or return value"),
    (TypeErrorV128TagAccessInJS, "a v128 parameter of a tag may not be accessed from JS"),
    (TypeErrorUnexpectedNullReference, "Host function incorrectly returned null for a nonnullable reference type"),
    (NullRefAsNonNull, "ref.as_non_null to a null reference"),
    (CastFailure, "ref.cast failed to cast reference to target heap type"),
    (OutOfBoundsDataSegmentAccess, "Offset + array length would exceed the size of a data segment"),
    (OutOfBoundsElementSegmentAccess, "Offset + array length would exceed the length of an element segment"),
    (OutOfMemory, "Out of memory"),
    (IllegalArgument, "Illegal argument"),
    (Termination, "Termination"),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_follow_the_header() {
        assert_eq!(error_message_for_exception_type(ExceptionType::DivisionByZero), "Division by zero");
        assert_eq!(error_message_for_exception_type(ExceptionType::Unreachable), "Unreachable code should not be executed");
    }
}
