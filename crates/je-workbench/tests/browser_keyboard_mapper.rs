//! Porte de `src/vs/workbench/services/keybinding/test/browser/browserKeyboardMapper.test.ts` (suíte `keyboard layout
//! loader`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `teardown` (`instantiationService.dispose()`) e o `ds.add(...)` viram o descarte do
//!   `LeakGuard` ao fim do corpo, que também faz o `ensureNoDisposablesAreLeakedInTestSuite()`. O `TestInstantiationService` e
//!   os serviços que o `setup` cria só para entregá-los ao construtor (`TestStorageService`, `TestNotificationService` e o
//!   `ICommandService` vazio) não existem aqui: o `TestKeyboardMapperFactory` os recebe e os ignora (o `super` dele só passa o
//!   `configurationService`), então nada neles é exercitado. O `TestConfigurationService` é o de `je-platform`.
//! - **Os `import '../../browser/keyboardLayouts/en.darwin.js'` e `de.darwin.js`** registram os layouts ao carregar o módulo; no
//!   porte é a chamada de `register()` de cada um, no começo de `run` (a chamada repetida na mesma thread não duplica).
//! - **`class TestKeyboardMapperFactory extends BrowserKeyboardMapperFactoryBase`.** Sem herança, o construtor da subclasse é
//!   [`create_test_keyboard_mapper_factory`], que chama o construtor da base e usa os métodos públicos dos membros `protected`
//!   (`_keymapInfos.push`, `_mru = _keymapInfos`, `_initialized = true`); o `new KeymapInfo(info.layout, ...)` é
//!   `KeymapInfo::from_source`.
//! - **`navigator.keyboard`.** O teste roda sem teclado de navegador (`None`), então o `getLayoutMap` não é consultado e o
//!   `setLayoutFromBrowserAPI()` do construtor termina sem trocar o layout, como no original sob um `navigator` sem `keyboard`.
//! - **`OS`.** O sistema do teste é o Linux.
//! - **Os literais de mapa de teclas** (`IKeyboardMapping`) são [`RawMixedKeyboardMapping`] com as quatro teclas mortas falsas.
//! - **`assert.strictEqual(!!x.isUSStandard, ...)`** é o valor lógico de `Option<bool>`; `notStrictEqual(x, null)` é `is_some()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::platform::OperatingSystem;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::services::keybinding::keyboard_layout_service::BrowserKeyboardMapperFactoryBase;
use je_workbench::services::keybinding::keyboard_layouts::contribution::KeyboardLayoutContribution;
use je_workbench::services::keybinding::keyboard_layouts::{de_darwin, en_darwin};
use je_workbench::services::keybinding::keymap_info::{KeymapInfo, RawMixedKeyMapping, RawMixedKeyboardMapping};

const OS: OperatingSystem = OperatingSystem::Linux;

/// `class TestKeyboardMapperFactory extends BrowserKeyboardMapperFactoryBase`: o construtor da subclasse.
fn create_test_keyboard_mapper_factory(configuration_service: Rc<dyn IConfigurationService>) -> Rc<BrowserKeyboardMapperFactoryBase> {
    let factory = BrowserKeyboardMapperFactoryBase::new(configuration_service, OS, None);

    let keymap_infos = KeyboardLayoutContribution::with_instance(|contribution| contribution.layout_infos());
    factory.push_keymap_infos(keymap_infos.iter().map(|info| KeymapInfo::from_source(info, OS)).collect());
    factory.alias_mru_to_keymap_infos();
    factory.set_initialized(true);
    factory.set_layout_from_browser_api();
    if let Some(us_layout) = factory.get_us_standard_layout() {
        let mapping = us_layout.borrow().mapping.clone();
        factory.set_active_key_mapping(Some(&mapping));
    }
    factory
}

/// `setup`, o corpo do teste e `teardown`.
fn run(body: impl FnOnce(&BrowserKeyboardMapperFactoryBase)) {
    en_darwin::register();
    de_darwin::register();

    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);

    let instance = ds.add_rc(create_test_keyboard_mapper_factory(configuration_service));
    body(&instance);
}

/// Um literal `{ [código]: { value, valueIsDeadKey: false, withShift, ... } }`.
fn mapping(keys: &[(&str, [&str; 4])]) -> RawMixedKeyboardMapping {
    keys.iter().map(|(code, [value, with_shift, with_alt_gr, with_shift_alt_gr])| ((*code).to_string(), RawMixedKeyMapping::simple(value, with_shift, with_alt_gr, with_shift_alt_gr))).collect()
}

fn key_a() -> (&'static str, [&'static str; 4]) {
    ("KeyA", ["a", "A", "å", "Å"])
}

fn key_z_us() -> (&'static str, [&'static str; 4]) {
    ("KeyZ", ["z", "Z", "Ω", "¸"])
}

fn key_z_as_y() -> (&'static str, [&'static str; 4]) {
    ("KeyZ", ["y", "Y", "¥", "Ÿ"])
}

#[test]
fn load_default_us_keyboard_layout() {
    run(|instance| {
        assert!(instance.active_keyboard_layout().is_some());
    });
}

#[test]
fn is_key_mapping_active() {
    run(|instance| {
        instance.set_us_keyboard_layout();
        assert!(instance.is_key_mapping_active(Some(&mapping(&[key_a()]))));

        assert!(instance.is_key_mapping_active(Some(&mapping(&[key_a(), key_z_us()]))));

        assert!(!instance.is_key_mapping_active(Some(&mapping(&[key_z_as_y()]))));
    });
}

#[test]
fn switch_keymapping() {
    run(|instance| {
        instance.set_active_key_mapping(Some(&mapping(&[key_z_as_y()])));
        assert!(!instance.active_keyboard_layout().unwrap().is_us_standard.unwrap_or(false));
        assert!(instance.is_key_mapping_active(Some(&mapping(&[key_z_as_y()]))));

        instance.set_us_keyboard_layout();
        assert_eq!(instance.active_keyboard_layout().unwrap().is_us_standard, Some(true));
    });
}

#[test]
fn switch_keyboard_layout_info() {
    run(|instance| {
        instance.set_keyboard_layout("com.apple.keylayout.German");
        assert!(!instance.active_keyboard_layout().unwrap().is_us_standard.unwrap_or(false));
        assert!(instance.is_key_mapping_active(Some(&mapping(&[key_z_as_y()]))));

        instance.set_us_keyboard_layout();
        assert_eq!(instance.active_keyboard_layout().unwrap().is_us_standard, Some(true));
    });
}
