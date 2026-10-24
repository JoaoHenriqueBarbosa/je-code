//! Porte de `src/vs/workbench/services/themes/test/common/colorThemeData.test.ts` (suíte `ColorThemeData - transient
//! colors`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ColorThemeData.createUnloadedTheme('vs-dark', { [id]: hex })` é `create_unloaded_theme("vs-dark", &[(id, hex)])`;
//!   os ids de cor (`foreground`, `editorBackground`, `inputForeground`, `inputBackground`) são os valores dos
//!   identificadores registrados: `foreground`, `editor.background`, `input.foreground` e `input.background`.
//! - `Color.fromHex` é o de `je_theme::color::Color`; `IColorMap` é `HashMap<String, Color>` e as customizações de
//!   `setCustomColors` são o objeto JSON (`serde_json::Map`).
//! - `color?.toString()` é `map(|color| color.to_string())` (o `Display` do `Color` é o `Color.Format.CSS.format`).
//! - `theme.tokenColors[0].settings.background` é a chave `background` das `settings` da primeira regra; a
//!   comparação por identidade `theme.tokenColors !== beforeTokens` é `Rc::ptr_eq` sobre o `Rc` do cache.
//! - O `assert.deepStrictEqual` de objetos é um `assert_eq!` entre tuplas, com os campos na ordem do original.
//! - `InMemoryStorageService` é o de `je_platform::storage`, registrado no guarda de vazamentos como o
//!   `disposables.add(...)` do original; `storageService.get(key, StorageScope.PROFILE)` é `get(key, Profile, None)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::storage::{InMemoryStorageService, StorageScope, StorageService};
use je_theme::color::Color;
use je_workbench::services::themes::color_theme_data::ColorThemeData;
use serde_json::{Map, Value, json};

const FOREGROUND: &str = "foreground";
const EDITOR_BACKGROUND: &str = "editor.background";
const INPUT_FOREGROUND: &str = "input.foreground";
const INPUT_BACKGROUND: &str = "input.background";

fn transient(colors: &[(&str, &str)]) -> Option<HashMap<String, Color>> {
    Some(colors.iter().map(|(id, hex)| ((*id).to_owned(), Color::from_hex(hex))).collect())
}

fn customizations(colors: Value) -> Map<String, Value> {
    colors.as_object().expect("as customizações são um objeto").clone()
}

fn as_text(color: Option<Color>) -> Option<String> {
    color.map(|color| color.to_string())
}

/// `theme.tokenColors[0].settings.background`.
fn token_background(theme: &ColorThemeData) -> Option<String> {
    let rules = theme.token_colors();
    let settings = rules[0].settings.as_ref().expect("a regra padrão tem settings");
    settings.get("background").and_then(Value::as_str).map(str::to_owned)
}

#[test]
fn updates_programmatic_colors_derived_defaults_and_token_caches() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme = ColorThemeData::create_unloaded_theme("vs-dark", &[(FOREGROUND, "#eeeeee"), (EDITOR_BACKGROUND, "#202020")]);
    let before_tokens = theme.token_colors();
    theme.set_transient_colors(transient(&[(FOREGROUND, "#ddccbb"), (EDITOR_BACKGROUND, "#28221e")]));
    assert_eq!(
        (
            as_text(theme.get_color(FOREGROUND, false)),
            as_text(theme.get_color(INPUT_FOREGROUND, true)),
            theme.defines(FOREGROUND),
            token_background(&theme),
            !Rc::ptr_eq(&theme.token_colors(), &before_tokens),
        ),
        (Some("#ddccbb".to_owned()), Some("#ddccbb".to_owned()), true, Some("#28221E".to_owned()), true)
    );
}

#[test]
fn clearing_runtime_colors_restores_theme_and_user_customized_colors() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme = ColorThemeData::create_unloaded_theme("vs-dark", &[(EDITOR_BACKGROUND, "#202020")]);
    theme.set_custom_colors(&customizations(json!({ (INPUT_BACKGROUND): "#123456" })));
    theme.set_transient_colors(transient(&[(INPUT_BACKGROUND, "#654321"), (EDITOR_BACKGROUND, "#28221e")]));
    theme.set_transient_colors(None);
    assert_eq!(
        (
            as_text(theme.get_color(EDITOR_BACKGROUND, true)),
            as_text(theme.get_color(INPUT_BACKGROUND, true)),
            as_text(theme.get_color_customization(INPUT_BACKGROUND)),
        ),
        (Some("#202020".to_owned()), Some("#123456".to_owned()), Some("#123456".to_owned()))
    );
}

#[test]
fn exposes_untinted_startup_colors_without_changing_the_active_theme() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme = ColorThemeData::create_unloaded_theme("vs-dark", &[(EDITOR_BACKGROUND, "#202020")]);
    theme.set_custom_colors(&customizations(json!({ (INPUT_BACKGROUND): "#123456" })));
    theme.set_transient_colors(transient(&[(INPUT_BACKGROUND, "#654321"), (EDITOR_BACKGROUND, "#28221e")]));
    let base_theme = theme.get_base_theme();
    assert_eq!(
        (
            as_text(theme.get_color(EDITOR_BACKGROUND, true)),
            as_text(base_theme.get_color(EDITOR_BACKGROUND, true)),
            as_text(base_theme.get_color(INPUT_BACKGROUND, true)),
            token_background(&theme),
            token_background(&base_theme),
        ),
        (
            Some("#28221e".to_owned()),
            Some("#202020".to_owned()),
            Some("#123456".to_owned()),
            Some("#28221E".to_owned()),
            Some("#202020".to_owned()),
        )
    );
}

#[test]
fn does_not_persist_generated_colors_or_affect_another_theme_instance() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let storage_service = disposables.add_rc(InMemoryStorageService::new());
    let mut theme = ColorThemeData::create_unloaded_theme("vs-dark", &[(EDITOR_BACKGROUND, "#202020")]);
    theme.to_storage(&storage_service);
    let original_storage = storage_service.get(ColorThemeData::STORAGE_KEY, StorageScope::Profile, None);
    let other_window_theme = ColorThemeData::from_storage_data(&storage_service).expect("o tema gravado");
    theme.set_transient_colors(transient(&[(EDITOR_BACKGROUND, "#28221e")]));
    theme.to_storage(&storage_service);
    let restored = ColorThemeData::from_storage_data(&storage_service).expect("o tema gravado");
    assert_eq!(
        (
            as_text(theme.get_color(EDITOR_BACKGROUND, true)),
            as_text(other_window_theme.get_color(EDITOR_BACKGROUND, true)),
            as_text(restored.get_color(EDITOR_BACKGROUND, true)),
            storage_service.get(ColorThemeData::STORAGE_KEY, StorageScope::Profile, None) == original_storage,
        ),
        (Some("#28221e".to_owned()), Some("#202020".to_owned()), Some("#202020".to_owned()), true)
    );
}
