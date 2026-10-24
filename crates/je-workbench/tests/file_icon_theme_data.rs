//! Porte de `src/vs/workbench/services/themes/test/browser/fileIconThemeData.test.ts` (suíte `FileIconThemeData`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `JSON.stringify({...})` do documento do tema é `json!({...}).to_string()`; o espalhamento condicional de
//!   `usesCurrentColor` é a inserção da chave só quando o parâmetro é `true`.
//! - O `mock<IExtensionResourceLoaderService>()` com `readExtensionResource` sobrescrito é o [`DocumentResourceLoader`],
//!   que devolve o documento numa `Promise` já resolvida. O `mock<ILanguageService>()` (sem nenhum método, que o caso
//!   nunca chama por causa do `showLanguageModeIcons: false`) é o [`LanguageService`] real e vazio, registrado no guarda
//!   de vazamentos.
//! - `URI.file('/themes/test/icon-theme.json')` é `uri_file(...)`; `new FileIconThemeLoader(...).load(themeData)` é
//!   `load(&mut theme_data).await`, dentro de `run_with_real_timers` (o corpo `async` do caso).
//! - `content?.includes(x)` é `content.as_deref().map(|content| content.contains(x))`, e o `assert.deepStrictEqual` de
//!   objetos é um `assert_eq!` entre tuplas, com os campos na ordem do original.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o [`LeakGuard`] de `common`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::LanguageService;
use je_platform::resources::uri_file;
use je_workbench::contrib::snippets::snippets_file::IExtensionResourceLoaderService;
use je_workbench::services::themes::file_icon_theme_data::{FileIconThemeData, FileIconThemeLoader};
use serde_json::{Value, json};

/// O `readExtensionResource` sobrescrito do original: devolve sempre o documento do tema.
struct DocumentResourceLoader {
    theme_document: String,
}

impl IExtensionResourceLoaderService for DocumentResourceLoader {
    fn read_extension_resource(&self, _uri: &URI) -> Promise<String> {
        Promise::resolved(self.theme_document.clone())
    }
}

async fn load_theme(uses_current_color: Option<bool>) -> Option<String> {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut theme_document = json!({
        "iconDefinitions": { "_test": { "iconPath": "./test.svg" } },
        "fileExtensions": { "test": "_test" },
    });
    if uses_current_color == Some(true) {
        theme_document["usesCurrentColor"] = Value::Bool(true);
    }
    theme_document["showLanguageModeIcons"] = Value::Bool(false);
    let resource_loader_service = Rc::new(DocumentResourceLoader { theme_document: theme_document.to_string() });
    let language_service = guard.add_rc(LanguageService::new(false));
    let mut theme_data = FileIconThemeData::create_unloaded_theme("test");
    theme_data.location = Some(uri_file("/themes/test/icon-theme.json"));

    FileIconThemeLoader::new(resource_loader_service, language_service)
        .load(&mut theme_data)
        .await
        .expect("o tema carrega")
}

fn includes(content: &Option<String>, text: &str) -> Option<bool> {
    content.as_deref().map(|content| content.contains(text))
}

#[test]
fn renders_image_icons_with_current_color_when_enabled() {
    run_with_real_timers(async move {
        let content = load_theme(Some(true)).await;

        assert_eq!(
            (
                includes(&content, "background-color: currentColor"),
                includes(&content, "mask: url("),
                includes(&content, "no-repeat var(--file-icon-mask-position, left center)"),
                includes(&content, "mask-size: var(--file-icon-mask-size, 16px)"),
                includes(&content, "-webkit-mask: url("),
                includes(&content, "-webkit-mask-size: var(--file-icon-mask-size, 16px)"),
                includes(&content, "background-image: none"),
            ),
            (Some(true), Some(true), Some(true), Some(true), Some(true), Some(true), Some(true))
        );
    });
}

#[test]
fn renders_image_icons_as_background_images_by_default() {
    run_with_real_timers(async move {
        let content = load_theme(None).await;

        assert_eq!(
            (
                includes(&content, "background-image: url("),
                includes(&content, "background-color: currentColor"),
                includes(&content, "mask: url("),
            ),
            (Some(true), Some(false), Some(false))
        );
    });
}
