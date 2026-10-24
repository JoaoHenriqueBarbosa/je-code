//! Porte do registro de editores do `AbstractCodeEditorService`
//! (`src/vs/editor/browser/services/abstractCodeEditorService.ts`): `willCreateCodeEditor`, `addCodeEditor`,
//! `removeCodeEditor`, `listCodeEditors`, `getFocusedCodeEditor` e os eventos `onWillCreateCodeEditor`,
//! `onCodeEditorAdd` e `onCodeEditorRemove`. A parte de decorações do mesmo serviço está em
//! [`DecorationRenderService`](crate::services::decoration_render_options::DecorationRenderService), que o registro
//! carrega e expõe em [`CodeEditorRegistry::decoration_service`].
//!
//! Divergências:
//! - O `ICodeEditorService` do porte (`opener_service.rs`) é o recorte que o `EditorOpener` usa e não muda. O que o
//!   `CodeEditorWidget` chama no serviço (`willCreateCodeEditor`, `addCodeEditor`, `removeCodeEditor`) é o trait
//!   [`ICodeEditorRegistry`], que o widget recebe em `CodeEditorWidgetServices::code_editor_service`.
//! - O editor registrado é o [`CodeEditorWidget`] concreto (o `ICodeEditor` do original): o porte não tem outra
//!   implementação (o editor de diff não existe), então os eventos e a lista trazem `Rc<CodeEditorWidget>`.
//! - `willCreateDiffEditor`, `addDiffEditor`, `removeDiffEditor`, `listDiffEditors` e os eventos de editor de diff
//!   ficam de fora (sem `IDiffEditor`).
//! - `removeCodeEditor` do original faz `delete this._codeEditors[editor.getId()]`, que devolve `true` mesmo quando a
//!   chave não existe, então o evento `onCodeEditorRemove` dispara com o editor recebido. O porte recebe o `getId()`
//!   (o editor já está sendo descartado) e dispara com o editor que estava registrado; sem registro não há editor
//!   para o evento carregar e ele não dispara (o `CodeEditorWidget` só remove o que adicionou).
//! - A ordem de `listCodeEditors` (`Object.keys`) é a de inserção, que o [`OrderedMap`] preserva.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::OrderedMap;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::services::decoration_render_options::DecorationRenderService;

/// O que o `CodeEditorWidget` chama no `ICodeEditorService` ao nascer e ao morrer.
pub trait ICodeEditorRegistry {
    /// `willCreateCodeEditor()`.
    fn will_create_code_editor(&self);

    /// `addCodeEditor(editor)`.
    fn add_code_editor(&self, editor: Rc<CodeEditorWidget>);

    /// `removeCodeEditor(editor)`, pelo `getId()` do editor.
    fn remove_code_editor(&self, editor_id: &str);

    /// A parte de decorações do serviço (`registerDecorationType`, `removeDecorationType`,
    /// `resolveDecorationOptions`), que o `setDecorationsByType` do editor usa.
    fn decoration_service(&self) -> Rc<DecorationRenderService>;
}

/// O registro de editores do `AbstractCodeEditorService`.
pub struct CodeEditorRegistry {
    base: Rc<Disposable>,
    on_will_create_code_editor: Rc<Emitter<()>>,
    on_code_editor_add: Rc<Emitter<Rc<CodeEditorWidget>>>,
    on_code_editor_remove: Rc<Emitter<Rc<CodeEditorWidget>>>,
    code_editors: RefCell<OrderedMap<String, Rc<CodeEditorWidget>>>,
    decoration_service: Rc<DecorationRenderService>,
}

impl CodeEditorRegistry {
    pub fn new(decoration_service: Rc<DecorationRenderService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_will_create_code_editor: Rc<Emitter<()>> = Emitter::new();
        base.register(on_will_create_code_editor.clone());
        let on_code_editor_add: Rc<Emitter<Rc<CodeEditorWidget>>> = Emitter::new();
        base.register(on_code_editor_add.clone());
        let on_code_editor_remove: Rc<Emitter<Rc<CodeEditorWidget>>> = Emitter::new();
        base.register(on_code_editor_remove.clone());
        base.register(decoration_service.clone());
        Rc::new(Self {
            base,
            on_will_create_code_editor,
            on_code_editor_add,
            on_code_editor_remove,
            code_editors: RefCell::new(OrderedMap::new()),
            decoration_service,
        })
    }

    /// `onWillCreateCodeEditor`.
    pub fn on_will_create_code_editor(&self) -> Event<()> {
        self.on_will_create_code_editor.event()
    }

    /// `onCodeEditorAdd`.
    pub fn on_code_editor_add(&self) -> Event<Rc<CodeEditorWidget>> {
        self.on_code_editor_add.event()
    }

    /// `onCodeEditorRemove`.
    pub fn on_code_editor_remove(&self) -> Event<Rc<CodeEditorWidget>> {
        self.on_code_editor_remove.event()
    }

    /// `listCodeEditors()`.
    pub fn list_code_editors(&self) -> Vec<Rc<CodeEditorWidget>> {
        self.code_editors.borrow().values().cloned().collect()
    }

    /// `getFocusedCodeEditor()`: o editor com o foco do texto, ou o último com o foco do widget.
    pub fn get_focused_code_editor(&self) -> Option<Rc<CodeEditorWidget>> {
        let mut editor_with_widget_focus: Option<Rc<CodeEditorWidget>> = None;

        for editor in self.list_code_editors() {
            if ICodeEditor::has_text_focus(&*editor) {
                // bingo!
                return Some(editor);
            }

            if ICodeEditor::has_widget_focus(&*editor) {
                editor_with_widget_focus = Some(editor);
            }
        }

        editor_with_widget_focus
    }

    /// A parte de decorações do serviço (`registerDecorationType`, `removeDecorationType`,
    /// `resolveDecorationOptions`).
    pub fn decoration_service(&self) -> &Rc<DecorationRenderService> {
        &self.decoration_service
    }
}

impl ICodeEditorRegistry for CodeEditorRegistry {
    fn will_create_code_editor(&self) {
        self.on_will_create_code_editor.fire(());
    }

    fn add_code_editor(&self, editor: Rc<CodeEditorWidget>) {
        let id = ICodeEditor::get_id(&*editor);
        self.code_editors.borrow_mut().insert(id, editor.clone());
        self.on_code_editor_add.fire(editor);
    }

    fn remove_code_editor(&self, editor_id: &str) {
        let removed = self.code_editors.borrow_mut().remove(&editor_id.to_string());
        // `delete obj[key]` é `true` mesmo sem a chave: o evento dispara com o editor que estava registrado.
        if let Some(editor) = removed {
            self.on_code_editor_remove.fire(editor);
        }
    }

    fn decoration_service(&self) -> Rc<DecorationRenderService> {
        self.decoration_service.clone()
    }
}

impl IDisposable for CodeEditorRegistry {
    fn dispose(&self) {
        self.base.dispose();
    }
}
