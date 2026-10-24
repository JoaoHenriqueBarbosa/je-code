//! Dublês dos testes do multicursor: o `FindReplaceState` e o `CommonFindController` (`contrib/find`), que ainda não
//! foram portados, só com o que o `multicursor.test.ts` observa.
//!
//! - [`FindReplaceState`] (agora em `je_editor::multicursor_find_state`, reexportado aqui) é o porte de `findState.ts` sem o que o teste não toca (`changeMatchInfo`, `matchesPosition`,
//!   `matchesCount`, `currentMatch`, `isSearching`/`filters` como dado de busca, `canNavigate*`): os campos, os
//!   `Override`, o `change` com o cálculo do evento e o `onFindReplaceStateChange`, linha a linha. Os acessos `actual...`
//!   (o valor guardado, sem o `Override`) são métodos próprios.
//! - [`TestFindController`] é o `CommonFindController` reduzido ao `getState` e ao `highlightFindOptions`. O
//!   `sinon.spy(findController, 'highlightFindOptions')` é a contagem de chamadas, zerada por
//!   [`TestFindController::spy_on_highlight_find_options`] e lida por
//!   [`TestFindController::highlight_find_options_call_count`]. O efeito visual (decorar as opções) e a reação do
//!   `FindModelBoundToEditorModel` ao estado (que só troca decorações e atualiza `matchesCount`) não alteram nada que o
//!   teste observe.
//! - [`TestFindControllerSource`] é o `CommonFindController.get(editor)` (a contribuição do editor, por tipo) e o
//!   `createInstance(CommonFindController, editor)`.
//! - [`NoDocumentHighlightProviders`] é o `ILanguageFeaturesService.documentHighlightProvider.has(model)` do
//!   `createCodeEditorServices` do teste, que não registra provedor algum.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor, IEditorContribution};
use je_editor::multicursor::{
    FindOptionOverride, FindReplaceStateChangedEvent, ICommonFindController, ICommonFindControllerSource,
    IDocumentHighlightProviderRegistry, IFindReplaceState, INewFindReplaceState,
};
use je_editor::range::Range;
use je_editor::text_model::TextModel;

pub use je_editor::multicursor_find_state::FindReplaceState;

/// O `CommonFindController` (`CommonFindController.ID`).
pub struct TestFindController {
    me: Weak<TestFindController>,
    state: Rc<FindReplaceState>,
    highlight_find_options_calls: Cell<usize>,
}

impl TestFindController {
    pub const ID: &'static str = "editor.contrib.findController";

    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|me| Self { me: me.clone(), state: FindReplaceState::new(), highlight_find_options_calls: Cell::new(0) })
    }

    /// O `FindReplaceState` concreto (`getState()`), com os acessos `actual...`.
    pub fn find_state(&self) -> &Rc<FindReplaceState> {
        &self.state
    }

    /// `sinon.spy(findController, 'highlightFindOptions')`: a contagem recomeça do zero.
    pub fn spy_on_highlight_find_options(&self) {
        self.highlight_find_options_calls.set(0);
    }

    /// `highlightFindOptions.callCount`.
    pub fn highlight_find_options_call_count(&self) -> usize {
        self.highlight_find_options_calls.get()
    }
}

impl ICommonFindController for TestFindController {
    fn get_state(&self) -> Rc<dyn IFindReplaceState> {
        self.state.clone()
    }

    fn highlight_find_options(&self, _ignore_when_visible: bool) {
        self.highlight_find_options_calls.set(self.highlight_find_options_calls.get() + 1);
    }
}

impl IDisposable for TestFindController {
    fn dispose(&self) {
        self.state.dispose();
    }
}

impl IEditorContribution for TestFindController {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// `CommonFindController.get(editor)` e `createInstance(CommonFindController, editor)`.
pub struct TestFindControllerSource;

impl ICommonFindControllerSource for TestFindControllerSource {
    fn get(&self, editor: &CodeEditorWidget) -> Option<Rc<dyn ICommonFindController>> {
        let contribution = editor.get_contribution(TestFindController::ID)?;
        let controller: Rc<dyn ICommonFindController> =
            contribution.as_any()?.downcast_ref::<TestFindController>()?.me.upgrade()?;
        Some(controller)
    }

    fn create_instance(&self, _editor: &Rc<CodeEditorWidget>) -> Rc<dyn ICommonFindController> {
        TestFindController::new()
    }
}

/// `ILanguageFeaturesService.documentHighlightProvider` sem provedor registrado.
pub struct NoDocumentHighlightProviders;

impl IDocumentHighlightProviderRegistry for NoDocumentHighlightProviders {
    fn has(&self, _model: &TextModel) -> bool {
        false
    }
}
