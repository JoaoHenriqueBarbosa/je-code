//! Porte de `src/vs/workbench/api/common/shared/dataTransferCache.ts`: o cache dos arquivos de uma transferência de dados,
//! que o `MainThreadTreeViews` e os provedores de colar e soltar de `mainThreadLanguageFeatures.ts` guardam por requisição
//! até o host de extensões pedir o conteúdo (`$resolveDropFileData`, `$resolvePasteFileData`...).
//!
//! Divergências:
//! - `DataTransferFileCache.add` devolve `{ id, dispose }`: é o [`DataTransferRequest`], que guarda o cache por `Rc`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::data_transfer::{DataTransferFile, VSDataTransfer};
use je_base::errors::Error;
use je_base::promise::Promise;

/// `DataTransferFileCache`.
#[derive(Default)]
pub struct DataTransferFileCache {
    request_id_pool: Cell<u32>,
    data_transfer_files: RefCell<HashMap<u32, Vec<DataTransferFile>>>,
}

/// O `{ id, dispose }` que `DataTransferFileCache.add` devolve.
pub struct DataTransferRequest {
    pub id: u32,
    cache: Rc<DataTransferFileCache>,
}

impl DataTransferRequest {
    pub fn dispose(&self) {
        self.cache.data_transfer_files.borrow_mut().remove(&self.id);
    }
}

impl DataTransferFileCache {
    /// `add(dataTransfer)`.
    pub fn add(self: &Rc<Self>, data_transfer: &VSDataTransfer) -> DataTransferRequest {
        let request_id = self.request_id_pool.get();
        self.request_id_pool.set(request_id + 1);
        let files = data_transfer.iter().filter_map(|(_, item)| item.as_file()).collect();
        self.data_transfer_files.borrow_mut().insert(request_id, files);
        DataTransferRequest { id: request_id, cache: Rc::clone(self) }
    }

    /// `resolveFileData(requestId, dataItemId)`.
    pub fn resolve_file_data(&self, request_id: u32, data_item_id: &str) -> Promise<VSBuffer> {
        let found = {
            let files = self.data_transfer_files.borrow();
            match files.get(&request_id) {
                None => Err(Error::new("No data transfer found")),
                Some(files) => files.iter().find(|file| file.id == data_item_id).cloned().ok_or_else(|| Error::new("No matching file found in data transfer")),
            }
        };
        async_fn(async move { Ok(VSBuffer::wrap(found?.data().await?)) })
    }
}
