//! Porte de `src/vs/base/browser/ui/table/table.ts`: os contratos da tabela (a coluna, o delegate de alturas, o renderer de
//! célula e os eventos) e o `TableError`.
//!
//! Do TypeScript para o Rust:
//! - `ITableColumn<TRow, TCell>`: o `project(row)` é um `Rc<dyn Fn(&TRow) -> TCell>`, o `tooltip` vazio é ausente (o original o
//!   testa por veracidade) e o `onDidChangeWidthConstraints` é um `Event<()>` opcional.
//! - `ITableRenderer<TCell, TTemplateData>` estende o `IListRenderer`; o `TTemplateData` é o [`TemplateData`] da lista. Onde o
//!   original deixa a posição do conteúdo da célula para o CSS do `.monaco-table-td` (`display: flex`, `padding-left`), o
//!   porte pede ao renderer da coluna que posicione o conteúdo: [`ITableRenderer::layout_cell`] recebe a caixa de conteúdo da
//!   célula (já sem o recuo à esquerda) em coordenadas do nó da célula. O padrão não faz nada, para renderers que só têm
//!   filhos de fluxo simples.
//! - Os eventos da tabela são os da lista (`ITableEvent extends IListEvent` e as irmãs). O `IListContextMenuEvent` não tem
//!   porte (ver `list_types`), então o `ITableContextMenuEvent` é o [`ListMouseEvent`] do menu de contexto do ponteiro.

use std::rc::Rc;

use je_base::errors::Error;
use je_base::event::Event;
use je_render::kurbo::Rect;

use crate::list::list_types::{IListRenderer, ListEvent, ListMouseEvent, TemplateData};

/// `ITableColumn<TRow, TCell>`.
pub struct ITableColumn<TRow, TCell> {
    pub label: String,
    /// `tooltip?`: vazio vale como ausente.
    pub tooltip: Option<String>,
    pub weight: f64,
    pub template_id: String,

    pub minimum_width: Option<f64>,
    pub maximum_width: Option<f64>,
    pub on_did_change_width_constraints: Option<Event<()>>,

    /// `project(row)`.
    pub project: Rc<dyn Fn(&TRow) -> TCell>,
}

/// `ITableVirtualDelegate<TRow>`.
pub trait ITableVirtualDelegate<TRow> {
    fn header_row_height(&self) -> f64;
    fn get_height(&self, row: &TRow) -> f64;
}

/// `ITableRenderer<TCell, TTemplateData>`.
pub trait ITableRenderer<TCell>: IListRenderer<TCell> {
    /// A caixa de conteúdo da célula (ver as notas do módulo), chamada ao renderizar a linha, ao redimensionar a coluna e
    /// quando o estado da linha (ponteiro, foco, seleção) muda. O padrão não faz nada.
    fn layout_cell(&self, _template_data: &TemplateData, _content: Rect) {}
}

/// `ITableEvent<TRow>`.
pub type ITableEvent<TRow> = ListEvent<TRow>;
/// `ITableMouseEvent<TRow>`.
pub type ITableMouseEvent<TRow> = ListMouseEvent<TRow>;
/// `ITableContextMenuEvent<TRow>` (ver as notas do módulo).
pub type ITableContextMenuEvent<TRow> = ListMouseEvent<TRow>;

/// `TableError`.
pub fn table_error(user: &str, message: &str) -> Error {
    Error::new(format!("TableError [{user}] {message}"))
}
