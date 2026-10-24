//! `extHostTypeConverters.ts`: `ViewBadge`, `DataTransferItem` e `DataTransfer`, a conversão entre os tipos da API de
//! árvores de visões e os DTOs do protocolo.
//!
//! Divergências:
//! - `DataTransferItem.from` e `DataTransfer.from` aceitam só o `vscode.DataTransferItem` da API
//!   ([`types::DataTransferItem`]). O `IDataTransferItem` interno de `je_base::data_transfer` (que `fromList` usa,
//!   com o `id` do próprio item) entra junto do primeiro consumidor dele.
//! - O `resolveFileData: (id) => Promise<Uint8Array>` é um fecho `Rc` sobre `&str`; `createSingleCallFunction` é a
//!   memória da primeira promessa devolvida.
//! - `URI.revive(file.uri)` e `URI.revive(part)` são o próprio [`URI`] (os DTOs de Rust já guardam [`URI`]).
//! - `URI.parse(part)` de `serializeUriList` não lança na prática; se lançar, o trecho continua texto, como o `catch`
//!   do original.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::data_transfer::{uri_list, UriListEntry};
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::mime::mimes;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;

use crate::api::common::ext_host_protocol::{DataTransferDto, DataTransferFileDto, DataTransferItemDto, UriListPart};
use crate::api::common::ext_host_types as types;
use crate::common::views::IViewBadge;

/// `resolveFileData`: o conteúdo de um arquivo da transferência a partir do `id` dele.
pub type ResolveFileData = Rc<dyn Fn(&str) -> Promise<Vec<u8>>>;

/// `typeConverters.ViewBadge`.
pub struct ViewBadge;

impl ViewBadge {
    /// `from(badge)`: um selo ausente lê como ausente.
    pub fn from(badge: Option<&types::ViewBadge>) -> Option<IViewBadge> {
        badge.map(|badge| IViewBadge { value: badge.value, tooltip: badge.tooltip.clone() })
    }
}

/// `typeConverters.DataTransferItem`.
pub struct DataTransferItem;

impl DataTransferItem {
    /// `to(mime, item, resolveFileData)`.
    pub fn to(mime: &str, item: &DataTransferItemDto, resolve_file_data: &ResolveFileData) -> Rc<types::DataTransferItem> {
        if let Some(file) = &item.file_data {
            let resolve = resolve_file_data.clone();
            let file_id = file.id.clone();
            let first_call: RefCell<Option<Promise<Vec<u8>>>> = RefCell::new(None);
            let get_data = move || first_call.borrow_mut().get_or_insert_with(|| resolve(&file_id)).clone();
            return types::DataTransferItem::new_internal_file(Rc::new(types::DataTransferFile::new(file.name.clone(), file.uri.clone(), file.id.clone(), get_data)));
        }

        if mime == mimes::URI_LIST
            && let Some(uri_list_data) = &item.uri_list_data
        {
            return types::DataTransferItem::new_internal(Dynamic::string(&revive_uri_list(uri_list_data)));
        }

        types::DataTransferItem::new_internal(Dynamic::string(&item.as_string))
    }

    /// `from(mime, item, id = generateUuid())`.
    pub fn from(mime: &str, item: &Rc<types::DataTransferItem>, id: Option<String>) -> Promise<DataTransferItemDto> {
        let (mime, item) = (mime.to_owned(), item.clone());
        async_fn(async move {
            let id = id.unwrap_or_else(generate_uuid);
            let string_value = item.as_string().await?;

            if mime == mimes::URI_LIST {
                return Ok(DataTransferItemDto { id, uri_list_data: Some(serialize_uri_list(&string_value)), as_string: string_value, file_data: None });
            }

            let file_value = item.as_file();
            Ok(DataTransferItemDto {
                id,
                as_string: string_value,
                file_data: file_value.map(|file| DataTransferFileDto { name: file.name.clone(), uri: file.uri.clone(), id: file.item_id.clone() }),
                uri_list_data: None,
            })
        })
    }
}

