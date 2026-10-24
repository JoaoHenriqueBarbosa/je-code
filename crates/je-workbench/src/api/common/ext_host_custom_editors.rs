//! Porte de `src/vs/workbench/api/common/extHostCustomEditors.ts`: `window.registerCustomEditorProvider` do lado das
//! extensões. O host guarda os provedores por `viewType`, os documentos personalizados por `viewType` e recurso, e
//! responde ao protocolo da thread principal (`$createCustomDocument`, `$resolveCustomEditor`, `$undo`, `$onSave`,
//! `$backup`, a navegação do editor de texto personalizado e os diffs).
//!
//! Divergências:
//! - Os provedores da API (`CustomReadonlyEditorProvider`, `CustomEditorProvider`, `CustomTextEditorProvider`) são
//!   objetos com membros opcionais, distinguidos por `typeof x.membro === 'function'`. Aqui são traits, e o membro
//!   opcional é um método `as_*` que devolve `None` por padrão e `Some(self)` em quem o implementa:
//!   [`CustomReadonlyEditorProvider::as_editable`] (o `onDidChangeCustomDocument`),
//!   [`CustomReadonlyEditorProvider::as_inline_diff`], [`CustomReadonlyEditorProvider::as_side_by_side_diff`],
//!   [`CustomTextEditorProvider::as_move`], [`CustomTextEditorProvider::as_navigation`],
//!   [`CustomTextEditorProvider::as_inline_diff`] e [`CustomTextEditorProvider::as_side_by_side_diff`]. O teste
//!   `isCustomTextEditorProvider` (`resolveCustomTextEditor` é função) é a escolha explícita entre
//!   [`EditorProvider::Text`] e [`EditorProvider::Custom`] no registro.
//! - `ProviderResult`/`Thenable<T> | T` é [`MaybePromise`]; um lançamento síncrono de um provedor é uma promessa
//!   rejeitada (dentro de uma função `async` do original, os dois dão a mesma rejeição). `CustomDocument.dispose`
//!   pode lançar: devolve [`Result`].
//! - `CustomDocumentEditEvent | CustomDocumentContentChangeEvent` (distinguidos por `undo` e `redo` serem funções,
//!   `isEditEvent`) é o enum [`CustomDocumentChangeEvent`].
//! - `registerCustomEditorProvider` lança no original (viewType já registrado, proposta de API não habilitada): aqui
//!   devolve [`Result`]. O `vscode.Disposable` é o [`Disposable`] de `ext_host_types`.
//! - `ExtHostWebviews` e `ExtHostWebviewPanels` (`extHostWebview.ts`, `extHostWebviewPanels.ts`) são os porte reais de
//!   `ext_host_webview` e `ext_host_webview_panels` (`vscode.WebviewPanel` é o [`ExtHostWebviewPanel`]); `IExtensionStoragePaths`
//!   (`extHostStoragePaths.ts`) ainda não tem porte: este arquivo declara, como trait, só o que o `ExtHostCustomEditors`
//!   usa dele ([`IExtensionStoragePaths`]). `toExtensionData` e `shouldSerializeBuffersForPostMessage`, de `extHostWebview.ts`,
//!   são `to_extension_data` e `should_serialize_buffers_for_post_message` de `ext_host_webview`.
//! - O `ExtHostCustomEditors` é um `Rc` interno ([`ExtHostCustomEditors::new`]): as promessas e os ouvintes seguram o
//!   estado. `IMainContext` só serve para pedir o proxy (`MainThreadCustomEditors`) no construtor.
//! - `vscode.CustomTextEditorNavigation.captureViewState/restoreViewState` trafegam `unknown`: é [`Dynamic`].
//! - `untitledDocumentData?.buffer` (um `Uint8Array`) é `Vec<u8>`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::async_::{async_fn, async_fn_maybe, await_maybe};
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::hash::hash;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::network::schemas;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::range::Range as EditorRange;
use je_editor::selection::Selection as EditorSelection;
use je_extensions::scanner::ExtensionDescription;

use super::cache::Cache;
use super::ext_host_document_data::TextDocument;
use super::ext_host_documents::ExtHostDocuments;
use super::ext_host_protocol::{
    CreateCustomDocumentResult, CustomEditorDiffInitData, CustomEditorProviderCapabilities, CustomEditorResolveInitData, CustomEditorSideBySideDiffInitData,
    CustomEditorSideBySideDiffWebviewHandles, CustomTextEditorNavigationResult, EditorGroupColumn, ExtHostCustomEditorsShape, IMainContext, MainContext,
    MainThreadCustomEditorsShape, WebviewPanelOptions,
};
use super::ext_host_type_converters::{Range as RangeConverter, Selection as SelectionConverter, ViewColumn};
use super::ext_host_types::{Disposable, Range, Selection};
use super::ext_host_webview::{ExtHostWebviews, should_serialize_buffers_for_post_message, to_extension_data};
use super::ext_host_webview_panels::{ExtHostWebviewPanel, ExtHostWebviewPanels};
use crate::services::extensions::extensions::{check_proposed_api_enabled, is_proposed_api_enabled};

// ---- colaborador ainda não portado

/// `IExtensionStoragePaths`, só `workspaceValue` e `globalValue`.
pub trait IExtensionStoragePaths {
    fn workspace_value(&self, extension: &ExtensionDescription) -> Option<URI>;
    fn global_value(&self, extension: &ExtensionDescription) -> URI;
}

// ---- a API de editores personalizados (`vscode.d.ts`)

/// `vscode.CustomDocument`.
pub trait CustomDocument {
    /// `uri`: pode mudar durante a vida do documento (por exemplo, depois de um "salvar como").
    fn uri(&self) -> URI;

    /// `dispose()`: o `Err` é o lançamento do original.
    fn dispose(&self) -> Result<(), Error>;
}

/// `vscode.CustomDocumentOpenContext`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomDocumentOpenContext {
    pub backup_id: Option<String>,
    pub untitled_document_data: Option<Vec<u8>>,
}

