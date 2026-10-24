//! `DocumentDropEditAdapter` e `DocumentPasteEditProvider` de `extHostLanguageFeatures.ts`.
//!
//! Divergências:
//! - O `resolveFileData` de `prepareDocumentPaste` (`() => { throw new NotImplementedError(); }`) devolve uma promessa
//!   rejeitada com o mesmo erro (a função devolve `Promise`).
//! - `(await proxy.$resolve...FileData(...)).buffer` é `VSBuffer::to_vec`.
//! - A `vscode.DocumentDropEdit` e a `vscode.DocumentPasteEdit` são o [`DocumentDropOrPasteEdit`]; o `insertText` convertido
//!   (`string | { snippet }`) e o `yieldTo` e o `additionalEdit` do DTO saem das funções [`insert_text_dto`],
//!   [`yield_to_values`] e [`additional_edit_dto`], compartilhadas pelos dois adaptadores (no original, o mesmo código
//!   escrito duas vezes).
//! - O `Promise.all` de `prepareDocumentPaste` espera todas as promessas dos itens, na ordem.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_extensions::scanner::ExtensionDescription;

use super::ExtHostLanguageFeatures;
use crate::api::common::cache::Cache;
use crate::api::common::ext_host_documents::ExtHostDocuments;
use crate::api::common::ext_host_language_features_api::{
    ApiDocumentDropEditProvider, ApiDocumentPasteContext, ApiDocumentPasteEditProvider, DocumentDropOrPasteEdit, DocumentDropOrPasteEditKind,
    DocumentEditInsertText, DocumentEditRef, OneOrMany,
};
use crate::api::common::ext_host_protocol::{
    ChainedCacheId, DataTransferDto, DropPasteInsertTextDto, EditKindDto, IDocumentDropEditDto, IDocumentPasteContextDto, IPasteEditDto, IWorkspaceEditDto,
    MainThreadLanguageFeaturesShape, ResolvedDropEditDto, ResolvedPasteEditDto,
};
use crate::api::common::ext_host_type_converters::{self as type_convert, DataTransfer, DataTransferItem, ResolveFileData};
use crate::api::common::ext_host_types::{self as types};

/// `typeof edit.insertText === 'string' ? edit.insertText : { snippet: edit.insertText.value }`.
fn insert_text_dto(insert_text: &DocumentEditInsertText) -> DropPasteInsertTextDto {
    match insert_text {
        DocumentEditInsertText::Text(text) => DropPasteInsertTextDto::Text(text.clone()),
        DocumentEditInsertText::Snippet(snippet) => DropPasteInsertTextDto::Snippet(snippet.value.clone()),
    }
}

/// `edit.yieldTo?.map(x => x.value)`.
fn yield_to_values(edit: &DocumentDropOrPasteEdit) -> Option<Vec<String>> {
    edit.yield_to.as_ref().map(|kinds| kinds.iter().map(|kind| kind.value.clone()).collect())
}

/// `edit.additionalEdit ? typeConvert.WorkspaceEdit.from(edit.additionalEdit, undefined) : undefined`.
fn additional_edit_dto(edit: &DocumentDropOrPasteEdit) -> Option<IWorkspaceEditDto> {
    edit.additional_edit.as_ref().map(|additional_edit| type_convert::WorkspaceEdit::from(additional_edit, None))
}

/// O `resolveFileData` que pede os dados de um arquivo à thread principal.
fn resolve_file_data(proxy: &Rc<dyn MainThreadLanguageFeaturesShape>, handle: u32, request_id: u32, drop: bool) -> ResolveFileData {
    let proxy = proxy.clone();
    Rc::new(move |id: &str| {
        let buffer = if drop {
            proxy.rpc_resolve_document_on_drop_file_data(handle, request_id, id)
        } else {
            proxy.rpc_resolve_paste_file_data(handle, request_id, id)
        };
        buffer.then(|buffer| buffer.to_vec())
    })
}

/// `DocumentDropEditAdapter`.
pub struct DocumentDropEditAdapter {
    cache: Cache<DocumentEditRef>,
    proxy: Rc<dyn MainThreadLanguageFeaturesShape>,
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentDropEditProvider>,
    handle: u32,
    extension: Rc<ExtensionDescription>,
}

impl DocumentDropEditAdapter {
    pub fn new(
        proxy: Rc<dyn MainThreadLanguageFeaturesShape>,
        documents: Rc<ExtHostDocuments>,
        provider: Rc<dyn ApiDocumentDropEditProvider>,
        handle: u32,
        extension: Rc<ExtensionDescription>,
    ) -> Rc<Self> {
        Rc::new(Self { cache: Cache::new("DocumentDropEdit"), proxy, documents, provider, handle, extension })
    }

