//! Porte de `src/vs/platform/keyboardLayout`: os tipos de layout e de mapa de teclas por sistema, o serviço
//! (`IKeyboardLayoutService`, `INativeKeyboardLayoutService`), o mapeador em cache (`CachedKeyboardMapper`), a
//! leitura da configuração `keyboard.*` e a detecção do layout do sistema (`detect`).
//!
//! Fora do porte (e por quê):
//! - `electron-main/keyboardLayoutMainService.ts`: usa o `native-keymap` (C++ do Electron), proibido pela regra de Rust
//!   puro. A detecção do layout vive em [`detect`], por arquivos de configuração e utilitários do sistema.
//! - O registro das configurações `keyboard.dispatch` e `keyboard.mapAltGrToCtrlAlt` (`keyboardConfig.ts`): é um nó do
//!   registro de configuração com `included` por sistema, que o registro deste porte ainda não modela por plataforma.
//! - O mapa de teclas (`getKeyMap()`, escancode para os caracteres com Shift e AltGr): vem de APIs do sistema
//!   (`xkbcommon`, `ToUnicodeEx`, `UCKeyTranslate`) que exigem `unsafe` ou C, ambos proibidos. O
//!   [`KeyboardLayoutData::keyboard_mapping`] fica `None` até a camada de janela (winit) fornecer o mapa.
//!
//! Representação: `IKeyboardLayoutInfo` (uma união por sistema, com `isUserKeyboardLayout` e `isUSStandard`) é
//! [`KeyboardLayoutInfo`]; `IKeyboardMapping` é [`KeyboardMapping`], com as chaves de escancode na ordem do original.

pub mod detect;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::event::Event;
use je_base::key_codes::{ScanCode, ScanCodeUtils};
use je_base::keybindings::{Keybinding, ResolvedKeybinding};
use je_base::promise::Promise;
use je_instantiation::service_id;

use crate::configuration::service::IConfigurationService;
use crate::keybinding::keybinding::KeyboardEvent;

/// `IWindowsKeyMapping`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowsKeyMapping {
    pub vkey: String,
    pub value: String,
    pub with_shift: String,
    pub with_alt_gr: String,
    pub with_shift_alt_gr: String,
}

/// `ILinuxKeyMapping`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinuxKeyMapping {
    pub value: String,
    pub with_shift: String,
    pub with_alt_gr: String,
    pub with_shift_alt_gr: String,
}

/// `IMacKeyMapping`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MacKeyMapping {
    pub value: String,
    pub value_is_dead_key: bool,
    pub with_shift: String,
    pub with_shift_is_dead_key: bool,
    pub with_alt_gr: String,
    pub with_alt_gr_is_dead_key: bool,
    pub with_shift_alt_gr: String,
    pub with_shift_alt_gr_is_dead_key: bool,
}

/// `IMacLinuxKeyMapping`: o que o mapeador lê de uma tecla, no Mac ou no Linux.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MacLinuxKeyMapping {
    pub value: String,
    pub with_shift: String,
    pub with_alt_gr: String,
    pub with_shift_alt_gr: String,
}

impl MacLinuxKeyMapping {
    pub fn new(value: &str, with_shift: &str, with_alt_gr: &str, with_shift_alt_gr: &str) -> Self {
        Self {
            value: value.to_string(),
            with_shift: with_shift.to_string(),
            with_alt_gr: with_alt_gr.to_string(),
            with_shift_alt_gr: with_shift_alt_gr.to_string(),
        }
    }
}

impl From<&LinuxKeyMapping> for MacLinuxKeyMapping {
    fn from(mapping: &LinuxKeyMapping) -> Self {
        Self {
            value: mapping.value.clone(),
            with_shift: mapping.with_shift.clone(),
            with_alt_gr: mapping.with_alt_gr.clone(),
            with_shift_alt_gr: mapping.with_shift_alt_gr.clone(),
        }
    }
}

impl From<&MacKeyMapping> for MacLinuxKeyMapping {
    fn from(mapping: &MacKeyMapping) -> Self {
        Self {
            value: mapping.value.clone(),
            with_shift: mapping.with_shift.clone(),
            with_alt_gr: mapping.with_alt_gr.clone(),
            with_shift_alt_gr: mapping.with_shift_alt_gr.clone(),
        }
    }
}

/// `IKeyboardMapping`: o mapa de escancode (`"KeyA"`) para a tecla, num dos três formatos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyboardMapping {
    Windows(IndexMap<String, WindowsKeyMapping>),
    Linux(IndexMap<String, LinuxKeyMapping>),
    Mac(IndexMap<String, MacKeyMapping>),
}

/// `IWindowsKeyboardLayoutInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsKeyboardLayoutInfo {
    pub name: String,
    pub id: String,
    pub text: String,
}

/// `ILinuxKeyboardLayoutInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxKeyboardLayoutInfo {
    pub model: String,
    pub group: i32,
    pub layout: String,
    pub variant: String,
    pub options: String,
    pub rules: String,
}

/// `IMacKeyboardLayoutInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacKeyboardLayoutInfo {
    pub id: String,
    pub lang: String,
    pub localized_name: Option<String>,
}

/// A parte específica de sistema de um [`KeyboardLayoutInfo`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyboardLayoutKind {
    Windows(WindowsKeyboardLayoutInfo),
    Linux(LinuxKeyboardLayoutInfo),
    Mac(MacKeyboardLayoutInfo),
}

/// `IKeyboardLayoutInfo`: o layout do sistema mais `isUserKeyboardLayout` e `isUSStandard`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardLayoutInfo {
    pub kind: KeyboardLayoutKind,
    pub is_user_keyboard_layout: Option<bool>,
    pub is_us_standard: Option<bool>,
}

impl KeyboardLayoutInfo {
    pub fn new(kind: KeyboardLayoutKind) -> Self {
        Self { kind, is_user_keyboard_layout: None, is_us_standard: None }
    }

    /// O `name` da forma do Windows (as três uniões de campos se sobrepõem no original: `name` só existe no Windows).
    fn name(&self) -> Option<&str> {
        match &self.kind {
            KeyboardLayoutKind::Windows(info) => Some(&info.name),
            _ => None,
        }
    }

    /// O `id`, que existe no Windows e no Mac.
    fn id(&self) -> Option<&str> {
        match &self.kind {
            KeyboardLayoutKind::Windows(info) => Some(&info.id),
            KeyboardLayoutKind::Mac(info) => Some(&info.id),
            KeyboardLayoutKind::Linux(_) => None,
        }
    }

    fn model(&self) -> Option<&str> {
        match &self.kind {
            KeyboardLayoutKind::Linux(info) => Some(&info.model),
            _ => None,
        }
    }

    fn layout(&self) -> Option<&str> {
        match &self.kind {
            KeyboardLayoutKind::Linux(info) => Some(&info.layout),
            _ => None,
        }
    }
}

/// `IKeyboardMapper`.
pub trait IKeyboardMapper {
    fn dump_debug_info(&self) -> String;
    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding>;
    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>>;
}

/// `CachedKeyboardMapper`: guarda o resultado de `resolveKeybinding` pelo `hashCode` do atalho.
pub struct CachedKeyboardMapper {
    actual: Rc<dyn IKeyboardMapper>,
    cache: RefCell<HashMap<String, Vec<Rc<dyn ResolvedKeybinding>>>>,
}

impl CachedKeyboardMapper {
    pub fn new(actual: Rc<dyn IKeyboardMapper>) -> Self {
        Self { actual, cache: RefCell::new(HashMap::new()) }
    }
}

impl IKeyboardMapper for CachedKeyboardMapper {
    fn dump_debug_info(&self) -> String {
        self.actual.dump_debug_info()
    }

    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        self.actual.resolve_keyboard_event(keyboard_event)
    }

    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        let hash_code = keybinding.get_hash_code();
        if let Some(resolved) = self.cache.borrow().get(&hash_code) {
            return resolved.clone();
        }
        let resolved = self.actual.resolve_keybinding(keybinding);
        self.cache.borrow_mut().insert(hash_code, resolved.clone());
        resolved
    }
}

/// `IKeyboardLayoutService`.
pub trait IKeyboardLayoutService {
    fn on_did_change_keyboard_layout(&self) -> Event<()>;
    fn get_raw_keyboard_mapping(&self) -> Option<KeyboardMapping>;
    fn get_current_keyboard_layout(&self) -> Option<KeyboardLayoutInfo>;
    fn get_all_keyboard_layouts(&self) -> Vec<KeyboardLayoutInfo>;
    fn get_keyboard_mapper(&self) -> Rc<dyn IKeyboardMapper>;
    fn validate_current_keyboard_mapping(&self, keyboard_event: &KeyboardEvent);
}

service_id!(pub I_KEYBOARD_LAYOUT_SERVICE: dyn IKeyboardLayoutService = "keyboardLayoutService");

/// `IKeyboardLayoutData`.
#[derive(Clone, Debug)]
pub struct KeyboardLayoutData {
    pub keyboard_layout_info: KeyboardLayoutInfo,
    /// `None` quando o sistema não entrega o mapa (ver o comentário do módulo).
    pub keyboard_mapping: Option<KeyboardMapping>,
}

/// `INativeKeyboardLayoutService`.
pub trait INativeKeyboardLayoutService {
    fn on_did_change_keyboard_layout(&self) -> Event<KeyboardLayoutData>;
    fn get_keyboard_layout_data(&self) -> Promise<KeyboardLayoutData>;
}

/// `DispatchConfig`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchConfig {
    Code,
    KeyCode,
}

