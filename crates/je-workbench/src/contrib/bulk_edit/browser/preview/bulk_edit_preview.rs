//! Porte de `src/vs/workbench/contrib/bulkEdit/browser/preview/bulkEditPreview.ts`: o modelo da pré-visualização de uma
//! edição em massa (`CheckedStates`, `BulkFileOperation`, `BulkCategory` e `BulkFileOperations`).
//!
//! Divergências:
//! - **Identidade das edições.** O `CheckedStates<T extends object>` do original guarda o estado numa `WeakMap` pela
//!   identidade do objeto da edição. As edições do porte são valores (`ResourceEdit` é um enum clonável), então a chave é o
//!   índice da edição na lista original (`usize`), o mesmo número que o original guarda em `originalEdits`. Por isso
//!   `checked.isChecked(edits[0])` é `checked.is_checked(&0)`, e o `onDidChange` carrega o índice.
//! - **`getWorkspaceEdit`.** O original devolve o próprio array quando tudo está marcado (`edits === ops.getWorkspaceEdit()`);
//!   aqui a lista é um `Rc<Vec<ResourceEdit>>` e a identidade é `Rc::ptr_eq`.
//! - **Referências ao pai.** `BulkTextEdit.parent` e `BulkFileOperation.parent` apontariam para o dono e fechariam um
//!   ciclo de `Rc`. A operação guarda o `Rc<CheckedStates>` de que o `needsConfirmation` precisa, e o `BulkTextEdit` guarda o
//!   URI da operação a que pertence.
//! - **Coleções.** Os `Map` do original que dependem da ordem de inserção são `Vec` de pares.
//! - **Construtor.** `BulkFileOperations.create(accessor, edits)` cria a instância pelo serviço de instanciação e chama o
//!   `_init` (`init`), que devolve a promessa da própria instância.
//! - **`BulkEditPreviewProvider`.** Fica fora: é a ponte do `ITextModelService` com a vista `BulkEditPane`, que não foi
//!   portada (ver o módulo pai).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::collator::locale_compare;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::bulk_edit_service::{ResourceEdit, ResourceFileEdit, ResourceTextEdit};
use je_editor::code_action::languages::{IconPath, WorkspaceEditMetadata};
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::range::Range;
use je_editor::snippet::snippet_parser::SnippetParser;
use je_instantiation::{I_INSTANTIATION_SERVICE, Injectable, InstantiationService, ServicesAccessor};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::ext_uri;

use crate::contrib::bulk_edit::browser::conflicts::ConflictDetector;

/// `CheckedStates<T>`, com o índice da edição no lugar do objeto.
pub struct CheckedStates<K: Eq + Hash + Clone + 'static> {
    states: RefCell<HashMap<K, bool>>,
    checked_count: Cell<usize>,
    on_did_change: Rc<Emitter<K>>,
}

impl<K: Eq + Hash + Clone + 'static> CheckedStates<K> {
    pub fn new() -> Self {
        Self { states: RefCell::new(HashMap::new()), checked_count: Cell::new(0), on_did_change: Emitter::new() }
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<K> {
        self.on_did_change.event()
    }

    /// `checkedCount`.
    pub fn checked_count(&self) -> usize {
        self.checked_count.get()
    }

    /// `isChecked(obj)`.
    pub fn is_checked(&self, obj: &K) -> bool {
        self.states.borrow().get(obj).copied().unwrap_or(false)
    }

    /// `updateChecked(obj, value)`.
    pub fn update_checked(&self, obj: &K, value: bool) {
        let value_now = self.states.borrow().get(obj).copied();
        if value_now == Some(value) {
            return;
        }
        match value_now {
            None => {
                if value {
                    self.checked_count.set(self.checked_count.get() + 1);
                }
            }
            Some(_) => {
                if value {
                    self.checked_count.set(self.checked_count.get() + 1);
                } else {
                    self.checked_count.set(self.checked_count.get() - 1);
                }
            }
        }
        self.states.borrow_mut().insert(obj.clone(), value);
        self.on_did_change.fire(obj.clone());
    }
}

impl<K: Eq + Hash + Clone + 'static> Default for CheckedStates<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash + Clone + 'static> IDisposable for CheckedStates<K> {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}

/// `BulkTextEdit`.
#[derive(Clone)]
pub struct BulkTextEdit {
    /// O URI da `BulkFileOperation` dona da edição (o `parent` do original).
    pub parent: URI,
    pub text_edit: ResourceTextEdit,
}

/// `BulkFileOperationType` (máscara de bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkFileOperationType;

impl BulkFileOperationType {
    pub const TEXT_EDIT: u8 = 1;
    pub const CREATE: u8 = 2;
    pub const DELETE: u8 = 4;
    pub const RENAME: u8 = 8;
}

