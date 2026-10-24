//! Porte de `runtime/temporal/core/TemporalCoreTypes.h`: o erro do núcleo puro de Temporal
//! (`TemporalError`, `TemporalResult<T>`) e os construtores `rangeError`/`typeError`.
//!
//! DIVERGÊNCIA: `std::expected<T, TemporalError>` é `Result<T, TemporalError>`. Das mensagens de erro de
//! ICU (`icuOpenCalendarFailed` e irmãs), de `TransitionDirection` e do que o núcleo de calendário e de
//! fuso usa, só entra o que `TemporalCoreTypes.h` declara e os portes atuais precisam: o resto chega com
//! `CalendarICUBridge` e `TimeZoneICUBridge`.

use crate::runtime::host_call::Thrown;

/// `enum class TemporalErrorKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalErrorKind {
    RangeError,
    TypeError,
}

/// `struct TemporalError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemporalError {
    pub kind: TemporalErrorKind,
    pub message: String,
}

/// `TemporalResult<T>`.
pub type TemporalResult<T> = Result<T, TemporalError>;

/// `TemporalCore::rangeError(msg)`.
pub fn range_error(message: &str) -> TemporalError {
    TemporalError { kind: TemporalErrorKind::RangeError, message: message.to_string() }
}

/// `enum class TransitionDirection : bool`: a direção de `GetNamedTimeZoneNextTransition` e
/// `GetNamedTimeZonePreviousTransition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionDirection {
    Next,
    Previous,
}

/// `throwTemporalError(globalObject, scope, error)`: o `RangeError` ou o `TypeError` do `kind`.
impl From<TemporalError> for Thrown {
    fn from(error: TemporalError) -> Thrown {
        match error.kind {
            TemporalErrorKind::RangeError => Thrown::RangeError(error.message),
            TemporalErrorKind::TypeError => Thrown::TypeError(error.message),
        }
    }
}
