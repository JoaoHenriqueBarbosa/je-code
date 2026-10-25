//! Os contratos de arrastar e soltar de `src/vs/base/browser/ui/list/list.ts` (`IListDragAndDrop`,
//! `IListDragOverReaction`, `ListDragOverEffect*`) e de `listView.ts` (`ListViewTargetSector`,
//! `IListViewDragAndDrop`, `ElementsDragAndDropData`, `ExternalElementsDragAndDropData`,
//! `NativeDragAndDropData` e o `StaticDND`).
//!
//! Do TypeScript para o Rust:
//! - `IListDragAndDrop<T>` é o trait [`IListDragAndDrop`], com o tipo do resultado de `onDragOver` como
//!   parâmetro: `boolean | IListDragOverReaction` ([`ListDragOverResult`], o padrão) na lista e
//!   `boolean | ITreeDragOverReaction` na árvore (`ITreeDragAndDrop` redeclara o `onDragOver`). Os métodos
//!   opcionais (`getDragLabel?`, `onDragStart?`, `onDragLeave?`, `onDragEnd?`) são métodos com corpo vazio.
//! - Os alvos opcionais (`T | undefined`, `number | undefined`) são `Option`. `originalEvent: DragEvent` é o
//!   [`DragEvent`](crate::dnd::DragEvent).
//! - O `T` da lista não tem igualdade, então o `getDragElements(element)` do `IListViewDragAndDrop` recebe
//!   também o índice da linha (`selection.indexOf(element)` do original é `selection.contains(index)`).
//! - O `context` do `ElementsDragAndDropData<T, TContext>` é um `Rc<dyn Any>` (sem o parâmetro `TContext`): as
//!   conversões da árvore e da árvore de dados assíncrona compartilham o mesmo `context` do dado original (o
//!   getter e o setter repassados do original) e precisam trocá-lo sem conhecer o tipo.
//! - O `StaticDND.CurrentDragAndDropData` é uma variável da thread ([`current_drag_and_drop_data`]).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use std::path::PathBuf;

use je_ui::DataTransfer;

use crate::dnd::{DragEvent, IDragAndDropData};

/// `ListViewTargetSector`: a posição do ponteiro relativa ao topo do item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListViewTargetSector {
    /// `[0%-25%)`.
    Top = 0,
    /// `[25%-50%)`.
    CenterTop = 1,
    /// `[50%-75%)`.
    CenterBottom = 2,
    /// `[75%-100%)`.
    Bottom = 3,
}

impl ListViewTargetSector {
    /// `getTargetSector`: `clamp(floor((offsetY / size) / 0.25), 0, 3)`.
    pub fn at(offset_y: f64, size: f64) -> Self {
        let sector = ((offset_y / size) / 0.25).floor();
        match sector {
            s if s >= 3.0 => Self::Bottom,
            s if s >= 2.0 => Self::CenterBottom,
            s if s >= 1.0 => Self::CenterTop,
            _ => Self::Top,
        }
    }
}

/// `ListDragOverEffectType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListDragOverEffectType {
    Copy,
    Move,
}

/// `ListDragOverEffectPosition`: o valor de cada uma é a classe de CSS da linha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListDragOverEffectPosition {
    Over,
    Before,
    After,
}

impl ListDragOverEffectPosition {
    /// `'drop-target'`, `'drop-target-before'` ou `'drop-target-after'`.
    pub fn class_name(self) -> &'static str {
        match self {
            Self::Over => "drop-target",
            Self::Before => "drop-target-before",
            Self::After => "drop-target-after",
        }
    }
}

/// `ListDragOverEffect` (o `type` do original é `kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListDragOverEffect {
    pub kind: ListDragOverEffectType,
    pub position: Option<ListDragOverEffectPosition>,
}

/// `IListDragOverReaction`. `feedback` usa `-1` para a lista inteira.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IListDragOverReaction {
    pub accept: bool,
    pub effect: Option<ListDragOverEffect>,
    pub feedback: Option<Vec<i64>>,
}

