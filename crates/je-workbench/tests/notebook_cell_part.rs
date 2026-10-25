//! Porte de `src/vs/workbench/contrib/notebook/test/browser/view/cellPart.test.ts` (suíte `CellPart`), caso a caso e na mesma
//! ordem. Os 7 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso.
//! - Os stubs de objeto literal do original (`stubEditor`, `editorPart`, `viewCell`, `notebookEditor`, `{ debug }`) são os
//!   tipos [`StubEditor`], [`StubEditorPart`], [`StubCell`], [`StubNotebook`] e [`NoopLog`], que implementam os contratos de
//!   `CodeCellLayout`; os campos que o original muda no meio do caso (`contentHeight`, `scrollTop`, `layoutInfo.editorHeight`)
//!   são `Cell`.
//! - `editorPart.style.top` (`''` ou `'NNNpx'`) é `Option<f64>`; `parseInt((top || '0').replace(/px$/, ''))` é `unwrap_or(0.0)`.
//! - `scrollBottom` é `scrollTop + viewportHeight` em todos os casos (no original é um getter sobre o mesmo valor).
//! - O cast parcial `as unknown as CodeCellLayoutInfo` é o [`layout_info`], que completa os campos que o original deixa
//!   `undefined` (e que a conta não lê) com zero.
//! - `stubEditor.layoutCalls.at(-1)?.height` é `last_layout_height()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::notebook::browser::view::cell_parts::code_cell_layout::{
    CellEditorDimension, CellLayoutChangeReason, CodeCellLayout, ICodeCellLayoutCell, ICodeCellLayoutEditor, ICodeCellLayoutEditorPart,
    ICodeCellLayoutLogService, ICodeCellLayoutNotebook,
};
use je_workbench::contrib::notebook::browser::view_model::cell_view_model_types::{CellLayoutState, CodeCellLayoutInfo};

struct StubEditor {
    layout_calls: RefCell<Vec<CellEditorDimension>>,
    last_scroll_top_set: Cell<f64>,
    layout_height: Cell<f64>,
    content_height: Cell<f64>,
}

impl StubEditor {
    fn new(layout_height: f64, content_height: f64) -> Self {
        Self { layout_calls: RefCell::new(Vec::new()), last_scroll_top_set: Cell::new(-1.0), layout_height: Cell::new(layout_height), content_height: Cell::new(content_height) }
    }

    fn last_layout_height(&self) -> Option<f64> {
        self.layout_calls.borrow().last().map(|dimension| dimension.height)
    }
}

impl ICodeCellLayoutEditor for StubEditor {
    fn get_layout_info(&self) -> CellEditorDimension {
        CellEditorDimension { width: 600.0, height: self.layout_height.get() }
    }

    fn get_content_height(&self) -> f64 {
        self.content_height.get()
    }

    fn layout(&self, dimension: CellEditorDimension) {
        self.layout_calls.borrow_mut().push(dimension);
    }

    fn set_scroll_top(&self, scroll_top: f64) {
        self.last_scroll_top_set.set(scroll_top);
    }
}

struct StubEditorPart {
    top: Cell<Option<f64>>,
}

impl StubEditorPart {
    fn new() -> Self {
        Self { top: Cell::new(None) }
    }

    /// `parseInt((editorPart.style.top || '0').replace(/px$/, ''))`.
    fn parsed_top(&self) -> f64 {
        self.top.get().unwrap_or(0.0)
    }
}

impl ICodeCellLayoutEditorPart for StubEditorPart {
    fn set_top(&self, top: Option<f64>) {
        self.top.set(top);
    }
}

struct StubCell {
    layout_info: RefCell<CodeCellLayoutInfo>,
}

impl ICodeCellLayoutCell for StubCell {
    fn is_input_collapsed(&self) -> bool {
        false
    }

    fn layout_info(&self) -> CodeCellLayoutInfo {
        self.layout_info.borrow().clone()
    }
}

