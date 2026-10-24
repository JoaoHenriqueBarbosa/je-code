//! Porte de `src/vs/editor/contrib/suggest/browser/suggestMemory.ts`: a memória de qual sugestão selecionar
//! (`first`, `recentlyUsed`, `recentlyUsedByPrefix`) e o serviço que guarda as memórias entre sessões.
//!
//! Diferenças do original, nenhuma de comportamento:
//! - A classe abstrata `Memory` é o trait [`Memory`]; o `select` com corpo é o método padrão do trait, e o
//!   `super.select(...)` de [`LruMemory`] e [`PrefixMemory`] é [`select_top_preselected`], que guarda o corpo.
//! - `toJSON` e `fromJSON` trabalham com o [`Value`] do `je-base`. O `fromJSON` aplica as entradas na ordem e
//!   para na primeira malformada (o original lança no mesmo ponto), devolvendo [`MalformedMemory`].
//! - `MemItem.type` (`string | CompletionItemKind`) é sempre o [`CompletionItemKind`]: o texto lido do JSON passa
//!   por `CompletionItemKinds.fromString`.
//! - `JSON.stringify(strategy)` com `toJSON` devolvendo `undefined` (o [`NoMemory`]) não gera texto, e o
//!   `store(key, undefined)` do original remove a chave: o serviço chama `remove`.
//! - O valor de `editor.suggestSelection` que não é texto vale `undefined` no original: nenhuma estratégia
//!   tem esse nome, a memória é recriada a cada chamada e a chave de armazenamento termina em `undefined`.
//! - O `_serviceBrand` e o `registerSingleton` não existem; o serviço recebe o armazenamento e a configuração
//!   em [`SuggestMemoryService::new`].
//! - A `LRUMemory` e a `PrefixMemory` do original guardam no `touch` do item uma referência que o `toJSON`
//!   normaliza no próprio item da árvore; aqui o `toJSON` normaliza a cópia que devolve (o `touch` da árvore
//!   só é lido de novo depois de um `fromJSON`, que o refaz).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::RunOnceScheduler;
use je_base::dynamic::{Dynamic, Truthy, json_stringify};
use je_base::js_string::is_js_whitespace;
use je_base::json::{Object, Value, parse_str};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::map::LRUCache;
use je_base::ternary_search_tree::TernarySearchTree;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget, WillSaveStateReason};

use crate::cursor_common::CursorSimpleModel;
use crate::languages::CompletionItemKind;
use crate::position::Position;
use crate::suggest::suggest::CompletionItem;
use crate::text_model::TextModel;

/// `MemMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemMode {
    First,
    RecentlyUsed,
    RecentlyUsedByPrefix,
}

impl MemMode {
    /// O texto do modo, como na configuração `editor.suggestSelection`.
    pub fn as_str(self) -> &'static str {
        match self {
            MemMode::First => "first",
            MemMode::RecentlyUsed => "recentlyUsed",
            MemMode::RecentlyUsedByPrefix => "recentlyUsedByPrefix",
        }
    }

    /// O modo de um texto da configuração; `None` para o que o original não conhece.
    pub fn parse(mode: &str) -> Option<MemMode> {
        match mode {
            "first" => Some(MemMode::First),
            "recentlyUsed" => Some(MemMode::RecentlyUsed),
            "recentlyUsedByPrefix" => Some(MemMode::RecentlyUsedByPrefix),
            _ => None,
        }
    }
}

/// O dado lido por `fromJSON` não tem a forma que o `toJSON` escreve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MalformedMemory;

/// `Memory.select`: o primeiro item do grupo de maior pontuação marcado como `preselect`, ou o 0. É o corpo da
/// classe base, que [`LruMemory`] e [`PrefixMemory`] chamam como `super.select`.
pub fn select_top_preselected(items: &[CompletionItem]) -> usize {
    if items.is_empty() {
        return 0;
    }
    let top_score = items[0].score.borrow()[0];
    for (i, item) in items.iter().enumerate() {
        if item.score.borrow()[0] != top_score {
            // stop when leaving the group of top matches
            break;
        }
        if item.completion.borrow().preselect == Some(true) {
            // stop when seeing an auto-select-item
            return i;
        }
    }
    0
}

/// `Memory`.
pub trait Memory {
    fn name(&self) -> MemMode;

    fn select(&self, _model: &TextModel, _pos: Position, items: &[CompletionItem]) -> usize {
        select_top_preselected(items)
    }

    fn memorize(&mut self, model: &TextModel, pos: Position, item: &CompletionItem);

    fn to_json(&self) -> Option<Value>;

