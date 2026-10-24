//! `MainThreadPasteEditProvider` e `MainThreadDocumentOnDropEditProvider`, de `mainThreadLanguageFeatures.ts`. O
//! `DataTransferFileCache` de `api/common/shared/dataTransferCache.ts` está em
//! `crate::api::common::shared::data_transfer_cache`, porque o `MainThreadTreeViews` também o usa.
//!
//! Divergências:
//! - O `resolveDocumentDropEdit` do original chama `$resolvePasteEdit` (e não `$resolveDropEdit`) com o `_cacheId` da edição
//!   de soltar; o porte repete a chamada do original.
//! - Os campos opcionais do provedor de colar (`prepareDocumentPaste?`, `provideDocumentPasteEdits?`,
//!   `resolveDocumentPasteEdit?`) são os `has_*` do trait, ligados pelo `supportsCopy`, `supportsPaste` e `supportsResolve`.
//! - `selections: Selection[]` do `$providePasteEdits` (um `Selection` é um `IRange`) vai como o [`Range`] de cada seleção.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::data_transfer::{VSDataTransfer, create_string_data_transfer_item};
use je_base::errors::Error;
use je_base::hierarchical_kind::HierarchicalKind;
use je_base::promise::Promise;
use je_editor::language_features_service::{
    DocumentDropEdit, DocumentDropEditProvider, DocumentDropEditRef, DocumentDropEditsSession, DocumentEditInsertText, DocumentPasteContext,
    DocumentPasteEdit, DocumentPasteEditProvider, DocumentPasteEditRef, DocumentPasteEditsSession, DropYieldTo,
};
use je_editor::language_providers::ProviderResult;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::providers::{Remote, ResolveDataTransferFile, revive_workspace_edit_dto};
use crate::api::common::ext_host_protocol::{
    DropPasteInsertTextDto, IDocumentDropEditDto, IDocumentDropEditProviderMetadata, IDocumentPasteContextDto, IPasteEditDto, IPasteEditProviderMetadataDto,
};
use crate::api::common::ext_host_type_converters::DataTransfer;
use crate::api::common::shared::data_transfer_cache::DataTransferFileCache;

fn kind_or_empty(kind: Option<&str>) -> HierarchicalKind {
    HierarchicalKind::new(kind.unwrap_or(""))
}

fn yield_to(yield_to: &Option<Vec<String>>) -> Option<Vec<DropYieldTo>> {
    yield_to.as_ref().map(|kinds| kinds.iter().map(|kind| DropYieldTo { kind: HierarchicalKind::new(kind.clone()) }).collect())
}

fn insert_text(text: &DropPasteInsertTextDto) -> DocumentEditInsertText {
    match text {
        DropPasteInsertTextDto::Text(text) => DocumentEditInsertText::Text(text.clone()),
        DropPasteInsertTextDto::Snippet(snippet) => DocumentEditInsertText::Snippet(snippet.clone()),
    }
}

/// `MainThreadPasteEditProvider`.
pub(super) struct MainThreadPasteEditProvider {
    remote: Remote,
    data_transfers: Rc<DataTransferFileCache>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    copy_mime_types: Vec<String>,
    paste_mime_types: Vec<String>,
    provided_paste_edit_kinds: Vec<HierarchicalKind>,
    supports_copy: bool,
    supports_paste: bool,
    supports_resolve: bool,
}

impl MainThreadPasteEditProvider {
    pub fn new(handle: u32, proxy: Rc<dyn crate::api::common::ext_host_protocol::ExtHostLanguageFeaturesShape>, metadata: &IPasteEditProviderMetadataDto, uri_identity_service: Rc<dyn IUriIdentityService>) -> Rc<Self> {
        Rc::new(Self {
            remote: Remote { handle, proxy },
            data_transfers: Rc::new(DataTransferFileCache::default()),
            uri_identity_service,
            copy_mime_types: metadata.copy_mime_types.clone().unwrap_or_default(),
            paste_mime_types: metadata.paste_mime_types.clone().unwrap_or_default(),
            provided_paste_edit_kinds: metadata
                .provided_paste_edit_kinds
                .as_ref()
                .map(|kinds| kinds.iter().map(|kind| HierarchicalKind::new(kind.clone())).collect())
                .unwrap_or_default(),
            supports_copy: metadata.supports_copy,
            supports_paste: metadata.supports_paste,
            supports_resolve: metadata.supports_resolve,
        })
    }