struct StubNotebook {
    scroll_top: Cell<f64>,
    viewport_height: f64,
    element_top: f64,
    element_height: f64,
    output_container_offset: f64,
}

impl ICodeCellLayoutNotebook for StubNotebook {
    fn scroll_top(&self) -> f64 {
        self.scroll_top.get()
    }

    fn scroll_bottom(&self) -> f64 {
        self.scroll_top.get() + self.viewport_height
    }

    fn layout_line_height(&self) -> f64 {
        LINE_HEIGHT
    }

    fn layout_height(&self) -> f64 {
        self.viewport_height
    }

    fn get_absolute_top_of_element(&self) -> f64 {
        self.element_top
    }

    fn get_absolute_bottom_of_element(&self) -> f64 {
        self.element_top + self.output_container_offset
    }

    fn get_height_of_element(&self) -> f64 {
        self.element_height
    }

    fn editor_top_padding(&self) -> f64 {
        6.0
    }
}

struct NoopLog;

impl ICodeCellLayoutLogService for NoopLog {
    fn debug(&self, _output: &str) {}
}

const LINE_HEIGHT: f64 = 21.0;
const STATUSBAR_HEIGHT: f64 = 22.0;
const CELL_TOP_MARGIN: f64 = 6.0;
const CELL_OUTLINE_WIDTH: f64 = 1.0;

fn layout_info(editor_height: f64, output_container_offset: f64, editor_width: f64) -> CodeCellLayoutInfo {
    CodeCellLayoutInfo {
        font_info: None,
        editor_height,
        editor_width,
        chat_height: 0.0,
        status_bar_height: STATUSBAR_HEIGHT,
        comment_offset: 0.0,
        comment_height: 0.0,
        output_container_offset,
        output_total_height: 0.0,
        output_show_more_container_height: 0.0,
        output_show_more_container_offset: 0.0,
        total_height: 0.0,
        code_indicator_height: 0.0,
        output_indicator_height: 0.0,
        bottom_toolbar_offset: 0.0,
        layout_state: CellLayoutState::Uninitialized,
        estimated_has_horizontal_scrolling: false,
        outline_width: CELL_OUTLINE_WIDTH,
        top_margin: CELL_TOP_MARGIN,
        bottom_margin: 0.0,
    }
}

fn notebook(scroll_top: f64, viewport_height: f64, element_top: f64, element_height: f64, output_container_offset: f64) -> StubNotebook {
    StubNotebook { scroll_top: Cell::new(scroll_top), viewport_height, element_top, element_height, output_container_offset }
}

fn cell(editor_height: f64, output_container_offset: f64, editor_width: f64) -> StubCell {
    StubCell { layout_info: RefCell::new(layout_info(editor_height, output_container_offset, editor_width)) }
}

fn dimension(width: f64, height: f64) -> CellEditorDimension {
    CellEditorDimension { width, height }
}

struct Scenario {
    name: &'static str,
    scroll_top: f64,
    viewport_height: f64,
    editor_content_height: f64,
    editor_height: f64,
    output_container_offset: f64,
    expected: &'static str,
    element_top: f64,
    element_height: f64,
    expected_top: f64,
    expected_editor_scroll_top: f64,
}

