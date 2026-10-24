//! `TestCodeEditorService` de `src/vs/editor/test/browser/editorTestServices.ts`.
//!
//! O original estende o `AbstractCodeEditorService`. Aqui o registro de editores e a parte de decorações do serviço
//! abstrato são o [`CodeEditorRegistry`] (`src/services/abstract_code_editor_service.rs`), que o dublê guarda por
//! composição e expõe em [`TestCodeEditorService::registry`] (`listCodeEditors`, `onCodeEditorAdd`,
//! `onCodeEditorRemove`, `getFocusedCodeEditor`, `decoration_service`).
//!
//! Divergências:
//! - O registro nasce na primeira chamada de `registry()`, com o `IThemeService` recebido em
//!   [`TestCodeEditorService::new`] (ou um `TestThemeService` vazio no [`Default`]). O `opener_service.rs` cria o dublê
//!   sem usar o registro, e assim nenhum `Disposable` nasce à toa.
//! - `openCodeEditor` guarda a entrada em `last_input` e resolve com `null`; `getActiveCodeEditor` do original é
//!   `null`. `getFocusedCodeEditor` é o do registro (sem editor registrado, `None`).
//! - O `TestGlobalStyleSheet` e o `_createGlobalStyleSheet` dependem do DOM e ficam de fora.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::opener_service::{CodeEditorHandle, ICodeEditorService, TextResourceEditorInput};
use je_editor::services::abstract_code_editor_service::CodeEditorRegistry;
use je_editor::services::decoration_render_options::DecorationRenderService;
use je_editor::services::theme_service::IThemeService;

use super::test_color_theme_service::TestThemeService;

#[derive(Default)]
pub struct TestCodeEditorService {
    pub last_input: RefCell<Option<TextResourceEditorInput>>,
    theme_service: Option<Rc<dyn IThemeService>>,
    registry: OnceCell<Rc<CodeEditorRegistry>>,
}

impl TestCodeEditorService {
    /// `new TestCodeEditorService(themeService)`.
    pub fn new(theme_service: Rc<dyn IThemeService>) -> Self {
        Self { last_input: RefCell::new(None), theme_service: Some(theme_service), registry: OnceCell::new() }
    }

    /// O registro de editores e as decorações do `AbstractCodeEditorService`.
    pub fn registry(&self) -> &Rc<CodeEditorRegistry> {
        self.registry.get_or_init(|| {
            let theme_service: Rc<dyn IThemeService> = self.theme_service.clone().unwrap_or_else(|| TestThemeService::new_default());
            CodeEditorRegistry::new(DecorationRenderService::new(theme_service, Rc::new(|uri: &URI| uri.clone())))
        })
    }
}

impl IDisposable for TestCodeEditorService {
    fn dispose(&self) {
        if let Some(registry) = self.registry.get() {
            registry.dispose();
        }
    }
}

impl ICodeEditorService for TestCodeEditorService {
    fn get_focused_code_editor(&self) -> Option<CodeEditorHandle> {
        let registry = self.registry.get()?;
        registry.get_focused_code_editor().map(|editor| -> CodeEditorHandle { editor })
    }

    fn open_code_editor(
        &self,
        input: TextResourceEditorInput,
        _source: Option<CodeEditorHandle>,
        _side_by_side: Option<bool>,
    ) -> Promise<Option<CodeEditorHandle>> {
        *self.last_input.borrow_mut() = Some(input);
        Promise::resolved(None)
    }
}