/// `BulkFileOperation`.
pub struct BulkFileOperation {
    pub uri: URI,
    checked: Rc<CheckedStates<usize>>,
    pub r#type: Cell<u8>,
    pub text_edits: RefCell<Vec<BulkTextEdit>>,
    /// `originalEdits`: índice da edição e a edição, em ordem de índice.
    pub original_edits: RefCell<BTreeMap<usize, ResourceEdit>>,
    pub new_uri: RefCell<Option<URI>>,
}

impl BulkFileOperation {
    fn new(uri: URI, checked: Rc<CheckedStates<usize>>) -> Self {
        Self { uri, checked, r#type: Cell::new(0), text_edits: RefCell::new(Vec::new()), original_edits: RefCell::new(BTreeMap::new()), new_uri: RefCell::new(None) }
    }

    /// `addEdit(index, type, edit)`.
    pub fn add_edit(&self, index: usize, r#type: u8, edit: &ResourceEdit) {
        self.r#type.set(self.r#type.get() | r#type);
        self.original_edits.borrow_mut().insert(index, edit.clone());
        match edit {
            ResourceEdit::Text(text_edit) => {
                self.text_edits.borrow_mut().push(BulkTextEdit { parent: self.uri.clone(), text_edit: text_edit.clone() });
            }
            ResourceEdit::File(file_edit) if r#type == BulkFileOperationType::RENAME => {
                *self.new_uri.borrow_mut() = file_edit.new_resource.clone();
            }
            _ => {}
        }
    }

    /// `needsConfirmation()`.
    pub fn needs_confirmation(&self) -> bool {
        self.original_edits.borrow().keys().any(|index| !self.checked.is_checked(index))
    }
}

/// `BulkCategory`.
pub struct BulkCategory {
    pub metadata: WorkspaceEditMetadata,
    /// `operationByResource`: chave de comparação do URI e a operação, em ordem de inserção.
    operation_by_resource: RefCell<Vec<(String, Rc<BulkFileOperation>)>>,
}

impl BulkCategory {
    /// `_defaultMetadata`.
    fn default_metadata() -> WorkspaceEditMetadata {
        WorkspaceEditMetadata {
            label: localize("default", "Other", &[]),
            icon_path: Some(IconPath::ThemeIcon(ThemeIcon::from_id("symbol-file"))),
            needs_confirmation: false,
            description: None,
        }
    }

    /// `keyOf(metadata)`.
    pub fn key_of(metadata: Option<&WorkspaceEditMetadata>) -> String {
        match metadata {
            Some(metadata) if !metadata.label.is_empty() => metadata.label.clone(),
            _ => "<default>".to_string(),
        }
    }

    fn new(metadata: Option<WorkspaceEditMetadata>) -> Self {
        Self { metadata: metadata.unwrap_or_else(Self::default_metadata), operation_by_resource: RefCell::new(Vec::new()) }
    }

    /// `fileOperations`.
    pub fn file_operations(&self) -> Vec<Rc<BulkFileOperation>> {
        self.operation_by_resource.borrow().iter().map(|(_, operation)| operation.clone()).collect()
    }
}

/// O `insert` do `_init`: acha a operação pela chave do URI (ou pelo URI antigo de um renomear) ou cria uma nova.
fn insert_operation(
    uri: &URI,
    map: &mut Vec<(String, Rc<BulkFileOperation>)>,
    new_to_old_uri: &[(String, URI)],
    checked: &Rc<CheckedStates<usize>>,
) -> Rc<BulkFileOperation> {
    let ext = ext_uri();
    let mut uri = uri.clone();
    let mut key = ext.get_comparison_key(&uri, true).to_string_lossy();
    let mut operation = map.iter().find(|(k, _)| *k == key).map(|(_, operation)| operation.clone());

    // rename
    if operation.is_none()
        && let Some((_, old)) = new_to_old_uri.iter().find(|(k, _)| *k == ext.get_comparison_key(&uri, false).to_string_lossy())
    {
        uri = old.clone();
        key = ext.get_comparison_key(&uri, true).to_string_lossy();
        operation = map.iter().find(|(k, _)| *k == key).map(|(_, operation)| operation.clone());
    }

    match operation {
        Some(operation) => operation,
        None => {
            let operation = Rc::new(BulkFileOperation::new(uri, checked.clone()));
            map.push((key, operation.clone()));
            operation
        }
    }
}

/// `BulkFileOperations`.
pub struct BulkFileOperations {
    bulk_edit: Rc<Vec<ResourceEdit>>,
    file_service: Rc<dyn IFileService>,
    pub checked: Rc<CheckedStates<usize>>,
    file_operations: RefCell<Vec<Rc<BulkFileOperation>>>,
    categories: RefCell<Vec<Rc<BulkCategory>>>,
    pub conflicts: Rc<ConflictDetector>,
}

