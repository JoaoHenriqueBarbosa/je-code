//! Porte de `src/vs/workbench/contrib/bulkEdit/browser/conflicts.ts`: o `ConflictDetector`, que marca os recursos de uma
//! edição em massa que mudaram (no disco ou no modelo) depois que a edição foi montada.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados são o `Deps` do [`Injectable`] e `edits` é o `Args`.
//! - **`onDidConflict: Event<this>`.** O evento não carrega o detector (ele seria um `Rc` que se contém): é um `Event<()>`,
//!   e quem escuta lê o detector que já tem.
//! - **`ResourceNotebookCellEdit` e `ResourceAttachmentEdit`.** As duas chegam como [`ResourceEdit::Custom`] e não
//!   se distinguem aqui; como a edição de célula (o `BulkCellEdits`) não foi portada, a variante cai no ramo `UNKNOWN edit
//!   type`, o mesmo que o `ResourceAttachmentEdit` percorre no original.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceEdit;
use je_editor::services_model::IModelService;
use je_instantiation::Injectable;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogService, Logger};

/// `ConflictDetector`.
pub struct ConflictDetector {
    conflicts: Rc<RefCell<ResourceMap<bool>>>,
    disposables: Rc<DisposableStore>,
    on_did_conflict: Rc<Emitter<()>>,
}

impl ConflictDetector {
    /// `onDidConflict`.
    pub fn on_did_conflict(&self) -> Event<()> {
        self.on_did_conflict.event()
    }

    /// `list()`.
    pub fn list(&self) -> Vec<URI> {
        self.conflicts.borrow().keys().cloned().collect()
    }

    /// `hasConflicts()`.
    pub fn has_conflicts(&self) -> bool {
        self.conflicts.borrow().size() > 0
    }
}

impl Injectable for ConflictDetector {
    type Args = Vec<ResourceEdit>;
    type Deps = (Rc<dyn IFileService>, Rc<dyn IModelService>, Rc<dyn LogService>);

    fn create(edits: Self::Args, (file_service, model_service, log_service): Self::Deps) -> Rc<Self> {
        let conflicts: Rc<RefCell<ResourceMap<bool>>> = Rc::new(RefCell::new(ResourceMap::new()));
        let disposables = DisposableStore::new();
        let on_did_conflict: Rc<Emitter<()>> = Emitter::new();

        let mut workspace_edit_resources: ResourceMap<bool> = ResourceMap::new();

        for edit in &edits {
            match edit {
                ResourceEdit::Text(edit) => {
                    workspace_edit_resources.set(edit.resource.clone(), true);
                    if let Some(version_id) = edit.version_id
                        && model_service.get_model(&edit.resource).is_some_and(|model| model.get_version_id() != version_id)
                    {
                        conflicts.borrow_mut().set(edit.resource.clone(), true);
                        on_did_conflict.fire(());
                    }
                }
                ResourceEdit::File(edit) => {
                    if let Some(new_resource) = &edit.new_resource {
                        workspace_edit_resources.set(new_resource.clone(), true);
                    } else if let Some(old_resource) = &edit.old_resource {
                        workspace_edit_resources.set(old_resource.clone(), true);
                    }
                }
                ResourceEdit::Custom(_) => log_service.warn("UNKNOWN edit type"),
            }
        }

        let workspace_edit_resources = Rc::new(workspace_edit_resources);

        // listen to file changes
        disposables.add(file_service.on_did_files_change().listen({
            let (conflicts, on_did_conflict, model_service) = (conflicts.clone(), on_did_conflict.clone(), model_service.clone());
            let workspace_edit_resources = workspace_edit_resources.clone();
            move |event| {
                for uri in workspace_edit_resources.keys() {
                    // conflict happens when a file that we are working
                    // on changes on disk. ignore changes for which a model
                    // exists because we have a better check for models
                    if model_service.get_model(uri).is_none() && event.contains(uri, &[]) {
                        conflicts.borrow_mut().set(uri.clone(), true);
                        on_did_conflict.fire(());
                        break;
                    }
                }
            }
        }));

        // listen to model changes...?
        for model in model_service.get_models() {
            let uri = model.uri();
            disposables.add(model.on_did_change_content().listen({
                let (conflicts, on_did_conflict, workspace_edit_resources) = (conflicts.clone(), on_did_conflict.clone(), workspace_edit_resources.clone());
                move |_| {
                    // conflict
                    if workspace_edit_resources.has(&uri) {
                        conflicts.borrow_mut().set(uri.clone(), true);
                        on_did_conflict.fire(());
                    }
                }
            }));
        }

        Rc::new(Self { conflicts, disposables, on_did_conflict })
    }
}

impl IDisposable for ConflictDetector {
    fn dispose(&self) {
        self.disposables.dispose();
        self.on_did_conflict.dispose();
    }
}
