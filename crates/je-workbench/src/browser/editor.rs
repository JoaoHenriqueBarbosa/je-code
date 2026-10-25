//! Porte de `src/vs/workbench/browser/editor.ts`: utilitários de editor que dependem dos serviços do workbench.
//!
//! Divergências:
//! - **`ServicesAccessor`.** O `accessor.get(IEditorService)` do original é o `ServicesAccessor::get` com o identificador.
//! - **Listener `async`.** O `onDidCloseEditor(async event => ...)` roda sincronamente até o primeiro `await`, como o
//!   `async_fn`; a promessa dele não é observada, como no original.
//! - **`resolve` e `listener`.** O `listener` de `new Promise(resolve => ...)` fica numa célula compartilhada, porque o
//!   fecho precisa descartá-lo de dentro.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::{async_fn, promises};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_instantiation::ServicesAccessor;
use je_platform::uri_identity::uri_identity::I_URI_IDENTITY_SERVICE;

use crate::common::editor::{EditorCloseContext, EditorInputOrUntyped, EditorResourceAccessor, EditorResourceAccessorOptions, SideBySideEditor};
use crate::services::editor::editor_service::I_EDITOR_SERVICE;
use crate::services::working_copy::working_copy_service::I_WORKING_COPY_SERVICE;

/// `getOriginalUri(editor, { supportSideBySide })`.
fn original_uri(editor: Option<&EditorInputOrUntyped>, side: SideBySideEditor) -> Option<URI> {
    EditorResourceAccessor::get_original_uri(editor, Some(&EditorResourceAccessorOptions { support_side_by_side: Some(side), filter_by_scheme: None }))
}

/// Descarta o `listener` guardado na célula, se ainda houver um.
fn dispose_listener(listener: &RefCell<Option<DisposableRef>>) {
    if let Some(listener) = listener.borrow_mut().take() {
        listener.dispose();
    }
}

/// `whenEditorClosed(accessor, resources)`: resolve quando os editores de todos os `resources` foram fechados (e, se algum
/// ainda estiver sujo, depois de salvo).
pub fn when_editor_closed(accessor: &ServicesAccessor, resources: &[URI]) -> Promise<()> {
    let editor_service = accessor.get(I_EDITOR_SERVICE);
    let uri_identity_service = accessor.get(I_URI_IDENTITY_SERVICE);
    let working_copy_service = accessor.get(I_WORKING_COPY_SERVICE);

    let (promise, resolver) = Promise::new();
    let resources: Vec<URI> = resources.to_vec();
    let remaining_resources: Rc<RefCell<Vec<URI>>> = Rc::new(RefCell::new(resources.clone()));
    let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();

    // Observe any editor closing from this moment on
    let handle = {
        let editor_service = editor_service.clone();
        let listener = listener.clone();

        editor_service.clone().on_did_close_editor().listen(move |event| {
            if event.context == EditorCloseContext::Move {
                return; // ignore move events where the editor will open in another group
            }

            let ext_uri = uri_identity_service.ext_uri();
            let closed = EditorInputOrUntyped::Typed(event.editor.clone());
            let mut primary_resource = original_uri(Some(&closed), SideBySideEditor::Primary);
            let mut secondary_resource = original_uri(Some(&closed), SideBySideEditor::Secondary);

            // Specially handle an editor getting replaced: if the new active editor
            // matches any of the resources from the closed editor, ignore those
            // resources because they were actually not closed, but replaced.
            // (see https://github.com/microsoft/vscode/issues/134299)
            if event.context == EditorCloseContext::Replace {
                let active_editor = editor_service.active_editor();
                let new_primary_resource = original_uri(active_editor.as_ref(), SideBySideEditor::Primary);
                let new_secondary_resource = original_uri(active_editor.as_ref(), SideBySideEditor::Secondary);

                if ext_uri.is_equal(primary_resource.as_ref(), new_primary_resource.as_ref(), false) {
                    primary_resource = None;
                }

                if ext_uri.is_equal(secondary_resource.as_ref(), new_secondary_resource.as_ref(), false) {
                    secondary_resource = None;
                }
            }

            // Remove from resources to wait for being closed based on the
            // resources from editors that got closed
            remaining_resources.borrow_mut().retain(|resource| {
                // Closing editor matches resource directly: remove from remaining
                if ext_uri.is_equal(Some(resource), primary_resource.as_ref(), false) || ext_uri.is_equal(Some(resource), secondary_resource.as_ref(), false) {
                    return false;
                }

                // Closing editor is untitled with associated resource
                // that matches resource directly: remove from remaining
                // but only if the editor was not replaced, otherwise
                // saving an untitled with associated resource would
                // release the `--wait` call.
                // (see https://github.com/microsoft/vscode/issues/141237)
                if event.context != EditorCloseContext::Replace {
                    let with_scheme_of_resource =
                        |untitled: &URI| untitled.with(&UriChange { scheme: Field::from(resource.scheme().clone()), ..UriChange::default() });
                    let matches_untitled = |candidate: &Option<URI>| {
                        candidate.as_ref().is_some_and(|candidate| {
                            candidate.scheme().to_string_lossy() == schemas::UNTITLED
                                && ext_uri.is_equal(Some(resource), Some(&with_scheme_of_resource(candidate)), false)
                        })
                    };

                    if matches_untitled(&primary_resource) || matches_untitled(&secondary_resource) {
                        return false;
                    }
                }

                // Editor is not yet closed, so keep it in waiting mode
                true
            });

            // All resources to wait for being closed are closed
            if remaining_resources.borrow().is_empty() {
                let resources = resources.clone();
                let resolver = resolver.clone();
                let listener = listener.clone();
                let working_copy_service = working_copy_service.clone();
                let uri_identity_service = uri_identity_service.clone();

                // If auto save is configured with the default delay (1s) it is possible
                // to close the editor while the save still continues in the background. As such
                // we have to also check if the editors to track for are dirty and if so wait
                // for them to get saved.
                let _ = async_fn(async move {
                    let dirty_resources: Vec<URI> = resources.iter().filter(|resource| working_copy_service.is_dirty(resource, None)).cloned().collect();
                    if !dirty_resources.is_empty() {
                        let saved: Vec<Promise<()>> = dirty_resources
                            .into_iter()
                            .map(|resource| {
                                let (saved, saved_resolver) = Promise::new();
                                if !working_copy_service.is_dirty(&resource, None) {
                                    saved_resolver.resolve(()); // return early if resource is not dirty
                                    return saved;
                                }

                                // Otherwise resolve promise when resource is saved
                                let dirty_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                                let handle = {
                                    let dirty_listener = dirty_listener.clone();
                                    let uri_identity_service = uri_identity_service.clone();
                                    let resource = resource.clone();

                                    working_copy_service.on_did_change_dirty().listen(move |working_copy| {
                                        if !working_copy.is_dirty()
                                            && uri_identity_service.ext_uri().is_equal(Some(&resource), Some(&working_copy.resource()), false)
                                        {
                                            dispose_listener(&dirty_listener);
                                            saved_resolver.resolve(());
                                        }
                                    })
                                };
                                *dirty_listener.borrow_mut() = Some(handle);

                                saved
                            })
                            .collect();

                        promises::settled(saved).await?;
                    }

                    dispose_listener(&listener);
                    resolver.resolve(());

                    Ok(())
                });
            }
        })
    };
    *listener.borrow_mut() = Some(handle);

    promise
}