    fn from_json(&mut self, data: &Value) -> Result<(), MalformedMemory>;
}

/// `NoMemory`.
pub struct NoMemory;

impl NoMemory {
    pub fn new() -> Self {
        NoMemory
    }
}

impl Default for NoMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for NoMemory {
    fn name(&self) -> MemMode {
        MemMode::First
    }

    fn memorize(&mut self, _model: &TextModel, _pos: Position, _item: &CompletionItem) {
        // no-op
    }

    fn to_json(&self) -> Option<Value> {
        None
    }

    fn from_json(&mut self, _data: &Value) -> Result<(), MalformedMemory> {
        Ok(())
    }
}

/// `MemItem`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemItem {
    pub kind: CompletionItemKind,
    pub insert_text: String,
    pub touch: usize,
}

/// A entrada `[chave, item]` do `toJSON`. O `LRUMemory` escreve `touch, type, insertText` e o `PrefixMemory`
/// escreve `type, insertText, touch`: `touch_first` escolhe a ordem das chaves.
fn entry_to_value(key: &str, item: &MemItem, touch_first: bool) -> Value {
    let touch = ("touch", Value::Number(item.touch as f64));
    let kind = ("type", Value::Number(item.kind.to_index() as f64));
    let insert_text = ("insertText", Value::String(JsString::from(item.insert_text.as_str())));
    let fields = if touch_first { [touch, kind, insert_text] } else { [kind, insert_text, touch] };
    Value::Array(vec![Value::String(JsString::from(key)), Value::Object(Object::from_iter(fields))])
}

/// A entrada `[chave, item]` de volta; o `type` vem como número ou como o nome do tipo.
fn entry_from_value(entry: &Value) -> Result<(String, MemItem), MalformedMemory> {
    let Value::Array(pair) = entry else { return Err(MalformedMemory) };
    let [Value::String(key), Value::Object(item)] = pair.as_slice() else { return Err(MalformedMemory) };
    let kind = match item.get("type") {
        Some(Value::Number(index)) => CompletionItemKind::from_index(*index as usize).ok_or(MalformedMemory)?,
        Some(Value::String(name)) => CompletionItemKind::from_string(&name.to_string_lossy(), false).ok_or(MalformedMemory)?,
        _ => return Err(MalformedMemory),
    };
    let Some(Value::String(insert_text)) = item.get("insertText") else { return Err(MalformedMemory) };
    let touch = match item.get("touch") {
        Some(Value::Number(touch)) => *touch as usize,
        _ => return Err(MalformedMemory),
    };
    Ok((key.to_string_lossy(), MemItem { kind, insert_text: insert_text.to_string_lossy(), touch }))
}

/// `/\s$/.test(model.getLineContent(pos.lineNumber).substr(pos.column - 10, pos.column - 1))`: o `substr`
/// começa em `column - 10` (de trás para frente quando negativo) e leva `column - 1` unidades.
fn line_suffix_ends_with_whitespace(model: &TextModel, pos: Position) -> bool {
    let line = model.get_line_content(pos.line_number);
    let units = line.units();
    let length = units.len() as i64;
    let start = i64::from(pos.column) - 10;
    let start = if start < 0 { (length + start).max(0) } else { start.min(length) };
    let end = (start + i64::from(pos.column) - 1).min(length);
    end > start && is_js_whitespace(units[(end - 1) as usize])
}

/// `LRUMemory`.
pub struct LruMemory {
    cache: LRUCache<String, MemItem>,
    seq: usize,
}

impl LruMemory {
    pub fn new() -> Self {
        Self { cache: LRUCache::new(300, 0.66), seq: 0 }
    }
}

impl Default for LruMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for LruMemory {
    fn name(&self) -> MemMode {
        MemMode::RecentlyUsed
    }

    fn memorize(&mut self, model: &TextModel, _pos: Position, item: &CompletionItem) {
        let key = format!("{}/{}", model.get_language_id(), item.text_label);
        let completion = item.completion.borrow();
        self.cache.set(key, MemItem { touch: self.seq, kind: completion.kind, insert_text: completion.insert_text.clone() });
        self.seq += 1;
    }

