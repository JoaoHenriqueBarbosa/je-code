//! Porte de `src/vs/workbench/api/common/extHostDocumentsAndEditors.ts`: o conjunto de documentos e editores
//! de texto que o lado das extensões conhece, atualizado pelas mudanças (`IDocumentsAndEditorsDelta`) que a
//! thread principal manda.
//!
//! Divergências:
//! - `$acceptDocumentsAndEditorsDelta` e `acceptDocumentsAndEditorsDelta` fazem a mesma coisa (o primeiro só
//!   repassa ao segundo): a regra do projeto proíbe a função de repasse, e o método é um só, o
//!   `rpc_accept_documents_and_editors_delta` do [`ExtHostDocumentsAndEditorsShape`]. O decorador de serviço
//!   `IExtHostDocumentsAndEditors` (`createDecorator`) não é portado: o serviço se registra pelo mecanismo do
//!   porte quando o contêiner de serviços do extension host existir.
//! - `IExtHostRpcService` é o [`IMainContext`] (guardado, porque o original pede os proxies a cada mudança).
//!   `ILogService` é o [`LogServiceRef`].
//! - Os documentos e editores guardados são `Rc` (o original compartilha os objetos): o [`ExtHostDocumentData`]
//!   que o `ExtHostDocuments` também usa e o [`ExtHostTextEditor`] que os eventos entregam por `value`.
//! - `Reference<T>` é o contador de referências local, com `ref` chamado [`Reference::add_ref`]. O `_count` é
//!   inteiro com sinal, porque `unref` de um contador zerado dá `-1` (diferente de zero) no original.
//! - `_documents` (`ResourceMap`) e `_editors` (`Map`) são o `ResourceMap` e o `OrderedMap` de `je-base`.
//!   `allDocuments()` (um `Iterable`) e `allEditors()` devolvem um `Vec` copiado, e os eventos entregam
//!   `Vec<Rc<ExtHostDocumentData>>` e `Vec<TextEditor>`. `activeEditor()` e `activeEditor(true)` são
//!   [`ExtHostDocumentsAndEditors::active_editor`] e [`ExtHostDocumentsAndEditors::active_editor_internal`].
//! - O `URI.revive(...)` do original já está feito nos DTOs ([`DocumentsAndEditorsDelta`] guarda [`URI`]).
//! - `dispose(removedDocuments)` e `dispose(removedEditors)` usam `lifecycle::dispose_all`: o documento entra
//!   por um adaptador, porque o [`ExtHostDocumentData`] tem o `dispose` como método próprio (e não é um
//!   `IDisposable`). Como no original, um erro ao descartar impede os eventos.
//! - `assert.ok(cond, message)` é `ok(&cond, Some(message))`, que lança (pânico com [`Error`]); o `throw new
//!   Error(...)` do documento repetido é `errors::throw`.
//! - O `_onDidChange...` emitters não são descartados: o original não os registra em nada.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::assert::ok;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lazy::Lazy;
use je_base::lifecycle::{IDisposable, dispose_all};
use je_base::map::{OrderedMap, ResourceMap};
use je_base::network::schemas;
use je_base::uri::URI;
use je_platform::log::LogServiceRef;

use super::ext_host_document_data::{ExtHostDocumentData, IExtHostDocumentSaveDelegate};
use super::ext_host_protocol::{DocumentsAndEditorsDelta, ExtHostDocumentsAndEditorsShape, IMainContext, MainContext};
use super::ext_host_text_editor::{ExtHostTextEditor, TextEditor};
use super::ext_host_type_converters as type_converters;

/// `Reference<T>`: o valor e quantas vezes foi referenciado.
struct Reference<T> {
    count: Cell<i64>,
    value: Rc<T>,
}

impl<T> Reference<T> {
    fn new(value: Rc<T>) -> Self {
        Self { count: Cell::new(0), value }
    }

    /// `ref()`.
    fn add_ref(&self) {
        self.count.set(self.count.get() + 1);
    }

    /// `unref()`: `true` quando ninguém mais referencia o valor.
    fn unref(&self) -> bool {
        self.count.set(self.count.get() - 1);
        self.count.get() == 0
    }
}

