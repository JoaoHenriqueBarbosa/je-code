//! Tradução de `runtime/DefinePropertyAttributes.h`.

use crate::wtf::tri_state::TriState;

pub const CONFIGURABLE_SHIFT: u32 = 0;
pub const ENUMERABLE_SHIFT: u32 = 2;
pub const WRITABLE_SHIFT: u32 = 4;
pub const VALUE_SHIFT: u32 = 6;
pub const GET_SHIFT: u32 = 7;
pub const SET_SHIFT: u32 = 8;

/// `DefinePropertyAttributes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefinePropertyAttributes {
    attributes: u32,
}

impl Default for DefinePropertyAttributes {
    fn default() -> Self {
        DefinePropertyAttributes {
            attributes: ((TriState::Indeterminate as u32) << CONFIGURABLE_SHIFT)
                | ((TriState::Indeterminate as u32) << ENUMERABLE_SHIFT)
                | ((TriState::Indeterminate as u32) << WRITABLE_SHIFT)
                | ((TriState::False as u32) << VALUE_SHIFT)
                | ((TriState::False as u32) << GET_SHIFT)
                | ((TriState::False as u32) << SET_SHIFT),
        }
    }
}

impl DefinePropertyAttributes {
    /// `explicit DefinePropertyAttributes(unsigned attributes)`.
    pub fn from_raw(attributes: u32) -> Self {
        DefinePropertyAttributes { attributes }
    }

    pub fn raw_representation(&self) -> u32 {
        self.attributes
    }

    pub fn has_value(&self) -> bool {
        self.attributes & (1 << VALUE_SHIFT) != 0
    }

    pub fn set_value(&mut self) {
        self.attributes |= 1 << VALUE_SHIFT;
    }

    pub fn has_get(&self) -> bool {
        self.attributes & (1 << GET_SHIFT) != 0
    }

    pub fn set_get(&mut self) {
        self.attributes |= 1 << GET_SHIFT;
    }

    pub fn has_set(&self) -> bool {
        self.attributes & (1 << SET_SHIFT) != 0
    }

    pub fn set_set(&mut self) {
        self.attributes |= 1 << SET_SHIFT;
    }

    pub fn has_writable(&self) -> bool {
        self.extract_tri_state(WRITABLE_SHIFT) != TriState::Indeterminate
    }

    pub fn writable(&self) -> Option<bool> {
        self.optional_flag(WRITABLE_SHIFT)
    }

    pub fn has_configurable(&self) -> bool {
        self.extract_tri_state(CONFIGURABLE_SHIFT) != TriState::Indeterminate
    }

    pub fn configurable(&self) -> Option<bool> {
        self.optional_flag(CONFIGURABLE_SHIFT)
    }

    pub fn has_enumerable(&self) -> bool {
        self.extract_tri_state(ENUMERABLE_SHIFT) != TriState::Indeterminate
    }

    pub fn enumerable(&self) -> Option<bool> {
        self.optional_flag(ENUMERABLE_SHIFT)
    }

    pub fn set_writable(&mut self, value: bool) {
        self.fill_with_tri_state(TriState::from_bool(value), WRITABLE_SHIFT);
    }

    pub fn set_configurable(&mut self, value: bool) {
        self.fill_with_tri_state(TriState::from_bool(value), CONFIGURABLE_SHIFT);
    }

    pub fn set_enumerable(&mut self, value: bool) {
        self.fill_with_tri_state(TriState::from_bool(value), ENUMERABLE_SHIFT);
    }

    /// Fatora o corpo comum de `writable()`, `configurable()` e `enumerable()`.
    fn optional_flag(&self, shift: u32) -> Option<bool> {
        match self.extract_tri_state(shift) {
            TriState::Indeterminate => None,
            state => Some(state == TriState::True),
        }
    }

    fn fill_with_tri_state(&mut self, state: TriState, shift: u32) {
        let mask = 0b11u32 << shift;
        self.attributes = (self.attributes & !mask) | ((state as u32) << shift);
    }

    fn extract_tri_state(&self, shift: u32) -> TriState {
        TriState::from_bits((self.attributes >> shift) & 0b11)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_setters() {
        let mut a = DefinePropertyAttributes::default();
        assert_eq!(a.raw_representation(), (2 << 0) | (2 << 2) | (2 << 4));
        assert_eq!(a.writable(), None);
        a.set_writable(true);
        a.set_configurable(false);
        assert_eq!(a.writable(), Some(true));
        assert_eq!(a.configurable(), Some(false));
        assert_eq!(a.enumerable(), None);
        a.set_value();
        assert!(a.has_value() && !a.has_get() && !a.has_set());
    }
}
