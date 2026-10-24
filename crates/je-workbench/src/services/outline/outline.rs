//! Porte de `src/vs/workbench/services/outline/browser/outline.ts`: o contrato do serviço de estrutura de tópicos
//! (`IOutlineService`), do que ele cria (`IOutline`) e dos criadores registrados por editor (`IOutlineCreator`).
//!
//! Notas de tradução:
//! - **O elemento `E`.** `IOutline<E>` é genérico no original, e os consumidores (as migalhas, o painel de estrutura)
//!   o usam como `IOutline<unknown>` ou `IOutline<any>`. Aqui o elemento é o [`OutlineElement`] (`Rc<dyn Any>`): quem cria
//!   o outline sabe o tipo concreto e quem o consome só o repassa de volta.
//! - **O que é interface fica de fora de [`IOutlineListConfig`].** `treeDataSource` (`IDataSource` da árvore),
//!   `delegate` (`IListVirtualDelegate`), `renderers` (`ITreeRenderer`) e `options` (`IWorkbenchDataTreeOptions`) são
//!   contratos dos widgets de árvore e lista, que o je-code desenha nativamente; os três contratos de dados
//!   (`breadcrumbsDataSource`, `comparator` e `quickPickDataSource`) ficam.
//! - **`OutlineTarget`** é um `const enum` de bandeiras (1, 2 e 4): é [`OutlineTarget`], com `BitOr` e
//!   [`OutlineTarget::contains`].
//! - **`matches(candidate): candidate is P`** de `IOutlineCreator` não tem tipo de guarda em Rust: o criador recebe o
//!   `IEditorPane` e devolve se o reconhece; `createOutline` recebe o mesmo painel (o criador faz o `downcast` que o
//!   guarda de tipo fazia).
//! - **Promessas.** `Promise<IOutline<any> | undefined>` é `Promise<Option<Rc<dyn IOutline>>>`.
//! - **`reveal`** devolve `Promise<void> | void`: é um [`MaybePromise<()>`](je_base::promise::MaybePromise).
//! - **`OutlineConfigKeys`** e **`OutlineConfigCollapseItemsValues`** (`const enum` de textos) são as estruturas de
//!   constantes de mesmo nome.

use std::any::Any;
use std::ops::BitOr;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::opener::editor_types::EditorOptions;

use crate::common::editor::IEditorPane;

/// O elemento de um outline (o `E` de `IOutline<E>`).
pub type OutlineElement = Rc<dyn Any>;

/// `OutlineTarget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutlineTarget(pub u8);

impl OutlineTarget {
    pub const OUTLINE_PANE: OutlineTarget = OutlineTarget(1);
    pub const BREADCRUMBS: OutlineTarget = OutlineTarget(2);
    pub const QUICK_PICK: OutlineTarget = OutlineTarget(4);

    /// Se todas as bandeiras de `other` estão neste alvo.
    pub fn contains(self, other: OutlineTarget) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for OutlineTarget {
    type Output = OutlineTarget;

    fn bitor(self, rhs: OutlineTarget) -> OutlineTarget {
        OutlineTarget(self.0 | rhs.0)
    }
}

/// `IOutlineService`.
pub trait IOutlineService {
    fn on_did_change(&self) -> Event<()>;
    fn can_create_outline(&self, editor: &dyn IEditorPane) -> bool;
    fn create_outline(&self, editor: Rc<dyn IEditorPane>, target: OutlineTarget, token: CancellationToken) -> Promise<Option<Rc<dyn IOutline>>>;
    fn register_outline_creator(&self, creator: Rc<dyn IOutlineCreator>) -> DisposableRef;
    fn notify_outline_changed(&self);
}

service_id!(pub I_OUTLINE_SERVICE: dyn IOutlineService = "IOutlineService");

/// `IOutlineCreator<P, E>`.
pub trait IOutlineCreator {
    /// `matches(candidate): candidate is P`.
    fn matches(&self, candidate: &dyn IEditorPane) -> bool;
    fn create_outline(&self, editor: Rc<dyn IEditorPane>, target: OutlineTarget, token: CancellationToken) -> Promise<Option<Rc<dyn IOutline>>>;
}

/// `IBreadcrumbsOutlineElement<E>`.
#[derive(Clone)]
pub struct BreadcrumbsOutlineElement {
    pub element: OutlineElement,
    pub label: String,
}

/// `IBreadcrumbsDataSource<E>`.
pub trait IBreadcrumbsDataSource {
    fn get_breadcrumb_elements(&self) -> Vec<BreadcrumbsOutlineElement>;
}

/// `IOutlineComparator<E>`.
pub trait IOutlineComparator {
    fn compare_by_position(&self, a: &OutlineElement, b: &OutlineElement) -> f64;
    fn compare_by_type(&self, a: &OutlineElement, b: &OutlineElement) -> f64;
    fn compare_by_name(&self, a: &OutlineElement, b: &OutlineElement) -> f64;
}

/// `IQuickPickOutlineElement<E>`.
#[derive(Clone)]
pub struct QuickPickOutlineElement {
    pub element: OutlineElement,
    pub label: String,
    pub icon_classes: Option<Vec<String>>,
    pub aria_label: Option<String>,
    pub description: Option<String>,
}

/// `IQuickPickDataSource<E>`.
pub trait IQuickPickDataSource {
    fn get_quick_pick_elements(&self) -> Vec<QuickPickOutlineElement>;
}

/// `IOutlineListConfig<E>`, sem os contratos dos widgets de árvore e lista (ver a nota do módulo).
#[derive(Clone)]
pub struct IOutlineListConfig {
    pub breadcrumbs_data_source: Rc<dyn IBreadcrumbsDataSource>,
    pub comparator: Rc<dyn IOutlineComparator>,
    pub quick_pick_data_source: Rc<dyn IQuickPickDataSource>,
}

/// `OutlineChangeEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutlineChangeEvent {
    /// `affectOnlyActiveElement?: true`.
    pub affect_only_active_element: bool,
}

/// `IOutline<E>`.
pub trait IOutline: IDisposable {
    fn uri(&self) -> Option<URI>;
    fn config(&self) -> &IOutlineListConfig;
    fn outline_kind(&self) -> String;
    fn is_empty(&self) -> bool;
    fn active_element(&self) -> Option<OutlineElement>;
    fn on_did_change(&self) -> Event<OutlineChangeEvent>;
    fn reveal(&self, entry: &OutlineElement, options: &EditorOptions, side_by_side: bool, select: bool) -> MaybePromise<()>;
    fn preview(&self, entry: &OutlineElement) -> DisposableRef;
    fn capture_view_state(&self) -> DisposableRef;
}

/// `OutlineConfigKeys`.
pub struct OutlineConfigKeys;

impl OutlineConfigKeys {
    pub const ICONS: &'static str = "outline.icons";
    pub const COLLAPSE_ITEMS: &'static str = "outline.collapseItems";
    pub const PROBLEMS_ENABLED: &'static str = "outline.problems.enabled";
    pub const PROBLEMS_COLORS: &'static str = "outline.problems.colors";
    pub const PROBLEMS_BADGES: &'static str = "outline.problems.badges";
}

/// `OutlineConfigCollapseItemsValues`.
pub struct OutlineConfigCollapseItemsValues;

impl OutlineConfigCollapseItemsValues {
    pub const COLLAPSED: &'static str = "alwaysCollapse";
    pub const EXPANDED: &'static str = "alwaysExpand";
}
