//! Tradução de `runtime/InlineAttribute.h`.

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineAttribute {
    None = 0,
    Always = 1,
}
