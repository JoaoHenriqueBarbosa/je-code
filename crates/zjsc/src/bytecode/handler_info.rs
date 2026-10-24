//! Porte de `bytecode/HandlerInfo.h`.
//!
//! `HandlerInfo::initialize(.., CodeLocationLabel)` e `nativeCode` ficam sob `ENABLE(JIT)` e somem.
//! `typeName` é só depuração.

/// `enum class HandlerType : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandlerType {
    Catch = 0,
    Finally = 1,
    SynthesizedCatch = 2,
    SynthesizedFinally = 3,
}

/// `enum class RequiredHandler`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequiredHandler {
    CatchHandler,
    AnyHandler,
}

impl HandlerType {
    /// `static_cast<HandlerType>(typeBits)`; `typeBits` tem 2 bits.
    fn from_bits(bits: u8) -> HandlerType {
        match bits & 3 {
            0 => HandlerType::Catch,
            1 => HandlerType::Finally,
            2 => HandlerType::SynthesizedCatch,
            _ => HandlerType::SynthesizedFinally,
        }
    }
}

/// `HandlerInfoBase`: `start`, `end`, `target` e `typeBits : 2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandlerInfoBase {
    pub start: u32,
    pub end: u32,
    pub target: u32,
    type_bits: u8,
}

impl Default for HandlerInfoBase {
    fn default() -> Self {
        HandlerInfoBase { start: 0, end: 0, target: 0, type_bits: HandlerType::Catch as u8 }
    }
}

impl HandlerInfoBase {
    pub fn type_(&self) -> HandlerType {
        HandlerType::from_bits(self.type_bits)
    }

    pub fn set_type(&mut self, handler_type: HandlerType) {
        self.type_bits = handler_type as u8;
    }

    pub fn is_catch_handler(&self) -> bool {
        self.type_() == HandlerType::Catch
    }

    /// `handlerForIndex`: os handlers vêm do mais interno ao mais externo, então o primeiro que
    /// contém o índice é o correto.
    pub fn handler_for_index<'a, H, I>(handlers: I, index: u32, required_handler: RequiredHandler) -> Option<&'a H>
    where
        H: AsRef<HandlerInfoBase> + 'a,
        I: IntoIterator<Item = &'a H>,
    {
        for handler in handlers {
            let base = handler.as_ref();
            if required_handler == RequiredHandler::CatchHandler && !base.is_catch_handler() {
                continue;
            }
            if base.start <= index && base.end > index {
                return Some(handler);
            }
        }
        None
    }
}

impl AsRef<HandlerInfoBase> for HandlerInfoBase {
    fn as_ref(&self) -> &HandlerInfoBase {
        self
    }
}

/// `UnlinkedHandlerInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct UnlinkedHandlerInfo {
    pub base: HandlerInfoBase,
}

impl UnlinkedHandlerInfo {
    pub fn new(start: u32, end: u32, target: u32, handler_type: HandlerType) -> Self {
        let mut base = HandlerInfoBase { start, end, target, type_bits: 0 };
        base.set_type(handler_type);
        debug_assert!(base.type_() == handler_type);
        UnlinkedHandlerInfo { base }
    }
}

impl AsRef<HandlerInfoBase> for UnlinkedHandlerInfo {
    fn as_ref(&self) -> &HandlerInfoBase {
        &self.base
    }
}

/// `HandlerInfo` (sem `nativeCode`, que existe só com JIT).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct HandlerInfo {
    pub base: HandlerInfoBase,
}

impl HandlerInfo {
    pub fn initialize(&mut self, unlinked_info: &UnlinkedHandlerInfo) {
        self.base = unlinked_info.base;
    }
}

impl AsRef<HandlerInfoBase> for HandlerInfo {
    fn as_ref(&self) -> &HandlerInfoBase {
        &self.base
    }
}
