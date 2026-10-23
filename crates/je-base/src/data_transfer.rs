//! Porte de `src/vs/base/common/dataTransfer.ts`: o `DataTransfer` do editor (arrastar e soltar,
//! colar), com entradas de texto e de arquivo por tipo MIME.
//!
//! Diferenças de forma: `IDataTransferItem` e `IDataTransferFile` são structs que guardam os
//! `asString`/`asFile`/`data` do original como closures; `value: unknown` é um [`Dynamic`];
//! `Thenable<string>` e `Promise<Uint8Array>` são [`Promise`]; o iterador `[Symbol.iterator]` é
//! [`VSDataTransfer::iter`], que devolve os pares `(mime, item)` já reunidos.

use std::rc::Rc;

use crate::arrays::distinct;
use crate::dynamic::Dynamic;
use crate::iterator;
use crate::promise::Promise;
use crate::uri::URI;
use crate::uuid::generate_uuid;

/// `IDataTransferFile`.
#[derive(Clone)]
pub struct DataTransferFile {
    pub id: String,
    pub name: String,
    pub uri: Option<URI>,
    data: Rc<dyn Fn() -> Promise<Vec<u8>>>,
}

impl DataTransferFile {
    /// `data()`.
    pub fn data(&self) -> Promise<Vec<u8>> {
        (self.data)()
    }
}

/// `IDataTransferItem`.
#[derive(Clone)]
pub struct DataTransferItem {
    pub id: Option<String>,
    as_string: Rc<dyn Fn() -> Promise<String>>,
    as_file: Rc<dyn Fn() -> Option<DataTransferFile>>,
    pub value: Dynamic,
}

impl DataTransferItem {
    /// `asString()`.
    pub fn as_string(&self) -> Promise<String> {
        (self.as_string)()
    }

    /// `asFile()`.
    pub fn as_file(&self) -> Option<DataTransferFile> {
        (self.as_file)()
    }
}

/// O `string | Promise<string>` de `createStringDataTransferItem`.
#[derive(Clone)]
pub enum StringOrPromise {
    String(String),
    Promise(Promise<String>),
}

impl From<&str> for StringOrPromise {
    fn from(text: &str) -> Self {
        Self::String(text.to_string())
    }
}

impl From<String> for StringOrPromise {
    fn from(text: String) -> Self {
        Self::String(text)
    }
}

impl From<Promise<String>> for StringOrPromise {
    fn from(promise: Promise<String>) -> Self {
        Self::Promise(promise)
    }
}

/// `createStringDataTransferItem`.
pub fn create_string_data_transfer_item(string_or_promise: impl Into<StringOrPromise>, id: Option<String>) -> DataTransferItem {
    let source = string_or_promise.into();
    let value = match &source {
        StringOrPromise::String(text) => Dynamic::string(text),
        StringOrPromise::Promise(_) => Dynamic::Undefined,
    };
    DataTransferItem {
        id,
        as_string: Rc::new(move || match &source {
            StringOrPromise::String(text) => Promise::resolved(text.clone()),
            StringOrPromise::Promise(promise) => promise.then(|text| text),
        }),
        as_file: Rc::new(|| None),
        value,
    }
}

/// `createFileDataTransferItem`.
pub fn create_file_data_transfer_item(
    file_name: &str,
    uri: Option<URI>,
    data: impl Fn() -> Promise<Vec<u8>> + 'static,
    id: Option<String>,
) -> DataTransferItem {
    let file = DataTransferFile { id: generate_uuid(), name: file_name.to_string(), uri, data: Rc::new(data) };
    DataTransferItem {
        id,
        as_string: Rc::new(|| Promise::resolved(String::new())),
        as_file: Rc::new(move || Some(file.clone())),
        value: Dynamic::Undefined,
    }
}

/// `VSDataTransfer`. As entradas mantêm a ordem de inserção dos tipos, como o `Map` do original.
#[derive(Default)]
pub struct VSDataTransfer {
    entries: Vec<(String, Vec<DataTransferItem>)>,
}

impl VSDataTransfer {
    pub fn new() -> Self {
        Self::default()
    }