/// `vscode.CustomDocumentBackupContext`.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomDocumentBackupContext {
    pub destination: URI,
}

/// `vscode.CustomDocumentBackup`.
pub trait CustomDocumentBackup {
    fn id(&self) -> String;
    fn delete(&self);
}

/// `vscode.CustomDocumentEditEvent`.
pub trait CustomDocumentEditEvent {
    fn document(&self) -> Rc<dyn CustomDocument>;
    fn label(&self) -> Option<String>;
    fn undo(&self) -> MaybePromise<()>;
    fn redo(&self) -> MaybePromise<()>;
}

/// `vscode.CustomDocumentEditEvent | vscode.CustomDocumentContentChangeEvent`.
#[derive(Clone)]
pub enum CustomDocumentChangeEvent {
    Edit(Rc<dyn CustomDocumentEditEvent>),
    /// `CustomDocumentContentChangeEvent`: só o `document`.
    ContentChange(Rc<dyn CustomDocument>),
}

impl CustomDocumentChangeEvent {
    fn document(&self) -> Rc<dyn CustomDocument> {
        match self {
            CustomDocumentChangeEvent::Edit(edit) => edit.document(),
            CustomDocumentChangeEvent::ContentChange(document) => document.clone(),
        }
    }
}

/// `vscode.CustomEditorDiffDocuments<T>`.
#[derive(Clone)]
pub struct CustomEditorDiffDocuments<T> {
    pub original: T,
    pub modified: T,
}

/// `vscode.CustomEditorSideBySideDiffWebviewPanels`.
#[derive(Clone)]
pub struct CustomEditorSideBySideDiffWebviewPanels {
    pub original: Rc<ExtHostWebviewPanel>,
    pub modified: Rc<ExtHostWebviewPanel>,
}

/// `vscode.CustomTextEditorRevealOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomTextEditorRevealOptions {
    pub selection: Option<Selection>,
    pub preserve_focus: Option<bool>,
}

/// `vscode.CustomTextEditorNavigation`.
pub trait CustomTextEditorNavigation: IDisposable {
    fn selection(&self) -> Option<Selection>;
    fn on_did_change_selection(&self) -> Event<Option<Selection>>;
    fn reveal_range(&self, range: &Range, options: CustomTextEditorRevealOptions, token: CancellationToken) -> MaybePromise<()>;
    fn capture_view_state(&self) -> MaybePromise<Dynamic>;
    fn restore_view_state(&self, state: Dynamic, token: CancellationToken) -> MaybePromise<()>;
}

/// `resolveCustomEditorInlineDiff` de `vscode.CustomReadonlyEditorProvider` (proposta `customEditorDiffs`).
pub trait CustomEditorInlineDiffProvider {
    fn resolve_custom_editor_inline_diff(
        &self,
        documents: CustomEditorDiffDocuments<Rc<dyn CustomDocument>>,
        panel: Rc<ExtHostWebviewPanel>,
        token: CancellationToken,
    ) -> MaybePromise<()>;
}

/// `resolveCustomEditorSideBySideDiff` de `vscode.CustomReadonlyEditorProvider` (proposta `customEditorDiffs`).
pub trait CustomEditorSideBySideDiffProvider {
    fn resolve_custom_editor_side_by_side_diff(
        &self,
        documents: CustomEditorDiffDocuments<Rc<dyn CustomDocument>>,
        panels: CustomEditorSideBySideDiffWebviewPanels,
        token: CancellationToken,
    ) -> MaybePromise<()>;
}

/// `resolveCustomTextEditorInlineDiff` de `vscode.CustomTextEditorProvider` (proposta `customEditorDiffs`).
pub trait CustomTextEditorInlineDiffProvider {
    fn resolve_custom_text_editor_inline_diff(&self, documents: CustomEditorDiffDocuments<TextDocument>, panel: Rc<ExtHostWebviewPanel>, token: CancellationToken) -> MaybePromise<()>;
}

/// `resolveCustomTextEditorSideBySideDiff` de `vscode.CustomTextEditorProvider` (proposta `customEditorDiffs`).
pub trait CustomTextEditorSideBySideDiffProvider {
    fn resolve_custom_text_editor_side_by_side_diff(
        &self,
        documents: CustomEditorDiffDocuments<TextDocument>,
        panels: CustomEditorSideBySideDiffWebviewPanels,
        token: CancellationToken,
    ) -> MaybePromise<()>;
}

/// `moveCustomTextEditor` de `vscode.CustomTextEditorProvider`.
pub trait CustomTextEditorMoveProvider {
    fn move_custom_text_editor(&self, document: TextDocument, panel: Rc<ExtHostWebviewPanel>, token: CancellationToken) -> MaybePromise<()>;
}

/// `resolveCustomTextEditorNavigation` de `vscode.CustomTextEditorProvider` (proposta `customTextEditorNavigation`).
pub trait CustomTextEditorNavigationProvider {
    fn resolve_custom_text_editor_navigation(
        &self,
        document: TextDocument,
        panel: Rc<ExtHostWebviewPanel>,
        token: CancellationToken,
    ) -> MaybePromise<Rc<dyn CustomTextEditorNavigation>>;
}

/// `vscode.CustomTextEditorProvider`.
pub trait CustomTextEditorProvider {
    /// `resolveCustomTextEditor(document, webviewPanel, token)`.
    fn resolve_custom_text_editor(&self, document: TextDocument, panel: Rc<ExtHostWebviewPanel>, token: CancellationToken) -> MaybePromise<()>;

    fn as_move(&self) -> Option<&dyn CustomTextEditorMoveProvider> {
        None
    }

    fn as_navigation(&self) -> Option<&dyn CustomTextEditorNavigationProvider> {
        None
    }

    fn as_inline_diff(&self) -> Option<&dyn CustomTextEditorInlineDiffProvider> {
        None
    }