    /// `resolveFileData(requestId, dataId)`.
    pub fn resolve_file_data(&self, request_id: u32, data_id: &str) -> Promise<VSBuffer> {
        self.data_transfers.resolve_file_data(request_id, data_id)
    }
}

fn paste_edit_from_dto(
    edit: &IPasteEditDto,
    uri_identity_service: &Rc<dyn IUriIdentityService>,
    resolve: &ResolveDataTransferFile,
) -> Result<DocumentPasteEdit, Error> {
    let additional_edit = match &edit.additional_edit {
        Some(additional_edit) => Some(revive_workspace_edit_dto(additional_edit, uri_identity_service, Some(resolve))?),
        None => None,
    };
    Ok(DocumentPasteEdit {
        title: edit.title.clone(),
        kind: kind_or_empty(edit.kind.as_ref().map(|kind| kind.value.as_str())),
        yield_to: yield_to(&edit.yield_to),
        insert_text: insert_text(&edit.insert_text),
        additional_edit,
        cache_id: edit.cache_id,
    })
}

impl DocumentPasteEditProvider for MainThreadPasteEditProvider {
    fn copy_mime_types(&self) -> &[String] {
        &self.copy_mime_types
    }

    fn paste_mime_types(&self) -> &[String] {
        &self.paste_mime_types
    }

    fn provided_paste_edit_kinds(&self) -> &[HierarchicalKind] {
        &self.provided_paste_edit_kinds
    }

    fn has_prepare_document_paste(&self) -> bool {
        self.supports_copy
    }

    fn prepare_document_paste(
        &self,
        model: &Rc<TextModel>,
        selections: &[Range],
        data_transfer: &Rc<VSDataTransfer>,
        token: &CancellationToken,
    ) -> ProviderResult<Rc<VSDataTransfer>> {
        let (remote, uri, selections, data_transfer, token) = (self.remote.clone(), model.uri(), selections.to_vec(), Rc::clone(data_transfer), token.clone());
        async_fn(async move {
            let data_transfer_dto = DataTransfer::from_list(&data_transfer).await?;
            if token.is_cancellation_requested() {
                return Ok(None);
            }

            let Some(new_data_transfer) = remote.proxy.rpc_prepare_document_paste(remote.handle, &uri, selections, data_transfer_dto, token).await? else {
                return Ok(None);
            };

            let mut data_transfer_out = VSDataTransfer::new();
            for (type_, item) in new_data_transfer.items {
                data_transfer_out.replace(&type_, create_string_data_transfer_item(item.as_string, Some(item.id)));
            }
            Ok(Some(Rc::new(data_transfer_out)))
        })
    }

    fn has_provide_document_paste_edits(&self) -> bool {
        self.supports_paste
    }

    fn provide_document_paste_edits(
        &self,
        model: &Rc<TextModel>,
        selections: &[Selection],
        data_transfer: &Rc<VSDataTransfer>,
        context: &DocumentPasteContext,
        token: &CancellationToken,
    ) -> ProviderResult<Rc<DocumentPasteEditsSession>> {
        let (remote, uri, data_transfer, token) = (self.remote.clone(), model.uri(), Rc::clone(data_transfer), token.clone());
        let ranges: Vec<Range> = selections.iter().map(|selection| **selection).collect();
        let (cache, uri_identity_service) = (Rc::clone(&self.data_transfers), Rc::clone(&self.uri_identity_service));
        let context = IDocumentPasteContextDto { only: context.only.as_ref().map(|only| only.value.clone()), trigger_kind: paste_trigger_kind(context) };
        async_fn(async move {
            let request = cache.add(&data_transfer);
            let result: Result<Option<Rc<DocumentPasteEditsSession>>, Error> = async {
                let data_transfer_dto = DataTransfer::from_list(&data_transfer).await?;
                if token.is_cancellation_requested() {
                    return Ok(None);
                }

                let Some(edits) = remote.proxy.rpc_provide_paste_edits(remote.handle, request.id, &uri, ranges, data_transfer_dto, context, token).await? else {
                    return Ok(None);
                };

                let request_id = request.id;
                let resolve: ResolveDataTransferFile = {
                    let cache = Rc::clone(&cache);
                    Rc::new(move |data_id: &str| cache.resolve_file_data(request_id, data_id))
                };
                let mut revived: Vec<DocumentPasteEditRef> = Vec::with_capacity(edits.len());
                for edit in &edits {
                    revived.push(Rc::new(RefCell::new(paste_edit_from_dto(edit, &uri_identity_service, &resolve)?)));
                }
                let cache_id = edits.first().and_then(|edit| edit.cache_id).map(|cache_id| cache_id.0);
                Ok(Some(Rc::new(DocumentPasteEditsSession::new(revived, move || {
                    if let Some(cache_id) = cache_id {
                        remote.proxy.rpc_release_paste_edits(remote.handle, cache_id);
                    }
                }))))
            }
            .await;
            request.dispose();
            result
        })
    }