impl Injectable for BulkFileOperations {
    type Args = Rc<Vec<ResourceEdit>>;
    type Deps = (Rc<dyn IFileService>, Rc<InstantiationService>);

    fn create(bulk_edit: Self::Args, (file_service, insta_service): Self::Deps) -> Rc<Self> {
        let conflicts = insta_service.create_instance::<ConflictDetector>(bulk_edit.as_ref().clone());
        Rc::new(Self {
            bulk_edit,
            file_service,
            checked: Rc::new(CheckedStates::new()),
            file_operations: RefCell::new(Vec::new()),
            categories: RefCell::new(Vec::new()),
            conflicts,
        })
    }
}

impl BulkFileOperations {
    /// `BulkFileOperations.create(accessor, bulkEdit)`.
    pub fn create(accessor: &ServicesAccessor, bulk_edit: Rc<Vec<ResourceEdit>>) -> Promise<Rc<BulkFileOperations>> {
        accessor.get(I_INSTANTIATION_SERVICE).create_instance::<BulkFileOperations>(bulk_edit).init()
    }

    /// `fileOperations`.
    pub fn file_operations(&self) -> Vec<Rc<BulkFileOperation>> {
        self.file_operations.borrow().clone()
    }

    /// `categories`.
    pub fn categories(&self) -> Vec<Rc<BulkCategory>> {
        self.categories.borrow().clone()
    }