    fn as_side_by_side_diff(&self) -> Option<&dyn CustomTextEditorSideBySideDiffProvider> {
        None
    }
}

/// `vscode.CustomReadonlyEditorProvider`.
pub trait CustomReadonlyEditorProvider {
    /// `openCustomDocument(uri, openContext, token)`.
    fn open_custom_document(&self, uri: &URI, open_context: CustomDocumentOpenContext, token: CancellationToken) -> MaybePromise<Rc<dyn CustomDocument>>;

    /// `resolveCustomEditor(document, webviewPanel, token)`.
    fn resolve_custom_editor(&self, document: Rc<dyn CustomDocument>, panel: Rc<ExtHostWebviewPanel>, token: CancellationToken) -> MaybePromise<()>;

    /// O provedor com `onDidChangeCustomDocument` (`vscode.CustomEditorProvider`), o que o torna editável.
    fn as_editable(&self) -> Option<&dyn CustomEditorProvider> {
        None
    }

    fn as_inline_diff(&self) -> Option<&dyn CustomEditorInlineDiffProvider> {
        None
    }

    fn as_side_by_side_diff(&self) -> Option<&dyn CustomEditorSideBySideDiffProvider> {
        None
    }
}

/// `vscode.CustomEditorProvider`: o provedor editável.
pub trait CustomEditorProvider: CustomReadonlyEditorProvider {
    /// `onDidChangeCustomDocument`.
    fn on_did_change_custom_document(&self) -> Event<CustomDocumentChangeEvent>;
    fn save_custom_document(&self, document: Rc<dyn CustomDocument>, token: CancellationToken) -> MaybePromise<()>;
    fn save_custom_document_as(&self, document: Rc<dyn CustomDocument>, destination: &URI, token: CancellationToken) -> MaybePromise<()>;
    fn revert_custom_document(&self, document: Rc<dyn CustomDocument>, token: CancellationToken) -> MaybePromise<()>;
    fn backup_custom_document(&self, document: Rc<dyn CustomDocument>, context: CustomDocumentBackupContext, token: CancellationToken)
    -> MaybePromise<Rc<dyn CustomDocumentBackup>>;
}

/// O provedor registrado: `CustomTextEditorProvider` (`CustomEditorType.Text`) ou `CustomReadonlyEditorProvider`
/// (`CustomEditorType.Custom`).
#[derive(Clone)]
pub enum EditorProvider {
    Text(Rc<dyn CustomTextEditorProvider>),
    Custom(Rc<dyn CustomReadonlyEditorProvider>),
}

/// O `options` de `registerCustomEditorProvider`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CustomEditorRegistrationOptions {
    pub webview_options: Option<WebviewPanelOptions>,
    pub supports_multiple_editors_per_document: Option<bool>,
}

// ---- os armazenamentos

struct CustomTextEditorNavigationEntry {
    view_type: String,
    disposables: Rc<DisposableStore>,
    states: RefCell<HashMap<u32, Dynamic>>,
    pending_states: RefCell<HashSet<u32>>,
    controller: RefCell<Option<Rc<dyn CustomTextEditorNavigation>>>,
}

struct CustomDocumentStoreEntry {
    backup_counter: Cell<u32>,
    document: Rc<dyn CustomDocument>,
    storage_path: Option<URI>,
    edits: Cache<Rc<dyn CustomDocumentEditEvent>>,
    backup: RefCell<Option<Rc<dyn CustomDocumentBackup>>>,
}

impl CustomDocumentStoreEntry {
    fn new(document: Rc<dyn CustomDocument>, storage_path: Option<URI>) -> Self {
        Self {
            backup_counter: Cell::new(1),
            document,
            storage_path,
            edits: Cache::new("custom documents"),
            backup: RefCell::new(None),
        }
    }

    fn add_edit(&self, item: Rc<dyn CustomDocumentEditEvent>) -> u32 {
        self.edits.add(vec![item])
    }

    fn undo(self: &Rc<Self>, edit_id: u32, is_dirty: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let edit = this.get_edit(edit_id)?;
            await_maybe(edit.undo()).await?;
            if !is_dirty {
                this.dispose_backup();
            }
            Ok(())
        })
    }

    fn redo(self: &Rc<Self>, edit_id: u32, is_dirty: bool) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let edit = this.get_edit(edit_id)?;
            await_maybe(edit.redo()).await?;
            if !is_dirty {
                this.dispose_backup();
            }
            Ok(())
        })
    }

    fn dispose_edits(&self, edit_ids: &[u32]) {
        for id in edit_ids {
            self.edits.delete(*id);
        }
    }

    fn get_new_backup_uri(&self) -> Result<URI, Error> {
        let Some(storage_path) = &self.storage_path else {
            return Err(Error::new("Backup requires a valid storage path"));
        };
        let counter = self.backup_counter.get();
        self.backup_counter.set(counter + 1);
        let file_name = format!("{}{}", hash_path(&self.document.uri()), counter);
        Ok(URI::join_path(storage_path, &[file_name.as_str()], Platform::detect().path_flavor()))
    }

    fn update_backup(&self, backup: Rc<dyn CustomDocumentBackup>) {
        self.delete_current_backup();
        *self.backup.borrow_mut() = Some(backup);
    }

    fn dispose_backup(&self) {
        self.delete_current_backup();
        *self.backup.borrow_mut() = None;
    }

    /// `this._backup?.delete()`.
    fn delete_current_backup(&self) {
        let current = self.backup.borrow().clone();
        if let Some(backup) = current {
            backup.delete();
        }
    }

    fn get_edit(&self, edit_id: u32) -> Result<Rc<dyn CustomDocumentEditEvent>, Error> {
        self.edits.get(edit_id, 0).ok_or_else(|| Error::new("No edit found"))
    }
}

#[derive(Default)]
struct CustomDocumentStore {
    documents: RefCell<HashMap<String, Rc<CustomDocumentStoreEntry>>>,
}

