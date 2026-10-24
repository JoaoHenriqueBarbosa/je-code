//! Porte de `src/vs/workbench/services/themes/test/common/workbenchThemeService.test.ts` (suíte
//! `WorkbenchThemeService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `suite('migrateThemeSettingsId')` são os três primeiros casos (`migrate_theme_settings_id_*`); o
//!   `assert.deepStrictEqual` entre listas é `assert_eq!` entre `Vec<String>`. `ThemeSettingDefaults.COLOR_THEME_DARK`
//!   e `COLOR_THEME_LIGHT` são as constantes de [`theme_setting_defaults`].
//! - `TestHostColorSchemeService` é a struct de mesmo nome, com `dark` e `highContrast` em `Cell` (o original os
//!   atribui depois da construção) e `onDidChangeColorScheme = Event.None` como `Event::none()`.
//! - `new TestConfigurationService({ ... })` recebe o objeto de configuração como [`Dynamic`] (as chaves são as
//!   [`ThemeSettings`] de texto, lidas direto pelo `getValue`); `await setUserConfiguration(...)` é
//!   `set_user_configuration(...).await`, dentro de `run_with_real_timers` (o corpo `async` do caso).
//! - `isPreferredColorSchemeChange({ dark, highContrast })` recebe um [`IColorScheme`].
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o [`LeakGuard`] de `common`, criado no começo de cada caso.
//!
//! Fora do porte: nada (os quatro casos do original estão aqui).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::services::themes::host_color_scheme_service::{IColorScheme, IHostColorSchemeService};
use je_workbench::services::themes::theme_configuration::ThemeConfiguration;
use je_workbench::services::themes::workbench_theme_service::{
    ThemeSettings, migrate_theme_settings_id, theme_setting_defaults,
};

#[derive(Default)]
struct TestHostColorSchemeService {
    dark: Cell<bool>,
    high_contrast: Cell<bool>,
}

impl IHostColorSchemeService for TestHostColorSchemeService {
    fn dark(&self) -> bool {
        self.dark.get()
    }
    fn high_contrast(&self) -> bool {
        self.high_contrast.get()
    }
    fn on_did_change_color_scheme(&self) -> Event<()> {
        Event::none()
    }
}

fn migrate_all(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| migrate_theme_settings_id(id)).collect()
}

fn owned(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn migrate_theme_settings_id_migrates_default_prefixed_theme_ids() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        migrate_all(&["Default Dark Modern", "Default Light Modern", "Default Dark+", "Default Light+"]),
        owned(&["Dark Modern", "Light Modern", "Dark+", "Light+"])
    );
}

#[test]
fn migrate_theme_settings_id_migrates_experimental_theme_ids_to_vs_code_themes() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        migrate_all(&["Experimental Dark", "Experimental Light", "VS Code Dark", "VS Code Light"]),
        owned(&[
            theme_setting_defaults::COLOR_THEME_DARK,
            theme_setting_defaults::COLOR_THEME_LIGHT,
            theme_setting_defaults::COLOR_THEME_DARK,
            theme_setting_defaults::COLOR_THEME_LIGHT,
        ])
    );
}

#[test]
fn migrate_theme_settings_id_returns_unknown_ids_unchanged() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        migrate_all(&["Dark Modern", "Dark 2026", "Some Custom Theme", ""]),
        owned(&["Dark Modern", "Dark 2026", "Some Custom Theme", ""])
    );
}

#[test]
fn restores_only_for_preferred_color_scheme_changes() {
    run_with_real_timers(async move {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service = TestConfigurationService::new(Some(Dynamic::object([
            (ThemeSettings::DetectColorScheme.as_str(), Dynamic::Bool(false)),
            (ThemeSettings::DetectHc.as_str(), Dynamic::Bool(true)),
        ])));
        let host_color_scheme_service = Rc::new(TestHostColorSchemeService::default());
        let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
        let host: Rc<dyn IHostColorSchemeService> = host_color_scheme_service.clone();
        let theme_configuration = ThemeConfiguration::new(configuration, host);

        let mut results: Vec<bool> = Vec::new();
        host_color_scheme_service.dark.set(true);
        results.push(theme_configuration.is_preferred_color_scheme_change(&IColorScheme { dark: false, high_contrast: false }));
        host_color_scheme_service.high_contrast.set(true);
        results.push(theme_configuration.is_preferred_color_scheme_change(&IColorScheme { dark: true, high_contrast: false }));
        host_color_scheme_service.dark.set(false);
        results.push(theme_configuration.is_preferred_color_scheme_change(&IColorScheme { dark: true, high_contrast: true }));
        host_color_scheme_service.high_contrast.set(false);
        results.push(theme_configuration.is_preferred_color_scheme_change(&IColorScheme { dark: false, high_contrast: true }));

        configuration_service
            .set_user_configuration(ThemeSettings::DetectColorScheme.as_str(), Dynamic::Bool(true), None)
            .await
            .expect("a configuração do usuário é gravada");
        host_color_scheme_service.dark.set(true);
        results.push(theme_configuration.is_preferred_color_scheme_change(&IColorScheme { dark: false, high_contrast: false }));

        assert_eq!(results, vec![false, true, true, true, true]);
    });
}
