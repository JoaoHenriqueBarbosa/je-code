//! Porte de `src/vs/editor/contrib/folding/browser/syntaxRangeProvider.ts`: o provedor de regiões de dobra que
//! consulta os `FoldingRangeProvider` das linguagens (`SyntaxRangeProvider`), `IFoldingRangeData` e
//! `sanitizeRanges`.
//!
//! Decisões de tradução:
//! - `FoldingContext`, `FoldingRange`, `FoldingRangeKind` e `FoldingRangeProvider` são tipos de
//!   `src/vs/editor/common/languages.ts`; moram aqui por este ser o único consumidor no porte até agora.
//! - `ProviderResult<FoldingRange[]>` é [`ProviderResult`] (`Promise<Option<Vec<FoldingRange>>>`): `null` e
//!   `undefined` do provedor são `None`, e o `Array.isArray(ranges)` do original é o `Some`.
//! - `ITextModel` é o `Rc<TextModel>`; o `model === this.model` que o provedor de teste faz é `Rc::ptr_eq`.
//! - `onDidChange?: Event<this>` é [`FoldingRangeProvider::on_did_change`], um `Option<Event<()>>`: o valor do
//!   evento (o próprio provedor) não é lido por quem escuta.
//! - O objeto `FoldingLimitReporter`, o `RangeProvider` de recurso e os tratadores são `Rc<dyn ...>`, como em
//!   [`IndentRangeProvider`](super::indent_range_provider::IndentRangeProvider).
//! - `onUnexpectedExternalError` como segundo argumento de `then` só vê rejeições do provedor, não erros lançados
//!   dentro do primeiro tratador: é a mesma divisão do `match` sobre o resultado.
//! - O `RangesCollector` guarda os tipos como `Vec<Option<String>>` (o `Array<string | undefined>`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_external_error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};

use super::folding_ranges::{FoldingRegions, MAX_LINE_NUMBER};
use super::{FoldingLimitReporter, RangeProvider};
use crate::cursor_common::CursorSimpleModel;
use crate::language_providers::ProviderResult;
use crate::text_model::TextModel;

/// `FoldingContext`: sem campos no original.
pub struct FoldingContext;

/// `FoldingRangeKind`: o tipo de um intervalo de dobra (`comment`, `imports`, `region`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldingRangeKind {
    pub value: String,
}

impl FoldingRangeKind {
    pub fn new(value: impl Into<String>) -> Self {
        Self { value: value.into() }
    }

    /// `FoldingRangeKind.Comment`.
    pub fn comment() -> Self {
        Self::new("comment")
    }

    /// `FoldingRangeKind.Imports`.
    pub fn imports() -> Self {
        Self::new("imports")
    }

    /// `FoldingRangeKind.Region`.
    pub fn region() -> Self {
        Self::new("region")
    }
}

/// `FoldingRange`: as linhas (a partir de 1) de início e fim de uma região de dobra.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldingRange {
    pub start: i32,
    pub end: i32,
    pub kind: Option<FoldingRangeKind>,
}

/// `FoldingRangeProvider`.
pub trait FoldingRangeProvider {
    /// `onDidChange?`: `None` quando o provedor não tem o evento.
    fn on_did_change(&self) -> Option<Event<()>> {
        None
    }

    /// `provideFoldingRanges(model, context, token)`.
    fn provide_folding_ranges(
        &self,
        model: &Rc<TextModel>,
        context: &FoldingContext,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<FoldingRange>>;
}

/// `IFoldingRangeData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldingRangeData {
    pub start: i32,
    pub end: i32,
    pub kind: Option<FoldingRangeKind>,
    pub rank: usize,
}

const FOLDING_CONTEXT: FoldingContext = FoldingContext;

const ID_SYNTAX_PROVIDER: &str = "syntax";

/// `SyntaxRangeProvider`.
pub struct SyntaxRangeProvider {
    editor_model: Rc<TextModel>,
    providers: Vec<Rc<dyn FoldingRangeProvider>>,
    pub handle_folding_ranges_change: Rc<dyn Fn()>,
    folding_ranges_limit: Rc<dyn FoldingLimitReporter>,
    /// Usado quando todos os provedores devolvem `null`.
    fallback_range_provider: Option<Rc<dyn RangeProvider>>,
    pub disposables: Rc<DisposableStore>,
}