impl CustomDocumentStore {
    fn get(&self, view_type: &str, resource: &URI) -> Option<Rc<CustomDocumentStoreEntry>> {
        self.documents.borrow().get(&Self::key(view_type, resource)).cloned()
    }

    fn add(&self, view_type: &str, document: Rc<dyn CustomDocument>, storage_path: Option<URI>) -> Result<Rc<CustomDocumentStoreEntry>, Error> {
        let uri = document.uri();
        let key = Self::key(view_type, &uri);
        if self.documents.borrow().contains_key(&key) {
            return Err(Error::new(format!("Document already exists for viewType:{view_type} resource:{}", uri.to_string(false))));
        }
        let entry = Rc::new(CustomDocumentStoreEntry::new(document, storage_path));
        self.documents.borrow_mut().insert(key, entry.clone());
        Ok(entry)
    }

    fn delete(&self, view_type: &str, resource: &URI) {
        // Use the resource parameter directly instead of document.uri, because the document's
        // URI may have changed (e.g., after SaveAs from untitled to a file path).
        self.documents.borrow_mut().remove(&Self::key(view_type, resource));
    }

    fn key(view_type: &str, resource: &URI) -> String {
        format!("{view_type}@@@{}", resource.to_string(false))
    }
}

#[derive(Clone)]
struct ProviderEntry {
    extension: Rc<ExtensionDescription>,
    provider: EditorProvider,
}

#[derive(Default)]
struct EditorProviderStore {
    providers: Rc<RefCell<HashMap<String, ProviderEntry>>>,
}

impl EditorProviderStore {
    fn get(&self, view_type: &str) -> Option<ProviderEntry> {
        self.providers.borrow().get(view_type).cloned()
    }

    fn add(&self, view_type: &str, entry: ProviderEntry) -> Result<Disposable, Error> {
        if self.providers.borrow().contains_key(view_type) {
            return Err(Error::new(format!("Provider for viewType:{view_type} already registered")));
        }
        self.providers.borrow_mut().insert(view_type.to_owned(), entry);
        let providers = self.providers.clone();
        let view_type = view_type.to_owned();
        Ok(Disposable::new(move || {
            providers.borrow_mut().remove(&view_type);
        }))
    }
}

// ---- o ExtHostCustomEditors

struct Inner {
    proxy: Rc<dyn MainThreadCustomEditorsShape>,
    editor_providers: EditorProviderStore,
    documents: CustomDocumentStore,
    navigation: RefCell<HashMap<String, Rc<CustomTextEditorNavigationEntry>>>,
    ext_host_documents: Rc<ExtHostDocuments>,
    extension_storage_paths: Option<Rc<dyn IExtensionStoragePaths>>,
    ext_host_webview: Rc<ExtHostWebviews>,
    ext_host_webview_panels: Rc<ExtHostWebviewPanels>,
}

/// `ExtHostCustomEditors`.
pub struct ExtHostCustomEditors {
    inner: Rc<Inner>,
}

impl ExtHostCustomEditors {
    pub fn new(
        main_context: &dyn IMainContext,
        ext_host_documents: Rc<ExtHostDocuments>,
        extension_storage_paths: Option<Rc<dyn IExtensionStoragePaths>>,
        ext_host_webview: Rc<ExtHostWebviews>,
        ext_host_webview_panels: Rc<ExtHostWebviewPanels>,
    ) -> Self {
        Self {
            inner: Rc::new(Inner {
                proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_CUSTOM_EDITORS),
                editor_providers: EditorProviderStore::default(),
                documents: CustomDocumentStore::default(),
                navigation: RefCell::new(HashMap::new()),
                ext_host_documents,
                extension_storage_paths,
                ext_host_webview,
                ext_host_webview_panels,
            }),
        }
    }

    pub fn register_custom_editor_provider(
        &self,
        extension: &ExtensionDescription,
        view_type: &str,
        provider: EditorProvider,
        options: CustomEditorRegistrationOptions,
    ) -> Result<Disposable, Error> {
        let inner = &self.inner;
        if let EditorProvider::Text(text_provider) = &provider
            && text_provider.as_navigation().is_some()
        {
            check_proposed_api_enabled(extension, "customTextEditorNavigation")?;
        }
        let disposables = DisposableStore::new();
        let webview_options = options.webview_options.unwrap_or_default();
        let entry = ProviderEntry { extension: Rc::new(extension.clone()), provider: provider.clone() };
        match &provider {
            EditorProvider::Text(text_provider) => {
                disposables.add_rc(Rc::new(inner.editor_providers.add(view_type, entry)?));
                let supports_diffs = is_proposed_api_enabled(extension, "customEditorDiffs");
                inner.proxy.rpc_register_text_editor_provider(
                    to_extension_data(extension),
                    view_type,
                    webview_options,
                    CustomEditorProviderCapabilities {
                        supports_move: Some(text_provider.as_move().is_some()),
                        supports_navigation: Some(text_provider.as_navigation().is_some()),
                        supports_inline_diff: Some(supports_diffs && text_provider.as_inline_diff().is_some()),
                        supports_side_by_side_diff: Some(supports_diffs && text_provider.as_side_by_side_diff().is_some()),
                    },
                    should_serialize_buffers_for_post_message(extension),
                );
            }
            EditorProvider::Custom(custom_provider) => {
                disposables.add_rc(Rc::new(inner.editor_providers.add(view_type, entry)?));
                let supports_custom_editor_diffs = is_proposed_api_enabled(extension, "customEditorDiffs");

                if let Some(editable) = custom_provider.as_editable() {
                    let listener_inner = inner.clone();
                    let listener_view_type = view_type.to_owned();
                    disposables.add(editable.on_did_change_custom_document().listen(move |event| {
                        let document = event.document();
                        let resource = document.uri();
                        match listener_inner.get_custom_document_entry(&listener_view_type, &resource) {
                            Ok(entry) => match event {
                                CustomDocumentChangeEvent::Edit(edit) => {
                                    let edit_id = entry.add_edit(edit.clone());
                                    listener_inner.proxy.rpc_on_did_edit(&resource, &listener_view_type, edit_id, edit.label().as_deref());
                                }
                                CustomDocumentChangeEvent::ContentChange(_) => listener_inner.proxy.rpc_on_content_change(&resource, &listener_view_type),
                            },
                            Err(error) => errors::on_unexpected_error(&error),
                        }
                    }));
                }

                inner.proxy.rpc_register_custom_editor_provider(
                    to_extension_data(extension),
                    view_type,
                    webview_options,
                    CustomEditorProviderCapabilities {
                        supports_move: None,
                        supports_navigation: None,
                        supports_inline_diff: Some(supports_custom_editor_diffs && custom_provider.as_inline_diff().is_some()),
                        supports_side_by_side_diff: Some(supports_custom_editor_diffs && custom_provider.as_side_by_side_diff().is_some()),
                    },
                    options.supports_multiple_editors_per_document.unwrap_or(false),
                    should_serialize_buffers_for_post_message(extension),
                );
            }
        }

        let navigation_inner = inner.clone();
        let navigation_view_type = view_type.to_owned();
        disposables.add(to_disposable(move || {
            let handles: Vec<String> =
                navigation_inner.navigation.borrow().iter().filter(|(_, entry)| entry.view_type == navigation_view_type).map(|(handle, _)| handle.clone()).collect();
            for handle in handles {
                navigation_inner.dispose_navigation(&handle);
            }
        }));

        let unregister_inner = inner.clone();
        let unregister_view_type = view_type.to_owned();
        let unregister: DisposableRef = Rc::new(Disposable::new(move || {
            unregister_inner.proxy.rpc_unregister_editor_provider(&unregister_view_type);
        }));
        let registered: DisposableRef = disposables;
        Ok(Disposable::from(vec![Some(registered), Some(unregister)]))
    }
}

