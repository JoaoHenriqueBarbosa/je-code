//! Tradução de `runtime/ConstructorKind.h`.

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructorKind {
    /// All the other functions.
    None = 0,
    /// Class base constructor.
    Base = 1,
    /// Class derived constructor.
    Extends = 2,
    /// Naked constructor, only used for builtin functions.
    Naked = 3,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(ConstructorKind::None as u8, 0);
        assert_eq!(ConstructorKind::Base as u8, 1);
        assert_eq!(ConstructorKind::Extends as u8, 2);
        assert_eq!(ConstructorKind::Naked as u8, 3);
    }
}