/// O `dispose(removedDocuments)` do original trata o documento como `IDisposable`.
struct DocumentDisposable(Rc<ExtHostDocumentData>);

impl IDisposable for DocumentDisposable {
    fn dispose(&self) {
        self.0.dispose();
    }
}

/// `ExtHostDocumentsAndEditors`.
pub struct ExtHostDocumentsAndEditors {
    ext_host_rpc: Rc<dyn IMainContext>,
    log_service: LogServiceRef,

    active_editor_id: RefCell<Option<String>>,

    editors: RefCell<OrderedMap<String, Rc<ExtHostTextEditor>>>,
    documents: RefCell<ResourceMap<Reference<ExtHostDocumentData>>>,

    on_did_add_documents: Rc<Emitter<Vec<Rc<ExtHostDocumentData>>>>,
    on_did_remove_documents: Rc<Emitter<Vec<Rc<ExtHostDocumentData>>>>,
    on_did_change_visible_text_editors: Rc<Emitter<Vec<TextEditor>>>,
    on_did_change_active_text_editor: Rc<Emitter<Option<TextEditor>>>,
}

impl ExtHostDocumentsAndEditors {
    pub fn new(ext_host_rpc: Rc<dyn IMainContext>, log_service: LogServiceRef) -> Self {
        Self {
            ext_host_rpc,
            log_service,
            active_editor_id: RefCell::new(None),
            editors: RefCell::new(OrderedMap::new()),
            documents: RefCell::new(ResourceMap::new()),
            on_did_add_documents: Emitter::new(),
            on_did_remove_documents: Emitter::new(),
            on_did_change_visible_text_editors: Emitter::new(),
            on_did_change_active_text_editor: Emitter::new(),
        }
    }

    /// `onDidAddDocuments`.
    pub fn on_did_add_documents(&self) -> Event<Vec<Rc<ExtHostDocumentData>>> {
        self.on_did_add_documents.event()
    }

    /// `onDidRemoveDocuments`.
    pub fn on_did_remove_documents(&self) -> Event<Vec<Rc<ExtHostDocumentData>>> {
        self.on_did_remove_documents.event()
    }

    /// `onDidChangeVisibleTextEditors`.
    pub fn on_did_change_visible_text_editors(&self) -> Event<Vec<TextEditor>> {
        self.on_did_change_visible_text_editors.event()
    }

    /// `onDidChangeActiveTextEditor`.
    pub fn on_did_change_active_text_editor(&self) -> Event<Option<TextEditor>> {
        self.on_did_change_active_text_editor.event()
    }

    pub fn get_document(&self, uri: &URI) -> Option<Rc<ExtHostDocumentData>> {
        self.documents.borrow().get(uri).map(|reference| reference.value.clone())
    }

    pub fn all_documents(&self) -> Vec<Rc<ExtHostDocumentData>> {
        self.documents.borrow().values().map(|reference| reference.value.clone()).collect()
    }

    pub fn get_editor(&self, id: &str) -> Option<Rc<ExtHostTextEditor>> {
        self.editors.borrow().get(&id.to_owned()).cloned()
    }

    /// `activeEditor(true)`: o editor interno.
    pub fn active_editor_internal(&self) -> Option<Rc<ExtHostTextEditor>> {
        let active_editor_id = self.active_editor_id.borrow().clone().filter(|id| !id.is_empty())?;
        self.get_editor(&active_editor_id)
    }

    /// `activeEditor()`: o `vscode.TextEditor` do editor ativo.
    pub fn active_editor(&self) -> Option<TextEditor> {
        self.active_editor_internal().map(|editor| editor.value())
    }

    pub fn all_editors(&self) -> Vec<Rc<ExtHostTextEditor>> {
        self.editors.borrow().values().cloned().collect()
    }
}