    /// `_init()`.
    pub fn init(self: Rc<Self>) -> Promise<Rc<Self>> {
        async_fn(async move {
            let mut operation_by_resource: Vec<(String, Rc<BulkFileOperation>)> = Vec::new();
            let mut operation_by_category: Vec<(String, Rc<BulkCategory>)> = Vec::new();
            let ext = ext_uri();

            // newToOldUri: chave de comparação do URI novo e o URI antigo.
            let mut new_to_old_uri: Vec<(String, URI)> = Vec::new();

            for (idx, edit) in self.bulk_edit.iter().enumerate() {
                let uri: URI;
                let r#type: u8;
                let metadata = edit.metadata();

                // store inital checked state
                self.checked.update_checked(&idx, !metadata.as_ref().is_some_and(|m| m.needs_confirmation));

                match edit {
                    ResourceEdit::Text(edit) => {
                        r#type = BulkFileOperationType::TEXT_EDIT;
                        uri = edit.resource.clone();
                    }
                    ResourceEdit::File(edit) => {
                        let options = &edit.options;
                        if let (Some(new_resource), Some(old_resource)) = (&edit.new_resource, &edit.old_resource) {
                            r#type = BulkFileOperationType::RENAME;
                            uri = old_resource.clone();
                            if options.overwrite.is_none() && options.ignore_if_exists == Some(true) && self.file_service.exists(&uri).await? {
                                // noop -> "soft" rename to something that already exists
                                continue;
                            }
                            // map newResource onto oldResource so that text-edit appear for
                            // the same file element
                            let key = ext.get_comparison_key(new_resource, false).to_string_lossy();
                            new_to_old_uri.retain(|(k, _)| *k != key);
                            new_to_old_uri.push((key, uri.clone()));
                        } else if let Some(old_resource) = &edit.old_resource {
                            r#type = BulkFileOperationType::DELETE;
                            uri = old_resource.clone();
                            if options.ignore_if_not_exists == Some(true) && !self.file_service.exists(&uri).await? {
                                // noop -> "soft" delete something that doesn't exist
                                continue;
                            }
                        } else if let Some(new_resource) = &edit.new_resource {
                            r#type = BulkFileOperationType::CREATE;
                            uri = new_resource.clone();
                            if options.overwrite.is_none() && options.ignore_if_exists == Some(true) && self.file_service.exists(&uri).await? {
                                // noop -> "soft" create something that already exists
                                continue;
                            }
                        } else {
                            // invalid edit -> skip
                            continue;
                        }
                    }
                    // unsupported edit
                    ResourceEdit::Custom(_) => continue,
                }

                insert_operation(&uri, &mut operation_by_resource, &new_to_old_uri, &self.checked).add_edit(idx, r#type, edit);

                // insert into "this" category
                let key = BulkCategory::key_of(metadata.as_ref());
                let category = match operation_by_category.iter().find(|(k, _)| *k == key) {
                    Some((_, category)) => category.clone(),
                    None => {
                        let category = Rc::new(BulkCategory::new(metadata));
                        operation_by_category.push((key, category.clone()));
                        category
                    }
                };
                let operation = insert_operation(&uri, &mut category.operation_by_resource.borrow_mut(), &new_to_old_uri, &self.checked);
                operation.add_edit(idx, r#type, edit);
            }

            *self.file_operations.borrow_mut() = operation_by_resource.into_iter().map(|(_, operation)| operation).collect();
            *self.categories.borrow_mut() = operation_by_category.into_iter().map(|(_, category)| category).collect();

            // "correct" invalid parent-check child states that is
            // unchecked file edits (rename, create, delete) uncheck
            // all edits for a file, e.g no text change without rename
            for file in self.file_operations.borrow().iter() {
                if file.r#type.get() != BulkFileOperationType::TEXT_EDIT {
                    let mut checked = true;
                    for (index, edit) in file.original_edits.borrow().iter() {
                        if matches!(edit, ResourceEdit::File(_)) {
                            checked = checked && self.checked.is_checked(index);
                        }
                    }
                    if !checked {
                        for index in file.original_edits.borrow().keys() {
                            self.checked.update_checked(index, checked);
                        }
                    }
                }
            }

            // sort (once) categories atop which have unconfirmed edits
            self.categories.borrow_mut().sort_by(|a, b| {
                if a.metadata.needs_confirmation == b.metadata.needs_confirmation {
                    locale_compare(&a.metadata.label, &b.metadata.label)
                } else if a.metadata.needs_confirmation {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            });

            Ok(self)
        })
    }

    /// `getWorkspaceEdit()`.
    pub fn get_workspace_edit(&self) -> Rc<Vec<ResourceEdit>> {
        let mut result: Vec<ResourceEdit> = Vec::new();
        let mut all_accepted = true;

        for (i, edit) in self.bulk_edit.iter().enumerate() {
            if self.checked.is_checked(&i) {
                result.push(edit.clone());
                continue;
            }
            all_accepted = false;
        }

        if all_accepted {
            return self.bulk_edit.clone();
        }

        // not all edits have been accepted
        Rc::new(result)
    }

    /// `getFileEditOperation(edit)`.
    fn get_file_edit_operation(edit: &ResourceFileEdit) -> Promise<Option<SingleEditOperation>> {
        let contents = edit.options.contents.clone();
        async_fn(async move {
            let Some(contents) = contents else { return Ok(None) };
            let content = contents.await?;
            let text = String::from_utf8_lossy(&content.to_vec()).into_owned();
            Ok(Some(EditOperation::replace_move(Range::new(0, 0, i32::MAX, 0), Some(JsString::from(text)))))
        })
    }

    /// `getFileEdits(uri)`.
    pub fn get_file_edits(self: Rc<Self>, uri: URI) -> Promise<Vec<SingleEditOperation>> {
        async_fn(async move {
            let files = self.file_operations();
            for file in files {
                if file.uri.to_string(false) == uri.to_string(false) {
                    let mut result: Vec<Promise<Option<SingleEditOperation>>> = Vec::new();
                    let mut ignore_all = false;

                    let original: Vec<(usize, ResourceEdit)> = file.original_edits.borrow().iter().map(|(i, e)| (*i, e.clone())).collect();
                    for (index, edit) in &original {
                        match edit {
                            ResourceEdit::File(edit) => result.push(Self::get_file_edit_operation(edit)),
                            ResourceEdit::Text(edit) => {
                                if self.checked.is_checked(index) {
                                    let text = if edit.text_edit.insert_as_snippet != Some(true) {
                                        JsString::from(edit.text_edit.text.clone())
                                    } else {
                                        SnippetParser::as_insert_text(JsString::from(edit.text_edit.text.clone()).units())
                                    };
                                    result.push(Promise::resolved(Some(EditOperation::replace_move(edit.text_edit.range, Some(text)))));
                                }
                            }
                            ResourceEdit::Custom(_) => {
                                if !self.checked.is_checked(index) {
                                    // UNCHECKED WorkspaceFileEdit disables all text edits
                                    ignore_all = true;
                                }
                            }
                        }
                    }

                    if ignore_all {
                        return Ok(Vec::new());
                    }

                    let mut edits = Vec::new();
                    for promise in result {
                        if let Some(edit) = promise.await? {
                            edits.push(edit);
                        }
                    }
                    edits.sort_by(|a, b| Range::compare_ranges_using_starts(Some(&a.range), Some(&b.range)).cmp(&0));
                    return Ok(edits);
                }
            }
            Ok(Vec::new())
        })
    }

    /// `getUriOfEdit(edit)`: a edição é o índice dela na lista original.
    pub fn get_uri_of_edit(&self, edit: usize) -> URI {
        for file in self.file_operations.borrow().iter() {
            if file.original_edits.borrow().contains_key(&edit) {
                return file.uri.clone();
            }
        }
        je_base::errors::throw(je_base::errors::Error::new("invalid edit"))
    }
}

impl IDisposable for BulkFileOperations {
    fn dispose(&self) {
        self.checked.dispose();
        self.conflicts.dispose();
    }
}
