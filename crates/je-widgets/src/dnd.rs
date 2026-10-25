//! Porte de `src/vs/base/browser/dnd.ts` (os formatos de transferência e o `IDragAndDropData`) e do que o
//! arrastar e soltar do DOM entrega aos widgets: o [`DataTransfer`] e o [`DragEvent`].
//!
//! Do DOM para o Rust:
//! - `DataTransfer` é um objeto compartilhado por todos os eventos do mesmo arrasto e muda por referência
//!   (`Rc` com `RefCell`). `setData` troca o dado do formato; `types` lista os formatos e acrescenta `Files`
//!   se há arquivos; `effectAllowed` começa em `uninitialized` e `dropEffect` em `none`. `setDragImage` guarda o
//!   nó da imagem ([`apply_drag_image`](crate::drag_image::apply_drag_image)); quem termina o arrasto o destrói.
//! - `DragEvent` é o que os widgets leem de um evento `drag*`: o alvo, o ponteiro (modificadores e posição), o
//!   `offsetY` (a distância do ponteiro ao topo do alvo) e o `dataTransfer`. O `preventDefault()` é o retorno do
//!   ouvinte ([`DragHandler`]): `true` o chama.
//! - **Ligação com o `je-ui`.** O `je-ui` ainda não tem os eventos de arrastar e soltar (`dragstart`, `dragover`,
//!   `drop`, `dragleave`, `dragend`), então o único ponto que os conhece é [`listen_drag_event`]: ele
//!   devolve `None` (nenhum ouvinte instalado) até o `je-ui` entregá-los. Com os eventos, o corpo dele é
//!   `ui.listen(node, <o EventKind de kind>, false, ...)`, que monta o [`DragEvent`] do evento e chama o
//!   [`DragHandler`]; se o ouvinte devolver `true`, `event.prevent_default()`. Tudo o mais (a lógica da `ListView`, da
//!   árvore e da árvore de dados assíncrona) já está pronto e roda sobre o [`DragEvent`]. Se o `DataTransfer` do
//!   `je-ui` tiver a mesma interface (`set_data`, `get_data`, `types`, `files`, `effect_allowed`, `drop_effect`,
//!   `set_drag_image`), este módulo pode reexportá-lo no lugar do tipo local.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_ui::{NodeId, PointerData, Ui};

/// `DataTransfers` de `dnd.ts`: os formatos de transferência comuns.
pub struct DataTransfers;

impl DataTransfers {
    /// `RESOURCES`: a transferência de recursos da aplicação.
    pub const RESOURCES: &'static str = "ResourceURLs";
    /// `DOWNLOAD_URL`: a transferência do navegador para baixar.
    pub const DOWNLOAD_URL: &'static str = "DownloadURL";
    /// `FILES`: a transferência do navegador para arquivos.
    pub const FILES: &'static str = "Files";
    /// `TEXT` (`Mimes.text`): a transferência de copiar e colar.
    pub const TEXT: &'static str = "text/plain";
    /// `INTERNAL_URI_LIST`: `text/uri-list` de uso interno.
    pub const INTERNAL_URI_LIST: &'static str = "application/vnd.code.uri-list";
}

/// Um item de `DataTransfer.files`: só o que os widgets leem (o conteúdo é do sistema de arquivos).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragFile {
    pub name: String,
    pub size: u64,
    /// O `File.type` (o tipo MIME, vazio se desconhecido).
    pub mime_type: String,
}

/// O que `setDragImage` guarda: o nó da imagem e o deslocamento dele em relação ao ponteiro.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragImage {
    pub node: NodeId,
    pub x: f64,
    pub y: f64,
}

/// `DataTransfer`.
#[derive(Debug)]
pub struct DataTransfer {
    items: RefCell<Vec<(String, String)>>,
    files: RefCell<Vec<DragFile>>,
    effect_allowed: RefCell<String>,
    drop_effect: RefCell<String>,
    drag_image: RefCell<Option<DragImage>>,
}

impl Default for DataTransfer {
    fn default() -> Self {
        Self::new()
    }
}

impl DataTransfer {
    pub fn new() -> Self {
        Self {
            items: RefCell::new(Vec::new()),
            files: RefCell::new(Vec::new()),
            effect_allowed: RefCell::new("uninitialized".to_owned()),
            drop_effect: RefCell::new("none".to_owned()),
            drag_image: RefCell::new(None),
        }
    }