impl ExtHostDocumentsAndEditorsShape for ExtHostDocumentsAndEditors {
    fn rpc_accept_documents_and_editors_delta(&self, delta: DocumentsAndEditorsDelta) {
        let DocumentsAndEditorsDelta {
            removed_documents: removed_uris,
            added_documents: added_data,
            removed_editors: removed_ids,
            added_editors: added_editor_data,
            new_active_editor,
        } = delta;
        let has_removed_documents = removed_uris.is_some();
        let has_added_documents = added_data.is_some();
        let has_editors_delta = removed_ids.is_some() || added_editor_data.is_some();

        let mut removed_documents: Vec<Rc<ExtHostDocumentData>> = Vec::new();
        let mut added_documents: Vec<Rc<ExtHostDocumentData>> = Vec::new();
        let mut removed_editors: Vec<Rc<ExtHostTextEditor>> = Vec::new();

        for uri in removed_uris.iter().flatten() {
            let mut documents = self.documents.borrow_mut();
            let data = documents.get(uri).filter(|data| data.unref()).map(|data| data.value.clone());
            if let Some(data) = data {
                documents.delete(uri);
                removed_documents.push(data);
            }
        }

        for data in added_data.into_iter().flatten() {
            let resource = data.uri.clone();
            let mut documents = self.documents.borrow_mut();
            let exists = documents.has(&resource);

            // double check -> only notebook cell documents should be
            // referenced/opened more than once...
            if exists && resource.scheme() != schemas::VSCODE_NOTEBOOK_CELL && resource.scheme() != schemas::VSCODE_INTERACTIVE_INPUT {
                drop(documents);
                errors::throw(Error::new(format!("document '{} already exists!'", resource.to_string(false))));
            }
            if !exists {
                let proxy: Rc<dyn IExtHostDocumentSaveDelegate> = self.ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_DOCUMENTS);
                let value = Rc::new(ExtHostDocumentData::new(
                    proxy,
                    resource.clone(),
                    data.lines,
                    data.eol,
                    data.version_id,
                    &data.language_id,
                    data.is_dirty,
                    &data.encoding,
                ));
                documents.set(resource.clone(), Reference::new(value.clone()));
                added_documents.push(value);
            }

            documents.get(&resource).expect("o documento foi guardado acima").add_ref();
        }

        for id in removed_ids.into_iter().flatten() {
            let editor = self.editors.borrow_mut().remove(&id);
            if let Some(editor) = editor {
                removed_editors.push(editor);
            }
        }

        for data in added_editor_data.into_iter().flatten() {
            let resource = data.document_uri.clone();
            let has_document = self.documents.borrow().has(&resource);
            ok(&has_document, Some(&format!("document '{}' does not exist", resource.to_string(false))));
            let has_editor = self.editors.borrow().contains_key(&data.id);
            ok(&!has_editor, Some(&format!("editor '{}' already exists!", data.id)));

            let document_data = self.documents.borrow().get(&resource).expect("conferido acima").value.clone();
            let editor = ExtHostTextEditor::new(
                &data.id,
                self.ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_TEXT_EDITORS),
                self.log_service.clone(),
                Lazy::new(move || document_data.document()),
                data.selections.iter().map(|selection| type_converters::Selection::to(selection).unwrap_or_else(|error| errors::throw(error))).collect(),
                &data.options,
                data.visible_ranges.iter().map(|range| type_converters::Range::to(range).unwrap_or_else(|error| errors::throw(error))).collect(),
                data.editor_position.map(type_converters::ViewColumn::to),
            );
            self.editors.borrow_mut().insert(data.id.clone(), Rc::new(editor));
        }

        if let Some(new_active_editor) = &new_active_editor {
            let exists = new_active_editor.as_ref().is_none_or(|id| self.editors.borrow().contains_key(id));
            ok(&exists, Some(&format!("active editor '{}' does not exist", new_active_editor.as_deref().unwrap_or("null"))));
            *self.active_editor_id.borrow_mut() = new_active_editor.clone();
        }

        dispose_all(&removed_documents.iter().cloned().map(DocumentDisposable).collect::<Vec<_>>());
        dispose_all(&removed_editors);

        // now that the internal state is complete, fire events
        if has_removed_documents {
            self.on_did_remove_documents.fire(removed_documents);
        }
        if has_added_documents {
            self.on_did_add_documents.fire(added_documents);
        }

        if has_editors_delta {
            self.on_did_change_visible_text_editors.fire(self.all_editors().iter().map(|editor| editor.value()).collect());
        }
        if new_active_editor.is_some() {
            self.on_did_change_active_text_editor.fire(self.active_editor());
        }
    }
}