impl SyntaxRangeProvider {
    pub fn new(
        editor_model: Rc<TextModel>,
        providers: Vec<Rc<dyn FoldingRangeProvider>>,
        handle_folding_ranges_change: Rc<dyn Fn()>,
        folding_ranges_limit: Rc<dyn FoldingLimitReporter>,
        fallback_range_provider: Option<Rc<dyn RangeProvider>>,
    ) -> Self {
        let disposables = DisposableStore::new();
        if let Some(fallback) = &fallback_range_provider {
            let fallback = Rc::clone(fallback);
            disposables.add(to_disposable(move || fallback.dispose()));
        }

        for provider in &providers {
            if let Some(on_did_change) = provider.on_did_change() {
                let handler = Rc::clone(&handle_folding_ranges_change);
                disposables.add(on_did_change.listen(move |_| handler()));
            }
        }

        Self { editor_model, providers, handle_folding_ranges_change, folding_ranges_limit, fallback_range_provider, disposables }
    }
}

impl RangeProvider for SyntaxRangeProvider {
    fn id(&self) -> &str {
        ID_SYNTAX_PROVIDER
    }

    fn compute(&self, cancellation_token: &CancellationToken) -> Promise<Option<Rc<FoldingRegions>>> {
        let editor_model = Rc::clone(&self.editor_model);
        let folding_ranges_limit = Rc::clone(&self.folding_ranges_limit);
        let fallback_range_provider = self.fallback_range_provider.clone();
        let cancellation_token = cancellation_token.clone();
        collect_syntax_ranges(&self.providers, &self.editor_model, &cancellation_token).then_maybe(move |ranges| {
            if editor_model.is_disposed() {
                return MaybePromise::Value(None);
            }
            if let Some(ranges) = ranges {
                return MaybePromise::Value(Some(Rc::new(sanitize_ranges(ranges, &*folding_ranges_limit))));
            }
            match fallback_range_provider {
                Some(fallback) => MaybePromise::Promise(fallback.compute(&cancellation_token)),
                None => MaybePromise::Value(None),
            }
        })
    }

    fn dispose(&self) {
        self.disposables.dispose();
    }
}

fn collect_syntax_ranges(
    providers: &[Rc<dyn FoldingRangeProvider>],
    model: &Rc<TextModel>,
    cancellation_token: &CancellationToken,
) -> Promise<Option<Vec<FoldingRangeData>>> {
    let range_data: Rc<RefCell<Option<Vec<FoldingRangeData>>>> = Rc::new(RefCell::new(None));
    let promises: Vec<Promise<()>> = providers
        .iter()
        .enumerate()
        .map(|(rank, provider)| {
            let range_data = Rc::clone(&range_data);
            let model = Rc::clone(model);
            let cancellation_token = cancellation_token.clone();
            provider.provide_folding_ranges(&model, &FOLDING_CONTEXT, &cancellation_token).then_result(move |result| {
                let ranges = match result {
                    Ok(ranges) => ranges,
                    Err(error) => {
                        on_unexpected_external_error(&error);
                        return Ok(());
                    }
                };
                if cancellation_token.is_cancellation_requested() {
                    return Ok(());
                }
                if let Some(ranges) = ranges {
                    let mut range_data = range_data.borrow_mut();
                    let range_data = range_data.get_or_insert_with(Vec::new);
                    let n_lines = model.get_line_count();
                    for r in ranges {
                        if r.start > 0 && r.end > r.start && r.end <= n_lines {
                            range_data.push(FoldingRangeData { start: r.start, end: r.end, rank, kind: r.kind });
                        }
                    }
                }
                Ok(())
            })
        })
        .collect();
    Promise::all(promises).then(move |_| range_data.borrow_mut().take())
}

struct RangesCollector<'a> {
    start_indexes: Vec<u32>,
    end_indexes: Vec<u32>,
    nesting_levels: Vec<usize>,
    nesting_level_counts: Vec<usize>,
    types: Vec<Option<String>>,
    folding_ranges_limit: &'a dyn FoldingLimitReporter,
}

