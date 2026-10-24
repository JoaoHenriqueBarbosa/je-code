//! Porte parcial de `src/vs/workbench/contrib/scm/browser/quickDiffModel.ts` (e de `common/quickDiff.ts`): o contrato
//! que o `MainThreadTextEditors` lê para calcular o diff de um editor (`IQuickDiffModelService`, o `QuickDiffModel` e
//! os tipos de resultado). A implementação (`QuickDiffModelService`, `QuickDiffModel`, o cálculo do diff por
//! provedor de SCM) depende do SCM e do editor de diff e fica para o porte do `contrib/scm`.
//!
//! Divergências:
//! - `QuickDiffModel` é o trait [`IQuickDiffModel`], com `onDidChange`, `quickDiffs`, `changes` e `getQuickDiffResults`.
//!   O `{ changes, diff: ISplice[] }` do evento é o [`QuickDiffModelChange`] sem o `diff` (que o consumidor não lê).
//! - `IReference<QuickDiffModel>` é a [`QuickDiffModelReference`] (o objeto e o descarte da referência).
//! - `IChange` (o `change` de [`QuickDiffChange`]) é o [`LineChange`] do editor, e `LineRangeMapping` o do editor de diff.
//! - `QuickDiffProviderKind` (`'primary' | 'secondary' | 'contributed'`) é o enum de mesmo nome.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::uri::URI;
use je_editor::diff::legacy_lines_diff_computer::LineChange;
use je_editor::diff::range_mapping::LineRangeMapping;
use je_instantiation::service_id;

/// `QuickDiffProviderKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickDiffProviderKind {
    Primary,
    Secondary,
    Contributed,
}

/// `QuickDiff` (`common/quickDiff.ts`).
#[derive(Clone, Debug)]
pub struct QuickDiff {
    pub id: String,
    pub label: String,
    pub original_resource: URI,
    pub kind: QuickDiffProviderKind,
}

/// `QuickDiffChange`.
#[derive(Clone, Debug)]
pub struct QuickDiffChange {
    pub provider_id: String,
    pub original: URI,
    pub modified: URI,
    pub change: LineChange,
    pub change2: LineRangeMapping,
}

/// `QuickDiffResult`.
#[derive(Clone, Debug)]
pub struct QuickDiffResult {
    pub provider_id: String,
    pub provider_kind: QuickDiffProviderKind,
    pub original: URI,
    pub modified: URI,
    pub changes: Vec<LineChange>,
    pub changes2: Vec<LineRangeMapping>,
}

/// `QuickDiffModelOptions`: `{ algorithm?: DiffAlgorithmName }` (`'legacy' | 'advanced'`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickDiffModelOptions {
    pub algorithm: Option<String>,
}

/// O valor do evento `onDidChange` do `QuickDiffModel`.
#[derive(Clone, Debug)]
pub struct QuickDiffModelChange {
    pub changes: Vec<QuickDiffChange>,
}

/// `QuickDiffModel`, no que a camada de API lê.
pub trait IQuickDiffModel {
    /// `onDidChange`.
    fn on_did_change(&self) -> Event<QuickDiffModelChange>;

    /// `quickDiffs`.
    fn quick_diffs(&self) -> Vec<QuickDiff>;

    /// `changes`.
    fn changes(&self) -> Vec<QuickDiffChange>;

    /// `getQuickDiffResults()`.
    fn get_quick_diff_results(&self) -> Vec<QuickDiffResult>;
}

/// `IReference<QuickDiffModel>`: o modelo e o descarte da referência.
#[derive(Clone)]
pub struct QuickDiffModelReference {
    /// `object`.
    pub object: Rc<dyn IQuickDiffModel>,
    release: DisposableRef,
}

impl QuickDiffModelReference {
    /// `release` é de chamada única, como o `dispose` de uma `IReference`.
    pub fn new(object: Rc<dyn IQuickDiffModel>, release: DisposableRef) -> Self {
        Self { object, release }
    }
}

impl IDisposable for QuickDiffModelReference {
    fn dispose(&self) {
        self.release.dispose();
    }
}

/// `IQuickDiffModelService`.
pub trait IQuickDiffModelService {
    /// Returns `None` if the editor model is not resolved. The model reference has to be disposed once not needed
    /// anymore.
    fn create_quick_diff_model_reference(&self, resource: &URI, options: Option<QuickDiffModelOptions>) -> Option<QuickDiffModelReference>;
}

service_id!(pub I_QUICK_DIFF_MODEL_SERVICE: dyn IQuickDiffModelService = "quickDiffModelService");