#[test]
fn code_cell_layout_editor_visibility_states() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    const DEFAULT_ELEMENT_TOP: f64 = 100.0;
    const DEFAULT_ELEMENT_HEIGHT: f64 = 900.0;

    let scenarios = [
        Scenario {
            name: "Full",
            scroll_top: 0.0,
            viewport_height: 400.0,
            editor_content_height: 300.0,
            editor_height: 300.0,
            output_container_offset: 300.0,
            expected: "Full",
            element_top: DEFAULT_ELEMENT_TOP,
            element_height: DEFAULT_ELEMENT_HEIGHT,
            expected_top: 0.0,
            expected_editor_scroll_top: 0.0,
        },
        Scenario {
            name: "Bottom Clipped",
            scroll_top: 0.0,
            viewport_height: 350.0,
            editor_content_height: 300.0,
            editor_height: 300.0,
            output_container_offset: 300.0,
            expected: "Bottom Clipped",
            element_top: DEFAULT_ELEMENT_TOP,
            element_height: DEFAULT_ELEMENT_HEIGHT,
            expected_top: 0.0,
            expected_editor_scroll_top: 0.0,
        },
        Scenario {
            name: "Full (Small Viewport)",
            scroll_top: DEFAULT_ELEMENT_TOP + CELL_TOP_MARGIN + 20.0,
            viewport_height: 220.0,
            editor_content_height: 500.0,
            editor_height: 500.0,
            output_container_offset: 600.0,
            expected: "Full (Small Viewport)",
            element_top: DEFAULT_ELEMENT_TOP,
            element_height: DEFAULT_ELEMENT_HEIGHT,
            expected_top: 19.0,
            expected_editor_scroll_top: 19.0,
        },
        Scenario {
            name: "Top Clipped",
            scroll_top: DEFAULT_ELEMENT_TOP + CELL_TOP_MARGIN + 40.0,
            viewport_height: 600.0,
            editor_content_height: 200.0,
            editor_height: 200.0,
            output_container_offset: 450.0,
            expected: "Top Clipped",
            element_top: DEFAULT_ELEMENT_TOP,
            element_height: DEFAULT_ELEMENT_HEIGHT,
            expected_top: 39.0,
            expected_editor_scroll_top: 40.0,
        },
        Scenario {
            name: "Invisible",
            scroll_top: DEFAULT_ELEMENT_TOP + 1000.0,
            viewport_height: 400.0,
            editor_content_height: 300.0,
            editor_height: 300.0,
            output_container_offset: 300.0,
            expected: "Invisible",
            element_top: DEFAULT_ELEMENT_TOP,
            element_height: DEFAULT_ELEMENT_HEIGHT,
            expected_top: 278.0,
            expected_editor_scroll_top: 279.0,
        },
    ];

    for s in &scenarios {
        let editor = StubEditor::new(s.editor_height, s.editor_content_height);
        let editor_part = StubEditorPart::new();
        let view_cell = cell(s.editor_height, s.output_container_offset, 0.0);
        let notebook_editor = notebook(s.scroll_top, s.viewport_height, s.element_top, s.element_height, s.output_container_offset);
        let layout = CodeCellLayout::new(true, &notebook_editor, &view_cell, &editor_part, &editor, &NoopLog, dimension(600.0, s.editor_height));

        layout.layout_editor(CellLayoutChangeReason::Init);
        assert_eq!(
            layout.editor_visibility().map(|visibility| visibility.as_str()),
            Some(s.expected),
            "Scenario '{}' (scrollTop={}) expected visibility {} but got {:?}",
            s.name,
            s.scroll_top,
            s.expected,
            layout.editor_visibility()
        );
        assert_eq!(
            editor_part.parsed_top(),
            s.expected_top,
            "Scenario '{}' (scrollTop={}) expected top {}px but got {:?}",
            s.name,
            s.scroll_top,
            s.expected_top,
            editor_part.top.get()
        );
        assert_eq!(
            editor.last_scroll_top_set.get(),
            s.expected_editor_scroll_top,
            "Scenario '{}' (scrollTop={}) expected editor.setScrollTop({}) but got {}",
            s.name,
            s.scroll_top,
            s.expected_editor_scroll_top,
            editor.last_scroll_top_set.get()
        );

        // Sanidade: o `style.top` é sempre escrito (o layout rodou), inclusive no estado Invisible.
        assert!(editor_part.top.get().is_some(), "Scenario '{}' should set a top style value", s.name);
    }
}

struct Expected {
    top: f64,
    visibility: &'static str,
    editor_scroll_top: f64,
}

