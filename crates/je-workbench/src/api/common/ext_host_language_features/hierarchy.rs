//! `CallHierarchyAdapter` e `TypeHierarchyAdapter` de `extHostLanguageFeatures.ts`.
//!
//! Divergências:
//! - Os dois adaptadores do original repetem a mesma gestão de sessões (`_idPool`, `_cache`, `_cacheAndConvertItem`,
//!   `_itemFromCache`, `releaseSession`); aqui ela é o [`HierarchyCache`], compartilhado, e só a conversão do item muda.
//! - `map.size.toString(36)` é [`to_base36`]. Um `sessionId` já liberado faz o `_cacheAndConvertItem` do original lançar
//!   `TypeError` ao ler `size` de `undefined`: devolve o mesmo erro.
//! - Os itens do cache são cópias (a classe da API é um valor aqui).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::id_generator::IdGenerator;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::position::Position as EditorPosition;

use crate::api::common::ext_host_documents::ExtHostDocuments;
use crate::api::common::ext_host_language_features_api::{ApiCallHierarchyProvider, ApiTypeHierarchyProvider, OneOrMany};
use crate::api::common::ext_host_protocol::{ICallHierarchyItemDto, IIncomingCallDto, IOutgoingCallDto, ITypeHierarchyItemDto};
use crate::api::common::ext_host_type_converters::{self as type_convert, HierarchyItemDto};
use crate::api::common::ext_host_types::{CallHierarchyItem, TypeHierarchyItem};

/// `number.toString(36)`.
fn to_base36(mut value: usize) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_owned();
    }
    let mut digits = Vec::new();
    while value > 0 {
        digits.push(DIGITS[value % 36]);
        value /= 36;
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_default()
}

/// O `_idPool` e o `_cache` dos dois adaptadores de hierarquia: sessão para itens (`Map<string, Map<string, T>>`).
struct HierarchyCache<T> {
    id_pool: IdGenerator,
    cache: RefCell<HashMap<String, HashMap<String, T>>>,
}

impl<T: Clone> HierarchyCache<T> {
    fn new() -> Self {
        Self { id_pool: IdGenerator::new(""), cache: RefCell::new(HashMap::new()) }
    }

    /// `const sessionId = this._idPool.nextId(); this._cache.set(sessionId, new Map());`.
    fn new_session(&self) -> String {
        let session_id = self.id_pool.next_id();
        self.cache.borrow_mut().insert(session_id.clone(), HashMap::new());
        session_id
    }

    /// `releaseSession`.
    fn release_session(&self, session_id: &str) {
        self.cache.borrow_mut().remove(session_id);
    }

    /// `_itemFromCache`.
    fn item_from_cache(&self, session_id: &str, item_id: &str) -> Option<T> {
        self.cache.borrow().get(session_id).and_then(|map| map.get(item_id).cloned())
    }

    /// `_cacheAndConvertItem`: `convert(item, sessionId, itemId)` é `typeConvert.XItem.from`.
    fn cache_and_convert_item(
        &self,
        session_id: &str,
        item: &T,
        convert: impl FnOnce(&T, &str, &str) -> Result<HierarchyItemDto, Error>,
    ) -> Result<HierarchyItemDto, Error> {
        let size = self
            .cache
            .borrow()
            .get(session_id)
            .map(HashMap::len)
            .ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'size')"))?;
        let dto = convert(item, session_id, &to_base36(size))?;
        if let Some(map) = self.cache.borrow_mut().get_mut(session_id) {
            map.insert(dto.item_id.clone(), item.clone());
        }
        Ok(dto)
    }
}

/// `CallHierarchyAdapter`.
pub struct CallHierarchyAdapter {
    cache: HierarchyCache<CallHierarchyItem>,
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiCallHierarchyProvider>,
}