impl Inner {
    fn get_custom_document_entry(&self, view_type: &str, resource: &URI) -> Result<Rc<CustomDocumentStoreEntry>, Error> {
        self.documents.get(view_type, resource).ok_or_else(|| Error::new("No custom document found"))
    }

    fn get_provider(&self, view_type: &str) -> Result<ProviderEntry, Error> {
        self.editor_providers.get(view_type).ok_or_else(|| Error::new(format!("No provider found for '{view_type}'")))
    }

    fn get_custom_editor_provider(&self, view_type: &str) -> Result<Rc<dyn CustomReadonlyEditorProvider>, Error> {
        match self.editor_providers.get(view_type).map(|entry| entry.provider) {
            Some(EditorProvider::Custom(provider)) if provider.as_editable().is_some() => Ok(provider),
            Some(EditorProvider::Text(_)) | Some(EditorProvider::Custom(_)) | None => Err(not_editable()),
        }
    }

    fn dispose_navigation(&self, handle: &str) {
        let entry = self.navigation.borrow_mut().remove(handle);
        if let Some(entry) = entry {
            entry.states.borrow_mut().clear();
            entry.pending_states.borrow_mut().clear();
            entry.disposables.dispose();
        }
    }

    fn create_custom_editor_diff_panel(
        &self,
        handle: &str,
        view_type: &str,
        init_data: &CustomEditorDiffInitData,
        position: EditorGroupColumn,
    ) -> Result<(ProviderEntry, Rc<ExtHostWebviewPanel>), Error> {
        let entry = self.get_provider(view_type)?;
        let view_column = view_column_to(position)?;
        let webview = self.ext_host_webview.create_new_webview(handle, &init_data.content_options, &entry.extension);
        self.ext_host_webview.ensure_default_content_options(handle, &init_data.content_options, &entry.extension);
        let panel = self.ext_host_webview_panels.create_new_webview_panel(handle, view_type, &init_data.title, Some(view_column), init_data.options, webview, init_data.active);
        Ok((entry, panel))
    }
}

impl ExtHostCustomEditorsShape for ExtHostCustomEditors {
    fn rpc_create_custom_document(
        &self,
        resource: &URI,
        view_type: &str,
        backup_id: Option<String>,
        untitled_document_data: Option<VSBuffer>,
        cancellation: CancellationToken,
    ) -> Promise<CreateCustomDocumentResult> {
        let inner = self.inner.clone();
        let (resource, view_type) = (resource.clone(), view_type.to_owned());
        async_fn(async move {
            let entry = inner.get_provider(&view_type)?;
            let EditorProvider::Custom(provider) = &entry.provider else {
                return Err(Error::new(format!("Invalid provide type for '{view_type}'")));
            };

            let open_context = CustomDocumentOpenContext { backup_id, untitled_document_data: untitled_document_data.map(|data| data.to_vec()) };
            let document = await_maybe(provider.open_custom_document(&resource, open_context, cancellation)).await?;

            let editable = provider.as_editable().is_some();
            let mut storage_root: Option<URI> = None;
            if editable && let Some(paths) = &inner.extension_storage_paths {
                storage_root = paths.workspace_value(&entry.extension).or_else(|| Some(paths.global_value(&entry.extension)));
            }
            inner.documents.add(&view_type, document, storage_root)?;

            Ok(CreateCustomDocumentResult { editable })
        })
    }

    fn rpc_dispose_custom_document(&self, resource: &URI, view_type: &str) -> Promise<()> {
        let inner = self.inner.clone();
        let (resource, view_type) = (resource.clone(), view_type.to_owned());
        async_fn(async move {
            // Unregistering a provider disposes its models, so the provider may already be gone.
            let document = inner.get_custom_document_entry(&view_type, &resource)?.document.clone();
            // Pass the resource we used to look up the document, not document.uri,
            // because the document's URI may have changed (e.g., after SaveAs).
            inner.documents.delete(&view_type, &resource);
            document.dispose()
        })
    }