impl DataTransferItem {
    /// `from(mime, item, id)` sobre o `IDataTransferItem` interno de `je_base::data_transfer` (o que `fromList` usa):
    /// a mesma conversão de [`DataTransferItem::from`], com o arquivo do `asFile()` interno.
    pub fn from_internal(mime: &str, item: &je_base::data_transfer::DataTransferItem, id: Option<String>) -> Promise<DataTransferItemDto> {
        let (mime, item) = (mime.to_owned(), item.clone());
        async_fn(async move {
            let id = id.unwrap_or_else(generate_uuid);
            let string_value = item.as_string().await?;

            if mime == mimes::URI_LIST {
                return Ok(DataTransferItemDto { id, uri_list_data: Some(serialize_uri_list(&string_value)), as_string: string_value, file_data: None });
            }

            let file_value = item.as_file();
            Ok(DataTransferItemDto {
                id,
                as_string: string_value,
                file_data: file_value.map(|file| DataTransferFileDto { name: file.name.clone(), uri: file.uri.clone(), id: file.id.clone() }),
                uri_list_data: None,
            })
        })
    }
}

impl DataTransfer {
    /// `fromList(dataTransfer)`: o `id` de cada item é o do próprio item interno.
    pub fn from_list(data_transfer: &je_base::data_transfer::VSDataTransfer) -> Promise<DataTransferDto> {
        let entries: Vec<(String, je_base::data_transfer::DataTransferItem)> =
            data_transfer.iter().map(|(mime, item)| (mime.to_owned(), item.clone())).collect();
        async_fn(async move {
            let promises: Vec<Promise<(String, DataTransferItemDto)>> = entries
                .into_iter()
                .map(|(mime, value)| {
                    async_fn(async move {
                        let id = value.id.clone();
                        let item = DataTransferItem::from_internal(&mime, &value, id).await?;
                        Ok((mime, item))
                    })
                })
                .collect();
            Ok(DataTransferDto { items: Promise::all(promises).await? })
        })
    }
}

fn serialize_uri_list(string_value: &str) -> Vec<UriListPart> {
    uri_list::split(string_value)
        .into_iter()
        .map(|part| {
            if part.starts_with('#') {
                return UriListPart::Text(part);
            }
            match errors::catch(|| URI::parse(part.as_str(), false)) {
                Ok(uri) => UriListPart::Uri(uri),
                Err(_) => UriListPart::Text(part),
            }
        })
        .collect()
}

fn revive_uri_list(parts: &[UriListPart]) -> String {
    let entries: Vec<UriListEntry> = parts
        .iter()
        .map(|part| match part {
            UriListPart::Text(text) => UriListEntry::String(text.clone()),
            UriListPart::Uri(uri) => UriListEntry::Uri(uri.clone()),
        })
        .collect();
    uri_list::create(&entries)
}

/// `typeConverters.DataTransfer`.
pub struct DataTransfer;

impl DataTransfer {
    /// `toDataTransfer(value, resolveFileData)`.
    pub fn to_data_transfer(value: &DataTransferDto, resolve_file_data: &ResolveFileData) -> Rc<types::DataTransfer> {
        let init: Vec<(String, Rc<types::DataTransferItem>)> =
            value.items.iter().map(|(type_, item)| (type_.clone(), DataTransferItem::to(type_, item, resolve_file_data))).collect();
        types::DataTransfer::new(init)
    }

    /// `from(dataTransfer)`.
    pub fn from(data_transfer: &Rc<types::DataTransfer>) -> Promise<DataTransferDto> {
        let entries = data_transfer.iter();
        async_fn(async move {
            let promises: Vec<Promise<(String, DataTransferItemDto)>> = entries
                .into_iter()
                .map(|(mime, value)| {
                    async_fn(async move {
                        let item = DataTransferItem::from(&mime, &value, None).await?;
                        Ok((mime, item))
                    })
                })
                .collect();
            Ok(DataTransferDto { items: Promise::all(promises).await? })
        })
    }
}