impl<'a> RangesCollector<'a> {
    fn new(folding_ranges_limit: &'a dyn FoldingLimitReporter) -> Self {
        Self {
            start_indexes: Vec::new(),
            end_indexes: Vec::new(),
            nesting_levels: Vec::new(),
            nesting_level_counts: Vec::new(),
            types: Vec::new(),
            folding_ranges_limit,
        }
    }

    fn add(&mut self, start_line_number: u32, end_line_number: u32, r#type: Option<String>, nesting_level: usize) {
        if start_line_number > MAX_LINE_NUMBER || end_line_number > MAX_LINE_NUMBER {
            return;
        }
        self.start_indexes.push(start_line_number);
        self.end_indexes.push(end_line_number);
        self.nesting_levels.push(nesting_level);
        self.types.push(r#type);
        if nesting_level < 30 {
            if self.nesting_level_counts.len() <= nesting_level {
                self.nesting_level_counts.resize(nesting_level + 1, 0);
            }
            self.nesting_level_counts[nesting_level] += 1;
        }
    }

    fn to_indent_ranges(self) -> FoldingRegions {
        let length = self.start_indexes.len();
        let limit = self.folding_ranges_limit.limit();
        if length <= limit {
            self.folding_ranges_limit.update(length, None);
            FoldingRegions::new(self.start_indexes, self.end_indexes, Some(self.types))
        } else {
            self.folding_ranges_limit.update(length, Some(limit));

            let mut entries = 0usize;
            let mut max_level = self.nesting_level_counts.len();
            for (i, &n) in self.nesting_level_counts.iter().enumerate() {
                if n != 0 {
                    if n + entries > limit {
                        max_level = i;
                        break;
                    }
                    entries += n;
                }
            }

            let mut start_indexes: Vec<u32> = Vec::with_capacity(limit);
            let mut end_indexes: Vec<u32> = Vec::with_capacity(limit);
            let mut types: Vec<Option<String>> = Vec::with_capacity(limit);
            for i in 0..length {
                let level = self.nesting_levels[i];
                let fits = level < max_level
                    || (level == max_level && {
                        let within_limit = entries < limit;
                        entries += 1;
                        within_limit
                    });
                if fits {
                    start_indexes.push(self.start_indexes[i]);
                    end_indexes.push(self.end_indexes[i]);
                    types.push(self.types[i].clone());
                }
            }
            FoldingRegions::new(start_indexes, end_indexes, Some(types))
        }
    }
}

/// `sanitizeRanges(rangeData, foldingRangesLimit)`.
pub fn sanitize_ranges(mut range_data: Vec<FoldingRangeData>, folding_ranges_limit: &dyn FoldingLimitReporter) -> FoldingRegions {
    range_data.sort_by(|d1, d2| d1.start.cmp(&d2.start).then(d1.rank.cmp(&d2.rank)));
    let sorted = range_data;
    let mut collector = RangesCollector::new(folding_ranges_limit);

    let kind_value = |entry: &FoldingRangeData| entry.kind.as_ref().map(|kind| kind.value.clone());
    // `start` e `end` já passaram de `> 0` na coleta, então cabem em `u32`.
    let add = |collector: &mut RangesCollector<'_>, entry: &FoldingRangeData, nesting_level: usize| {
        collector.add(entry.start as u32, entry.end as u32, kind_value(entry), nesting_level);
    };

    let mut top: Option<usize> = None;
    let mut previous: Vec<usize> = Vec::new();
    for (index, entry) in sorted.iter().enumerate() {
        let Some(top_index) = top else {
            top = Some(index);
            add(&mut collector, entry, previous.len());
            continue;
        };
        let top_entry = &sorted[top_index];
        if entry.start > top_entry.start {
            if entry.end <= top_entry.end {
                previous.push(top_index);
                top = Some(index);
                add(&mut collector, entry, previous.len());
            } else {
                if entry.start > top_entry.end {
                    let mut new_top;
                    loop {
                        new_top = previous.pop();
                        match new_top {
                            Some(candidate) if entry.start > sorted[candidate].end => continue,
                            _ => break,
                        }
                    }
                    if let Some(candidate) = new_top {
                        previous.push(candidate);
                    }
                    top = Some(index);
                }
                add(&mut collector, entry, previous.len());
            }
        }
    }
    collector.to_indent_ranges()
}