/// `ListDragOverReactions`.
pub struct ListDragOverReactions;

impl ListDragOverReactions {
    /// `reject()`.
    pub fn reject() -> IListDragOverReaction {
        IListDragOverReaction { accept: false, effect: None, feedback: None }
    }

    /// `accept()`.
    pub fn accept() -> IListDragOverReaction {
        IListDragOverReaction { accept: true, effect: None, feedback: None }
    }
}

/// `boolean | IListDragOverReaction`: o resultado de `onDragOver`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListDragOverResult {
    Bool(bool),
    Reaction(IListDragOverReaction),
}

impl ListDragOverResult {
    /// `typeof result === 'boolean' ? result : result.accept`.
    pub fn accept(&self) -> bool {
        match self {
            Self::Bool(accept) => *accept,
            Self::Reaction(reaction) => reaction.accept,
        }
    }

    /// `typeof result !== 'boolean' ? result.effect : undefined`.
    pub fn effect(&self) -> Option<ListDragOverEffect> {
        match self {
            Self::Bool(_) => None,
            Self::Reaction(reaction) => reaction.effect,
        }
    }

    /// `typeof result !== 'boolean' ? result.feedback : undefined`.
    pub fn feedback(&self) -> Option<&Vec<i64>> {
        match self {
            Self::Bool(_) => None,
            Self::Reaction(reaction) => reaction.feedback.as_ref(),
        }
    }
}

impl From<bool> for ListDragOverResult {
    fn from(accept: bool) -> Self {
        Self::Bool(accept)
    }
}

impl From<IListDragOverReaction> for ListDragOverResult {
    fn from(reaction: IListDragOverReaction) -> Self {
        Self::Reaction(reaction)
    }
}

/// `IListDragAndDrop<T>`. "Once passed to a list, that list takes up the responsibility of disposing it":
/// [`IListDragAndDrop::dispose`] é o do `IDisposable`.
pub trait IListDragAndDrop<T, R = ListDragOverResult> {
    /// `getDragURI(element)`.
    fn get_drag_uri(&self, element: &T) -> Option<String>;

    /// `getDragLabel?(elements, originalEvent)`.
    fn get_drag_label(&self, _elements: &[T], _original_event: &DragEvent) -> Option<String> {
        None
    }

    /// `onDragStart?(data, originalEvent)`.
    fn on_drag_start(&self, _data: &Rc<dyn IDragAndDropData>, _original_event: &DragEvent) {}

    /// `onDragOver(data, targetElement, targetIndex, targetSector, originalEvent)`.
    fn on_drag_over(&self, data: &Rc<dyn IDragAndDropData>, target_element: Option<&T>, target_index: Option<usize>, target_sector: Option<ListViewTargetSector>, original_event: &DragEvent) -> R;

    /// `onDragLeave?(data, targetElement, targetIndex, originalEvent)`.
    fn on_drag_leave(&self, _data: &Rc<dyn IDragAndDropData>, _target_element: Option<&T>, _target_index: Option<usize>, _original_event: &DragEvent) {}

    /// `drop(data, targetElement, targetIndex, targetSector, originalEvent)`.
    fn drop(&self, data: &Rc<dyn IDragAndDropData>, target_element: Option<&T>, target_index: Option<usize>, target_sector: Option<ListViewTargetSector>, original_event: &DragEvent);

    /// `onDragEnd?(originalEvent)`.
    fn on_drag_end(&self, _original_event: &DragEvent) {}

    /// `dispose()`.
    fn dispose(&self) {}
}

/// `IListViewDragAndDrop<T>`.
pub trait IListViewDragAndDrop<T>: IListDragAndDrop<T> {
    /// `getDragElements(element)`; `index` é o índice da linha de `element` (ver o `//!`).
    fn get_drag_elements(&self, element: &T, index: usize) -> Vec<T>;
}

/// `ElementsDragAndDropData<T, TContext>`.
pub struct ElementsDragAndDropData<T> {
    pub elements: Vec<T>,
    context: Rc<RefCell<Option<Rc<dyn Any>>>>,
}

