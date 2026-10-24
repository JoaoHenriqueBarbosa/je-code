//! Porte de `src/vs/editor/common/services/modelUndoRedoParticipant.ts`: quando um modelo vai ser descartado, os
//! elementos de vários modelos (`MultiModelEditStackElement`) que ele deixa na pilha de desfazer passam a ter este
//! participante como delegado, que carrega os modelos que faltam antes de desfazer ou refazer.
//!
//! Divergências:
//! - O `IModelService` é o [`ModelService`] concreto: o trait `IModelService` do porte é um recorte que não tem o
//!   `onModelRemoved` (que é método próprio do `ModelService`).
//! - O `IUndoRedoService` é o trait [`IUndoRedoService`] e o `ITextModelService` é o recorte do porte
//!   ([`ITextModelService`], só com `createModelReference`).
//! - O `IDisposable` que `createModelReference` devolve é o [`ModelReference`]; o disposable devolvido por
//!   `prepareUndoRedo` o descarta pelo `dispose` dele.

use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, dispose_array, none, to_disposable};
use je_base::promise::{MaybePromise, Promise};

use crate::edit_stack::{MultiModelEditStackElement, UndoRedoDelegate};
use crate::editor_extensions::ITextModelService;
use crate::model_service::ModelService;
use crate::undo_redo::IUndoRedoService;

/// `ModelUndoRedoParticipant`.
pub struct ModelUndoRedoParticipant {
    base: Rc<Disposable>,
    text_model_service: Rc<dyn ITextModelService>,
}

impl ModelUndoRedoParticipant {
    /// `constructor(@IModelService modelService, @ITextModelService textModelService, @IUndoRedoService undoRedoService)`.
    pub fn new(model_service: &ModelService, text_model_service: Rc<dyn ITextModelService>, undo_redo_service: Rc<dyn IUndoRedoService>) -> Rc<Self> {
        Rc::new_cyclic(|this: &Weak<Self>| {
            let base = Disposable::new();
            let participant = this.clone();
            base.register(model_service.on_model_removed().listen(move |model| {
                let Some(participant) = participant.upgrade() else {
                    return;
                };
                // a model will get disposed, so let's check if the undo redo stack is maintained
                let elements = undo_redo_service.get_elements(&model.uri());
                if elements.past.is_empty() && elements.future.is_empty() {
                    return;
                }
                for element in elements.past.iter().chain(elements.future.iter()) {
                    if let Some(multi_model_element) = element.as_any().downcast_ref::<MultiModelEditStackElement>() {
                        multi_model_element.set_delegate(participant.clone());
                    }
                }
            }));
            Self { base, text_model_service }
        })
    }
}

impl UndoRedoDelegate for ModelUndoRedoParticipant {
    fn prepare_undo_redo(&self, element: &MultiModelEditStackElement) -> Option<MaybePromise<DisposableRef>> {
        // Load all the needed text models
        let missing_models = element.get_missing_models();
        if missing_models.is_empty() {
            // All models are available!
            return Some(MaybePromise::Value(none()));
        }

        let disposables_promises: Vec<Promise<DisposableRef>> = missing_models
            .into_iter()
            .map(|uri| {
                let text_model_service = self.text_model_service.clone();
                async_fn(async move {
                    match text_model_service.create_model_reference(&uri).await {
                        Ok(reference) => Ok(to_disposable(move || reference.dispose())),
                        // This model could not be loaded, maybe it was deleted in the meantime?
                        Err(_) => Ok(none()),
                    }
                })
            })
            .collect();

        Some(MaybePromise::Promise(Promise::all(disposables_promises).then(|disposables| {
            to_disposable(move || {
                dispose_array(&disposables);
            })
        })))
    }
}

impl IDisposable for ModelUndoRedoParticipant {
    fn dispose(&self) {
        self.base.dispose();
    }
}