    /// `setData(format, data)`: um dado por formato, na ordem em que entraram.
    pub fn set_data(&self, format: &str, data: &str) {
        let mut items = self.items.borrow_mut();
        match items.iter_mut().find(|(candidate, _)| candidate == format) {
            Some(item) => item.1 = data.to_owned(),
            None => items.push((format.to_owned(), data.to_owned())),
        }
    }

    /// `getData(format)`: o vazio quando o formato não existe.
    pub fn get_data(&self, format: &str) -> String {
        self.items.borrow().iter().find(|(candidate, _)| candidate == format).map(|(_, data)| data.clone()).unwrap_or_default()
    }

    /// `clearData(format?)`.
    pub fn clear_data(&self, format: Option<&str>) {
        match format {
            Some(format) => self.items.borrow_mut().retain(|(candidate, _)| candidate != format),
            None => self.items.borrow_mut().clear(),
        }
    }

    /// `types`: os formatos e, havendo arquivos, `Files` no fim.
    pub fn types(&self) -> Vec<String> {
        let mut types: Vec<String> = self.items.borrow().iter().map(|(format, _)| format.clone()).collect();
        if !self.files.borrow().is_empty() {
            types.push(DataTransfers::FILES.to_owned());
        }
        types
    }

    /// `files`.
    pub fn files(&self) -> Vec<DragFile> {
        self.files.borrow().clone()
    }

    /// Define os arquivos (para quem entrega um arrasto vindo do sistema operacional).
    pub fn set_files(&self, files: Vec<DragFile>) {
        *self.files.borrow_mut() = files;
    }

    /// `effectAllowed`.
    pub fn effect_allowed(&self) -> String {
        self.effect_allowed.borrow().clone()
    }

    pub fn set_effect_allowed(&self, effect: &str) {
        *self.effect_allowed.borrow_mut() = effect.to_owned();
    }

    /// `dropEffect`.
    pub fn drop_effect(&self) -> String {
        self.drop_effect.borrow().clone()
    }

    pub fn set_drop_effect(&self, effect: &str) {
        *self.drop_effect.borrow_mut() = effect.to_owned();
    }

    /// `setDragImage(element, x, y)`.
    pub fn set_drag_image(&self, node: NodeId, x: f64, y: f64) {
        *self.drag_image.borrow_mut() = Some(DragImage { node, x, y });
    }

    /// A imagem de arrasto, se `setDragImage` foi chamado.
    pub fn drag_image(&self) -> Option<DragImage> {
        *self.drag_image.borrow()
    }

    /// Tira a imagem de arrasto de dentro (quem a tira a destrói).
    pub fn take_drag_image(&self) -> Option<DragImage> {
        self.drag_image.borrow_mut().take()
    }
}

/// `DragEvent`, no que os widgets leem dele (ver o `//!`).
#[derive(Clone)]
pub struct DragEvent {
    /// O `target`: o nó onde o evento nasceu.
    pub target: NodeId,
    /// Posição (`pageX`/`pageY`) e modificadores do ponteiro.
    pub pointer: PointerData,
    /// O `offsetY`: a distância do ponteiro ao topo do alvo.
    pub offset_y: f64,
    /// O `dataTransfer` do arrasto.
    pub data_transfer: Rc<DataTransfer>,
}

/// O tipo de evento de arrastar e soltar que os widgets escutam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragEventKind {
    /// `dragstart`.
    DragStart,
    /// `dragover`.
    DragOver,
    /// `drop`.
    Drop,
    /// `dragleave`.
    DragLeave,
    /// `dragend`.
    DragEnd,
}

/// O ouvinte de um evento de arrastar e soltar: devolve se o evento deve receber `preventDefault()`.
pub type DragHandler = Rc<dyn Fn(&DragEvent) -> bool>;

/// `addDisposableListener(node, 'dragstart' | 'dragover' | 'drop' | 'dragleave' | 'dragend', handler)`. É o único
/// ponto que depende dos eventos de arrastar e soltar do `je-ui` (ver o `//!`): sem eles, devolve `None` e nenhum
/// ouvinte é instalado.
pub fn listen_drag_event(_ui: &Ui, _node: NodeId, _kind: DragEventKind, _handler: DragHandler) -> Option<DisposableRef> {
    None
}

/// `IDragAndDropData`.
pub trait IDragAndDropData {
    /// `update(dataTransfer)`.
    fn update(&self, data_transfer: &DataTransfer);

    /// `getData()`: o `unknown` do original, que quem conhece o tipo lê com `downcast_ref`.
    fn get_data(&self) -> Rc<dyn Any>;

    /// O próprio objeto, para o `instanceof` do original (`downcast_ref`).
    fn as_any(&self) -> &dyn Any;
}
