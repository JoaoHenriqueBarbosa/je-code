//! Porte de `src/vs/workbench/contrib/notebook/browser/services/notebookKernelHistoryServiceImpl.ts`: o `NotebookKernelHistoryService`,
//! que guarda, por tipo de notebook, os kernels usados mais recentemente (até cinco) e os persiste no armazenamento do workspace.
//!
//! Divergências:
//! - `registerAction2(...)` de `notebook.clearNotebookKernelsMRUCache` (que faz `accessor.get(INotebookKernelHistoryService) as
//!   NotebookKernelHistoryService` e chama `_clear()`) não é portado: o acessor devolve o contrato, não o tipo concreto. [`NotebookKernelHistoryService::clear`]
//!   é o `_clear()`.
//! - `{ [key: string]: LinkedMap<string, string> }` é um `Vec` de pares na ordem de inserção do objeto, com cada `LinkedMap` compartilhado.
//! - `JSON.parse` é o `parse_str` do je-base (que aceita comentários e vírgula final): o texto só vale sem erro de leitura, e qualquer
//!   forma inesperada (o `for...of` de um valor que não é lista lança no original) cai no `catch` e limpa o mapa.
//! - `JSON.stringify` é montado à mão com o `json_quote`.
//! - `this._register(this._storageService.onDidChangeValue(StorageScope.WORKSPACE, KEY, this._store)(...))` usa o `DisposableStore` do serviço.
//! - O construtor devolve `Rc<Self>` (os ouvintes guardam um `Weak`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::json_quote;
use je_base::json::{Value, parse_str};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::map::{LinkedMap, Touch};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

use crate::contrib::notebook::common::notebook_kernel_service::{
    INotebookKernel, INotebookKernelHistory, INotebookKernelHistoryService, INotebookKernelService, INotebookTextModelLike,
};
use crate::contrib::notebook::common::notebook_logging_service::INotebookLoggingService;

const MAX_KERNELS_IN_HISTORY: usize = 5;

const STORAGE_KEY: &str = "notebook.kernelHistory";

type MostRecentKernels = Vec<(String, Rc<LinkedMap<String, String>>)>;

/// `NotebookKernelHistoryService`.
pub struct NotebookKernelHistoryService {
    this: Weak<NotebookKernelHistoryService>,
    store: Rc<DisposableStore>,
    storage_service: Rc<dyn StorageService>,
    notebook_kernel_service: Rc<dyn INotebookKernelService>,
    notebook_logging_service: Rc<dyn INotebookLoggingService>,
    most_recent_kernels_map: RefCell<MostRecentKernels>,
}

impl NotebookKernelHistoryService {
    pub fn new(
        storage_service: Rc<dyn StorageService>,
        notebook_kernel_service: Rc<dyn INotebookKernelService>,
        notebook_logging_service: Rc<dyn INotebookLoggingService>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            store: DisposableStore::new(),
            storage_service: storage_service.clone(),
            notebook_kernel_service,
            notebook_logging_service,
            most_recent_kernels_map: RefCell::new(Vec::new()),
        });

        this.load_state();
        let weak = this.this.clone();
        this.store.add(storage_service.on_will_save_state().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.save_state();
            }
        }));
        let weak = this.this.clone();
        let change_listener = storage_service.on_did_change_value(StorageScope::Workspace, Some(STORAGE_KEY), &this.store).listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.load_state();
            }
        });
        this.store.add(change_listener);
        this
    }

    fn recent_kernels_of(&self, view_type: &str) -> Option<Rc<LinkedMap<String, String>>> {
        self.most_recent_kernels_map.borrow().iter().find(|(key, _)| key == view_type).map(|(_, kernels)| kernels.clone())
    }

    fn save_state(&self) {
        let not_empty = self.most_recent_kernels_map.borrow().iter().any(|(_, kernels)| kernels.size() > 0);

        if not_empty {
            let serialized = self.serialize();
            self.storage_service.store(STORAGE_KEY, StorageValue::String(serialized), StorageScope::Workspace, StorageTarget::User);
        } else {
            self.storage_service.remove(STORAGE_KEY, StorageScope::Workspace);
        }
    }

    fn load_state(&self) {
        let serialized = self.storage_service.get(STORAGE_KEY, StorageScope::Workspace, None);
        match serialized.filter(|serialized| !serialized.is_empty()) {
            Some(serialized) => {
                let mut errors = Vec::new();
                let parsed = parse_str(&serialized, &mut errors).filter(|_| errors.is_empty());
                if !self.deserialize(parsed) {
                    self.most_recent_kernels_map.borrow_mut().clear();
                }
            }
            None => self.most_recent_kernels_map.borrow_mut().clear(),
        }
    }

    /// `JSON.stringify(this._serialize())`.
    fn serialize(&self) -> String {
        let entries: Vec<String> = self
            .most_recent_kernels_map
            .borrow()
            .iter()
            .map(|(view_type, kernels)| {
                let values: Vec<String> = kernels.values().map(|kernel_id| json_quote(&JsString::from(kernel_id.as_str()))).collect();
                format!("{}:{{\"entries\":[{}]}}", json_quote(&JsString::from(view_type.as_str())), values.join(","))
            })
            .collect();
        format!("{{{}}}", entries.join(","))
    }

    /// `_deserialize(serialized)`: `false` quando a forma é inesperada (o `throw` que o `catch` de `_loadState` trata).
    fn deserialize(&self, serialized: Option<Value>) -> bool {
        let Some(Value::Object(serialized)) = serialized else {
            return false;
        };
        let mut result: MostRecentKernels = Vec::new();

        for (view_type, kernels) in serialized.iter() {
            let Value::Object(kernels) = kernels else { return false };
            let Some(Value::Array(entries)) = kernels.get("entries") else { return false };
            let mut map_values: Vec<(String, String)> = Vec::new();
            for entry in entries {
                let Value::String(entry) = entry else { return false };
                let entry = entry.to_string_lossy();
                map_values.push((entry.clone(), entry));
            }

            let linked_map = Rc::new(LinkedMap::<String, String>::new());
            linked_map.from_json(map_values);
            result.push((view_type.to_string_lossy(), linked_map));
        }
        *self.most_recent_kernels_map.borrow_mut() = result;
        true
    }

    /// `_clear()`.
    pub fn clear(&self) {
        self.most_recent_kernels_map.borrow_mut().clear();
        self.save_state();
    }
}