#[test]
fn scrolling() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    const VIEWPORT_HEIGHT: f64 = 300.0;
    const ELEMENT_TOP: f64 = 100.0;
    const EDITOR_CONTENT_HEIGHT: f64 = 800.0;
    const EDITOR_HEIGHT: f64 = EDITOR_CONTENT_HEIGHT;
    const OUTPUT_CONTAINER_OFFSET: f64 = 800.0;
    const ELEMENT_HEIGHT: f64 = 1200.0;

    fn clamp(v: f64, min: f64, max: f64) -> f64 {
        v.max(min).min(max)
    }

    // A conta de produção espelhada aqui (duplicação aceita no teste) para qualquer divergência aparecer.
    fn compute_expected(scroll_top: f64) -> Expected {
        let scroll_bottom = scroll_top + VIEWPORT_HEIGHT;
        let viewport_height = VIEWPORT_HEIGHT;
        let editor_bottom = ELEMENT_TOP + OUTPUT_CONTAINER_OFFSET;
        let mut top = (scroll_top - ELEMENT_TOP - CELL_TOP_MARGIN - CELL_OUTLINE_WIDTH).max(0.0);
        let possible_editor_height = EDITOR_HEIGHT - top;
        if possible_editor_height < LINE_HEIGHT {
            top = top - (LINE_HEIGHT - possible_editor_height) - CELL_OUTLINE_WIDTH;
        }
        let visibility;
        let editor_scroll_top;
        if scroll_top <= ELEMENT_TOP + CELL_TOP_MARGIN {
            if scroll_bottom >= editor_bottom {
                visibility = "Full";
            } else {
                visibility = "Bottom Clipped";
            }
            editor_scroll_top = 0.0;
        } else if viewport_height <= EDITOR_CONTENT_HEIGHT && scroll_bottom <= editor_bottom {
            visibility = "Full (Small Viewport)";
            editor_scroll_top = top;
        } else {
            let minimum_editor_height = LINE_HEIGHT;
            let height = clamp(EDITOR_CONTENT_HEIGHT - (scroll_top - (ELEMENT_TOP + CELL_TOP_MARGIN)), minimum_editor_height, EDITOR_CONTENT_HEIGHT);
            visibility = if scroll_top > editor_bottom { "Invisible" } else { "Top Clipped" };
            editor_scroll_top = EDITOR_CONTENT_HEIGHT - height;
        }
        Expected { top, visibility, editor_scroll_top }
    }

    let mut scroll_top = 0.0;
    while scroll_top <= VIEWPORT_HEIGHT + OUTPUT_CONTAINER_OFFSET + 20.0 {
        let expected = compute_expected(scroll_top);
        let editor = StubEditor::new(EDITOR_HEIGHT, EDITOR_CONTENT_HEIGHT);
        let editor_part = StubEditorPart::new();
        let view_cell = cell(EDITOR_HEIGHT, OUTPUT_CONTAINER_OFFSET, 0.0);
        let notebook_editor = notebook(scroll_top, VIEWPORT_HEIGHT, ELEMENT_TOP, ELEMENT_HEIGHT, OUTPUT_CONTAINER_OFFSET);
        let layout = CodeCellLayout::new(true, &notebook_editor, &view_cell, &editor_part, &editor, &NoopLog, dimension(600.0, EDITOR_HEIGHT));
        layout.layout_editor(CellLayoutChangeReason::NbDidScroll);
        assert_eq!(editor_part.parsed_top(), expected.top, "scrollTop={scroll_top}: expected top {}, got {}", expected.top, editor_part.parsed_top());
        assert_eq!(
            layout.editor_visibility().map(|visibility| visibility.as_str()),
            Some(expected.visibility),
            "scrollTop={scroll_top}: expected visibility {}, got {:?}",
            expected.visibility,
            layout.editor_visibility()
        );
        assert_eq!(
            editor.last_scroll_top_set.get(),
            expected.editor_scroll_top,
            "scrollTop={scroll_top}: expected editorScrollTop {}, got {}",
            expected.editor_scroll_top,
            editor.last_scroll_top_set.get()
        );
        scroll_top += 1.0;
    }
}

