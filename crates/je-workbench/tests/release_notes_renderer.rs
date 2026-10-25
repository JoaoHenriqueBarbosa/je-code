//! Porte de `src/vs/workbench/contrib/update/test/browser/releaseNotesRenderer.test.ts` (suítes `Release notes renderer` e
//! `Conditional blocks`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `TestInstantiationService` com `instantiationService.get(IExtensionService)` e `get(ILanguageService)` são o
//!   `TestExtensionService` e o `LanguageService` do [`Fixture`]; `createInstance(SimpleSettingRenderer)` é o
//!   `SimpleSettingRenderer::new` com o serviço de preferências que o caso definiu (o `instantiationService.stub(IPreferencesService, ...)`
//!   vem antes do `createInstance`, então o renderizador do caso já nasce com ele). Os demais serviços do renderizador
//!   (configuração, menu de contexto, telemetria, área de transferência) não são lidos por nenhum caso.
//! - `assertSnapshot(result.toString())` compara com o conteúdo de `__snapshots__/*.snap`, que o porte embute em
//!   [`TOC_SNAPSHOT`] e [`code_settings_snapshot`], byte a byte. O conteúdo do caso tem tabulação e o `.snap` traz quatro
//!   espaços no lugar dela: a renderização do original troca a tabulação, e a do porte também.
//! - O `stub(IPreferencesService, <Partial<IPreferencesService>>{ ... getSetting })` é o [`TestPreferencesService`], que só
//!   tem o `getSetting` (os outros métodos do `Partial` não são lidos).
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso; `new Set(['IN_PRODUCT',
//!   'STABLE'])` é o [`conditions`]; `await` é a espera da promessa em `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::HashSet;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_ui::Ui;
use je_widgets::context_view_api::{IContextMenuDelegate, IContextMenuProvider};
use je_workbench::contrib::markdown::markdown_setting_renderer::SimpleSettingRenderer;
use je_workbench::contrib::update::browser::release_notes_editor::{process_conditional_blocks, render_release_notes_markdown};
use je_workbench::services::extensions::extensions::IExtensionService;
use je_workbench::services::preferences::preferences::{IOpenSettingsOptions, IPreferencesService, ISetting, SettingSchemaType};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;

/// `<Partial<IPreferencesService>>{ getSetting }`: só conhece a configuração que o caso define.
struct TestPreferencesService {
    setting: Option<ISetting>,
}

impl IPreferencesService for TestPreferencesService {
    fn open_user_settings(&self, _options: Option<&IOpenSettingsOptions>) -> Promise<()> {
        panic!("openUserSettings is not a function")
    }

    fn open_application_settings(&self, _options: Option<&IOpenSettingsOptions>) -> Promise<()> {
        panic!("openApplicationSettings is not a function")
    }

    fn get_setting(&self, setting: &str) -> Option<ISetting> {
        self.setting.clone().filter(|known| known.key == setting)
    }
}

/// `<IContextMenuService>{}`.
struct TestContextMenuService;

impl IContextMenuProvider for TestContextMenuService {
    fn show_context_menu(&self, _delegate: Rc<dyn IContextMenuDelegate>) {
        panic!("showContextMenu is not a function")
    }
}

/// `{ writeText: async () => { } }`.
struct TestClipboardService;

impl IClipboardService for TestClipboardService {
    fn write_text(&self, _text: &str) -> Promise<()> {
        Promise::resolved(())
    }

    fn read_text(&self) -> Promise<String> {
        panic!("readText is not a function")
    }

    fn read_find_text(&self) -> Promise<JsString> {
        panic!("readFindText is not a function")
    }

    fn write_find_text(&self, _text: &JsString) -> Promise<()> {
        panic!("writeFindText is not a function")
    }

    fn write_resources(&self, _resources: Vec<URI>) -> Promise<()> {
        panic!("writeResources is not a function")
    }

    fn read_resources(&self) -> Promise<Vec<URI>> {
        panic!("readResources is not a function")
    }

    fn has_resources(&self) -> Promise<bool> {
        panic!("hasResources is not a function")
    }
}

/// O `setup` das suítes: os serviços de `instantiationService` e o `SimpleSettingRenderer` criado a partir deles.
struct Fixture {
    _guard: LeakGuard,
    extension_service: Rc<dyn IExtensionService>,
    language_service: Rc<dyn ILanguageService>,
    simple_setting_renderer: Rc<SimpleSettingRenderer>,
}

impl Fixture {
    fn new(setting: Option<ISetting>) -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let language_service = guard.add_rc(LanguageService::new(false));
        let configuration_service = TestConfigurationService::new(None);
        guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = configuration_service;
        let simple_setting_renderer = SimpleSettingRenderer::new(
            configuration_service,
            Rc::new(TestContextMenuService),
            Rc::new(TestPreferencesService { setting }),
            null_telemetry_service(),
            Rc::new(TestClipboardService),
            Ui::new(),
        );
        Self { _guard: guard, extension_service: TestExtensionService::new(), language_service, simple_setting_renderer }
    }

    /// `renderReleaseNotesMarkdown(content, extensionService, languageService, simpleSettingRenderer, quality)`.
    fn render(&self, content: &str, quality: Option<&str>) -> Promise<JsString> {
        render_release_notes_markdown(
            content,
            Rc::clone(&self.extension_service),
            Rc::clone(&self.language_service),
            &self.simple_setting_renderer,
            quality,
            None,
            None,
        )
    }
}

fn conditions(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// `__snapshots__/Release_notes_renderer_Should_render_TOC.0.snap`.
const TOC_SNAPSHOT: &str = concat!(
    "<table class=\"highlights-table\">\n",
    "    <tbody><tr>\n",
    "        <th>a</th>\n",
    "    </tr>\n",
    "</tbody></table>\n",
    "\n",
    "<br>\n",
    "\n",
    "<blockquote>\n",
    "<p>text</p>\n",
    "</blockquote>\n",
    "<div class=\"toc-nav-layout\">\n",
    "    <nav id=\"toc-nav\">\n",
    "        <div>In this update</div>\n",
    "        <ul>\n",
    "            <li><a href=\"#chat\">test</a></li>\n",
    "        </ul>\n",
    "    </nav>\n",
    "    <div class=\"notes-main\">\n",
    "\n",
    "<h2 id=\"test\">Test</h2>\n",
    "</div></div>",
);

const SETTING_ICON_PATH: &str = "M9.1 4.4L8.6 2H7.4l-.5 2.4-.7.3-2-1.3-.9.8 1.3 2-.2.7-2.4.5v1.2l2.4.5.3.8-1.3 2 .8.8 2-1.3.8.3.4 2.3h1.2l.5-2.4.8-.3 2 1.3.8-.8-1.3-2 .3-.8 2.3-.4V7.4l-2.4-.5-.3-.8 1.3-2-.8-.8-2 1.3-.7-.2zM9.4 1l.5 2.4L12 2.1l2 2-1.4 2.1 2.4.4v2.8l-2.4.5L14 12l-2 2-2.1-1.4-.5 2.4H6.6l-.5-2.4L4 13.9l-2-2 1.4-2.1L1 9.4V6.6l2.4-.5L2.1 4l2-2 2.1 1.4.4-2.4h2.8zm.6 7c0 1.1-.9 2-2 2s-2-.9-2-2 .9-2 2-2 2 .9 2 2zM8 9c.6 0 1-.4 1-1s-.4-1-1-1-1 .4-1 1 .4 1 1 1z";

/// O botão de configuração que o `SimpleSettingRenderer` escreve para `editor.wordWrap` com o valor dado.
fn code_setting_button(value: &str) -> String {
    format!(
        "<code tabindex=\"0\"><a href=\"code-setting://editor.wordWrap/{value}\" class=\"codesetting\" title=\"View or change setting\" aria-role=\"button\"><svg width=\"14\" height=\"14\" viewBox=\"0 0 15 15\" xmlns=\"http://www.w3.org/2000/svg\" fill=\"currentColor\"><path d=\"{SETTING_ICON_PATH}\"></path></svg>\n\t\t\t<span class=\"separator\"></span>\n\t\t\t<span class=\"setting-name\">editor.wordWrap</span>\n\t\t</a></code>"
    )
}

/// `__snapshots__/Release_notes_renderer_Should_render_code_settings.0.snap`.
fn code_settings_snapshot() -> String {
    format!("<p>Here is a setting: {} and another {}</p>\n", code_setting_button("on"), code_setting_button("off"))
}

#[test]
fn should_render_toc() {
    let fixture = Fixture::new(None);
    let content = "<table class=\"highlights-table\">\n\t<tr>\n\t\t<th>a</th>\n\t</tr>\n</table>\n\n<br>\n\n> text\n\n<!-- TOC\n<div class=\"toc-nav-layout\">\n\t<nav id=\"toc-nav\">\n\t\t<div>In this update</div>\n\t\t<ul>\n\t\t\t<li><a href=\"#chat\">test</a></li>\n\t\t</ul>\n\t</nav>\n\t<div class=\"notes-main\">\nNavigation End -->\n\n## Test";

    run_with_real_timers(async move {
        let result = fixture.render(content, None).await.expect("renderReleaseNotesMarkdown");
        assert_eq!(result.to_string_lossy(), TOC_SNAPSHOT);
    });
}

#[test]
fn keeps_shorthand_as_inline_code_without_the_tryout_renderer() {
    let fixture = Fixture::new(None);
    let content = "`try(editor.smart-diff,Try Smart Diff Layout)`\n\n<!-- %IF TRYOUTS %\n`try(editor.smart-diff,Conditional Example)`\n%ENDIF % -->";

    run_with_real_timers(async move {
        let result = fixture.render(content, None).await.expect("renderReleaseNotesMarkdown");
        assert_eq!(result.to_string_lossy().trim(), "<p><code>try(editor.smart-diff,Try Smart Diff Layout)</code></p>");
    });
}

#[test]
fn should_render_code_settings() {
    // Stub preferences service with a known setting so the SimpleSettingRenderer treats it as valid
    let test_setting_id = "editor.wordWrap";
    let setting = ISetting {
        key: test_setting_id.to_owned(),
        value: Dynamic::string("off"),
        r#type: Some(SettingSchemaType::Single("string".to_owned())),
        ..ISetting::default()
    };
    let fixture = Fixture::new(Some(setting));

    let content = format!("Here is a setting: `setting({test_setting_id}:on)` and another `setting({test_setting_id}:off)`");
    run_with_real_timers(async move {
        let result = fixture.render(&content, None).await.expect("renderReleaseNotesMarkdown");
        assert_eq!(result.to_string_lossy(), code_settings_snapshot());
    });
}

#[test]
fn in_product_block_is_revealed_when_in_product_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF IN_PRODUCT %\nin-product content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT"]));
    assert!(result.contains("in-product content"));
    assert!(!result.contains("%IF"));
    assert!(result.contains("before"));
    assert!(result.contains("after"));
}

#[test]
fn web_block_is_removed_when_only_in_product_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF WEB %\nweb-only content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT"]));
    assert!(!result.contains("web-only content"));
    assert!(result.contains("before"));
    assert!(result.contains("after"));
}

#[test]
fn stable_block_is_revealed_when_stable_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF STABLE %\nstable content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT", "STABLE"]));
    assert!(result.contains("stable content"));
    assert!(!result.contains("%IF"));
}

#[test]
fn stable_block_is_removed_when_insiders_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF STABLE %\nstable content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT", "INSIDERS"]));
    assert!(!result.contains("stable content"));
    assert!(result.contains("before"));
    assert!(result.contains("after"));
}

#[test]
fn insiders_block_is_revealed_when_insiders_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF INSIDERS %\ninsiders content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT", "INSIDERS"]));
    assert!(result.contains("insiders content"));
    assert!(!result.contains("%IF"));
}

#[test]
fn insiders_block_is_removed_when_stable_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF INSIDERS %\ninsiders content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT", "STABLE"]));
    assert!(!result.contains("insiders content"));
}

#[test]
fn conditions_are_case_insensitive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "<!-- %IF in_product %\ncontent\n%endif % -->";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT"]));
    assert!(result.contains("content"));
    assert!(!result.contains("%IF"));
}

#[test]
fn multiple_conditional_blocks_in_same_document() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [
        "shared content",
        "<!-- %IF IN_PRODUCT %",
        "in-product only",
        "%ENDIF % -->",
        "<!-- %IF WEB %",
        "web only",
        "%ENDIF % -->",
        "<!-- %IF STABLE %",
        "stable only",
        "%ENDIF % -->",
        "<!-- %IF INSIDERS %",
        "insiders only",
        "%ENDIF % -->",
        "more shared content",
    ]
    .join("\n");
    let result = process_conditional_blocks(&text, &conditions(&["IN_PRODUCT", "STABLE"]));
    assert!(result.contains("shared content"));
    assert!(result.contains("in-product only"));
    assert!(!result.contains("web only"));
    assert!(result.contains("stable only"));
    assert!(!result.contains("insiders only"));
    assert!(result.contains("more shared content"));
}

fn quality_content() -> String {
    ["## Title", "<!-- %IF STABLE %", "stable content", "%ENDIF % -->", "<!-- %IF INSIDERS %", "insiders content", "%ENDIF % -->"].join("\n")
}

#[test]
fn render_release_notes_markdown_passes_stable_quality_correctly() {
    let fixture = Fixture::new(None);
    let content = quality_content();

    run_with_real_timers(async move {
        let result = fixture.render(&content, Some("stable")).await.expect("renderReleaseNotesMarkdown");
        let html = result.to_string_lossy();
        assert!(html.contains("stable content"));
        assert!(!html.contains("insiders content"));
    });
}

#[test]
fn render_release_notes_markdown_passes_insider_quality_correctly() {
    let fixture = Fixture::new(None);
    let content = quality_content();

    run_with_real_timers(async move {
        let result = fixture.render(&content, Some("insider")).await.expect("renderReleaseNotesMarkdown");
        let html = result.to_string_lossy();
        assert!(!html.contains("stable content"));
        assert!(html.contains("insiders content"));
    });
}
