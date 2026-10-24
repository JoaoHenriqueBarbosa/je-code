//! Trecho de `runtime/JSCJSValue.h`: `SourceCodeRepresentation` (JSCJSValue.h:91).

/// `enum class SourceCodeRepresentation : uint8_t { Other, Integer, Double, LinkTimeConstant }`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceCodeRepresentation {
    Other = 0,
    Integer = 1,
    Double = 2,
    LinkTimeConstant = 3,
}