    fn rpc_resolve_custom_editor(
        &self,
        resource: &URI,
        new_webview_handle: &str,
        view_type: &str,
        init_data: CustomEditorResolveInitData,
        position: EditorGroupColumn,
        cancellation: CancellationToken,
    ) -> Promise<()> {
        let inner = self.inner.clone();
        let (resource, handle, view_type) = (resource.clone(), new_webview_handle.to_owned(), view_type.to_owned());
        async_fn_maybe(async move {
            let entry = inner.get_provider(&view_type)?;

            let view_column = view_column_to(position)?;

            let webview = inner.ext_host_webview.create_new_webview(&handle, &init_data.content_options, &entry.extension);
            // The main thread starts the custom editor's webview with empty content
            // options. Ensure `localResourceRoots` defaults to the workspace folders
            // and the providing extension's install directory, as documented on
            // `WebviewOptions.localResourceRoots`.
            inner.ext_host_webview.ensure_default_content_options(&handle, &init_data.content_options, &entry.extension);
            let panel = inner.ext_host_webview_panels.create_new_webview_panel(&handle, &view_type, &init_data.title, Some(view_column), init_data.options, webview, init_data.active);

            match &entry.provider {
                EditorProvider::Custom(provider) => {
                    let document = inner.get_custom_document_entry(&view_type, &resource)?.document.clone();
                    Ok(provider.resolve_custom_editor(document, panel, cancellation))
                }
                EditorProvider::Text(provider) => {
                    let document = inner.ext_host_documents.get_document(&resource)?;
                    Ok(provider.resolve_custom_text_editor(document, panel, cancellation))
                }
            }
        })
    }

    fn rpc_resolve_custom_text_editor_navigation(&self, handle: &str, view_type: &str, resource: &URI, token: CancellationToken) -> Promise<Option<CustomTextEditorNavigationResult>> {
        let inner = self.inner.clone();
        let (handle, view_type, resource) = (handle.to_owned(), view_type.to_owned(), resource.clone());
        async_fn(async move {
            let provider = inner.editor_providers.get(&view_type);
            let panel = inner.ext_host_webview_panels.get_webview_panel(&handle);
            let Some(provider_entry) = provider else {
                return Ok(None);
            };
            let EditorProvider::Text(text_provider) = &provider_entry.provider else {
                return Ok(None);
            };
            let Some(navigation_provider) = text_provider.as_navigation() else {
                return Ok(None);
            };
            let Some(panel) = panel else {
                return Ok(None);
            };
            if token.is_cancellation_requested() {
                return Ok(None);
            }
            check_proposed_api_enabled(&provider_entry.extension, "customTextEditorNavigation")?;
            inner.dispose_navigation(&handle);
            let entry = Rc::new(CustomTextEditorNavigationEntry {
                view_type: view_type.clone(),
                disposables: DisposableStore::new(),
                states: RefCell::new(HashMap::new()),
                pending_states: RefCell::new(HashSet::new()),
                controller: RefCell::new(None),
            });
            inner.navigation.borrow_mut().insert(handle.clone(), entry.clone());
            let cancellation = CancellationTokenSource::new(Some(&token));
            entry.disposables.add(to_disposable({
                let cancellation = cancellation.clone();
                move || cancellation.dispose_with(true)
            }));
            entry.disposables.add(panel.on_did_dispose().listen({
                let (inner, handle) = (inner.clone(), handle.clone());
                move |_| inner.dispose_navigation(&handle)
            }));
            let outcome: Result<Option<CustomTextEditorNavigationResult>, Error> = async {
                let document = inner.ext_host_documents.get_document(&resource)?;
                let controller = await_maybe(navigation_provider.resolve_custom_text_editor_navigation(document, panel.clone(), cancellation.token())).await?;
                let is_current = inner.navigation.borrow().get(&handle).is_some_and(|current| Rc::ptr_eq(current, &entry));
                if cancellation.token().is_cancellation_requested() || !is_current {
                    controller.dispose();
                    inner.dispose_navigation(&handle);
                    return Ok(None);
                }
                *entry.controller.borrow_mut() = Some(controller.clone());
                entry.disposables.add(to_disposable({
                    let controller = controller.clone();
                    move || controller.dispose()
                }));
                entry.disposables.add(controller.on_did_change_selection().listen({
                    let (inner, handle) = (inner.clone(), handle.clone());
                    move |selection| {
                        inner.proxy.rpc_on_did_change_custom_text_editor_selection(&handle, selection.as_ref().map(SelectionConverter::from));
                    }
                }));
                Ok(Some(CustomTextEditorNavigationResult { selection: controller.selection().as_ref().map(SelectionConverter::from) }))
            }
            .await;
            match outcome {
                Ok(result) => Ok(result),
                Err(error) => {
                    inner.dispose_navigation(&handle);
                    if !errors::is_cancellation_error(&error) {
                        errors::on_unexpected_error(&error);
                    }
                    Ok(None)
                }
            }
        })
    }

    fn rpc_dispose_custom_text_editor_navigation(&self, handle: &str) {
        self.inner.dispose_navigation(handle);
    }

    fn rpc_reveal_custom_text_editor_range(&self, handle: &str, range: EditorRange, selection: Option<EditorSelection>, preserve_focus: bool, token: CancellationToken) -> Promise<()> {
        let inner = self.inner.clone();
        let handle = handle.to_owned();
        async_fn(async move {
            let controller = inner.navigation.borrow().get(&handle).and_then(|entry| entry.controller.borrow().clone());
            if let Some(controller) = controller
                && !token.is_cancellation_requested()
            {
                let range = RangeConverter::to(&range)?;
                let selection = match &selection {
                    Some(selection) => Some(SelectionConverter::to(selection)?),
                    None => None,
                };
                await_maybe(controller.reveal_range(&range, CustomTextEditorRevealOptions { selection, preserve_focus: Some(preserve_focus) }, token)).await?;
            }
            Ok(())
        })
    }

