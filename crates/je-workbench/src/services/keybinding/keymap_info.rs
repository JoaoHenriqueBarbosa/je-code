//! Porte de `src/vs/workbench/services/keybinding/common/keymapInfo.ts`: o layout de teclado com o mapa de teclas
//! (`KeymapInfo`), a forma serializada que os arquivos de layout registram (`IKeymapInfo`) e o mapa já decodificado
//! (`IRawMixedKeyboardMapping`).
//!
//! Notas de tradução:
//! - **`ISerializedMapping`** (`{ [key: string]: (string | number)[] }`) é [`SerializedMapping`], um `IndexMap` de
//!   escancode para [`SerializedKey`]. A lista vazia do original (`Enter: []`, a tecla que não produz texto) é `None`; a
//!   lista de cinco ou seis itens é `Some`, com o item 4 (a máscara de teclas mortas, que o original passa por
//!   `Number(...)`) em `mask` e o item 5 (opcional) em `vkey`.
//! - **`IRawMixedKeyboardMapping`** é [`RawMixedKeyboardMapping`], um `IndexMap` de escancode para [`RawMixedKeyMapping`].
//!   Os quatro `...IsDeadKey?` opcionais são `bool` (a ausência vale falso, como o `undefined` nas leituras do original), e
//!   o `vkey`, que o `deserializeMapping` grava mas a interface não declara, é `Option<String>`.
//! - **`isWindows` e `isLinux`** (constantes de `platform.ts`) são lidos do [`OperatingSystem`] que o [`KeymapInfo`] guarda
//!   (`os`), passado na construção: o porte não tem constantes globais de sistema.
//! - **Identidade e mutação.** O `KeymapInfo` é mutado (`update`) e comparado por identidade (`indexOf`) pelo serviço, então o
//!   handle compartilhado é [`KeymapInfoRef`] (`Rc<RefCell<KeymapInfo>>`). O construtor do original grava
//!   `layout.isUserKeyboardLayout` no objeto recebido, que o `IKeymapInfo` dos arquivos de layout compartilha; aqui o layout
//!   é copiado para o `KeymapInfo`, e a cópia é a que recebe o campo.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::platform::OperatingSystem;
use je_platform::keyboard_layout::{get_keyboard_layout_id, KeyboardLayoutInfo, KeyboardMapping, LinuxKeyMapping, MacKeyMapping, WindowsKeyMapping};

/// Uma tecla do `ISerializedMapping`: `[value, withShift, withAltGr, withShiftAltGr, mask, vkey?]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedKey {
    pub value: String,
    pub with_shift: String,
    pub with_alt_gr: String,
    pub with_shift_alt_gr: String,
    /// Os bits 1, 2, 4 e 8 marcam que `value`, `withShift`, `withAltGr` e `withShiftAltGr` são teclas mortas.
    pub mask: u32,
    pub vkey: Option<String>,
}

impl SerializedKey {
    pub fn new(value: &str, with_shift: &str, with_alt_gr: &str, with_shift_alt_gr: &str, mask: u32) -> Self {
        Self {
            value: value.to_string(),
            with_shift: with_shift.to_string(),
            with_alt_gr: with_alt_gr.to_string(),
            with_shift_alt_gr: with_shift_alt_gr.to_string(),
            mask,
            vkey: None,
        }
    }
}

/// `ISerializedMapping`: `None` é a lista vazia (`[]`).
pub type SerializedMapping = IndexMap<String, Option<SerializedKey>>;

/// Uma tecla do `IRawMixedKeyboardMapping`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawMixedKeyMapping {
    pub value: String,
    pub vkey: Option<String>,
    pub with_shift: String,
    pub with_alt_gr: String,
    pub with_shift_alt_gr: String,
    pub value_is_dead_key: bool,
    pub with_shift_is_dead_key: bool,
    pub with_alt_gr_is_dead_key: bool,
    pub with_shift_alt_gr_is_dead_key: bool,
}