    fn has_resolve_document_paste_edit(&self) -> bool {
        self.supports_resolve
    }

    fn resolve_document_paste_edit(&self, edit: &DocumentPasteEditRef, token: &CancellationToken) -> ProviderResult<DocumentPasteEditRef> {
        let (remote, edit, token) = (self.remote.clone(), Rc::clone(edit), token.clone());
        let uri_identity_service = Rc::clone(&self.uri_identity_service);
        let cache_id = edit.borrow().cache_id;
        async_fn(async move {
            let Some(cache_id) = cache_id else {
                return Err(Error::new("Cannot resolve a paste edit without a cache id"));
            };
            let resolved = remote.proxy.rpc_resolve_paste_edit(remote.handle, cache_id, token).await?;
            if let Some(text) = &resolved.insert_text {
                edit.borrow_mut().insert_text = insert_text(text);
            }
            if let Some(additional_edit) = &resolved.additional_edit {
                edit.borrow_mut().additional_edit = Some(revive_workspace_edit_dto(additional_edit, &uri_identity_service, None)?);
            }
            Ok(Some(edit))
        })
    }
}

fn paste_trigger_kind(context: &DocumentPasteContext) -> crate::api::common::ext_host_types::DocumentPasteTriggerKind {
    use crate::api::common::ext_host_types::DocumentPasteTriggerKind;
    use je_editor::language_features_service::DocumentPasteTriggerKind as EditorKind;
    match context.trigger_kind {
        EditorKind::Automatic => DocumentPasteTriggerKind::Automatic,
        EditorKind::PasteAs => DocumentPasteTriggerKind::PasteAs,
    }
}

/// `MainThreadDocumentOnDropEditProvider`.
pub(super) struct MainThreadDocumentOnDropEditProvider {
    remote: Remote,
    data_transfers: Rc<DataTransferFileCache>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    drop_mime_types: Vec<String>,
    provided_drop_edit_kinds: Option<Vec<HierarchicalKind>>,
    supports_resolve: bool,
}

impl MainThreadDocumentOnDropEditProvider {
    pub fn new(
        handle: u32,
        proxy: Rc<dyn crate::api::common::ext_host_protocol::ExtHostLanguageFeaturesShape>,
        metadata: Option<&IDocumentDropEditProviderMetadata>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        Rc::new(Self {
            remote: Remote { handle, proxy },
            data_transfers: Rc::new(DataTransferFileCache::default()),
            uri_identity_service,
            drop_mime_types: metadata.map(|metadata| metadata.drop_mime_types.clone()).unwrap_or_else(|| vec!["*/*".to_owned()]),
            provided_drop_edit_kinds: metadata
                .and_then(|metadata| metadata.provided_drop_kinds.as_ref())
                .map(|kinds| kinds.iter().map(|kind| HierarchicalKind::new(kind.clone())).collect()),
            supports_resolve: metadata.is_some_and(|metadata| metadata.supports_resolve),
        })
    }

    /// `resolveDocumentOnDropFileData(requestId, dataId)`.
    pub fn resolve_document_on_drop_file_data(&self, request_id: u32, data_id: &str) -> Promise<VSBuffer> {
        self.data_transfers.resolve_file_data(request_id, data_id)
    }
}