    pub fn provide_document_on_drop_edits(
        self: &Rc<Self>,
        request_id: u32,
        uri: URI,
        position: EditorPosition,
        data_transfer_dto: DataTransferDto,
        token: CancellationToken,
    ) -> Promise<Option<Vec<IDocumentDropEditDto>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&uri)?;
            let pos = type_convert::Position::to(&position)?;
            let data_transfer = DataTransfer::to_data_transfer(&data_transfer_dto, &resolve_file_data(&this.proxy, this.handle, request_id, true));

            let edits = await_maybe(this.provider.provide_document_drop_edits(&doc, &pos, &data_transfer, token)).await?;
            let Some(edits) = edits else {
                return Ok(None);
            };

            let edits_array = match edits {
                OneOrMany::One(edit) => vec![edit],
                OneOrMany::Many(edits) => edits,
            };
            if edits_array.is_empty() {
                return Ok(None);
            }
            let cache_id = this.cache.add(edits_array.clone());

            let extension_label = ExtHostLanguageFeatures::ext_label(&this.extension);
            Ok(Some(
                edits_array
                    .iter()
                    .enumerate()
                    .map(|(i, edit)| IDocumentDropEditDto {
                        cache_id: Some((cache_id, i as u32)),
                        title: edit.title.clone().unwrap_or_else(|| localize("defaultDropLabel", "Drop using '{0}' extension", &[extension_label.as_str()])),
                        kind: edit.kind.as_ref().map(|kind| kind.value.clone()),
                        yield_to: yield_to_values(edit),
                        insert_text: insert_text_dto(&edit.insert_text),
                        additional_edit: additional_edit_dto(edit),
                    })
                    .collect(),
            ))
        })
    }

    pub fn resolve_drop_edit(self: &Rc<Self>, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedDropEditDto> {
        let this = self.clone();
        async_fn(async move {
            let (session_id, item_id) = id;
            let item = this.cache.get(session_id, item_id as usize);
            let Some(item) = item.filter(|_| this.provider.has_resolve_document_drop_edit()) else {
                return Ok(ResolvedDropEditDto::default()); // this should not happen...
            };

            let resolved_item = await_maybe(this.provider.resolve_document_drop_edit(&item, token)).await?.unwrap_or(item);
            Ok(ResolvedDropEditDto { additional_edit: additional_edit_dto(&resolved_item) })
        })
    }

    pub fn release_drop_edits(&self, id: u32) {
        self.cache.delete(id);
    }
}

/// `DocumentPasteEditProvider`.
pub struct DocumentPasteEditProvider {
    cached_prepare: RefCell<Option<HashMap<String, Rc<types::DataTransferItem>>>>,
    edits_cache: Cache<DocumentEditRef>,
    proxy: Rc<dyn MainThreadLanguageFeaturesShape>,
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentPasteEditProvider>,
    handle: u32,
    extension: Rc<ExtensionDescription>,
}

impl DocumentPasteEditProvider {
    pub fn new(
        proxy: Rc<dyn MainThreadLanguageFeaturesShape>,
        documents: Rc<ExtHostDocuments>,
        provider: Rc<dyn ApiDocumentPasteEditProvider>,
        handle: u32,
        extension: Rc<ExtensionDescription>,
    ) -> Rc<Self> {
        Rc::new(Self { cached_prepare: RefCell::new(None), edits_cache: Cache::new("DocumentPasteEdit.edits"), proxy, documents, provider, handle, extension })
    }

