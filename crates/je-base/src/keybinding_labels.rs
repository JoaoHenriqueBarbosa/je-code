//! Porte de `src/vs/base/common/keybindingLabels.ts`: o texto de um atalho para a interface, o ARIA, os
//! aceleradores do Electron e as configurações do usuário, por sistema operacional.
//!
//! O original cria os quatro provedores como constantes do módulo, com os textos já localizados na
//! carga. Aqui cada um sai de um construtor ([`ModifierLabelProvider::ui`], [`ModifierLabelProvider::aria`],
//! [`ModifierLabelProvider::electron_accelerator`], [`ModifierLabelProvider::user_settings`]), que
//! consulta o [`crate::nls`] na hora, de modo que uma troca de language pack vale para o provedor seguinte.

use crate::keybindings::Modifiers;
use crate::nls::localize;
use crate::platform::OperatingSystem;

/// `ModifierLabels`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModifierLabels {
    pub ctrl_key: String,
    pub shift_key: String,
    pub alt_key: String,
    pub meta_key: String,
    pub separator: String,
}

impl ModifierLabels {
    fn new(ctrl_key: &str, shift_key: &str, alt_key: &str, meta_key: &str, separator: &str) -> Self {
        Self {
            ctrl_key: ctrl_key.to_string(),
            shift_key: shift_key.to_string(),
            alt_key: alt_key.to_string(),
            meta_key: meta_key.to_string(),
            separator: separator.to_string(),
        }
    }
}

pub struct ModifierLabelProvider {
    mac: ModifierLabels,
    windows: ModifierLabels,
    linux: ModifierLabels,
}

impl ModifierLabelProvider {
    /// `new ModifierLabelProvider(mac, windows, linux = windows)`.
    pub fn new(mac: ModifierLabels, windows: ModifierLabels, linux: Option<ModifierLabels>) -> Self {
        let linux = linux.unwrap_or_else(|| windows.clone());
        Self { mac, windows, linux }
    }

    /// `modifierLabels[OS]`.
    pub fn modifier_labels(&self, os: OperatingSystem) -> &ModifierLabels {
        match os {
            OperatingSystem::Macintosh => &self.mac,
            OperatingSystem::Windows => &self.windows,
            OperatingSystem::Linux => &self.linux,
        }
    }

    /// `toLabel`: `None` quando não há acordes ou quando um deles não tem rótulo de tecla.
    pub fn to_label<T: Modifiers>(&self, os: OperatingSystem, chords: &[T], key_label_provider: impl Fn(&T) -> Option<String>) -> Option<String> {
        if chords.is_empty() {
            return None;
        }

        let mut result = Vec::with_capacity(chords.len());
        for chord in chords {
            // this keybinding cannot be expressed...
            let key_label = key_label_provider(chord)?;
            result.push(simple_as_string(chord, &key_label, self.modifier_labels(os)));
        }
        Some(result.join(" "))
    }

    /// `UILabelProvider`: modificadores num formato próprio para exibir na interface.
    pub fn ui() -> Self {
        let ctrl = || localize("ctrlKey", "Ctrl", &[]);
        let shift = || localize("shiftKey", "Shift", &[]);
        let alt = || localize("altKey", "Alt", &[]);
        Self::new(
            ModifierLabels::new("\u{2303}", "⇧", "⌥", "⌘", ""),
            ModifierLabels::new(&ctrl(), &shift(), &alt(), &localize("windowsKey", "Windows", &[]), "+"),
            Some(ModifierLabels::new(&ctrl(), &shift(), &alt(), &localize("superKey", "Super", &[]), "+")),
        )
    }

    /// `AriaLabelProvider`: modificadores num formato próprio para o ARIA.
    pub fn aria() -> Self {
        let ctrl = || localize("ctrlKey.long", "Control", &[]);
        let shift = || localize("shiftKey.long", "Shift", &[]);
        let alt = || localize("altKey.long", "Alt", &[]);
        Self::new(
            ModifierLabels::new(&ctrl(), &shift(), &localize("optKey.long", "Option", &[]), &localize("cmdKey.long", "Command", &[]), "+"),
            ModifierLabels::new(&ctrl(), &shift(), &alt(), &localize("windowsKey.long", "Windows", &[]), "+"),
            Some(ModifierLabels::new(&ctrl(), &shift(), &alt(), &localize("superKey.long", "Super", &[]), "+")),
        )
    }

    /// `ElectronAcceleratorLabelProvider`: modificadores no formato dos aceleradores do Electron.
    /// See <https://github.com/electron/electron/blob/master/docs/api/accelerator.md>
    pub fn electron_accelerator() -> Self {
        Self::new(ModifierLabels::new("Ctrl", "Shift", "Alt", "Cmd", "+"), ModifierLabels::new("Ctrl", "Shift", "Alt", "Super", "+"), None)
    }

    /// `UserSettingsLabelProvider`: modificadores no formato das configurações do usuário.
    pub fn user_settings() -> Self {
        Self::new(
            ModifierLabels::new("ctrl", "shift", "alt", "cmd", "+"),
            ModifierLabels::new("ctrl", "shift", "alt", "win", "+"),
            Some(ModifierLabels::new("ctrl", "shift", "alt", "meta", "+")),
        )
    }
}

fn simple_as_string(modifiers: &impl Modifiers, key: &str, labels: &ModifierLabels) -> String {
    let mut result: Vec<&str> = Vec::new();

    // translate modifier keys: Ctrl-Shift-Alt-Meta
    if modifiers.ctrl_key() {
        result.push(&labels.ctrl_key);
    }

    if modifiers.shift_key() {
        result.push(&labels.shift_key);
    }

    if modifiers.alt_key() {
        result.push(&labels.alt_key);
    }

    if modifiers.meta_key() {
        result.push(&labels.meta_key);
    }

    // the actual key
    if !key.is_empty() {
        result.push(key);
    }

    result.join(&labels.separator)
}
