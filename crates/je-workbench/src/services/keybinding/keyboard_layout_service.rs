//! Porte de `src/vs/workbench/services/keybinding/browser/keyboardLayoutService.ts`, a parte do mapeador de teclado:
//! `BrowserKeyboardMapperFactoryBase` e `BrowserKeyboardMapperFactory`.
//!
//! Notas de tradução:
//! - **`navigator.keyboard`** (`INavigatorWithKeyboard`) é o trait [`NavigatorKeyboard`], opcional no construtor (`None` é o
//!   `navigator.keyboard === undefined`, que desliga o `getLayoutMap`). O `getLayoutMap()` devolve os pares `[código, tecla]`
//!   do `Map` e rejeita quando o original lançaria (contexto de navegação aninhado). O `addEventListener('layoutchange')` é
//!   `add_layout_change_listener`, que devolve o disposable do ouvinte (o original o deixa vivo para sempre, aqui ele sai com a
//!   fábrica) ou `None` quando o navegador não tem `addEventListener`.
//! - **`StandardKeyboardEvent`** é [`StandardKeyboardEvent`]: o [`KeyboardEvent`] da plataforma (`IKeyboardEvent`) mais o que
//!   o original lê de `browserEvent` (`key`, `code`, `isComposing`).
//! - **`OS`, `isWindows` e `isLinux`** (constantes globais) são o [`OperatingSystem`] do construtor.
//! - **Membros `protected`** (`_initialized`, `_keymapInfos`, `_mru`, o construtor `protected`): Rust não tem herança, então a
//!   subclasse do original (`BrowserKeyboardMapperFactory` e o `TestKeyboardMapperFactory` do teste) usa os métodos públicos
//!   [`BrowserKeyboardMapperFactoryBase::push_keymap_infos`], [`BrowserKeyboardMapperFactoryBase::alias_mru_to_keymap_infos`]
//!   e [`BrowserKeyboardMapperFactoryBase::set_initialized`]. `_mru = _keymapInfos` faz as duas variáveis apontarem para o
//!   mesmo vetor (o `splice` e o `unshift` do `_mru` mudam também o `_keymapInfos`); por isso as duas listas são
//!   `Rc<RefCell<Vec<_>>>` e o alias é copiar o `Rc`. O `splice(index, 1)` com `index === -1` remove o último item, e o
//!   porte mantém esse comportamento.
//! - **`BrowserKeyboardMapperFactory`**: o `import()` dinâmico do arquivo `layout.contribution.<sistema>.js` é a promessa
//!   `layout_infos` recebida por [`create_browser_keyboard_mapper_factory`]; os parâmetros `notificationService`,
//!   `storageService` e `commandService`, que o original recebe e não usa (o `super` deles está comentado), não existem.
//! - **Não portados ainda**: `UserKeyboardLayout`, `BrowserKeyboardLayoutService`, o `registerSingleton` e o registro da
//!   configuração `keyboard.layout`. Eles dependem de `IEnvironmentService.keyboardLayoutResource`, do `IFileService` e do
//!   nó de configuração por registro, e nenhum teste do mapeador passa por eles.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::async_fn;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::configuration::service::IConfigurationService;
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_platform::keyboard_layout::{
    get_keyboard_layout_id, read_keyboard_config, CachedKeyboardMapper, DispatchConfig, IKeyboardMapper, KeyboardLayoutInfo, KeyboardMapping, MacLinuxKeyMapping,
};

use super::fallback_keyboard_mapper::FallbackKeyboardMapper;
use super::keymap_info::{KeymapInfo, KeymapInfoRef, KeymapInfoSource, RawMixedKeyMapping, RawMixedKeyboardMapping};
use super::mac_linux_keyboard_mapper::MacLinuxKeyboardMapper;
use super::windows_keyboard_mapper::WindowsKeyboardMapper;

/// `INavigatorWithKeyboard['keyboard']`.
pub trait NavigatorKeyboard {
    /// `getLayoutMap()`, com as entradas do `Map` como pares `[código, tecla]`. Rejeita onde o original lançaria.
    fn get_layout_map(&self) -> Promise<Vec<(String, String)>>;