    pub fn prepare_document_paste(
        self: &Rc<Self>,
        resource: URI,
        ranges: Vec<EditorRange>,
        data_transfer_dto: DataTransferDto,
        token: CancellationToken,
    ) -> Promise<Option<DataTransferDto>> {
        let this = self.clone();
        async_fn(async move {
            if !this.provider.has_prepare_document_paste() {
                return Ok(None);
            }

            *this.cached_prepare.borrow_mut() = None;

            let doc = this.documents.get_document(&resource)?;
            let vscode_ranges = ranges.iter().map(type_convert::Range::to).collect::<Result<Vec<_>, _>>()?;

            let not_implemented: ResolveFileData = Rc::new(|_| Promise::rejected(Error::new("NotImplemented")));
            let data_transfer = DataTransfer::to_data_transfer(&data_transfer_dto, &not_implemented);
            await_maybe(this.provider.prepare_document_paste(&doc, &vscode_ranges, &data_transfer, token.clone())).await?;
            if token.is_cancellation_requested() {
                return Ok(None);
            }

            // Only send back values that have been added to the data transfer
            let new_entries: Vec<(String, Rc<types::DataTransferItem>)> = data_transfer.iter().into_iter().filter(|(_, value)| !value.is_internal()).collect();

            // Store off original data transfer items so we can retrieve them on paste
            let mut new_cache: HashMap<String, Rc<types::DataTransferItem>> = HashMap::new();

            let mut promises = Vec::with_capacity(new_entries.len());
            for (mime, value) in new_entries {
                let id = generate_uuid();
                new_cache.insert(id.clone(), value.clone());
                promises.push(async_fn(async move {
                    let item = DataTransferItem::from(&mime, &value, Some(id)).await?;
                    Ok((mime, item))
                }));
            }
            let items = Promise::all(promises).await?;

            *this.cached_prepare.borrow_mut() = Some(new_cache);

            Ok(Some(DataTransferDto { items }))
        })
    }

    pub fn provide_paste_edits(
        self: &Rc<Self>,
        request_id: u32,
        resource: URI,
        ranges: Vec<EditorRange>,
        data_transfer_dto: DataTransferDto,
        context: IDocumentPasteContextDto,
        token: CancellationToken,
    ) -> Promise<Option<Vec<IPasteEditDto>>> {
        let this = self.clone();
        async_fn(async move {
            if !this.provider.has_provide_document_paste_edits() {
                return Ok(Some(Vec::new()));
            }

            let doc = this.documents.get_document(&resource)?;
            let vscode_ranges = ranges.iter().map(type_convert::Range::to).collect::<Result<Vec<_>, _>>()?;

            let resolve = resolve_file_data(&this.proxy, this.handle, request_id, false);
            let items: Vec<(String, Rc<types::DataTransferItem>)> = data_transfer_dto
                .items
                .iter()
                .map(|(mime, value)| {
                    let cached = this.cached_prepare.borrow().as_ref().and_then(|cache| cache.get(&value.id).cloned());
                    match cached {
                        Some(cached) => (mime.clone(), cached),
                        None => (mime.clone(), DataTransferItem::to(mime, value, &resolve)),
                    }
                })
                .collect();

            let data_transfer = types::DataTransfer::new(items);

            let paste_context = ApiDocumentPasteContext { only: context.only.as_deref().map(DocumentDropOrPasteEditKind::new), trigger_kind: context.trigger_kind };
            let edits = await_maybe(this.provider.provide_document_paste_edits(&doc, &vscode_ranges, &data_transfer, &paste_context, token.clone())).await?;
            let Some(edits) = edits.filter(|edits| !edits.is_empty() && !token.is_cancellation_requested()) else {
                return Ok(Some(Vec::new()));
            };

            let cache_id = this.edits_cache.add(edits.clone());

            let extension_label = ExtHostLanguageFeatures::ext_label(&this.extension);
            Ok(Some(
                edits
                    .iter()
                    .enumerate()
                    .map(|(i, edit)| IPasteEditDto {
                        cache_id: Some((cache_id, i as u32)),
                        title: edit.title.clone().unwrap_or_else(|| localize("defaultPasteLabel", "Paste using '{0}' extension", &[extension_label.as_str()])),
                        kind: edit.kind.as_ref().map(|kind| EditKindDto { value: kind.value.clone() }),
                        yield_to: yield_to_values(edit),
                        insert_text: insert_text_dto(&edit.insert_text),
                        additional_edit: additional_edit_dto(edit),
                    })
                    .collect(),
            ))
        })
    }

    pub fn resolve_paste_edit(self: &Rc<Self>, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedPasteEditDto> {
        let this = self.clone();
        async_fn(async move {
            let (session_id, item_id) = id;
            let item = this.edits_cache.get(session_id, item_id as usize);
            let Some(item) = item.filter(|_| this.provider.has_resolve_document_paste_edit()) else {
                return Ok(ResolvedPasteEditDto::default()); // this should not happen...
            };

            let resolved_item = await_maybe(this.provider.resolve_document_paste_edit(&item, token)).await?.unwrap_or(item);
            Ok(ResolvedPasteEditDto { insert_text: Some(insert_text_dto(&resolved_item.insert_text)), additional_edit: additional_edit_dto(&resolved_item) })
        })
    }

    pub fn release_paste_edits(&self, id: u32) {
        self.edits_cache.delete(id);
    }
}
