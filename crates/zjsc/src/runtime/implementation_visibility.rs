//! Tradução de `runtime/ImplementationVisibility.h`.

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImplementationVisibility {
    Public = 0,
    Private = 1,
    PrivateRecursive = 2,
}

/// `bitWidthOfImplementationVisibility`.
pub const BIT_WIDTH_OF_IMPLEMENTATION_VISIBILITY: u32 = 2;

const _: () = assert!((ImplementationVisibility::PrivateRecursive as u32) < (1 << BIT_WIDTH_OF_IMPLEMENTATION_VISIBILITY));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(ImplementationVisibility::Public as u8, 0);
        assert_eq!(ImplementationVisibility::Private as u8, 1);
        assert_eq!(ImplementationVisibility::PrivateRecursive as u8, 2);
        assert_eq!(BIT_WIDTH_OF_IMPLEMENTATION_VISIBILITY, 2);
    }
}
