//! Porte parcial de `src/vs/workbench/contrib/scm/common/history.ts`: os tipos de dados do histórico do SCM que o
//! grafo (`browser/scmHistory.ts`) lê e produz: os ids dos itens de mudanças de entrada e de saída, a referência
//! (`ISCMHistoryItemRef`), o item (`ISCMHistoryItem`), o nó do grafo (`ISCMHistoryItemGraphNode`) e o view model
//! (`ISCMHistoryItemViewModel`).
//!
//! Fora do porte: `ISCMHistoryProvider` (depende de `IObservable` e das chamadas ao provedor da extensão) e os elementos de
//! árvore `SCMHistoryItem*TreeElement` (dependem de `ISCMRepository`, de `scm.ts`); nenhum deles entra no cálculo do grafo.
//!
//! Notas de tradução:
//! - **`ColorIdentifier`** é `String`.
//! - **`icon?: URI | { light: URI; dark: URI } | ThemeIcon`** é o enum [`HistoryItemIcon`].
//! - **`tooltip?: IMarkdownString | Array<IMarkdownString>`** é o `Option` do enum [`HistoryItemTooltip`].
//! - **`kind: 'HEAD' | 'node' | 'incoming-changes' | 'outgoing-changes'`** é o enum [`HistoryItemViewModelKind`] com
//!   [`HistoryItemViewModelKind::as_str`].
//! - **`limit?: number | { id?: string }`** de `ISCMHistoryOptions` é o enum [`HistoryLimit`].

use je_base::html_content::IMarkdownString;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;

pub const SCM_INCOMING_HISTORY_ITEM_ID: &str = "scm-graph-incoming-changes";
pub const SCM_OUTGOING_HISTORY_ITEM_ID: &str = "scm-graph-outgoing-changes";

/// `limit?: number | { id?: string }`.
#[derive(Clone, Debug, PartialEq)]
pub enum HistoryLimit {
    Count(f64),
    Id(Option<String>),
}

/// `ISCMHistoryOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ISCMHistoryOptions {
    pub skip: Option<f64>,
    pub limit: Option<HistoryLimit>,
    pub history_item_refs: Option<Vec<String>>,
    pub filter_text: Option<String>,
}

/// `ISCMHistoryItemStatistics`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ISCMHistoryItemStatistics {
    pub files: f64,
    pub insertions: f64,
    pub deletions: f64,
}

/// `URI | { light: URI; dark: URI } | ThemeIcon`.
#[derive(Clone, Debug)]
pub enum HistoryItemIcon {
    Uri(URI),
    LightDark { light: URI, dark: URI },
    Theme(ThemeIcon),
}

/// `ISCMHistoryItemRef`.
#[derive(Clone, Debug, Default)]
pub struct ISCMHistoryItemRef {
    pub id: String,
    pub name: String,
    pub revision: Option<String>,
    pub category: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<HistoryItemIcon>,
}

/// `ISCMHistoryItemRefsChangeEvent`.
#[derive(Clone, Debug, Default)]
pub struct ISCMHistoryItemRefsChangeEvent {
    pub added: Vec<ISCMHistoryItemRef>,
    pub removed: Vec<ISCMHistoryItemRef>,
    pub modified: Vec<ISCMHistoryItemRef>,
    pub silent: bool,
}

/// `IMarkdownString | Array<IMarkdownString>`.
#[derive(Clone, Debug)]
pub enum HistoryItemTooltip {
    One(IMarkdownString),
    Many(Vec<IMarkdownString>),
}

/// `ISCMHistoryItem`.
#[derive(Clone, Debug, Default)]
pub struct ISCMHistoryItem {
    pub id: String,
    pub parent_ids: Vec<String>,
    pub subject: String,
    pub message: String,
    pub display_id: Option<String>,
    pub author: Option<String>,
    pub author_email: Option<String>,
    pub author_icon: Option<HistoryItemIcon>,
    pub timestamp: Option<f64>,
    pub statistics: Option<ISCMHistoryItemStatistics>,
    pub references: Option<Vec<ISCMHistoryItemRef>>,
    pub tooltip: Option<HistoryItemTooltip>,
}

/// `ISCMHistoryItemGraphNode`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISCMHistoryItemGraphNode {
    pub id: String,
    pub color: String,
}

/// O `kind` de `ISCMHistoryItemViewModel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryItemViewModelKind {
    Head,
    Node,
    IncomingChanges,
    OutgoingChanges,
}

impl HistoryItemViewModelKind {
    /// O texto do original: `'HEAD'`, `'node'`, `'incoming-changes'` ou `'outgoing-changes'`.
    pub fn as_str(self) -> &'static str {
        match self {
            HistoryItemViewModelKind::Head => "HEAD",
            HistoryItemViewModelKind::Node => "node",
            HistoryItemViewModelKind::IncomingChanges => "incoming-changes",
            HistoryItemViewModelKind::OutgoingChanges => "outgoing-changes",
        }
    }
}

/// `ISCMHistoryItemViewModel`.
#[derive(Clone, Debug)]
pub struct ISCMHistoryItemViewModel {
    pub history_item: ISCMHistoryItem,
    pub input_swimlanes: Vec<ISCMHistoryItemGraphNode>,
    pub output_swimlanes: Vec<ISCMHistoryItemGraphNode>,
    pub kind: HistoryItemViewModelKind,
}

/// `ISCMHistoryItemChange`.
#[derive(Clone, Debug)]
pub struct ISCMHistoryItemChange {
    pub uri: URI,
    pub original_uri: Option<URI>,
    pub modified_uri: Option<URI>,
}