#[test]
fn code_cell_layout_reuses_content_height_after_init() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    const VIEWPORT_HEIGHT: f64 = 1000.0;
    const ELEMENT_TOP: f64 = 100.0;
    const ELEMENT_HEIGHT: f64 = 1200.0;
    const OUTPUT_CONTAINER_OFFSET: f64 = 300.0;
    const EDITOR_HEIGHT: f64 = 800.0;

    let editor = StubEditor::new(EDITOR_HEIGHT, 800.0);
    let editor_part = StubEditorPart::new();
    let view_cell = cell(EDITOR_HEIGHT, OUTPUT_CONTAINER_OFFSET, 600.0);
    let notebook_editor = notebook(0.0, VIEWPORT_HEIGHT, ELEMENT_TOP, ELEMENT_HEIGHT, OUTPUT_CONTAINER_OFFSET);
    let layout = CodeCellLayout::new(true, &notebook_editor, &view_cell, &editor_part, &editor, &NoopLog, dimension(600.0, EDITOR_HEIGHT));

    layout.layout_editor(CellLayoutChangeReason::Init);
    assert_eq!(layout.editor_visibility().map(|visibility| visibility.as_str()), Some("Full"));
    assert_eq!(editor.last_layout_height(), Some(800.0));

    // Simula o Monaco informando uma altura de conteúdo menor e transitória na rolagem.
    editor.content_height.set(200.0);
    layout.layout_editor(CellLayoutChangeReason::NbDidScroll);
    assert_eq!(layout.editor_visibility().map(|visibility| visibility.as_str()), Some("Full"));
    assert_eq!(editor.last_layout_height(), Some(800.0), "nbDidScroll should reuse the established content height");

    layout.layout_editor(CellLayoutChangeReason::OnDidContentSizeChange);
    assert_eq!(layout.editor_visibility().map(|visibility| visibility.as_str()), Some("Full"));
    assert_eq!(editor.last_layout_height(), Some(200.0), "onDidContentSizeChange should refresh the content height");
}

#[test]
fn code_cell_layout_refreshes_content_height_on_view_cell_layout_change() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    const VIEWPORT_HEIGHT: f64 = 1000.0;
    const ELEMENT_TOP: f64 = 100.0;
    const ELEMENT_HEIGHT: f64 = 1200.0;
    const INITIAL_CONTENT_HEIGHT: f64 = 37.0;
    const OUTPUT_CONTAINER_OFFSET: f64 = 300.0;
    const UPDATED_CONTENT_HEIGHT: f64 = 200.0;

    let editor = StubEditor::new(INITIAL_CONTENT_HEIGHT, INITIAL_CONTENT_HEIGHT);
    let editor_part = StubEditorPart::new();
    let view_cell = cell(INITIAL_CONTENT_HEIGHT, OUTPUT_CONTAINER_OFFSET, 600.0);
    let notebook_editor = notebook(0.0, VIEWPORT_HEIGHT, ELEMENT_TOP, ELEMENT_HEIGHT, OUTPUT_CONTAINER_OFFSET);
    let layout = CodeCellLayout::new(true, &notebook_editor, &view_cell, &editor_part, &editor, &NoopLog, dimension(600.0, INITIAL_CONTENT_HEIGHT));

    layout.layout_editor(CellLayoutChangeReason::Init);
    assert_eq!(editor.last_layout_height(), Some(INITIAL_CONTENT_HEIGHT));

    // Simula o aumento de altura por quebra de linha depois que largura e layout se assentam.
    editor.content_height.set(UPDATED_CONTENT_HEIGHT);
    layout.layout_editor(CellLayoutChangeReason::ViewCellLayoutChange);
    assert_eq!(editor.last_layout_height(), Some(UPDATED_CONTENT_HEIGHT), "viewCellLayoutChange should refresh the content height");

    // As rolagens seguintes continuam reaproveitando a altura (maior) estabelecida.
    editor.content_height.set(50.0);
    layout.layout_editor(CellLayoutChangeReason::NbDidScroll);
    assert_eq!(editor.last_layout_height(), Some(UPDATED_CONTENT_HEIGHT), "nbDidScroll should reuse the refreshed content height");
}

