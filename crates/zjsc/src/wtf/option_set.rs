//! Porte de `WTF/wtf/OptionSet.h`.
//!
//! `OptionSet<E>` guarda um conjunto de enumeradores numa máscara de bits. O C++ deduz o bit de
//! `static_cast<StorageType>(e)`; aqui o enumerador diz o seu bit por `OptionSetFlag::bit`
//! (alguns enums, como `SourceParseMode`, usam o valor como posição do bit).
//!
//! Diferenças de forma, sem mudança de semântica:
//! - `OptionSet(std::initializer_list<E>)` é `OptionSet::new(&[..])`, e `OptionSet()` é `empty()`.
//! - `OptionSet::new` não é `const fn`, porque chama o trait; use `from_raw` onde precisar de const.
//! - `begin()/end()` viram `iter()`, que percorre `OptionSetFlag::ALL` na ordem da declaração.

use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign, Not, Sub};

/// Enumerador que pode entrar num `OptionSet`: diz qual é o seu bit e o tipo da máscara.
pub trait OptionSetFlag: Copy + 'static {
    /// `OptionSet::StorageType`.
    type Mask: Copy
        + Eq
        + fmt::Debug
        + BitOr<Output = Self::Mask>
        + BitAnd<Output = Self::Mask>
        + Not<Output = Self::Mask>;

    /// A máscara sem nenhum bit.
    const NONE: Self::Mask;

    /// Todos os enumeradores, na ordem de iteração.
    const ALL: &'static [Self];

    /// O bit do enumerador na máscara.
    fn bit(self) -> Self::Mask;
}

/// `WTF::OptionSet<E>`.
pub struct OptionSet<E: OptionSetFlag> {
    storage: E::Mask,
}

impl<E: OptionSetFlag> OptionSet<E> {
    /// `OptionSet()`.
    pub const fn empty() -> Self {
        OptionSet { storage: E::NONE }
    }

    /// `OptionSet(std::initializer_list<E>)`.
    pub fn new(flags: &[E]) -> Self {
        let mut storage = E::NONE;
        for &flag in flags {
            storage = storage | flag.bit();
        }
        OptionSet { storage }
    }

    /// `OptionSet::fromRaw`.
    pub const fn from_raw(raw: E::Mask) -> Self {
        OptionSet { storage: raw }
    }

    /// `OptionSet::toRaw`.
    pub fn to_raw(self) -> E::Mask {
        self.storage
    }

    pub fn is_empty(self) -> bool {
        self.storage == E::NONE
    }

    /// `OptionSet::contains(E)`: igual a `containsAny(option)`.
    pub fn contains(self, option: E) -> bool {
        self.storage & option.bit() != E::NONE
    }

    /// `OptionSet::containsAny`.
    pub fn contains_any(self, other: Self) -> bool {
        self.storage & other.storage != E::NONE
    }

    /// `OptionSet::containsAll`.
    pub fn contains_all(self, other: Self) -> bool {
        self.storage & other.storage == other.storage
    }

    /// `OptionSet::containsOnly`.
    pub fn contains_only(self, other: Self) -> bool {
        self.storage == self.storage & other.storage
    }

    /// `OptionSet::add(E)`.
    pub fn add(&mut self, option: E) {
        self.storage = self.storage | option.bit();
    }

    /// `OptionSet::remove(E)`.
    pub fn remove(&mut self, option: E) {
        self.storage = self.storage & !option.bit();
    }

    /// `OptionSet::set(E, bool)`.
    pub fn set(&mut self, option: E, value: bool) {
        if value {
            self.add(option);
        } else {
            self.remove(option);
        }
    }

    /// Os enumeradores presentes, na ordem de `OptionSetFlag::ALL`.
    pub fn iter(self) -> impl Iterator<Item = E> {
        E::ALL.iter().copied().filter(move |&flag| self.contains(flag))
    }
}

impl<E: OptionSetFlag> Clone for OptionSet<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E: OptionSetFlag> Copy for OptionSet<E> {}

impl<E: OptionSetFlag> Default for OptionSet<E> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<E: OptionSetFlag> PartialEq for OptionSet<E> {
    fn eq(&self, other: &Self) -> bool {
        self.storage == other.storage
    }
}

impl<E: OptionSetFlag> Eq for OptionSet<E> {}

impl<E: OptionSetFlag> fmt::Debug for OptionSet<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OptionSet").field("storage", &self.storage).finish()
    }
}

impl<E: OptionSetFlag> From<E> for OptionSet<E> {
    /// `OptionSet(E e)`.
    fn from(option: E) -> Self {
        OptionSet { storage: option.bit() }
    }
}

impl<E: OptionSetFlag> BitOr for OptionSet<E> {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        OptionSet { storage: self.storage | other.storage }
    }
}

impl<E: OptionSetFlag> BitOrAssign for OptionSet<E> {
    fn bitor_assign(&mut self, other: Self) {
        *self = *self | other;
    }
}

impl<E: OptionSetFlag> BitAnd for OptionSet<E> {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        OptionSet { storage: self.storage & other.storage }
    }
}

impl<E: OptionSetFlag> Sub for OptionSet<E> {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        OptionSet { storage: self.storage & !other.storage }
    }
}

impl<E: OptionSetFlag> IntoIterator for OptionSet<E> {
    type Item = E;
    type IntoIter = std::vec::IntoIter<E>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter().collect::<Vec<E>>().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(u8)]
    enum Flag {
        A = 1,
        B = 2,
        C = 4,
    }

    impl OptionSetFlag for Flag {
        type Mask = u8;
        const NONE: u8 = 0;
        const ALL: &'static [Flag] = &[Flag::A, Flag::B, Flag::C];
        fn bit(self) -> u8 {
            self as u8
        }
    }

    #[test]
    fn basic_operations() {
        let mut set = OptionSet::new(&[Flag::A, Flag::C]);
        assert!(set.contains(Flag::A) && !set.contains(Flag::B));
        assert!(set.contains_any(OptionSet::new(&[Flag::B, Flag::C])));
        assert!(!set.contains_all(OptionSet::new(&[Flag::B, Flag::C])));
        set.add(Flag::B);
        set.remove(Flag::A);
        assert_eq!(set.to_raw(), 6);
        assert_eq!(set.iter().collect::<Vec<_>>(), vec![Flag::B, Flag::C]);
        assert_eq!((set & OptionSet::from(Flag::C)).to_raw(), 4);
        assert_eq!(OptionSet::<Flag>::default(), OptionSet::empty());
    }
}
