//! Porte de `src/vs/platform/keybinding/common/baseResolvedKeybinding.ts`.
//!
//! A classe abstrata vira o trait [`ChordLabels`] (os métodos `_getLabel`, `_getAriaLabel`... que a subclasse
//! implementa) mais o struct [`BaseResolvedKeybinding`], que implementa o `ResolvedKeybinding` sobre ele.

use je_base::errors::{self, Error};
use je_base::keybinding_labels::ModifierLabelProvider;
use je_base::keybindings::{ResolvedChord, ResolvedKeybinding, SingleModifierChord, Modifiers};
use je_base::platform::OperatingSystem;

/// O que um tipo de acorde (`T extends Chord`) precisa dar ao `BaseResolvedKeybinding`.
pub trait BaseChord: Modifiers + Clone {
    fn is_duplicate_modifier_case(&self) -> bool;
}

/// Os métodos protegidos que cada subclasse de `BaseResolvedKeybinding` implementa.
pub trait ChordLabels<T: BaseChord> {
    fn get_label(&self, os: OperatingSystem, chord: &T) -> Option<String>;
    fn get_aria_label(&self, os: OperatingSystem, chord: &T) -> Option<String>;
    fn get_electron_accelerator(&self, os: OperatingSystem, chord: &T) -> Option<String>;
    fn get_user_settings_label(&self, os: OperatingSystem, chord: &T) -> Option<String>;
    fn is_wysiwyg(&self, chord: &T) -> bool;
    fn get_chord_dispatch(&self, chord: &T) -> Option<String>;
    fn get_single_modifier_chord_dispatch(&self, chord: &T) -> Option<SingleModifierChord>;
}

/// `BaseResolvedKeybinding<T>`.
pub struct BaseResolvedKeybinding<T: BaseChord, L: ChordLabels<T>> {
    os: OperatingSystem,
    chords: Vec<T>,
    labels: L,
}

impl<T: BaseChord, L: ChordLabels<T>> BaseResolvedKeybinding<T, L> {
    /// Lança `illegalArgument('chords')` sem acordes.
    pub fn new(os: OperatingSystem, chords: Vec<T>, labels: L) -> Self {
        if chords.is_empty() {
            errors::throw(Error::new("Illegal argument: chords"));
        }
        Self { os, chords, labels }
    }

    fn get_chord(&self, keybinding: &T) -> ResolvedChord {
        ResolvedChord {
            ctrl_key: keybinding.ctrl_key(),
            shift_key: keybinding.shift_key(),
            alt_key: keybinding.alt_key(),
            meta_key: keybinding.meta_key(),
            key_label: self.labels.get_label(self.os, keybinding),
            key_aria_label: self.labels.get_aria_label(self.os, keybinding),
        }
    }
}

impl<T: BaseChord, L: ChordLabels<T>> ResolvedKeybinding for BaseResolvedKeybinding<T, L> {
    fn get_label(&self) -> Option<String> {
        ModifierLabelProvider::ui().to_label(self.os, &self.chords, |keybinding| self.labels.get_label(self.os, keybinding))
    }

    fn get_aria_label(&self) -> Option<String> {
        ModifierLabelProvider::aria().to_label(self.os, &self.chords, |keybinding| self.labels.get_aria_label(self.os, keybinding))
    }

    fn get_electron_accelerator(&self) -> Option<String> {
        if self.chords.len() > 1 {
            // [Electron Accelerators] Electron cannot handle chords
            return None;
        }
        if self.chords[0].is_duplicate_modifier_case() {
            // [Electron Accelerators] Electron cannot handle modifier only keybindings, e.g. "shift shift"
            return None;
        }
        ModifierLabelProvider::electron_accelerator()
            .to_label(self.os, &self.chords, |keybinding| self.labels.get_electron_accelerator(self.os, keybinding))
    }

    fn get_user_settings_label(&self) -> Option<String> {
        ModifierLabelProvider::user_settings()
            .to_label(self.os, &self.chords, |keybinding| self.labels.get_user_settings_label(self.os, keybinding))
    }

    fn is_wysiwyg(&self) -> bool {
        self.chords.iter().all(|keybinding| self.labels.is_wysiwyg(keybinding))
    }

    fn has_multiple_chords(&self) -> bool {
        self.chords.len() > 1
    }

    fn get_chords(&self) -> Vec<ResolvedChord> {
        self.chords.iter().map(|keybinding| self.get_chord(keybinding)).collect()
    }

    fn get_dispatch_chords(&self) -> Vec<Option<String>> {
        self.chords.iter().map(|keybinding| self.labels.get_chord_dispatch(keybinding)).collect()
    }

    fn get_single_modifier_dispatch_chords(&self) -> Vec<Option<SingleModifierChord>> {
        self.chords.iter().map(|keybinding| self.labels.get_single_modifier_chord_dispatch(keybinding)).collect()
    }
}