    /// `addEventListener('layoutchange', listener)`; `None` quando o navegador não tem `addEventListener`.
    fn add_layout_change_listener(&self, listener: Rc<dyn Fn()>) -> Option<DisposableRef>;
}

/// `StandardKeyboardEvent` com o `browserEvent` que o serviço lê.
pub struct StandardKeyboardEvent {
    pub event: KeyboardEvent,
    /// `browserEvent.key`.
    pub key: String,
    /// `code` (igual a `browserEvent.code`).
    pub code: String,
    /// `browserEvent.isComposing`.
    pub is_composing: bool,
}

/// O `{ result, score }` de `getMatchedKeymapInfo`.
pub struct MatchedKeymapInfo {
    pub result: KeymapInfoRef,
    pub score: i32,
}

/// A lista que `_keymapInfos` e `_mru` apontam (as duas podem ser o mesmo vetor, ver as notas do módulo).
type SharedKeymapList = Rc<RefCell<Vec<KeymapInfoRef>>>;

fn index_of(list: &[KeymapInfoRef], item: &KeymapInfoRef) -> isize {
    list.iter().position(|candidate| Rc::ptr_eq(candidate, item)).map_or(-1, |index| index as isize)
}

/// `list.splice(index, 1)`: um índice negativo conta a partir do fim.
fn splice_one(list: &mut Vec<KeymapInfoRef>, index: isize) {
    let len = list.len() as isize;
    let start = if index < 0 { (len + index).max(0) } else { index.min(len) };
    if (start as usize) < list.len() {
        list.remove(start as usize);
    }
}

/// `BrowserKeyboardMapperFactoryBase`.
pub struct BrowserKeyboardMapperFactoryBase {
    base: Rc<Disposable>,
    configuration_service: Rc<dyn IConfigurationService>,
    os: OperatingSystem,
    navigator_keyboard: Option<Rc<dyn NavigatorKeyboard>>,
    // keyboard mapper
    initialized: Cell<bool>,
    keyboard_mapper: RefCell<Option<Rc<dyn IKeyboardMapper>>>,
    on_did_change_keyboard_mapper: Rc<Emitter<()>>,
    // keymap infos
    keymap_infos: RefCell<SharedKeymapList>,
    mru: RefCell<SharedKeymapList>,
    active_keymap_info: RefCell<Option<KeymapInfoRef>>,
    keyboard_layout_map_allowed: Cell<bool>,
    this: Weak<Self>,
}

impl BrowserKeyboardMapperFactoryBase {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>, os: OperatingSystem, navigator_keyboard: Option<Rc<dyn NavigatorKeyboard>>) -> Rc<Self> {
        let this = Rc::new_cyclic(|weak: &Weak<Self>| {
            let base = Disposable::new();
            let on_did_change_keyboard_mapper = Emitter::<()>::new();
            base.register(on_did_change_keyboard_mapper.clone());
            Self {
                base,
                configuration_service,
                os,
                keyboard_layout_map_allowed: Cell::new(navigator_keyboard.is_some()),
                navigator_keyboard,
                initialized: Cell::new(false),
                keyboard_mapper: RefCell::new(None),
                on_did_change_keyboard_mapper,
                keymap_infos: RefCell::new(Rc::default()),
                mru: RefCell::new(Rc::default()),
                active_keymap_info: RefCell::new(None),
                this: weak.clone(),
            }
        });

        if let Some(navigator_keyboard) = &this.navigator_keyboard {
            let weak = this.this.clone();
            let listener = navigator_keyboard.add_layout_change_listener(Rc::new(move || {
                let Some(factory) = weak.upgrade() else { return };
                // Update user keyboard map settings
                let target = factory.clone();
                factory.get_browser_key_mapping(None).then(move |mapping| {
                    if target.is_key_mapping_active(mapping.as_ref()) {
                        return;
                    }

                    target.set_layout_from_browser_api();
                });
            }));
            if let Some(listener) = listener {
                this.base.register(listener);
            }
        }

        let weak = this.this.clone();
        let subscription = this.configuration_service.on_did_change_configuration().listen(move |e| {
            if e.affects_configuration("keyboard", None) {
                if let Some(factory) = weak.upgrade() {
                    *factory.keyboard_mapper.borrow_mut() = None;
                    factory.on_did_change_keyboard_mapper.fire(());
                }
            }
        });
        this.base.register(subscription);