impl INotebookKernelHistoryService for NotebookKernelHistoryService {
    fn get_kernels(&self, notebook: &dyn INotebookTextModelLike) -> INotebookKernelHistory {
        let all_available_kernels = self.notebook_kernel_service.get_matching_kernel(notebook);
        let all_kernels = &all_available_kernels.all;
        let selected_kernel = all_available_kernels.selected.clone();
        // We will suggest the only kernel
        let suggested = if all_kernels.len() == 1 { Some(all_kernels[0].clone()) } else { None };
        let label_of = |kernel: &Option<Rc<dyn INotebookKernel>>| kernel.as_ref().map_or_else(|| "undefined".to_string(), |kernel| kernel.label());
        self.notebook_logging_service.debug(
            "History",
            &format!(
                "getMatchingKernels: {} kernels available for {}. Selected: {}. Suggested: {}",
                all_kernels.len(),
                notebook.uri().path().to_string_lossy(),
                label_of(&all_available_kernels.selected),
                label_of(&suggested)
            ),
        );
        let most_recent_kernel_ids: Vec<String> = self.recent_kernels_of(&notebook.notebook_type()).map(|kernels| kernels.values().collect()).unwrap_or_default();
        let all: Vec<Rc<dyn INotebookKernel>> =
            most_recent_kernel_ids.iter().filter_map(|kernel_id| all_kernels.iter().find(|kernel| kernel.id() == *kernel_id).cloned()).collect();
        self.notebook_logging_service.debug("History", &format!("mru: {} kernels in history, {} registered already.", most_recent_kernel_ids.len(), all.len()));

        INotebookKernelHistory { selected: selected_kernel.or(suggested), all }
    }

    fn add_most_recent_kernel(&self, kernel: &Rc<dyn INotebookKernel>) {
        let key = kernel.id();
        let view_type = kernel.view_type();
        let recent_kernels = self.recent_kernels_of(&view_type).unwrap_or_else(|| Rc::new(LinkedMap::<String, String>::new()));

        recent_kernels.set(key.clone(), key, Touch::AsOld);

        if recent_kernels.size() > MAX_KERNELS_IN_HISTORY {
            let reserved: Vec<(String, String)> = recent_kernels.to_json().into_iter().take(MAX_KERNELS_IN_HISTORY).collect();
            recent_kernels.from_json(reserved);
        }

        let mut map = self.most_recent_kernels_map.borrow_mut();
        match map.iter_mut().find(|(key, _)| *key == view_type) {
            Some(slot) => slot.1 = recent_kernels,
            None => map.push((view_type, recent_kernels)),
        }
    }
}

impl IDisposable for NotebookKernelHistoryService {
    fn dispose(&self) {
        self.store.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookKernelHistoryService"
    }
}