    fn select(&self, model: &TextModel, pos: Position, items: &[CompletionItem]) -> usize {
        if items.is_empty() {
            return 0;
        }

        if line_suffix_ends_with_whitespace(model, pos) {
            return select_top_preselected(items);
        }

        let top_score = items[0].score.borrow()[0];
        let index_preselect: Option<usize> = None;
        let mut index_recency: Option<usize> = None;
        let mut seq: i64 = -1;
        for (i, item) in items.iter().enumerate() {
            if item.score.borrow()[0] != top_score {
                // consider only top items
                break;
            }
            let key = format!("{}/{}", model.get_language_id(), item.text_label);
            let completion = item.completion.borrow();
            if let Some(memorized) = self.cache.peek(&key)
                && memorized.touch as i64 > seq
                && memorized.kind == completion.kind
                && memorized.insert_text == completion.insert_text
            {
                seq = memorized.touch as i64;
                index_recency = Some(i);
            }
            if completion.preselect == Some(true) && index_preselect.is_none() {
                // stop when seeing an auto-select-item
                return i;
            }
        }
        index_recency.or(index_preselect).unwrap_or(0)
    }

    fn to_json(&self) -> Option<Value> {
        Some(Value::Array(self.cache.to_json().iter().map(|(key, item)| entry_to_value(key, item, true)).collect()))
    }

    fn from_json(&mut self, data: &Value) -> Result<(), MalformedMemory> {
        self.cache.clear();
        let Value::Array(entries) = data else { return Err(MalformedMemory) };
        for entry in entries {
            let (key, mut value) = entry_from_value(entry)?;
            value.touch = 0;
            self.cache.set(key, value);
        }
        self.seq = self.cache.size();
        Ok(())
    }
}

/// `PrefixMemory`.
pub struct PrefixMemory {
    trie: TernarySearchTree<String, MemItem>,
    seq: usize,
}

impl PrefixMemory {
    pub fn new() -> Self {
        Self { trie: TernarySearchTree::for_strings(), seq: 0 }
    }
}

impl Default for PrefixMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for PrefixMemory {
    fn name(&self) -> MemMode {
        MemMode::RecentlyUsedByPrefix
    }

    fn memorize(&mut self, model: &TextModel, pos: Position, item: &CompletionItem) {
        let word = model.get_word_until_position(pos).word;
        let key = format!("{}/{}", model.get_language_id(), word.to_string_lossy());
        let completion = item.completion.borrow();
        self.trie.set(key, MemItem { kind: completion.kind, insert_text: completion.insert_text.clone(), touch: self.seq });
        self.seq += 1;
    }

    fn select(&self, model: &TextModel, pos: Position, items: &[CompletionItem]) -> usize {
        let word = model.get_word_until_position(pos).word;
        if word.is_empty() {
            return select_top_preselected(items);
        }
        let key = format!("{}/{}", model.get_language_id(), word.to_string_lossy());
        let item = self.trie.get(&key).or_else(|| self.trie.find_substr(&key));
        if let Some(item) = item {
            for (i, candidate) in items.iter().enumerate() {
                let completion = candidate.completion.borrow();
                if completion.kind == item.kind && completion.insert_text == item.insert_text {
                    return i;
                }
            }
        }
        select_top_preselected(items)
    }

    fn to_json(&self) -> Option<Value> {
        let mut entries: Vec<(String, MemItem)> = Vec::new();
        self.trie.for_each(|value, key| entries.push((key.clone(), value.clone())));

        // sort by last recently used (touch), then
        // take the top 200 item and normalize their
        // touch
        entries.sort_by(|a, b| b.1.touch.cmp(&a.1.touch));
        for (i, entry) in entries.iter_mut().enumerate() {
            entry.1.touch = i;
        }

        Some(Value::Array(entries.iter().take(200).map(|(key, item)| entry_to_value(key, item, false)).collect()))
    }

    fn from_json(&mut self, data: &Value) -> Result<(), MalformedMemory> {
        self.trie.clear();
        let Value::Array(entries) = data else { return Err(MalformedMemory) };
        if let Some(first) = entries.first() {
            self.seq = entry_from_value(first)?.1.touch + 1;
            for entry in entries {
                let (key, value) = entry_from_value(entry)?;
                self.trie.set(key, value);
            }
        }
        Ok(())
    }
}

/// `ISuggestMemoryService`.
pub trait ISuggestMemoryService {
    fn memorize(&self, model: &TextModel, pos: Position, item: &CompletionItem);
    fn select(&self, model: &TextModel, pos: Position, items: &[CompletionItem]) -> usize;
}

const STORAGE_PREFIX: &str = "suggest/memories";

/// `SuggestMemoryService`.
pub struct SuggestMemoryService {
    storage_service: Rc<dyn StorageService>,
    config_service: Rc<dyn IConfigurationService>,
    persist_soon: Rc<RunOnceScheduler>,
    disposables: Rc<DisposableStore>,
    strategy: RefCell<Option<Box<dyn Memory>>>,
    /// O valor de `editor.suggestSelection` com que a estratégia foi criada (a chave de armazenamento dela).
    strategy_mode: RefCell<Option<String>>,
}

