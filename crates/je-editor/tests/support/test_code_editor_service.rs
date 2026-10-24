//! `TestCodeEditorService` de `src/vs/editor/test/browser/editorTestServices.ts`, só no que o `OpenerService` usa.
//!
//! O original estende o `AbstractCodeEditorService`, que ainda não foi portado. Aqui só existe o que o
//! `EditorOpener` chama: `openCodeEditor` guarda a entrada em `last_input` e resolve com `null`, e
//! `getFocusedCodeEditor` devolve `None`, que é o que o serviço abstrato devolve quando nenhum editor foi
//! registrado (o caso do teste do opener).

use std::cell::RefCell;

use je_base::promise::Promise;
use je_editor::opener_service::{CodeEditorHandle, ICodeEditorService, TextResourceEditorInput};

#[derive(Default)]
pub struct TestCodeEditorService {
    pub last_input: RefCell<Option<TextResourceEditorInput>>,
}

impl ICodeEditorService for TestCodeEditorService {
    fn get_focused_code_editor(&self) -> Option<CodeEditorHandle> {
        None
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