impl CallHierarchyAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiCallHierarchyProvider>) -> Rc<Self> {
        Rc::new(Self { cache: HierarchyCache::new(), documents, provider })
    }

    fn cache_and_convert_item(&self, session_id: &str, item: &CallHierarchyItem) -> Result<ICallHierarchyItemDto, Error> {
        self.cache.cache_and_convert_item(session_id, item, |item, session_id, item_id| {
            type_convert::CallHierarchyItem::from(item, Some(session_id), Some(item_id))
        })
    }

    pub fn prepare_session(self: &Rc<Self>, uri: URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ICallHierarchyItemDto>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&uri)?;
            let pos = type_convert::Position::to(&position)?;

            let items = await_maybe(this.provider.prepare_call_hierarchy(&doc, &pos, token.clone())).await?;
            let Some(items) = items.filter(|_| !token.is_cancellation_requested()) else {
                return Ok(None);
            };

            let session_id = this.cache.new_session();

            match items {
                OneOrMany::Many(items) => Ok(Some(items.iter().map(|item| this.cache_and_convert_item(&session_id, item)).collect::<Result<Vec<_>, _>>()?)),
                OneOrMany::One(item) => Ok(Some(vec![this.cache_and_convert_item(&session_id, &item)?])),
            }
        })
    }

    pub fn provide_calls_to(self: &Rc<Self>, session_id: String, item_id: String, token: CancellationToken) -> Promise<Option<Vec<IIncomingCallDto>>> {
        let this = self.clone();
        async_fn(async move {
            let Some(item) = this.cache.item_from_cache(&session_id, &item_id) else {
                return Err(Error::new("missing call hierarchy item"));
            };
            let calls = await_maybe(this.provider.provide_call_hierarchy_incoming_calls(&item, token)).await?;
            let Some(calls) = calls else {
                return Ok(None);
            };
            let result = calls
                .iter()
                .map(|call| {
                    Ok(IIncomingCallDto {
                        from: this.cache_and_convert_item(&session_id, &call.from)?,
                        from_ranges: call.from_ranges.iter().map(type_convert::Range::from).collect(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            Ok(Some(result))
        })
    }

    pub fn provide_calls_from(self: &Rc<Self>, session_id: String, item_id: String, token: CancellationToken) -> Promise<Option<Vec<IOutgoingCallDto>>> {
        let this = self.clone();
        async_fn(async move {
            let Some(item) = this.cache.item_from_cache(&session_id, &item_id) else {
                return Err(Error::new("missing call hierarchy item"));
            };
            let calls = await_maybe(this.provider.provide_call_hierarchy_outgoing_calls(&item, token)).await?;
            let Some(calls) = calls else {
                return Ok(None);
            };
            let result = calls
                .iter()
                .map(|call| {
                    Ok(IOutgoingCallDto {
                        to: this.cache_and_convert_item(&session_id, &call.to)?,
                        from_ranges: call.from_ranges.iter().map(type_convert::Range::from).collect(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            Ok(Some(result))
        })
    }

    pub fn release_session(&self, session_id: &str) {
        self.cache.release_session(session_id);
    }
}

/// `TypeHierarchyAdapter`.
pub struct TypeHierarchyAdapter {
    cache: HierarchyCache<TypeHierarchyItem>,
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiTypeHierarchyProvider>,
}

impl TypeHierarchyAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiTypeHierarchyProvider>) -> Rc<Self> {
        Rc::new(Self { cache: HierarchyCache::new(), documents, provider })
    }

    fn cache_and_convert_item(&self, session_id: &str, item: &TypeHierarchyItem) -> Result<ITypeHierarchyItemDto, Error> {
        self.cache.cache_and_convert_item(session_id, item, |item, session_id, item_id| {
            type_convert::TypeHierarchyItem::from(item, Some(session_id), Some(item_id))
        })
    }

    pub fn prepare_session(self: &Rc<Self>, uri: URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&uri)?;
            let pos = type_convert::Position::to(&position)?;

            let items = await_maybe(this.provider.prepare_type_hierarchy(&doc, &pos, token.clone())).await?;
            let Some(items) = items.filter(|_| !token.is_cancellation_requested()) else {
                return Ok(None);
            };

            let session_id = this.cache.new_session();

            match items {
                OneOrMany::Many(items) => Ok(Some(items.iter().map(|item| this.cache_and_convert_item(&session_id, item)).collect::<Result<Vec<_>, _>>()?)),
                OneOrMany::One(item) => Ok(Some(vec![this.cache_and_convert_item(&session_id, &item)?])),
            }
        })
    }

    pub fn provide_supertypes(self: &Rc<Self>, session_id: String, item_id: String, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>> {
        let this = self.clone();
        async_fn(async move {
            let Some(item) = this.cache.item_from_cache(&session_id, &item_id) else {
                return Err(Error::new("missing type hierarchy item"));
            };
            let supertypes = await_maybe(this.provider.provide_type_hierarchy_supertypes(&item, token)).await?;
            let Some(supertypes) = supertypes else {
                return Ok(None);
            };
            Ok(Some(supertypes.iter().map(|supertype| this.cache_and_convert_item(&session_id, supertype)).collect::<Result<Vec<_>, _>>()?))
        })
    }

    pub fn provide_subtypes(self: &Rc<Self>, session_id: String, item_id: String, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>> {
        let this = self.clone();
        async_fn(async move {
            let Some(item) = this.cache.item_from_cache(&session_id, &item_id) else {
                return Err(Error::new("missing type hierarchy item"));
            };
            let subtypes = await_maybe(this.provider.provide_type_hierarchy_subtypes(&item, token)).await?;
            let Some(subtypes) = subtypes else {
                return Ok(None);
            };
            Ok(Some(subtypes.iter().map(|subtype| this.cache_and_convert_item(&session_id, subtype)).collect::<Result<Vec<_>, _>>()?))
        })
    }

    pub fn release_session(&self, session_id: &str) {
        self.cache.release_session(session_id);
    }
}