#[test]
fn code_cell_layout_maintains_content_height_after_paste_when_scrolling() {
    // Regressão de https://github.com/microsoft/vscode/issues/284524
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    const VIEWPORT_HEIGHT: f64 = 1000.0;
    const ELEMENT_TOP: f64 = 100.0;
    const ELEMENT_HEIGHT: f64 = 1200.0;
    const INITIAL_CONTENT_HEIGHT: f64 = 37.0;
    const INITIAL_EDITOR_HEIGHT: f64 = INITIAL_CONTENT_HEIGHT;
    const OUTPUT_CONTAINER_OFFSET: f64 = 300.0;
    const PASTED_CONTENT_HEIGHT: f64 = 679.0;

    let editor = StubEditor::new(INITIAL_EDITOR_HEIGHT, INITIAL_CONTENT_HEIGHT);
    let editor_part = StubEditorPart::new();
    let view_cell = cell(INITIAL_EDITOR_HEIGHT, OUTPUT_CONTAINER_OFFSET, 600.0);
    let notebook_editor = notebook(0.0, VIEWPORT_HEIGHT, ELEMENT_TOP, ELEMENT_HEIGHT, OUTPUT_CONTAINER_OFFSET);
    let layout = CodeCellLayout::new(true, &notebook_editor, &view_cell, &editor_part, &editor, &NoopLog, dimension(600.0, INITIAL_EDITOR_HEIGHT));

    // Layout inicial.
    layout.layout_editor(CellLayoutChangeReason::Init);

    // Simula a colagem: o conteúdo cresce para 679 px.
    editor.content_height.set(PASTED_CONTENT_HEIGHT);
    view_cell.layout_info.borrow_mut().editor_height = PASTED_CONTENT_HEIGHT;
    layout.layout_editor(CellLayoutChangeReason::OnDidContentSizeChange);

    // Ao rolar, o Monaco informa uma altura menor e transitória (39 px); vale a estabelecida (679 px).
    editor.content_height.set(39.0);
    notebook_editor.scroll_top.set(200.0);
    layout.layout_editor(CellLayoutChangeReason::NbDidScroll);

    let final_height = editor.last_layout_height();

    assert_ne!(final_height, Some(39.0), "Should not use Monaco's transient value (39px)");
    assert_ne!(final_height, Some(37.0), "Should not use initial content height (37px)");
    assert!(
        final_height.is_some_and(|height| height > 100.0),
        "Layout height ({final_height:?}px) should be calculated from established 679px content, not transient 39px or initial 37px"
    );
}