    fn rpc_capture_custom_text_editor_view_state(&self, handle: &str, state_id: u32) -> Promise<()> {
        let inner = self.inner.clone();
        let handle = handle.to_owned();
        async_fn(async move {
            let entry = inner.navigation.borrow().get(&handle).cloned();
            let controller = entry.as_ref().and_then(|entry| entry.controller.borrow().clone());
            if let (Some(entry), Some(controller)) = (entry, controller) {
                entry.pending_states.borrow_mut().insert(state_id);
                let outcome = await_maybe(controller.capture_view_state()).await;
                let result = match outcome {
                    Ok(state) => {
                        let is_current = inner.navigation.borrow().get(&handle).is_some_and(|current| Rc::ptr_eq(current, &entry));
                        if is_current && entry.pending_states.borrow_mut().remove(&state_id) {
                            entry.states.borrow_mut().insert(state_id, state);
                        }
                        Ok(())
                    }
                    Err(error) => Err(error),
                };
                entry.pending_states.borrow_mut().remove(&state_id);
                return result;
            }
            Ok(())
        })
    }

    fn rpc_restore_custom_text_editor_view_state(&self, handle: &str, state_id: u32, token: CancellationToken) -> Promise<()> {
        let inner = self.inner.clone();
        let handle = handle.to_owned();
        async_fn(async move {
            let entry = inner.navigation.borrow().get(&handle).cloned();
            let controller = entry.as_ref().and_then(|entry| entry.controller.borrow().clone());
            if let (Some(entry), Some(controller)) = (entry, controller) {
                let has_state = entry.states.borrow().contains_key(&state_id);
                if has_state {
                    let state = entry.states.borrow_mut().remove(&state_id).unwrap_or(Dynamic::Undefined);
                    if !token.is_cancellation_requested() {
                        await_maybe(controller.restore_view_state(state, token)).await?;
                    }
                }
            }
            Ok(())
        })
    }

    fn rpc_release_custom_text_editor_view_state(&self, handle: &str, state_id: u32) {
        let entry = self.inner.navigation.borrow().get(handle).cloned();
        if let Some(entry) = entry {
            entry.states.borrow_mut().remove(&state_id);
            entry.pending_states.borrow_mut().remove(&state_id);
        }
    }

    fn rpc_resolve_custom_editor_inline_diff(
        &self,
        original_resource: &URI,
        modified_resource: &URI,
        new_webview_handle: &str,
        view_type: &str,
        init_data: CustomEditorDiffInitData,
        position: EditorGroupColumn,
        cancellation: CancellationToken,
    ) -> Promise<()> {
        let inner = self.inner.clone();
        let (original_resource, modified_resource, handle, view_type) =
            (original_resource.clone(), modified_resource.clone(), new_webview_handle.to_owned(), view_type.to_owned());
        async_fn_maybe(async move {
            let (entry, panel) = inner.create_custom_editor_diff_panel(&handle, &view_type, &init_data, position)?;

            match &entry.provider {
                EditorProvider::Text(provider) => {
                    let Some(diff_provider) = provider.as_inline_diff() else {
                        return Err(Error::new(format!("Provider for '{view_type}' does not support inline custom text editor diffs")));
                    };
                    let original = inner.ext_host_documents.get_document(&original_resource)?;
                    let modified = inner.ext_host_documents.get_document(&modified_resource)?;
                    Ok(diff_provider.resolve_custom_text_editor_inline_diff(CustomEditorDiffDocuments { original, modified }, panel, cancellation))
                }
                EditorProvider::Custom(provider) => {
                    let Some(diff_provider) = provider.as_inline_diff() else {
                        return Err(Error::new(format!("Provider for '{view_type}' does not support inline custom editor diffs")));
                    };
                    let original = inner.get_custom_document_entry(&view_type, &original_resource)?.document.clone();
                    let modified = inner.get_custom_document_entry(&view_type, &modified_resource)?.document.clone();
                    Ok(diff_provider.resolve_custom_editor_inline_diff(CustomEditorDiffDocuments { original, modified }, panel, cancellation))
                }
            }
        })
    }

    fn rpc_resolve_custom_editor_side_by_side_diff(
        &self,
        original_resource: &URI,
        modified_resource: &URI,
        webview_handles: CustomEditorSideBySideDiffWebviewHandles,
        view_type: &str,
        init_data: CustomEditorSideBySideDiffInitData,
        position: EditorGroupColumn,
        cancellation: CancellationToken,
    ) -> Promise<()> {
        let inner = self.inner.clone();
        let (original_resource, modified_resource, view_type) = (original_resource.clone(), modified_resource.clone(), view_type.to_owned());
        async_fn_maybe(async move {
            let (entry, original_panel) = inner.create_custom_editor_diff_panel(&webview_handles.original, &view_type, &init_data.original, position)?;
            let (_, modified_panel) = inner.create_custom_editor_diff_panel(&webview_handles.modified, &view_type, &init_data.modified, position)?;
            let panels = CustomEditorSideBySideDiffWebviewPanels { original: original_panel, modified: modified_panel };

            match &entry.provider {
                EditorProvider::Text(provider) => {
                    let Some(diff_provider) = provider.as_side_by_side_diff() else {
                        return Err(Error::new(format!("Provider for '{view_type}' does not support side by side custom text editor diffs")));
                    };
                    let original = inner.ext_host_documents.get_document(&original_resource)?;
                    let modified = inner.ext_host_documents.get_document(&modified_resource)?;
                    Ok(diff_provider.resolve_custom_text_editor_side_by_side_diff(CustomEditorDiffDocuments { original, modified }, panels, cancellation))
                }
                EditorProvider::Custom(provider) => {
                    let Some(diff_provider) = provider.as_side_by_side_diff() else {
                        return Err(Error::new(format!("Provider for '{view_type}' does not support side by side custom editor diffs")));
                    };
                    let original = inner.get_custom_document_entry(&view_type, &original_resource)?.document.clone();
                    let modified = inner.get_custom_document_entry(&view_type, &modified_resource)?.document.clone();
                    Ok(diff_provider.resolve_custom_editor_side_by_side_diff(CustomEditorDiffDocuments { original, modified }, panels, cancellation))
                }
            }
        })
    }