impl<T> ElementsDragAndDropData<T> {
    pub fn new(elements: Vec<T>) -> Self {
        Self { elements, context: Rc::default() }
    }

    /// Os mesmos dados com outros elementos e o **mesmo** `context` (o `TreeElementsDragAndDropData` e o
    /// `AsyncDataTreeElementsDragAndDropData` repassam o getter e o setter do `context` ao dado original).
    pub fn derive<U>(&self, elements: Vec<U>) -> ElementsDragAndDropData<U> {
        ElementsDragAndDropData { elements, context: self.context.clone() }
    }

    /// `context`.
    pub fn context(&self) -> Option<Rc<dyn Any>> {
        self.context.borrow().clone()
    }

    /// `context = value`.
    pub fn set_context(&self, context: Option<Rc<dyn Any>>) {
        *self.context.borrow_mut() = context;
    }
}

impl<T: Clone + 'static> IDragAndDropData for ElementsDragAndDropData<T> {
    fn update(&self, _data_transfer: &DataTransfer) {}

    fn get_data(&self) -> Rc<dyn Any> {
        Rc::new(self.elements.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `ExternalElementsDragAndDropData<T>`.
pub struct ExternalElementsDragAndDropData<T> {
    pub elements: Vec<T>,
}

impl<T> ExternalElementsDragAndDropData<T> {
    pub fn new(elements: Vec<T>) -> Self {
        Self { elements }
    }
}

impl<T: Clone + 'static> IDragAndDropData for ExternalElementsDragAndDropData<T> {
    fn update(&self, _data_transfer: &DataTransfer) {}

    fn get_data(&self) -> Rc<dyn Any> {
        Rc::new(self.elements.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// O que `NativeDragAndDropData.getData()` devolve: `{ types, files }` (os arquivos são os caminhos do sistema).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeDragData {
    pub types: Vec<String>,
    pub files: Vec<PathBuf>,
}

/// `NativeDragAndDropData`.
#[derive(Default)]
pub struct NativeDragAndDropData {
    data: RefCell<NativeDragData>,
}

impl NativeDragAndDropData {
    pub fn new() -> Self {
        Self::default()
    }
}

impl IDragAndDropData for NativeDragAndDropData {
    fn update(&self, data_transfer: &DataTransfer) {
        let mut data = self.data.borrow_mut();
        data.types = data_transfer.types();
        // `files.filter(f => f.size > 0 || f.type)`: tira as pastas, que o navegador entrega como `File` sem tamanho
        // e sem tipo. Aqui o arquivo é lido do sistema: só os arquivos regulares ficam.
        data.files = data_transfer.files().into_iter().filter(|file| std::fs::metadata(file).is_ok_and(|metadata| metadata.is_file())).collect();
    }

    fn get_data(&self) -> Rc<dyn Any> {
        Rc::new(self.data.borrow().clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

thread_local! {
    /// `StaticDND.CurrentDragAndDropData`.
    static CURRENT_DRAG_AND_DROP_DATA: RefCell<Option<Rc<dyn IDragAndDropData>>> = const { RefCell::new(None) };
}

/// `StaticDND.CurrentDragAndDropData`: os dados do arrasto de uma lista, que as outras listas leem.
pub fn current_drag_and_drop_data() -> Option<Rc<dyn IDragAndDropData>> {
    CURRENT_DRAG_AND_DROP_DATA.with(|data| data.borrow().clone())
}

/// `StaticDND.CurrentDragAndDropData = data`.
pub fn set_current_drag_and_drop_data(data: Option<Rc<dyn IDragAndDropData>>) {
    CURRENT_DRAG_AND_DROP_DATA.with(|current| *current.borrow_mut() = data);
}

/// `data.getData() === 'vscode-ui'`.
pub fn is_vscode_ui_data(data: &dyn IDragAndDropData) -> bool {
    let value = data.get_data();
    value.downcast_ref::<&'static str>().is_some_and(|text| *text == "vscode-ui") || value.downcast_ref::<String>().is_some_and(|text| text == "vscode-ui")
}