impl SuggestMemoryService {
    pub fn new(storage_service: Rc<dyn StorageService>, config_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let persist_target = weak.clone();
            let persist_soon = RunOnceScheduler::new(
                move || {
                    if let Some(service) = persist_target.upgrade() {
                        service.save_state();
                    }
                },
                500,
            );
            let disposables = DisposableStore::new();
            let save_target = weak.clone();
            storage_service.on_will_save_state().listen_in(
                move |event| {
                    if event.reason == WillSaveStateReason::Shutdown
                        && let Some(service) = save_target.upgrade()
                    {
                        service.save_state();
                    }
                },
                &disposables,
            );
            Self {
                storage_service,
                config_service,
                persist_soon,
                disposables,
                strategy: RefCell::new(None),
                strategy_mode: RefCell::new(None),
            }
        })
    }

    /// `ctor` de `_strategyCtors.get(mode) || NoMemory`.
    fn create_strategy(mode: Option<&str>) -> Box<dyn Memory> {
        match mode.and_then(MemMode::parse) {
            Some(MemMode::RecentlyUsedByPrefix) => Box::new(PrefixMemory::new()),
            Some(MemMode::RecentlyUsed) => Box::new(LruMemory::new()),
            Some(MemMode::First) | None => Box::new(NoMemory::new()),
        }
    }

    fn storage_scope(&self) -> StorageScope {
        let share = self.config_service.get_value(Some("editor.suggest.shareSuggestSelections"), None).is_truthy();
        if share { StorageScope::Profile } else { StorageScope::Workspace }
    }

    fn storage_key(mode: &Option<String>) -> String {
        format!("{STORAGE_PREFIX}/{}", mode.as_deref().unwrap_or("undefined"))
    }

    /// `_withStrategy(model, pos)`: troca a estratégia quando a configuração mudou (guardando a antiga e lendo
    /// a nova do armazenamento) e roda `action` nela.
    fn with_strategy<R>(&self, model: &TextModel, pos: Position, action: impl FnOnce(&mut dyn Memory) -> R) -> R {
        let overrides = ConfigurationOverrides {
            override_identifier: Some(JsString::from(model.get_language_id_at_position(pos.line_number, pos.column))),
            resource: Some(model.uri()),
        };
        let mode = match self.config_service.get_value(Some("editor.suggestSelection"), Some(&overrides)) {
            Dynamic::String(mode) => Some(mode.to_string_lossy()),
            _ => None,
        };

        let current_name = self.strategy.borrow().as_ref().map(|strategy| strategy.name().as_str());
        if current_name.is_none() || current_name != mode.as_deref() {
            self.save_state();
            let mut strategy = Self::create_strategy(mode.as_deref());

            let scope = self.storage_scope();
            if let Some(raw) = self.storage_service.get(&Self::storage_key(&mode), scope, None).filter(|raw| !raw.is_empty()) {
                // things can go wrong with JSON...
                let mut errors = Vec::new();
                if let Some(data) = parse_str(&raw, &mut errors).filter(|_| errors.is_empty()) {
                    let _ = strategy.from_json(&data);
                }
            }
            *self.strategy.borrow_mut() = Some(strategy);
            *self.strategy_mode.borrow_mut() = mode;
        }

        let mut strategy = self.strategy.borrow_mut();
        action(strategy.as_mut().expect("estratégia criada").as_mut())
    }

    /// `_saveState()`.
    fn save_state(&self) {
        let strategy = self.strategy.borrow();
        if let Some(strategy) = strategy.as_ref() {
            let key = Self::storage_key(&self.strategy_mode.borrow());
            let scope = self.storage_scope();
            match strategy.to_json().and_then(|data| json_stringify(&Dynamic::from_json(&data))) {
                Some(raw) => self.storage_service.store(&key, raw.as_str().into(), scope, StorageTarget::Machine),
                None => self.storage_service.remove(&key, scope),
            }
        }
    }
}

impl ISuggestMemoryService for SuggestMemoryService {
    fn memorize(&self, model: &TextModel, pos: Position, item: &CompletionItem) {
        self.with_strategy(model, pos, |strategy| strategy.memorize(model, pos, item));
        self.persist_soon.schedule(None);
    }

    fn select(&self, model: &TextModel, pos: Position, items: &[CompletionItem]) -> usize {
        self.with_strategy(model, pos, |strategy| strategy.select(model, pos, items))
    }
}

impl IDisposable for SuggestMemoryService {
    fn dispose(&self) {
        self.disposables.dispose();
        self.persist_soon.dispose();
    }
}