impl RawMixedKeyMapping {
    /// A tecla sem as marcas de tecla morta (a que o `getLayoutMap` do navegador monta).
    pub fn simple(value: &str, with_shift: &str, with_alt_gr: &str, with_shift_alt_gr: &str) -> Self {
        Self {
            value: value.to_string(),
            with_shift: with_shift.to_string(),
            with_alt_gr: with_alt_gr.to_string(),
            with_shift_alt_gr: with_shift_alt_gr.to_string(),
            ..Self::default()
        }
    }
}

/// `IRawMixedKeyboardMapping`.
pub type RawMixedKeyboardMapping = IndexMap<String, RawMixedKeyMapping>;

/// `deserializeMapping`.
fn deserialize_mapping(serialized_mapping: &SerializedMapping) -> RawMixedKeyboardMapping {
    let mut ret = RawMixedKeyboardMapping::new();
    for (key, result) in serialized_mapping {
        match result {
            Some(result) => {
                let mask = result.mask;
                ret.insert(
                    key.clone(),
                    RawMixedKeyMapping {
                        value: result.value.clone(),
                        vkey: result.vkey.clone(),
                        with_shift: result.with_shift.clone(),
                        with_alt_gr: result.with_alt_gr.clone(),
                        with_shift_alt_gr: result.with_shift_alt_gr.clone(),
                        value_is_dead_key: (mask & 1) > 0,
                        with_shift_is_dead_key: (mask & 2) > 0,
                        with_alt_gr_is_dead_key: (mask & 4) > 0,
                        with_shift_alt_gr_is_dead_key: (mask & 8) > 0,
                    },
                );
            }
            None => {
                ret.insert(key.clone(), RawMixedKeyMapping::default());
            }
        }
    }
    ret
}

/// `IKeymapInfo`.
#[derive(Clone, Debug)]
pub struct KeymapInfoSource {
    pub layout: KeyboardLayoutInfo,
    pub secondary_layouts: Vec<KeyboardLayoutInfo>,
    pub mapping: SerializedMapping,
    pub is_user_keyboard_layout: Option<bool>,
}

/// O handle compartilhado de um [`KeymapInfo`] (ver as notas do módulo).
pub type KeymapInfoRef = Rc<RefCell<KeymapInfo>>;

pub struct KeymapInfo {
    pub layout: KeyboardLayoutInfo,
    pub secondary_layouts: Vec<KeyboardLayoutInfo>,
    pub mapping: RawMixedKeyboardMapping,
    pub is_user_keyboard_layout: bool,
    os: OperatingSystem,
}

impl KeymapInfo {
    pub fn new(
        layout: KeyboardLayoutInfo,
        secondary_layouts: Vec<KeyboardLayoutInfo>,
        keyboard_mapping: &SerializedMapping,
        is_user_keyboard_layout: Option<bool>,
        os: OperatingSystem,
    ) -> Self {
        let is_user = is_user_keyboard_layout.unwrap_or(false);
        let mut layout = layout;
        layout.is_user_keyboard_layout = Some(is_user);
        Self { layout, secondary_layouts, mapping: deserialize_mapping(keyboard_mapping), is_user_keyboard_layout: is_user, os }
    }

    /// `new KeymapInfo(info.layout, info.secondaryLayouts, info.mapping, info.isUserKeyboardLayout)`, já no handle compartilhado.
    pub fn from_source(source: &KeymapInfoSource, os: OperatingSystem) -> KeymapInfoRef {
        Rc::new(RefCell::new(Self::new(
            source.layout.clone(),
            source.secondary_layouts.clone(),
            &source.mapping,
            source.is_user_keyboard_layout,
            os,
        )))
    }

    pub fn create_keyboard_layout_from_debug_info(layout: KeyboardLayoutInfo, value: RawMixedKeyboardMapping, os: OperatingSystem) -> KeymapInfo {
        let mut keyboard_layout_info = KeymapInfo::new(layout, Vec::new(), &SerializedMapping::new(), Some(true), os);
        keyboard_layout_info.mapping = value;
        keyboard_layout_info
    }