fn drop_edit_from_dto(
    edit: &IDocumentDropEditDto,
    uri_identity_service: &Rc<dyn IUriIdentityService>,
    resolve: &ResolveDataTransferFile,
) -> Result<DocumentDropEdit, Error> {
    let additional_edit = match &edit.additional_edit {
        Some(additional_edit) => Some(revive_workspace_edit_dto(additional_edit, uri_identity_service, Some(resolve))?),
        None => None,
    };
    Ok(DocumentDropEdit {
        title: edit.title.clone(),
        kind: edit.kind.as_ref().map(|kind| HierarchicalKind::new(kind.clone())),
        yield_to: yield_to(&edit.yield_to),
        insert_text: insert_text(&edit.insert_text),
        additional_edit,
        cache_id: edit.cache_id,
    })
}

impl DocumentDropEditProvider for MainThreadDocumentOnDropEditProvider {
    fn drop_mime_types(&self) -> &[String] {
        &self.drop_mime_types
    }

    fn provided_drop_edit_kinds(&self) -> Option<&[HierarchicalKind]> {
        self.provided_drop_edit_kinds.as_deref()
    }

    fn provide_document_drop_edits(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        data_transfer: &Rc<VSDataTransfer>,
        token: &CancellationToken,
    ) -> ProviderResult<Rc<DocumentDropEditsSession>> {
        let (remote, uri, data_transfer, token) = (self.remote.clone(), model.uri(), Rc::clone(data_transfer), token.clone());
        let (cache, uri_identity_service) = (Rc::clone(&self.data_transfers), Rc::clone(&self.uri_identity_service));
        async_fn(async move {
            let request = cache.add(&data_transfer);
            let result: Result<Option<Rc<DocumentDropEditsSession>>, Error> = async {
                let data_transfer_dto = DataTransfer::from_list(&data_transfer).await?;
                if token.is_cancellation_requested() {
                    return Ok(None);
                }

                let Some(edits) = remote.proxy.rpc_provide_document_on_drop_edits(remote.handle, request.id, &uri, position, data_transfer_dto, token).await? else {
                    return Ok(None);
                };

                let request_id = request.id;
                let resolve: ResolveDataTransferFile = {
                    let cache = Rc::clone(&cache);
                    Rc::new(move |data_id: &str| cache.resolve_file_data(request_id, data_id))
                };
                let mut revived: Vec<DocumentDropEditRef> = Vec::with_capacity(edits.len());
                for edit in &edits {
                    revived.push(Rc::new(RefCell::new(drop_edit_from_dto(edit, &uri_identity_service, &resolve)?)));
                }
                let cache_id = edits.first().and_then(|edit| edit.cache_id).map(|cache_id| cache_id.0);
                Ok(Some(Rc::new(DocumentDropEditsSession::new(revived, move || {
                    if let Some(cache_id) = cache_id {
                        remote.proxy.rpc_release_document_on_drop_edits(remote.handle, cache_id);
                    }
                }))))
            }
            .await;
            request.dispose();
            result
        })
    }

    fn has_resolve_document_drop_edit(&self) -> bool {
        self.supports_resolve
    }

    fn resolve_document_drop_edit(&self, edit: &DocumentDropEditRef, token: &CancellationToken) -> ProviderResult<DocumentDropEditRef> {
        let (remote, edit, token) = (self.remote.clone(), Rc::clone(edit), token.clone());
        let uri_identity_service = Rc::clone(&self.uri_identity_service);
        let cache_id = edit.borrow().cache_id;
        async_fn(async move {
            let Some(cache_id) = cache_id else {
                return Err(Error::new("Cannot resolve a drop edit without a cache id"));
            };
            let resolved = remote.proxy.rpc_resolve_paste_edit(remote.handle, cache_id, token).await?;
            if let Some(additional_edit) = &resolved.additional_edit {
                edit.borrow_mut().additional_edit = Some(revive_workspace_edit_dto(additional_edit, &uri_identity_service, None)?);
            }
            Ok(Some(edit))
        })
    }
}