#[test]
fn code_cell_layout_does_not_programmatically_scroll_editor_while_pointer_down() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    const VIEWPORT_HEIGHT: f64 = 220.0;
    const ELEMENT_TOP: f64 = 100.0;
    const EDITOR_CONTENT_HEIGHT: f64 = 500.0;
    const EDITOR_HEIGHT: f64 = EDITOR_CONTENT_HEIGHT;
    const OUTPUT_CONTAINER_OFFSET: f64 = 600.0;
    const ELEMENT_HEIGHT: f64 = 900.0;
    const SCROLL_TOP: f64 = ELEMENT_TOP + CELL_TOP_MARGIN + 20.0;

    let editor = StubEditor::new(EDITOR_HEIGHT, EDITOR_CONTENT_HEIGHT);
    let editor_part = StubEditorPart::new();
    let view_cell = cell(EDITOR_HEIGHT, OUTPUT_CONTAINER_OFFSET, 0.0);
    let notebook_editor = notebook(SCROLL_TOP, VIEWPORT_HEIGHT, ELEMENT_TOP, ELEMENT_HEIGHT, OUTPUT_CONTAINER_OFFSET);
    let layout = CodeCellLayout::new(true, &notebook_editor, &view_cell, &editor_part, &editor, &NoopLog, dimension(600.0, EDITOR_HEIGHT));

    layout.layout_editor(CellLayoutChangeReason::Init);
    editor.last_scroll_top_set.set(-1.0);

    layout.set_pointer_down(true);
    layout.layout_editor(CellLayoutChangeReason::NbDidScroll);
    assert_eq!(layout.editor_visibility().map(|visibility| visibility.as_str()), Some("Full (Small Viewport)"));
    assert_eq!(editor.last_scroll_top_set.get(), -1.0, "Expected no programmatic editor.setScrollTop while pointer is down");

    layout.set_pointer_down(false);
    layout.layout_editor(CellLayoutChangeReason::NbDidScroll);
    assert_eq!(layout.editor_visibility().map(|visibility| visibility.as_str()), Some("Full (Small Viewport)"));
    assert_ne!(editor.last_scroll_top_set.get(), -1.0, "Expected editor.setScrollTop to resume once pointer is released");
}

#[test]
fn code_cell_layout_init_ignores_stale_pooled_editor_content_height() {
    // Guarda de regressão da sobreposição em rolagem rápida com editores reaproveitados: o init usa a dimensão inicial da
    // própria célula, não o `getContentHeight()` do editor reaproveitado.
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    const VIEWPORT_HEIGHT: f64 = 400.0;
    const ELEMENT_TOP: f64 = 100.0;
    const ELEMENT_HEIGHT: f64 = 500.0;
    const OUTPUT_CONTAINER_OFFSET: f64 = 200.0;

    let pooled_content_height = 200.0; // célula alta anterior
    let pooled_editor = StubEditor::new(pooled_content_height, pooled_content_height);
    let editor_part = StubEditorPart::new();

    // Primeiro, o layout de uma célula alta estabelece uma altura grande no editor reaproveitado.
    let tall_view_cell = cell(200.0, OUTPUT_CONTAINER_OFFSET, 600.0);
    let tall_notebook_editor = notebook(0.0, VIEWPORT_HEIGHT, ELEMENT_TOP, ELEMENT_HEIGHT, OUTPUT_CONTAINER_OFFSET);
    let tall_layout = CodeCellLayout::new(true, &tall_notebook_editor, &tall_view_cell, &editor_part, &pooled_editor, &NoopLog, dimension(600.0, 200.0));

    tall_layout.layout_editor(CellLayoutChangeReason::Init);
    assert_eq!(pooled_editor.last_layout_height(), Some(200.0), "Expected tall cell to lay out using its own height");

    // Agora o mesmo editor serve a uma célula curta, com a altura de conteúdo grande (valor velho) ainda no editor.
    pooled_editor.content_height.set(200.0);
    let short_view_cell = cell(37.0, OUTPUT_CONTAINER_OFFSET, 600.0);
    let short_notebook_editor = notebook(0.0, VIEWPORT_HEIGHT, ELEMENT_TOP, ELEMENT_HEIGHT, OUTPUT_CONTAINER_OFFSET);
    let short_layout = CodeCellLayout::new(true, &short_notebook_editor, &short_view_cell, &editor_part, &pooled_editor, &NoopLog, dimension(600.0, 37.0));

    short_layout.layout_editor(CellLayoutChangeReason::Init);
    assert_eq!(
        pooled_editor.last_layout_height(),
        Some(37.0),
        "Init layout for a short cell should use the cell's initial height, not the pooled editor's stale content height"
    );
}
