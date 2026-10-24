//! Porte de `src/vs/workbench/services/editor/common/editorPaneService.ts` (o contrato `IEditorPaneService`) e de
//! `src/vs/workbench/services/editor/browser/editorPaneService.ts` (o [`EditorPaneService`]): o serviço que avisa quando um
//! painel de editor está para ser instanciado e diz se um painel de certo tipo já foi instanciado.
//!
//! Divergências:
//! - **Estado estático.** O `EditorPaneService` do original só repassa o `EditorPaneDescriptor.onWillInstantiateEditorPane` e o
//!   `EditorPaneDescriptor.didInstantiateEditorPane`, que são estáticos da classe; aqui o estado estático é um `thread_local`
//!   de [`editor_panes`](crate::browser::parts::editor::editor_panes), e o serviço repassa a ele.
//! - **Instanciação atrasada.** `registerSingleton(IEditorPaneService, EditorPaneService, InstantiationType.Delayed)` é
//!   [`register`], e o proxy da instanciação atrasada é o [`EditorPaneServiceProxy`].

use std::rc::Rc;

use je_base::event::Event;
use je_instantiation::{Injectable, InstantiationType, delayed_proxy, register_singleton, service_id, service_impl};

use crate::browser::parts::editor::editor_panes::EditorPaneDescriptor;
use crate::common::editor::WillInstantiateEditorPaneEvent;

/// `IEditorPaneService`.
pub trait IEditorPaneService {
    /// `onWillInstantiateEditorPane`: emitted when an editor pane is about to be instantiated.
    fn on_will_instantiate_editor_pane(&self) -> Event<WillInstantiateEditorPaneEvent>;

    /// `didInstantiateEditorPane(typeId)`: returns whether a editor pane with the given type id has been instantiated.
    fn did_instantiate_editor_pane(&self, type_id: &str) -> bool;
}

delayed_proxy!(pub EditorPaneServiceProxy for dyn IEditorPaneService {
    methods {
        fn did_instantiate_editor_pane(&self, type_id: &str) -> bool;
    }
    events {
        fn on_will_instantiate_editor_pane(&self) -> WillInstantiateEditorPaneEvent;
    }
});

service_id!(pub I_EDITOR_PANE_SERVICE: dyn IEditorPaneService = "editorPaneService", proxy EditorPaneServiceProxy);

/// `EditorPaneService`.
pub struct EditorPaneService;

impl Injectable for EditorPaneService {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

service_impl!(EditorPaneService => dyn IEditorPaneService);

/// `registerSingleton(IEditorPaneService, EditorPaneService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<EditorPaneService>(I_EDITOR_PANE_SERVICE, InstantiationType::Delayed);
}

impl IEditorPaneService for EditorPaneService {
    fn on_will_instantiate_editor_pane(&self) -> Event<WillInstantiateEditorPaneEvent> {
        EditorPaneDescriptor::on_will_instantiate_editor_pane()
    }

    fn did_instantiate_editor_pane(&self, type_id: &str) -> bool {
        EditorPaneDescriptor::did_instantiate_editor_pane(type_id)
    }
}