    /// `size`: o número de tipos MIME (não de itens).
    pub fn size(&self) -> usize {
        self.entries.len()
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|(k, _)| k == key)
    }

    /// `has`: correspondência exata, sem curinga.
    pub fn has(&self, mime_type: &str) -> bool {
        self.position(&to_key(mime_type)).is_some()
    }

    /// `matches`: aceita curingas (`image/*`) e o tipo especial `files`, que casa com qualquer arquivo.
    pub fn matches(&self, pattern: &str) -> bool {
        let mut mimes: Vec<String> = self.entries.iter().map(|(k, _)| k.clone()).collect();
        if iterator::some(self.iter(), |(_, item), _| item.as_file().is_some()) {
            mimes.push("files".to_string());
        }
        matches_mime_type_normalized(&normalize_mime_type(pattern), &mimes)
    }

    /// `get`: a primeira entrada do tipo.
    pub fn get(&self, mime_type: &str) -> Option<DataTransferItem> {
        let index = self.position(&to_key(mime_type))?;
        self.entries[index].1.first().cloned()
    }

    /// `append`: acrescenta uma entrada sem substituir as existentes.
    ///
    /// Como no original, o tipo é procurado SEM normalizar a caixa (`this._entries.get(mimeType)`) e só
    /// guardado normalizado: acrescentar `Text/Plain` quando já existe `text/plain` substitui as entradas.
    pub fn append(&mut self, mime_type: &str, value: DataTransferItem) {
        if let Some(index) = self.position(mime_type) {
            self.entries[index].1.push(value);
        } else {
            self.replace(mime_type, value);
        }
    }

    /// `replace`: troca todas as entradas do tipo.
    pub fn replace(&mut self, mime_type: &str, value: DataTransferItem) {
        let key = to_key(mime_type);
        match self.position(&key) {
            Some(index) => self.entries[index].1 = vec![value],
            None => self.entries.push((key, vec![value])),
        }
    }

    /// `delete`: remove todas as entradas do tipo.
    pub fn delete(&mut self, mime_type: &str) {
        if let Some(index) = self.position(&to_key(mime_type)) {
            self.entries.remove(index);
        }
    }

    /// `[Symbol.iterator]`: todos os pares `(mime, item)`; pode haver vários itens por tipo.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &DataTransferItem)> {
        self.entries.iter().flat_map(|(mime, items)| items.iter().map(move |item| (mime.as_str(), item)))
    }
}

fn to_key(mime_type: &str) -> String {
    normalize_mime_type(mime_type)
}

fn normalize_mime_type(mime_type: &str) -> String {
    mime_type.to_lowercase()
}

/// `matchesMimeType`.
pub fn matches_mime_type(pattern: &str, mime_types: &[&str]) -> bool {
    let normalized: Vec<String> = mime_types.iter().map(|m| normalize_mime_type(m)).collect();
    matches_mime_type_normalized(&normalize_mime_type(pattern), &normalized)
}

fn matches_mime_type_normalized(normalized_pattern: &str, normalized_mime_types: &[String]) -> bool {
    // Qualquer coisa.
    if normalized_pattern == "*/*" {
        return !normalized_mime_types.is_empty();
    }

    // Correspondência exata.
    if normalized_mime_types.iter().any(|m| m == normalized_pattern) {
        return true;
    }

    // Curinga, como `image/*`: /^([a-z]+)\/([a-z]+|\*)$/i (letras ASCII, sem unicode).
    let Some((type_, subtype)) = normalized_pattern.split_once('/') else {
        return false;
    };
    let letters = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic());
    if !letters(type_) || !(letters(subtype) || subtype == "*") {
        return false;
    }
    if subtype == "*" {
        let prefix = format!("{type_}/");
        return normalized_mime_types.iter().any(|m| m.starts_with(&prefix));
    }
    false
}

/// Um elemento de `UriList.create`: `string | URI`.
pub enum UriListEntry {
    String(String),
    Uri(URI),
}

/// `UriList`: o formato `text/uri-list` (http://amundsen.com/hypermedia/urilist/).
pub mod uri_list {
    use super::*;

    /// `create`.
    pub fn create(entries: &[UriListEntry]) -> String {
        let strings: Vec<String> = entries
            .iter()
            .map(|entry| match entry {
                UriListEntry::String(s) => s.clone(),
                UriListEntry::Uri(uri) => uri.to_string(false).to_string_lossy(),
            })
            .collect();
        distinct(&strings, |s| s.clone()).join("\r\n")
    }

    /// `split`.
    pub fn split(text: &str) -> Vec<String> {
        text.split("\r\n").map(str::to_string).collect()
    }

    /// `parse`: sem as linhas de comentário (`#`).
    pub fn parse(text: &str) -> Vec<String> {
        split(text).into_iter().filter(|value| !value.starts_with('#')).collect()
    }
}
