//! Porte do que o dobramento de código (`src/vs/editor/contrib/folding/browser`) precisa de
//! `folding.ts` para os provedores de intervalo: `RangeProvider`, `FoldingLimitReporter` e
//! `SelectedLines`. O controlador, as ações e o restante de `folding.ts` não fazem parte desta fatia.
//!
//! Os módulos filhos são `foldingRanges.ts` ([`folding_ranges`]) e `indentRangeProvider.ts`
//! ([`indent_range_provider`]).
//!
//! Decisões de tradução:
//! - `number | false` de `FoldingLimitReporter.update` é `Option<usize>` (`None` é `false`).
//! - `RangeProvider.compute` devolve `Promise<FoldingRegions | null>`; como `Promise<T>` exige `T: Clone`
//!   e o original compartilha o objeto por referência, o valor é `Option<Rc<FoldingRegions>>`.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;

pub mod folding_ranges;
pub mod indent_range_provider;

use folding_ranges::FoldingRegions;

/// `RangeProvider`.
pub trait RangeProvider {
    fn id(&self) -> &str;

    fn compute(&self, cancellation_token: &CancellationToken) -> Promise<Option<Rc<FoldingRegions>>>;

    fn dispose(&self);
}

/// `FoldingLimitReporter`.
pub trait FoldingLimitReporter {
    /// O máximo de regiões de dobra.
    fn limit(&self) -> usize;

    /// `computed` é quantas regiões foram calculadas; `limited` é o limite aplicado, ou `None` (o
    /// `false` do original) se o limite não foi atingido.
    fn update(&self, computed: usize, limited: Option<usize>);
}

/// `SelectedLines`.
pub trait SelectedLines {
    fn starts_inside(&self, start_line: u32, end_line: u32) -> bool;
}