/// `IKeyboardConfig`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardConfig {
    pub dispatch: DispatchConfig,
    pub map_alt_gr_to_ctrl_alt: bool,
}

/// `readKeyboardConfig`.
pub fn read_keyboard_config(configuration_service: &dyn IConfigurationService) -> KeyboardConfig {
    use je_base::dynamic::Truthy;
    let keyboard = configuration_service.get_value(Some("keyboard"), None);
    let dispatch = if keyboard.get(&"dispatch".into()) == je_base::dynamic::Dynamic::string("keyCode") { DispatchConfig::KeyCode } else { DispatchConfig::Code };
    let map_alt_gr_to_ctrl_alt = keyboard.get(&"mapAltGrToCtrlAlt".into()).is_truthy();
    KeyboardConfig { dispatch, map_alt_gr_to_ctrl_alt }
}

fn truthy(text: Option<&str>) -> bool {
    text.is_some_and(|t| !t.is_empty())
}

/// `areKeyboardLayoutsEqual`.
pub fn are_keyboard_layouts_equal(a: Option<&KeyboardLayoutInfo>, b: Option<&KeyboardLayoutInfo>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };

    if truthy(a.name()) && truthy(b.name()) && a.name() == b.name() {
        return true;
    }

    if truthy(a.id()) && truthy(b.id()) && a.id() == b.id() {
        return true;
    }

    truthy(a.model()) && truthy(b.model()) && a.model() == b.model() && a.layout() == b.layout()
}

/// O `{ label, description }` de `parseKeyboardLayoutDescription`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardLayoutDescription {
    pub label: String,
    pub description: String,
}

/// `parseKeyboardLayoutDescription`.
pub fn parse_keyboard_layout_description(layout: Option<&KeyboardLayoutInfo>) -> KeyboardLayoutDescription {
    let describe = |label: String, description: &str| KeyboardLayoutDescription { label, description: description.to_owned() };
    let Some(layout) = layout else { return describe(String::new(), "") };

    match &layout.kind {
        // windows
        KeyboardLayoutKind::Windows(windows) if !windows.name.is_empty() => describe(windows.text.clone(), ""),
        KeyboardLayoutKind::Windows(_) | KeyboardLayoutKind::Mac(_) if truthy(layout.id()) => {
            let KeyboardLayoutKind::Mac(mac) = &layout.kind else { return describe(String::new(), "") };
            if let Some(localized) = mac.localized_name.as_ref().filter(|n| !n.is_empty()) {
                return describe(localized.clone(), "");
            }
            if let Some(rest) = mac.id.strip_prefix("com.apple.keylayout.") {
                return describe(rest.replace('-', " "), "");
            }
            if let Some(position) = mac.id.find("inputmethod.") {
                let rest = &mac.id[position + "inputmethod.".len()..];
                return describe(rest.replace(['-', '.'], " "), &format!("Input Method ({})", mac.lang));
            }
            describe(mac.lang.clone(), "")
        }
        KeyboardLayoutKind::Linux(linux) => describe(linux.layout.clone(), ""),
        _ => describe(String::new(), ""),
    }
}

/// `getKeyboardLayoutId`.
pub fn get_keyboard_layout_id(layout: &KeyboardLayoutInfo) -> String {
    if let Some(name) = layout.name().filter(|n| !n.is_empty()) {
        return name.to_owned();
    }
    if let Some(id) = layout.id().filter(|i| !i.is_empty()) {
        return id.to_owned();
    }
    layout.layout().unwrap_or_default().to_owned()
}

fn mappings_equal<T: PartialEq>(a: &IndexMap<String, T>, b: &IndexMap<String, T>) -> bool {
    (0..ScanCode::MaxValue.0).all(|scan_code| {
        let key = ScanCodeUtils::to_string(ScanCode(scan_code));
        a.get(key) == b.get(key)
    })
}

/// `windowsKeyboardMappingEquals`.
pub fn windows_keyboard_mapping_equals(a: Option<&IndexMap<String, WindowsKeyMapping>>, b: Option<&IndexMap<String, WindowsKeyMapping>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => mappings_equal(a, b),
        _ => false,
    }
}

/// `macLinuxKeyboardMappingEquals`, para o mapa do Linux.
pub fn linux_keyboard_mapping_equals(a: Option<&IndexMap<String, LinuxKeyMapping>>, b: Option<&IndexMap<String, LinuxKeyMapping>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => mappings_equal(a, b),
        _ => false,
    }
}

/// `macLinuxKeyboardMappingEquals`, para o mapa do Mac.
pub fn mac_keyboard_mapping_equals(a: Option<&IndexMap<String, MacKeyMapping>>, b: Option<&IndexMap<String, MacKeyMapping>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => mappings_equal(a, b),
        _ => false,
    }
}