        this
    }

    /// `onDidChangeKeyboardMapper`.
    pub fn on_did_change_keyboard_mapper(&self) -> Event<()> {
        self.on_did_change_keyboard_mapper.event()
    }

    fn this(&self) -> Rc<Self> {
        self.this.upgrade().expect("a fábrica está viva enquanto um método dela roda")
    }

    //#region membros protected

    /// `this._keymapInfos.push(...infos)`.
    pub fn push_keymap_infos(&self, infos: Vec<KeymapInfoRef>) {
        self.keymap_infos.borrow().borrow_mut().extend(infos);
    }

    /// `this._mru = this._keymapInfos`: as duas passam a ser o mesmo vetor.
    pub fn alias_mru_to_keymap_infos(&self) {
        *self.mru.borrow_mut() = self.keymap_infos.borrow().clone();
    }

    /// `this._initialized = value`.
    pub fn set_initialized(&self, value: bool) {
        self.initialized.set(value);
    }

    //#endregion

    fn mru_snapshot(&self) -> Vec<KeymapInfoRef> {
        self.mru.borrow().borrow().clone()
    }

    pub fn active_keymap(&self) -> Option<KeymapInfoRef> {
        self.active_keymap_info.borrow().clone()
    }

    pub fn keymap_infos(&self) -> Vec<KeymapInfoRef> {
        self.keymap_infos.borrow().borrow().clone()
    }

    pub fn active_keyboard_layout(&self) -> Option<KeyboardLayoutInfo> {
        if !self.initialized.get() {
            return None;
        }

        self.active_keymap().map(|keymap_info| keymap_info.borrow().layout.clone())
    }

    pub fn active_key_mapping(&self) -> Option<KeyboardMapping> {
        if !self.initialized.get() {
            return None;
        }

        self.active_keymap().map(|keymap_info| keymap_info.borrow().to_keyboard_mapping())
    }

    pub fn keyboard_layouts(&self) -> Vec<KeyboardLayoutInfo> {
        self.keymap_infos().iter().map(|keymap_info| keymap_info.borrow().layout.clone()).collect()
    }

    pub fn register_keyboard_layout(&self, layout: KeymapInfoRef) {
        self.keymap_infos.borrow().borrow_mut().push(layout);
        self.alias_mru_to_keymap_infos();
    }

    pub fn remove_keyboard_layout(&self, layout: &KeymapInfoRef) {
        let mru = self.mru.borrow().clone();
        let index = index_of(&mru.borrow(), layout);
        if index != -1 {
            splice_one(&mut mru.borrow_mut(), index);
        }
        let keymap_infos = self.keymap_infos.borrow().clone();
        let index = index_of(&keymap_infos.borrow(), layout);
        if index != -1 {
            splice_one(&mut keymap_infos.borrow_mut(), index);
        }
    }

    pub fn get_matched_keymap_info(&self, key_mapping: Option<&RawMixedKeyboardMapping>) -> Option<MatchedKeymapInfo> {
        let key_mapping = key_mapping?;

        let us_standard = self.get_us_standard_layout();
        let mru = self.mru_snapshot();

        if let Some(us_standard) = us_standard {
            let mut max_score = us_standard.borrow().get_score(key_mapping);
            if max_score == 0 {
                return Some(MatchedKeymapInfo { result: us_standard, score: 0 });
            }

            let mut result = us_standard;
            for candidate in &mru {
                let score = candidate.borrow().get_score(key_mapping);
                if score > max_score {
                    if score == 0 {
                        return Some(MatchedKeymapInfo { result: candidate.clone(), score: 0 });
                    }

                    max_score = score;
                    result = candidate.clone();
                }
            }

            return Some(MatchedKeymapInfo { result, score: max_score });
        }

        for candidate in &mru {
            if candidate.borrow().fuzzy_equal(key_mapping) {
                return Some(MatchedKeymapInfo { result: candidate.clone(), score: 0 });
            }
        }

        None
    }

    pub fn get_us_standard_layout(&self) -> Option<KeymapInfoRef> {
        self.mru_snapshot().into_iter().find(|layout| layout.borrow().layout.is_us_standard == Some(true))
    }

    /// `isKeyMappingActive(keymap)`: o original devolve `this._activeKeymapInfo && keymap && ...`, e aqui o falso vale por
    /// qualquer um dos três ausentes.
    pub fn is_key_mapping_active(&self, keymap: Option<&RawMixedKeyboardMapping>) -> bool {
        match (self.active_keymap(), keymap) {
            (Some(active), Some(keymap)) => active.borrow().fuzzy_equal(keymap),
            _ => false,
        }
    }

    pub fn set_us_keyboard_layout(&self) {
        *self.active_keymap_info.borrow_mut() = self.get_us_standard_layout();
    }

    pub fn set_active_key_mapping(&self, keymap: Option<&RawMixedKeyboardMapping>) {
        let mut keymap_updated = false;
        let matched_keyboard_layout = self.get_matched_keymap_info(keymap);
        if let Some(matched_keyboard_layout) = matched_keyboard_layout {
            // Due to https://bugs.chromium.org/p/chromium/issues/detail?id=977609, any key after a dead key will generate a wrong mapping,
            // we shoud avoid yielding the false error.
            // O aviso "Fail to find matching keyboard layout" do original está comentado lá, e não existe aqui.
            let active = self.active_keymap();
            match (active, keymap) {
                (None, _) => {
                    *self.active_keymap_info.borrow_mut() = Some(matched_keyboard_layout.result);
                    keymap_updated = true;
                }
                (Some(active), Some(keymap)) => {
                    if matched_keyboard_layout.result.borrow().get_score(keymap) > active.borrow().get_score(keymap) {
                        *self.active_keymap_info.borrow_mut() = Some(matched_keyboard_layout.result);
                        keymap_updated = true;
                    }
                }
                (Some(_), None) => {}
            }
        }

        if self.active_keymap().is_none() {
            *self.active_keymap_info.borrow_mut() = self.get_us_standard_layout();
            keymap_updated = true;
        }

        let Some(active) = self.active_keymap() else { return };
        if !keymap_updated {
            return;
        }

        self.move_to_front_of_mru(&active);

        self.set_keyboard_data();
    }

    /// `const index = this._mru.indexOf(info); this._mru.splice(index, 1); this._mru.unshift(info);`
    fn move_to_front_of_mru(&self, keymap_info: &KeymapInfoRef) {
        let mru = self.mru.borrow().clone();
        let index = index_of(&mru.borrow(), keymap_info);
        splice_one(&mut mru.borrow_mut(), index);
        mru.borrow_mut().insert(0, keymap_info.clone());
    }

    pub fn set_active_keymap_info(&self, keymap_info: KeymapInfoRef) {
        *self.active_keymap_info.borrow_mut() = Some(keymap_info.clone());

        let index = index_of(&self.mru_snapshot(), &keymap_info);

        if index == 0 {
            return;
        }

        self.move_to_front_of_mru(&keymap_info);

        self.set_keyboard_data();
    }

    pub fn set_layout_from_browser_api(&self) {
        self.update_keyboard_layout_async(self.initialized.get(), None);
    }

    fn update_keyboard_layout_async(&self, initialized: bool, keyboard_event: Option<&StandardKeyboardEvent>) {
        if !initialized {
            return;
        }

        let this = self.this();
        self.get_browser_key_mapping(keyboard_event).then(move |key_map| {
            // might be false positive
            if this.is_key_mapping_active(key_map.as_ref()) {
                return;
            }
            this.set_active_key_mapping(key_map.as_ref());
        });
    }

    pub fn get_keyboard_mapper(&self) -> Rc<dyn IKeyboardMapper> {
        let config = read_keyboard_config(&*self.configuration_service);
        let active = self.active_keymap();
        let Some(active) = active.filter(|_| config.dispatch != DispatchConfig::KeyCode && self.initialized.get()) else {
            // Forcefully set to use keyCode
            return Rc::new(FallbackKeyboardMapper::new(config.map_alt_gr_to_ctrl_alt, self.os));
        };
        let cached = self.keyboard_mapper.borrow().clone();
        match cached {
            Some(mapper) => mapper,
            None => {
                let mapper: Rc<dyn IKeyboardMapper> =
                    Rc::new(CachedKeyboardMapper::new(Self::create_keyboard_mapper(&active.borrow(), config.map_alt_gr_to_ctrl_alt, self.os)));
                *self.keyboard_mapper.borrow_mut() = Some(mapper.clone());
                mapper
            }
        }
    }

    pub fn validate_current_keyboard_mapping(&self, keyboard_event: &StandardKeyboardEvent) {
        if !self.initialized.get() {
            return;
        }

        let is_current_keyboard = self.validate_current_keyboard_mapping_of(keyboard_event);

        if is_current_keyboard {
            return;
        }

        self.update_keyboard_layout_async(true, Some(keyboard_event));
    }

    pub fn set_keyboard_layout(&self, layout_name: &str) {
        let matched_layouts: Vec<KeymapInfoRef> =
            self.keymap_infos().into_iter().filter(|keymap_info| get_keyboard_layout_id(&keymap_info.borrow().layout) == layout_name).collect();

        if let Some(first) = matched_layouts.into_iter().next() {
            self.set_active_keymap_info(first);
        }
    }

    /// `_setKeyboardData`.
    fn set_keyboard_data(&self) {
        self.initialized.set(true);

        *self.keyboard_mapper.borrow_mut() = None;
        self.on_did_change_keyboard_mapper.fire(());
    }

    /// `_createKeyboardMapper`.
    fn create_keyboard_mapper(keymap_info: &KeymapInfo, map_alt_gr_to_ctrl_alt: bool, os: OperatingSystem) -> Rc<dyn IKeyboardMapper> {
        let is_us_standard = keymap_info.layout.is_us_standard == Some(true);
        if os != OperatingSystem::Windows && keymap_info.mapping.is_empty() {
            // Looks like reading the mappings failed (most likely Mac + Japanese/Chinese keyboard layouts)
            return Rc::new(FallbackKeyboardMapper::new(map_alt_gr_to_ctrl_alt, os));
        }
        match keymap_info.to_keyboard_mapping() {
            KeyboardMapping::Windows(raw_mapping) => Rc::new(WindowsKeyboardMapper::new(is_us_standard, raw_mapping.iter(), map_alt_gr_to_ctrl_alt)),
            KeyboardMapping::Linux(raw_mapping) => {
                let raw_mapping: IndexMap<String, MacLinuxKeyMapping> = raw_mapping.iter().map(|(key, mapping)| (key.clone(), MacLinuxKeyMapping::from(mapping))).collect();
                Rc::new(MacLinuxKeyboardMapper::new(is_us_standard, raw_mapping.iter(), map_alt_gr_to_ctrl_alt, os))
            }
            KeyboardMapping::Mac(raw_mapping) => {
                let raw_mapping: IndexMap<String, MacLinuxKeyMapping> = raw_mapping.iter().map(|(key, mapping)| (key.clone(), MacLinuxKeyMapping::from(mapping))).collect();
                Rc::new(MacLinuxKeyboardMapper::new(is_us_standard, raw_mapping.iter(), map_alt_gr_to_ctrl_alt, os))
            }
        }
    }

    //#region Browser API

    /// `_validateCurrentKeyboardMapping`.
    fn validate_current_keyboard_mapping_of(&self, keyboard_event: &StandardKeyboardEvent) -> bool {
        if !self.initialized.get() {
            return true;
        }

        let Some(current_keymap) = self.active_keymap() else {
            return true;
        };

        if keyboard_event.key == "Dead" || keyboard_event.is_composing {
            return true;
        }

        let mapping = current_keymap.borrow().mapping.get(&keyboard_event.code).cloned();

        let Some(mapping) = mapping else {
            return false;
        };

        if mapping.value.is_empty() {
            // The value is empty when the key is not a printable character, we skip validation.
            if keyboard_event.event.ctrl_key || keyboard_event.event.meta_key {
                let this = self.this();
                runtime::set_timeout(
                    move || {
                        let target = this.clone();
                        this.get_browser_key_mapping(None).then(move |keymap| {
                            if target.is_key_mapping_active(keymap.as_ref()) {
                                return;
                            }

                            target.set_layout_from_browser_api();
                        });
                    },
                    350,
                );
            }
            return true;
        }

        let (alt, shift) = (keyboard_event.event.alt_key, keyboard_event.event.shift_key);
        let expected_value = if alt && shift {
            &mapping.with_shift_alt_gr
        } else if alt {
            &mapping.with_alt_gr
        } else if shift {
            &mapping.with_shift
        } else {
            &mapping.value
        };

        let is_dead = (alt && shift && mapping.with_shift_alt_gr_is_dead_key)
            || (alt && mapping.with_alt_gr_is_dead_key)
            || (shift && mapping.with_shift_is_dead_key)
            || mapping.value_is_dead_key;

        if is_dead && keyboard_event.key != "Dead" {
            return false;
        }

        // Nota do original: esta suposição é errada, porque `browserEvent.key` não é necessariamente igual ao
        // `expectedValue` do mapa de teclas real.
        if !is_dead && keyboard_event.key != *expected_value {
            return false;
        }

        true
    }

    /// `_getBrowserKeyMapping`, uma função `async`.
    fn get_browser_key_mapping(&self, keyboard_event: Option<&StandardKeyboardEvent>) -> Promise<Option<RawMixedKeyboardMapping>> {
        let this = self.this();
        // (shiftKey, altKey, metaKey, code, key) do evento, copiados porque o corpo `async` sobrevive à chamada.
        let event = keyboard_event.map(|e| (e.event.shift_key, e.event.alt_key, e.event.meta_key, e.code.clone(), e.key.clone()));
        async_fn(async move {
            if this.keyboard_layout_map_allowed.get() {
                if let Some(navigator_keyboard) = this.navigator_keyboard.clone() {
                    match navigator_keyboard.get_layout_map().await {
                        Ok(entries) => {
                            let mut ret = RawMixedKeyboardMapping::new();
                            for (code, key) in entries {
                                ret.insert(code, RawMixedKeyMapping::simple(&key, "", "", ""));
                            }

                            return Ok(Some(ret));
                        }
                        Err(_) => {
                            // getLayoutMap can throw if invoked from a nested browsing context
                            this.keyboard_layout_map_allowed.set(false);
                        }
                    }
                }
            }
            if let Some((shift_key, alt_key, meta_key, code, key)) = event {
                // O original confere `metaKey` duas vezes (`!keyboardEvent.metaKey && !keyboardEvent.metaKey`).
                if !shift_key && !alt_key && !meta_key {
                    let mut ret = RawMixedKeyboardMapping::new();
                    ret.insert(code, RawMixedKeyMapping::simple(&key, "", "", ""));

                    let matched_keyboard_layout = this.get_matched_keymap_info(Some(&ret));

                    if matched_keyboard_layout.is_some() {
                        return Ok(Some(ret));
                    }

                    return Ok(None);
                }
            }

            Ok(None)
        })
    }

    //#endregion
}

impl IDisposable for BrowserKeyboardMapperFactoryBase {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `new BrowserKeyboardMapperFactory(configurationService, ...)`: a fábrica que carrega os layouts do sistema. O
/// `import(layout.contribution.<sistema>.js).then(m => ...)` do construtor é o `.then` sobre `layout_infos`.
pub fn create_browser_keyboard_mapper_factory(
    configuration_service: Rc<dyn IConfigurationService>,
    os: OperatingSystem,
    navigator_keyboard: Option<Rc<dyn NavigatorKeyboard>>,
    layout_infos: Promise<Vec<KeymapInfoSource>>,
) -> Rc<BrowserKeyboardMapperFactoryBase> {
    let factory = BrowserKeyboardMapperFactoryBase::new(configuration_service, os, navigator_keyboard);

    let loaded = factory.clone();
    layout_infos.then(move |keymap_infos| {
        loaded.push_keymap_infos(keymap_infos.iter().map(|info| KeymapInfo::from_source(info, os)).collect());
        loaded.alias_mru_to_keymap_infos();
        loaded.set_initialized(true);
        loaded.set_layout_from_browser_api();
    });

    factory
}
