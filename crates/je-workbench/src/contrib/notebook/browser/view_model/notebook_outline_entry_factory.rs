//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/notebookOutlineEntryFactory.ts`: monta as entradas do outline
//! de uma célula (cada cabeçalho de markdown, ou a primeira linha não vazia, mais os símbolos pré-carregados das células de
//! código).
//!
//! Divergências:
//! - `cell: ICellViewModel` é o [`CellViewModel`] (a única forma de `ICellViewModel` na camada lógica).
//! - `WeakMap<ICellViewModel, ...>` dos cabeçalhos em cache é um `HashMap` pelo `id` da célula (a camada lógica não tem coleta
//!   por referência fraca; o `alternativeId` continua sendo a chave de validade).
//! - Os três serviços injetados (`INotebookExecutionStateService`, `IOutlineModelService`, `ITextModelService`) são os
//!   `Rc<dyn ...>` do construtor; `INotebookOutlineEntryFactory` é o trait de mesmo nome.
//! - `cacheSymbols` devolve `Promise<()>` (o `try/finally` do original é o `finally` da promessa).
//! - `renderAsPlaintext({ value })` é `je_base::markdown_plaintext::render_as_plaintext`.

use std::cell::{LazyCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::markdown_plaintext::render_as_plaintext;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_editor::language_providers::{DocumentSymbol, SymbolKind};
use je_editor::outline_model::IOutlineModelService;
use je_editor::range::Range;
use zyarr::RegExp;

use super::folding_model::{MarkdownHeader, get_markdown_headers_in_cell};
use super::notebook_view_model_impl::CellViewModel;
use super::outline_entry::OutlineEntry;
use crate::contrib::notebook::common::notebook_common::CellKind;
use crate::contrib::notebook::common::notebook_execution_state_service::INotebookExecutionStateService;
use crate::services::textmodel_resolver::resolver_service::ITextModelService;

/// `NotebookOutlineConstants`.
pub struct NotebookOutlineConstants;

impl NotebookOutlineConstants {
    pub const NON_HEADER_OUTLINE_LEVEL: i64 = 7;
}

/// `entryDesc`.
#[derive(Clone)]
struct EntryDesc {
    name: String,
    range: Range,
    level: i64,
    kind: SymbolKind,
}

thread_local! {
    static HTML_HEADER_REGEX: LazyCell<RegExp> = LazyCell::new(|| {
        RegExp::from_str("<h([1-6]).*>(.*)<\\/h\\1>", "i").expect("regex de cabeçalho HTML válida")
    });
}

/// `fullContent.substring(0, max)` em unidades UTF-16.
fn substring_units(text: &str, max: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&units[..units.len().min(max)])
}

fn get_markdown_headers_in_cell_fallback_to_html_tags(full_content: &str) -> Vec<MarkdownHeader> {
    let mut headers = get_markdown_headers_in_cell(full_content);
    if !headers.is_empty() {
        return headers;
    }
    // no markdown syntax headers, try to find html tags
    let units: Vec<u16> = full_content.encode_utf16().collect();
    let matched = HTML_HEADER_REGEX.with(|regex| regex.exec(&units, 0));
    if let Some(matched) = matched {
        let text = |n: usize| matched.captures.get(n).cloned().flatten().map(|r| String::from_utf16_lossy(&units[r])).unwrap_or_default();
        let level = text(1).parse::<u8>().unwrap_or(0);
        headers.push(MarkdownHeader { depth: level, text: text(2).trim().to_string() });
    }
    headers
}

/// `INotebookOutlineEntryFactory`.
pub trait INotebookOutlineEntryFactory {
    fn get_outline_entries(&self, cell: &CellViewModel, index: usize) -> Vec<Rc<OutlineEntry>>;
    fn cache_symbols(&self, cell: &CellViewModel, cancel_token: &CancellationToken) -> Promise<()>;
}

je_instantiation::service_id!(pub I_NOTEBOOK_OUTLINE_ENTRY_FACTORY: dyn INotebookOutlineEntryFactory = "INotebookOutlineEntryFactory");

/// `NotebookOutlineEntryFactory`.
pub struct NotebookOutlineEntryFactory {
    execution_state_service: Rc<dyn INotebookExecutionStateService>,
    outline_model_service: Rc<dyn IOutlineModelService>,
    text_model_service: Rc<dyn ITextModelService>,
    cell_outline_entry_cache: Rc<RefCell<HashMap<String, Vec<EntryDesc>>>>,
    cached_markdown_outline_entries: RefCell<HashMap<String, (i64, Vec<MarkdownHeader>)>>,
}

impl NotebookOutlineEntryFactory {
    pub fn new(
        execution_state_service: Rc<dyn INotebookExecutionStateService>,
        outline_model_service: Rc<dyn IOutlineModelService>,
        text_model_service: Rc<dyn ITextModelService>,
    ) -> Self {
        Self {
            execution_state_service,
            outline_model_service,
            text_model_service,
            cell_outline_entry_cache: Rc::new(RefCell::new(HashMap::new())),
            cached_markdown_outline_entries: RefCell::new(HashMap::new()),
        }
    }
}

