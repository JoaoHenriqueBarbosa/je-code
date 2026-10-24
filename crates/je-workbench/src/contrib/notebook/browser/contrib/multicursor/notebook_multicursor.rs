//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/contrib/multicursor/notebookMulticursor.ts`: o relé de edição do
//! `NotebookMultiCursorController`, isto é, o que o controlador faz com um evento de operação do editor da célula âncora
//! (`handleEditorOperationEvent` e `executeEditorOperation`): repassá-lo aos controladores de cursor das demais células rastreadas.
//! O resto do arquivo (a busca e o rastreio de seleções, as decorações, o desfazer e refazer, as ações e as chaves de contexto) depende
//! do widget de notebook e do `ICodeEditor` da célula e fica para as fatias deles.
//!
//! Divergências:
//! - `CursorsController` é o trait [`ICursorsOperationTarget`], com os cinco comandos que `executeEditorOperation` chama; o
//!   `CursorsController` real o implementa. É o que permite, como o original (que troca `controller.paste` por uma função do caso),
//!   observar o que o relé repassa sem montar um modelo de texto.
//! - `ICellViewModel` é o [`IMultiCursorCell`], com o `handle` e o `uri` que o relé lê; o [`CellViewModel`] o implementa. O
//!   `ICellViewModel` do porte (o da busca) não tem esses dois membros.
//! - `anchorCell: [ICellViewModel, ICodeEditor] | undefined` é o [`AnchorCell`]; o relé só lê a célula, e o editor é opcional
//!   (nenhum caminho portado o usa).
//! - `TrackedCell` guarda só a `cellViewModel`: os outros campos (`initialSelection`, `matchSelections`, `editorConfig`,
//!   `cursorConfig`, `decorationIds`, `undoRedoHistory`) pertencem ao rastreio de seleções, que não está portado.
//! - `Object.assign(Object.create(NotebookMultiCursorController.prototype), { anchorCell, trackedCells, cursorsControllers })`
//!   (o controlador sem o construtor, que pede o editor de notebook e os serviços) é [`NotebookMultiCursorController::from_parts`];
//!   `handleEditorOperationEvent`, que no original é `private`, é público porque o caso o chama pelo tipo estrutural.
//! - `controller.paste(..., args.text || '', ...)` e os demais usam as leituras de `payload` do `CodeEditorWidget`
//!   (`payload_text`, `payload_count`, `payload_flag`, `payload_lines`), as mesmas do `trigger` do editor.
//! - `new ViewModelEventsCollector()` é o `ViewModelEventsCollector::new()`.

use std::rc::Rc;

use je_base::js_string::JsString;
use je_base::map::ResourceMap;
use je_base::uri::URI;
use je_editor::code_editor_widget::{Handler, ICodeEditor, ITriggerEditorOperationEvent, payload_count, payload_flag, payload_lines, payload_text};
use je_editor::cursor::CursorsController;
use je_editor::view_model_event_dispatcher::ViewModelEventsCollector;

use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;

/// Os comandos de `CursorsController` que `executeEditorOperation` chama.
pub trait ICursorsOperationTarget {
    fn start_composition(&self, events_collector: &ViewModelEventsCollector);
    fn end_composition(&self, events_collector: &ViewModelEventsCollector, source: Option<&str>);
    fn composition_type(
        &self,
        events_collector: &ViewModelEventsCollector,
        text: &[u16],
        replace_prev_char_cnt: i32,
        replace_next_char_cnt: i32,
        position_delta: i32,
        source: Option<&str>,
    );
    fn paste(
        &self,
        events_collector: &ViewModelEventsCollector,
        text: &[u16],
        paste_on_new_line: bool,
        multicursor_text: Option<&[JsString]>,
        source: Option<&str>,
        is_block: bool,
    );
    fn cut(&self, events_collector: &ViewModelEventsCollector, source: Option<&str>);
}

impl ICursorsOperationTarget for CursorsController {
    fn start_composition(&self, events_collector: &ViewModelEventsCollector) {
        CursorsController::start_composition(self, events_collector);
    }

    fn end_composition(&self, events_collector: &ViewModelEventsCollector, source: Option<&str>) {
        CursorsController::end_composition(self, events_collector, source);
    }

