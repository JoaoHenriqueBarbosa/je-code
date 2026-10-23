//! Um `ResolvedKeybinding` de teste e de galeria: os acordes dados, com os rótulos de UI e ARIA do
//! sistema operacional (o `USLayoutResolvedKeybinding` do VS Code é da camada de plataforma).
#![allow(dead_code)]

use je_base::keybinding_labels::ModifierLabelProvider;
use je_base::keybindings::{ResolvedChord, ResolvedKeybinding, SingleModifierChord};
use je_base::platform::OperatingSystem;

/// Um acorde: `(ctrl, shift, alt, meta, tecla)`.
pub type ChordSpec = (bool, bool, bool, bool, &'static str);

pub struct FixtureKeybinding {
    chords: Vec<ResolvedChord>,
    aria_label: String,
}

impl FixtureKeybinding {
    pub fn new(os: OperatingSystem, chords: &[ChordSpec]) -> Self {
        let chords: Vec<ResolvedChord> = chords
            .iter()
            .map(|&(ctrl_key, shift_key, alt_key, meta_key, key)| ResolvedChord { ctrl_key, shift_key, alt_key, meta_key, key_label: Some(key.to_owned()), key_aria_label: Some(key.to_owned()) })
            .collect();
        let aria = ModifierLabelProvider::aria();
        let labels = aria.modifier_labels(os);
        let aria_label = chords
            .iter()
            .map(|c| {
                let modifiers = [(c.ctrl_key, &labels.ctrl_key), (c.shift_key, &labels.shift_key), (c.alt_key, &labels.alt_key), (c.meta_key, &labels.meta_key)];
                let mut parts: Vec<&str> = modifiers.iter().filter(|(on, _)| *on).map(|(_, label)| label.as_str()).collect();
                parts.extend(c.key_aria_label.as_deref());
                parts.join(&labels.separator)
            })
            .collect::<Vec<_>>()
            .join(" ");
        Self { chords, aria_label }
    }
}

impl ResolvedKeybinding for FixtureKeybinding {
    fn get_label(&self) -> Option<String> {
        None
    }

    fn get_aria_label(&self) -> Option<String> {
        Some(self.aria_label.clone())
    }

    fn get_electron_accelerator(&self) -> Option<String> {
        None
    }

    fn get_user_settings_label(&self) -> Option<String> {
        None
    }

    fn is_wysiwyg(&self) -> bool {
        true
    }

    fn has_multiple_chords(&self) -> bool {
        self.chords.len() > 1
    }

    fn get_chords(&self) -> Vec<ResolvedChord> {
        self.chords.clone()
    }

    fn get_dispatch_chords(&self) -> Vec<Option<String>> {
        Vec::new()
    }

    fn get_single_modifier_dispatch_chords(&self) -> Vec<Option<SingleModifierChord>> {
        Vec::new()
    }
}