impl INotebookOutlineEntryFactory for NotebookOutlineEntryFactory {
    fn get_outline_entries(&self, cell: &CellViewModel, index: usize) -> Vec<Rc<OutlineEntry>> {
        let mut index = index as i64;
        let mut entries: Vec<Rc<OutlineEntry>> = Vec::new();

        let is_markdown = cell.cell_kind() == CellKind::Markup;

        // cap the amount of characters that we look at and use the following logic
        // - for MD prefer headings (each header is an entry)
        // - otherwise use the first none-empty line of the cell (MD or code)
        let mut content = get_cell_first_non_empty_line(cell);
        let mut has_header = false;

        if is_markdown {
            let full_content = substring_units(&cell.get_text(), 10000);
            let alternative_id = cell.base().get_alternative_id();
            let cached = self
                .cached_markdown_outline_entries
                .borrow()
                .get(&cell.id())
                .filter(|(cached_alternative_id, _)| *cached_alternative_id == alternative_id)
                .map(|(_, headers)| headers.clone());
            let headers = cached.unwrap_or_else(|| get_markdown_headers_in_cell_fallback_to_html_tags(&full_content));
            self.cached_markdown_outline_entries.borrow_mut().insert(cell.id(), (alternative_id, headers.clone()));

            for MarkdownHeader { depth, text } in headers {
                has_header = true;
                entries.push(OutlineEntry::new(index, i64::from(depth), cell.clone(), &text, false, false, None, None));
                index += 1;
            }

            if !has_header {
                content = render_as_plaintext(&content);
            }
        }

        if !has_header {
            let exe_state = if is_markdown { None } else { self.execution_state_service.get_cell_execution(&cell.uri()) };
            let mut preview = content.trim().to_string();

            if !is_markdown {
                let cached = self.cell_outline_entry_cache.borrow().get(&cell.id()).cloned();

                // Gathering symbols from the model is an async operation, but this provider is syncronous.
                // So symbols need to be precached before this function is called to get the full list.
                if let Some(cached) = cached {
                    // push code cell entry that is a parent of cached symbols, always necessary. filtering for quickpick done in that provider.
                    entries.push(OutlineEntry::new(
                        index,
                        NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL,
                        cell.clone(),
                        &preview,
                        exe_state.is_some(),
                        exe_state.as_ref().is_some_and(|state| state.is_paused()),
                        None,
                        None,
                    ));
                    index += 1;
                    for entry in cached {
                        entries.push(OutlineEntry::new(index, entry.level, cell.clone(), &entry.name, false, false, Some(entry.range), Some(entry.kind)));
                        index += 1;
                    }
                }
            }

            if entries.is_empty() {
                // if there are no cached entries, use the first line of the cell as a code cell
                if preview.is_empty() {
                    // empty or just whitespace
                    preview = localize("empty", "empty cell", &[]);
                }
                entries.push(OutlineEntry::new(
                    index,
                    NotebookOutlineConstants::NON_HEADER_OUTLINE_LEVEL,
                    cell.clone(),
                    &preview,
                    exe_state.is_some(),
                    exe_state.as_ref().is_some_and(|state| state.is_paused()),
                    None,
                    None,
                ));
            }
        }

        entries
    }

    fn cache_symbols(&self, cell: &CellViewModel, cancel_token: &CancellationToken) -> Promise<()> {
        if cell.cell_kind() == CellKind::Markup {
            return Promise::resolved(());
        }

        let cache = self.cell_outline_entry_cache.clone();
        let outline_model_service = self.outline_model_service.clone();
        let cell_id = cell.id();
        let token = cancel_token.clone();
        self.text_model_service.create_model_reference(&cell.uri()).then_promise(move |reference| {
            let release = reference.clone();
            let result = match reference.object.text_editor_model() {
                Some(text_model) => outline_model_service.get_or_create(&text_model, &token).then(move |outline_model| {
                    let entries = create_outline_entries(&outline_model.get_top_level_symbols(), 8);
                    cache.borrow_mut().insert(cell_id, entries);
                }),
                None => Promise::rejected(Error::new("O modelo de texto da célula não foi resolvido".to_string())),
            };
            result.finally(move || release.dispose())
        })
    }
}

fn create_outline_entries(symbols: &[DocumentSymbol], level: i64) -> Vec<EntryDesc> {
    let mut entries: Vec<EntryDesc> = Vec::new();
    for symbol in symbols {
        entries.push(EntryDesc { name: symbol.name.clone(), range: symbol.range, level, kind: symbol.kind });
        if let Some(children) = &symbol.children {
            entries.extend(create_outline_entries(children, level + 1));
        }
    }
    entries
}

fn get_cell_first_non_empty_line(cell: &CellViewModel) -> String {
    let text_buffer = cell.base().text_buffer();
    let text_buffer = text_buffer.borrow();
    for i in 0..text_buffer.get_line_count() {
        let line_number = i as i32 + 1;
        let first_non_whitespace = text_buffer.get_line_first_non_whitespace_column(line_number);
        let line_length = text_buffer.get_line_length(line_number);
        if first_non_whitespace < line_length {
            return text_buffer.get_line_content(line_number).to_string_lossy();
        }
    }

    substring_units(&cell.get_text(), 100)
}
