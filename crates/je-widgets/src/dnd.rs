//! Porte de `src/vs/base/browser/dnd.ts` (os formatos de transferência e o `IDragAndDropData`) e do que o
//! arrastar e soltar do DOM entrega aos widgets: o [`DragEvent`].
//!
//! Do DOM para o Rust:
//! - O `DataTransfer` é o do `je-ui` ([`je_ui::DataTransfer`]), o mesmo objeto compartilhado por todos os eventos do
//!   mesmo arrasto (com o modo de leitura e escrita do repositório, `effectAllowed`, `dropEffect` e `setDragImage`).
//!   A imagem de arrasto que o widget cria fica nele ([`apply_drag_image`](crate::drag_image::apply_drag_image)); quem
//!   termina o arrasto a destrói ([`discard_drag_image`](crate::drag_image::discard_drag_image)).
//! - `DragEvent` é o que os widgets leem de um evento `drag*`: o alvo, o ponteiro (modificadores e posição), o
//!   `offsetY` (a distância do ponteiro ao topo do alvo) e o `dataTransfer`. O `preventDefault()` é o retorno do
//!   ouvinte ([`DragHandler`]): `true` o chama.
//! - **Ligação com o `je-ui`.** [`listen_drag_event`] é o `addDisposableListener(node, 'dragstart' | ..., handler)`:
//!   instala um `ui.listen(node, <EventKind>, false, ...)` que monta o [`DragEvent`] do [`Event`] do `je-ui` e chama o
//!   [`DragHandler`]; se ele devolver `true`, o evento recebe `preventDefault()`. O tipo do evento é o próprio
//!   [`EventKind`] (`DragStart`, `Drag`, `DragEnter`, `DragOver`, `DragLeave`, `Drop` ou `DragEnd`). O arrasto de
//!   uma linha começa porque o nó dela tem o atributo `draggable="true"` (`Tree::set_attribute`), que o `je-ui` lê.

use std::any::Any;
use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_ui::{DataTransfer, Event, EventKind, NodeId, PointerData, Ui};

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
    pub data_transfer: DataTransfer,
}

impl DragEvent {
    /// Monta o evento do widget a partir do evento do `je-ui`; `None` se ele não carrega ponteiro e `dataTransfer`
    /// (só os sete eventos de arrastar e soltar carregam).
    pub fn from_event(ui: &Ui, event: &Event) -> Option<Self> {
        let pointer = event.pointer?;
        let data_transfer = event.data_transfer.clone()?;
        let offset_y = pointer.y - ui.tree().absolute_bounds(event.target).y0;
        Some(Self { target: event.target, pointer, offset_y, data_transfer })
    }
}

/// O ouvinte de um evento de arrastar e soltar: devolve se o evento deve receber `preventDefault()`.
pub type DragHandler = Rc<dyn Fn(&DragEvent) -> bool>;

/// `addDisposableListener(node, 'dragstart' | 'dragover' | 'drop' | 'dragleave' | 'dragend' | ..., handler)`: `kind` é
/// um dos sete [`EventKind`] de arrastar e soltar do `je-ui`.
pub fn listen_drag_event(ui: &Ui, node: NodeId, kind: EventKind, handler: DragHandler) -> DisposableRef {
    debug_assert!(
        matches!(kind, EventKind::DragStart | EventKind::Drag | EventKind::DragEnter | EventKind::DragOver | EventKind::DragLeave | EventKind::Drop | EventKind::DragEnd),
        "{kind:?} não é um evento de arrastar e soltar"
    );
    let reader = ui.clone();
    ui.listen(node, kind, false, move |event| {
        let Some(drag_event) = DragEvent::from_event(&reader, event) else { return };
        if handler(&drag_event) {
            event.prevent_default();
        }
    })
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
