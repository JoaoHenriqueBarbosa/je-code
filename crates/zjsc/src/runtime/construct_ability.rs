//! Tradução de `runtime/ConstructAbility.h`.

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructAbility {
    CanConstruct = 0,
    CannotConstruct = 1,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(ConstructAbility::CanConstruct as u8, 0);
        assert_eq!(ConstructAbility::CannotConstruct as u8, 1);
    }
}