    fn composition_type(
        &self,
        events_collector: &ViewModelEventsCollector,
        text: &[u16],
        replace_prev_char_cnt: i32,
        replace_next_char_cnt: i32,
        position_delta: i32,
        source: Option<&str>,
    ) {
        CursorsController::composition_type(self, events_collector, text, replace_prev_char_cnt, replace_next_char_cnt, position_delta, source);
    }

    fn paste(
        &self,
        events_collector: &ViewModelEventsCollector,
        text: &[u16],
        paste_on_new_line: bool,
        multicursor_text: Option<&[JsString]>,
        source: Option<&str>,
        is_block: bool,
    ) {
        CursorsController::paste(self, events_collector, text, paste_on_new_line, multicursor_text, source, is_block);
    }

    fn cut(&self, events_collector: &ViewModelEventsCollector, source: Option<&str>) {
        CursorsController::cut(self, events_collector, source);
    }
}

/// O que o relé lê do `ICellViewModel`.
pub trait IMultiCursorCell {
    fn handle(&self) -> i64;
    fn uri(&self) -> URI;
}

impl IMultiCursorCell for CellViewModel {
    fn handle(&self) -> i64 {
        CellViewModel::handle(self)
    }

    fn uri(&self) -> URI {
        CellViewModel::uri(self)
    }
}

/// `[ICellViewModel, ICodeEditor]` de `anchorCell`.
pub struct AnchorCell {
    pub cell: Rc<dyn IMultiCursorCell>,
    pub editor: Option<Rc<dyn ICodeEditor>>,
}

/// `TrackedCell` (só a `cellViewModel`, ver as divergências do módulo).
pub struct TrackedCell {
    pub cell_view_model: Rc<dyn IMultiCursorCell>,
}

/// `NotebookMultiCursorController` (só o relé de edição, ver as divergências do módulo).
pub struct NotebookMultiCursorController {
    anchor_cell: Option<AnchorCell>,
    tracked_cells: Vec<TrackedCell>,
    cursors_controllers: ResourceMap<Rc<dyn ICursorsOperationTarget>>,
}

impl NotebookMultiCursorController {
    pub const ID: &'static str = "notebook.multiCursorController";

    pub fn from_parts(
        anchor_cell: Option<AnchorCell>,
        tracked_cells: Vec<TrackedCell>,
        cursors_controllers: ResourceMap<Rc<dyn ICursorsOperationTarget>>,
    ) -> Self {
        Self { anchor_cell, tracked_cells, cursors_controllers }
    }

    pub fn handle_editor_operation_event(&self, e: &ITriggerEditorOperationEvent) {
        for cell in &self.tracked_cells {
            if self.anchor_cell.as_ref().is_some_and(|anchor| cell.cell_view_model.handle() == anchor.cell.handle()) {
                continue;
            }

            let events_collector = ViewModelEventsCollector::new();
            let Some(controller) = self.cursors_controllers.get(&cell.cell_view_model.uri()) else {
                continue;
            };
            Self::execute_editor_operation(controller.as_ref(), &events_collector, e);
        }
    }

    fn execute_editor_operation(controller: &dyn ICursorsOperationTarget, events_collector: &ViewModelEventsCollector, e: &ITriggerEditorOperationEvent) {
        let source = e.source.as_deref();
        let args = &e.payload;
        match e.handler_id.as_str() {
            Handler::COMPOSITION_START => controller.start_composition(events_collector),
            Handler::COMPOSITION_END => controller.end_composition(events_collector, source),
            Handler::REPLACE_PREVIOUS_CHAR => {
                controller.composition_type(events_collector, payload_text(args, "text").units(), payload_count(args, "replaceCharCnt"), 0, 0, source);
            }
            Handler::COMPOSITION_TYPE => {
                controller.composition_type(
                    events_collector,
                    payload_text(args, "text").units(),
                    payload_count(args, "replacePrevCharCnt"),
                    payload_count(args, "replaceNextCharCnt"),
                    payload_count(args, "positionDelta"),
                    source,
                );
            }
            Handler::PASTE => {
                controller.paste(
                    events_collector,
                    payload_text(args, "text").units(),
                    payload_flag(args, "pasteOnNewLine"),
                    payload_lines(args, "multicursorText").as_deref(),
                    source,
                    payload_flag(args, "isBlock"),
                );
            }
            Handler::CUT => controller.cut(events_collector, source),
            _ => {}
        }
    }
}