    pub fn update(&mut self, other: &KeymapInfo) {
        self.layout = other.layout.clone();
        self.secondary_layouts = other.secondary_layouts.clone();
        self.mapping = other.mapping.clone();
        self.is_user_keyboard_layout = other.is_user_keyboard_layout;
        self.layout.is_user_keyboard_layout = Some(other.is_user_keyboard_layout);
    }

    pub fn get_score(&self, other: &RawMixedKeyboardMapping) -> i32 {
        let mut score = 0;
        for (key, other_mapping) in other {
            if self.os == OperatingSystem::Windows && (key == "Backslash" || key == "KeyQ") {
                // keymap from Chromium is probably wrong.
                continue;
            }

            if self.os == OperatingSystem::Linux && (key == "Backspace" || key == "Escape") {
                // native keymap doesn't align with keyboard event
                continue;
            }

            let current_mapping = self.mapping.get(key);

            if current_mapping.is_none() {
                score -= 1;
            }

            if let Some(current_mapping) = current_mapping {
                if current_mapping.value != other_mapping.value {
                    score -= 1;
                }
            }
        }

        score
    }

    pub fn equal(&self, other: &KeymapInfo) -> bool {
        if self.is_user_keyboard_layout != other.is_user_keyboard_layout {
            return false;
        }

        if get_keyboard_layout_id(&self.layout) != get_keyboard_layout_id(&other.layout) {
            return false;
        }

        self.fuzzy_equal(&other.mapping)
    }

    pub fn fuzzy_equal(&self, other: &RawMixedKeyboardMapping) -> bool {
        for (key, other_mapping) in other {
            if self.os == OperatingSystem::Windows && (key == "Backslash" || key == "KeyQ") {
                // keymap from Chromium is probably wrong.
                continue;
            }
            let Some(current_mapping) = self.mapping.get(key) else {
                return false;
            };

            if current_mapping.value != other_mapping.value {
                return false;
            }
        }

        true
    }

    /// O `IKeyboardMapping` que o original devolve por `activeKeyMapping` (o mapa misto atribuído à união), no formato
    /// do sistema do `KeymapInfo`.
    pub fn to_keyboard_mapping(&self) -> KeyboardMapping {
        match self.os {
            OperatingSystem::Windows => KeyboardMapping::Windows(
                self.mapping
                    .iter()
                    .map(|(key, mapping)| {
                        (
                            key.clone(),
                            WindowsKeyMapping {
                                vkey: mapping.vkey.clone().unwrap_or_default(),
                                value: mapping.value.clone(),
                                with_shift: mapping.with_shift.clone(),
                                with_alt_gr: mapping.with_alt_gr.clone(),
                                with_shift_alt_gr: mapping.with_shift_alt_gr.clone(),
                            },
                        )
                    })
                    .collect(),
            ),
            OperatingSystem::Linux => KeyboardMapping::Linux(
                self.mapping
                    .iter()
                    .map(|(key, mapping)| {
                        (
                            key.clone(),
                            LinuxKeyMapping {
                                value: mapping.value.clone(),
                                with_shift: mapping.with_shift.clone(),
                                with_alt_gr: mapping.with_alt_gr.clone(),
                                with_shift_alt_gr: mapping.with_shift_alt_gr.clone(),
                            },
                        )
                    })
                    .collect(),
            ),
            OperatingSystem::Macintosh => KeyboardMapping::Mac(
                self.mapping
                    .iter()
                    .map(|(key, mapping)| {
                        (
                            key.clone(),
                            MacKeyMapping {
                                value: mapping.value.clone(),
                                value_is_dead_key: mapping.value_is_dead_key,
                                with_shift: mapping.with_shift.clone(),
                                with_shift_is_dead_key: mapping.with_shift_is_dead_key,
                                with_alt_gr: mapping.with_alt_gr.clone(),
                                with_alt_gr_is_dead_key: mapping.with_alt_gr_is_dead_key,
                                with_shift_alt_gr: mapping.with_shift_alt_gr.clone(),
                                with_shift_alt_gr_is_dead_key: mapping.with_shift_alt_gr_is_dead_key,
                            },
                        )
                    })
                    .collect(),
            ),
        }
    }
}