    fn rpc_undo(&self, resource: &URI, view_type: &str, edit_id: u32, is_dirty: bool) -> Promise<()> {
        let inner = self.inner.clone();
        let (resource, view_type) = (resource.clone(), view_type.to_owned());
        async_fn_maybe(async move {
            let entry = inner.get_custom_document_entry(&view_type, &resource)?;
            Ok(MaybePromise::Promise(entry.undo(edit_id, is_dirty)))
        })
    }

    fn rpc_redo(&self, resource: &URI, view_type: &str, edit_id: u32, is_dirty: bool) -> Promise<()> {
        let inner = self.inner.clone();
        let (resource, view_type) = (resource.clone(), view_type.to_owned());
        async_fn_maybe(async move {
            let entry = inner.get_custom_document_entry(&view_type, &resource)?;
            Ok(MaybePromise::Promise(entry.redo(edit_id, is_dirty)))
        })
    }

    fn rpc_revert(&self, resource: &URI, view_type: &str, cancellation: CancellationToken) -> Promise<()> {
        let inner = self.inner.clone();
        let (resource, view_type) = (resource.clone(), view_type.to_owned());
        async_fn(async move {
            let entry = inner.get_custom_document_entry(&view_type, &resource)?;
            let provider = inner.get_custom_editor_provider(&view_type)?;
            let editable = provider.as_editable().ok_or_else(not_editable)?;
            await_maybe(editable.revert_custom_document(entry.document.clone(), cancellation)).await?;
            entry.dispose_backup();
            Ok(())
        })
    }

    fn rpc_dispose_edits(&self, resource: &URI, view_type: &str, edit_ids: Vec<u32>) -> Result<(), Error> {
        let document = self.inner.get_custom_document_entry(view_type, resource)?;
        document.dispose_edits(&edit_ids);
        Ok(())
    }

    fn rpc_on_save(&self, resource: &URI, view_type: &str, cancellation: CancellationToken) -> Promise<()> {
        let inner = self.inner.clone();
        let (resource, view_type) = (resource.clone(), view_type.to_owned());
        async_fn(async move {
            let entry = inner.get_custom_document_entry(&view_type, &resource)?;
            let provider = inner.get_custom_editor_provider(&view_type)?;
            let editable = provider.as_editable().ok_or_else(not_editable)?;
            await_maybe(editable.save_custom_document(entry.document.clone(), cancellation)).await?;
            entry.dispose_backup();
            Ok(())
        })
    }

    fn rpc_on_save_as(&self, resource: &URI, view_type: &str, target_resource: &URI, cancellation: CancellationToken) -> Promise<()> {
        let inner = self.inner.clone();
        let (resource, view_type, target_resource) = (resource.clone(), view_type.to_owned(), target_resource.clone());
        async_fn_maybe(async move {
            let entry = inner.get_custom_document_entry(&view_type, &resource)?;
            let provider = inner.get_custom_editor_provider(&view_type)?;
            let editable = provider.as_editable().ok_or_else(not_editable)?;
            Ok(editable.save_custom_document_as(entry.document.clone(), &target_resource, cancellation))
        })
    }

    fn rpc_backup(&self, resource: &URI, view_type: &str, cancellation: CancellationToken) -> Promise<String> {
        let inner = self.inner.clone();
        let (resource, view_type) = (resource.clone(), view_type.to_owned());
        async_fn(async move {
            let entry = inner.get_custom_document_entry(&view_type, &resource)?;
            let provider = inner.get_custom_editor_provider(&view_type)?;
            let editable = provider.as_editable().ok_or_else(not_editable)?;

            let context = CustomDocumentBackupContext { destination: entry.get_new_backup_uri()? };
            let backup = await_maybe(editable.backup_custom_document(entry.document.clone(), context, cancellation)).await?;
            let id = backup.id();
            entry.update_backup(backup);
            Ok(id)
        })
    }

    fn rpc_on_move_custom_editor(&self, handle: &str, new_resource: &URI, view_type: &str) -> Promise<()> {
        let inner = self.inner.clone();
        let (handle, new_resource, view_type) = (handle.to_owned(), new_resource.clone(), view_type.to_owned());
        async_fn(async move {
            let entry = inner.get_provider(&view_type)?;

            let move_provider = match &entry.provider {
                EditorProvider::Text(provider) => provider.as_move(),
                EditorProvider::Custom(_) => None,
            };
            let Some(move_provider) = move_provider else {
                return Err(Error::new(format!("Provider does not implement move '{view_type}'")));
            };

            let Some(webview) = inner.ext_host_webview_panels.get_webview_panel(&handle) else {
                return Err(Error::new("No webview found"));
            };

            let document = inner.ext_host_documents.get_document(&new_resource)?;
            await_maybe(move_provider.move_custom_text_editor(document, webview, CancellationToken::None)).await
        })
    }
}

// ---- auxiliares

/// `typeConverters.ViewColumn.to(position)`, que lança numa posição que não é de coluna.
fn view_column_to(position: EditorGroupColumn) -> Result<f64, Error> {
    errors::catch(|| ViewColumn::to(position))
}

/// O erro de `getCustomEditorProvider` para um provedor ausente ou somente leitura.
fn not_editable() -> Error {
    Error::new("Custom document is not editable")
}

/// `hashPath(resource)`: o hash do caminho (arquivo e sem título) ou da URI, como texto.
fn hash_path(resource: &URI) -> String {
    let text: JsString = if resource.scheme() == schemas::FILE || resource.scheme() == schemas::UNTITLED {
        resource.fs_path(Platform::detect().path_flavor())
    } else {
        resource.to_string(false)
    };
    hash(&Dynamic::from(text)).to_string()
}
