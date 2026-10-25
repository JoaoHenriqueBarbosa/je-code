//! Porte de `src/vs/editor/test/browser/view/wordWrapIndicator.test.ts` (suíte `WordWrapIndicatorOverlay`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `IEditorOptions` é o objeto `Dynamic` das opções (`{ ...A, key: value }` é [`merge`], que sobrescreve as chaves
//!   repetidas) e `testViewModel` é `test_view_model` de `tests/support/test_view_model.rs`.
//! - O `render` do overlay devolve o `<div class="wwi" ...>↩</div>` no original; a UI é nativa e o resultado de cada
//!   linha é um `Option<WordWrapIndicator>` (`indicator(left, height)` é o `Some` equivalente, e a string vazia é
//!   `None`). Ver `word_wrap_indicator.rs`.
//! - `new ViewContext(configuration, new TestColorTheme(), viewModel)` usa o tema do porte (`je_theme::ColorTheme`,
//!   sem cores), que é o que o `ViewContext` guarda.
//! - `sinon.spy(viewModel, 'getViewportViewLineRenderingData')` não tem equivalente em Rust (não se troca um método
//!   de um objeto): no caso `does_not_construct_line_rendering_data_to_find_wrapped_lines` o que se confere é o
//!   resultado que só a consulta `getViewLineContinuesWithWrappedLine` dá, a mesma que o `ViewportData` usa, e a
//!   contagem de chamadas do espião não é observável.
//! - O `try { ... } finally { overlay.dispose() }` do `withOverlay` é um guarda com `Drop`, que descarta também
//!   quando o corpo do caso entra em pânico.
//! - O `Record<string, boolean>` das invalidações é a lista de pares `(nome, valor)`, na ordem em que o original
//!   preenche o registro.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::scrollable::{INewScrollPosition, ScrollEvent};
use je_editor::config::editor_configuration::IEditorConfiguration;
use je_editor::cursor_common::{CursorSimpleModel, EditorConfiguration};
use je_editor::dynamic_view_overlay::DynamicViewOverlay;
use je_editor::editor_common::ScrollType;
use je_editor::model::{ModelDecorationOptionsInit, ModelDeltaDecoration};
use je_editor::range::Range;
use je_editor::rendering_context::{HorizontalPosition, IViewLines, LineVisibleRanges, RenderingContext};
use je_editor::selection::Selection;
use je_editor::text_model::{ModelDecorationOptions, TextModel};
use je_editor::view_context::ViewContext;
use je_editor::view_events::{
    ViewDecorationsChangedEvent, ViewEventHandler, ViewFlushedEvent, ViewLineMappingChangedEvent,
    ViewLinesChangedEvent, ViewLinesDeletedEvent, ViewLinesInsertedEvent, ViewScrollChangedEvent,
    ViewTokensChangedEvent, ViewTokensChangedRange, ViewZonesChangedEvent,
};
use je_editor::view_lines_viewport_data::ViewportData;
use je_editor::view_model_types::{IPartialViewLinesViewportData, IViewModel};
use je_editor::position::Position;
use je_editor::word_wrap_indicator::{WORD_WRAP_INDICATOR_CHAR, WordWrapIndicator, WordWrapIndicatorOverlay};
use je_theme::{ColorTheme, ThemeType};
use support::test_view_model::{TestViewModel, test_view_model};

const LINE_HEIGHT: f64 = 19.0;

type Overlay = WordWrapIndicatorOverlay<TestViewModel>;

/// `IRenderOptions`.
#[derive(Clone, Copy, Default)]
struct RenderOptions {
    /// As linhas da visão que o viewport cobre. Por padrão, o documento inteiro.
    viewport: Option<(i32, i32)>,
    /// Pula a chamada de `prepareRender`, para exercitar `render` sozinho.
    skip_prepare_render: bool,
    /// O `prepare` de `renderIndicators`.
    prepare: Option<fn(&Rc<TestViewModel>, &Rc<TextModel>)>,
}

/// `IViewLines` do teste: `{ linesVisibleRangesForRange: () => null, visibleRangeForPosition: () => null }`.
struct NoViewLines;

impl IViewLines for NoViewLines {
    fn lines_visible_ranges_for_range(&self, _range: Range, _include_new_lines: bool) -> Option<Vec<LineVisibleRanges>> {
        None
    }

    fn visible_range_for_position(&self, _position: Position) -> Option<HorizontalPosition> {
        None
    }
}

/// O `finally { overlay.dispose(); }` do `withOverlay`.
struct DisposeOnDrop(Rc<Overlay>);

impl Drop for DisposeOnDrop {
    fn drop(&mut self) {
        self.0.dispose();
    }
}

/// `IOverlayContext`.
struct OverlayContext {
    overlay: Rc<Overlay>,
    configuration: Rc<dyn IEditorConfiguration>,
    view_model: Rc<TestViewModel>,
    model: Rc<TextModel>,
}

impl OverlayContext {
    /// Roda uma passada de render e devolve o que foi produzido para cada linha da visão do documento, inclusive as
    /// de fora do viewport.
    fn render(&self, render_options: RenderOptions) -> Vec<Option<WordWrapIndicator>> {
        render_overlay(&self.overlay, &self.view_model, render_options)
    }
}

/// `{ ...base, key: value, ... }`.
fn merge(base: &Dynamic, extra: &[(&str, Dynamic)]) -> Dynamic {
    let mut entries: Vec<(JsString, Dynamic)> = base.own_entries();
    for (key, value) in extra {
        let key = JsString::from(*key);
        match entries.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 = value.clone(),
            None => entries.push((key, value.clone())),
        }
    }
    Dynamic::object(entries)
}

fn object(entries: &[(&str, Dynamic)]) -> Dynamic {
    Dynamic::object(entries.iter().map(|(key, value)| (*key, value.clone())))
}

/// Cria um overlay sobre `text` e o entrega ao `callback` junto com o view model e a configuração de que ele lê,
/// para o caso poder tanto renderizá-lo quanto dirigir seus handlers de eventos da visão.
fn with_overlay(text: &[&str], options: &Dynamic, callback: impl FnOnce(&OverlayContext)) {
    test_view_model(text, options, |view_model, model, configuration| {
        let editor_configuration: Rc<dyn EditorConfiguration> = configuration.clone();
        let theme = Rc::new(ColorTheme::unloaded("test", ThemeType::Dark, HashMap::new()));
        let view_context = Rc::new(ViewContext::new(editor_configuration, theme, view_model.clone()));
        let overlay = WordWrapIndicatorOverlay::new(view_context);
        let _dispose = DisposeOnDrop(overlay.clone());
        callback(&OverlayContext {
            overlay,
            configuration: configuration.clone(),
            view_model: view_model.clone(),
            model: model.clone(),
        });
    });
}

fn render_overlay(
    overlay: &Rc<Overlay>,
    view_model: &Rc<TestViewModel>,
    render_options: RenderOptions,
) -> Vec<Option<WordWrapIndicator>> {
    let line_count = view_model.get_line_count();
    let (start_line_number, end_line_number) = render_options.viewport.unwrap_or((1, line_count));
    let viewport_line_count = end_line_number - start_line_number + 1;
    let viewport_data = ViewportData::new(
        vec![Selection::new(1, 1, 1, 1)],
        IPartialViewLinesViewportData {
            big_numbers_delta: 0.0,
            start_line_number,
            end_line_number,
            relative_vertical_offset: (0..viewport_line_count).map(|i| f64::from(i) * LINE_HEIGHT).collect(),
            centered_line_number: start_line_number,
            completely_visible_start_line_number: start_line_number,
            completely_visible_end_line_number: end_line_number,
            line_height: LINE_HEIGHT,
        },
        Vec::new(),
        view_model.clone(),
    );
    let ctx = RenderingContext::new(view_model.view_layout(), viewport_data, Rc::new(NoViewLines), None);

    if !render_options.skip_prepare_render {
        overlay.prepare_render(&ctx);
    }
    (1..=line_count).map(|line_number| overlay.render(start_line_number, line_number)).collect()
}

/// Renderiza o overlay sobre `text` e devolve o que ele produziu para cada linha da visão.
fn render_indicators(text: &[&str], options: &Dynamic, render_options: RenderOptions) -> Vec<Option<WordWrapIndicator>> {
    let mut result = Vec::new();
    with_overlay(text, options, |context| {
        if let Some(prepare) = render_options.prepare {
            prepare(&context.view_model, &context.model);
        }
        result = context.render(render_options);
    });
    result
}

fn indicator(left: f64, line_height: f64) -> Option<WordWrapIndicator> {
    Some(WordWrapIndicator { left, height: line_height, glyph: WORD_WRAP_INDICATOR_CHAR })
}

fn default_indicator(left: f64) -> Option<WordWrapIndicator> {
    indicator(left, LINE_HEIGHT)
}

fn scroll_event(scroll_top_changed: bool, scroll_left_changed: bool) -> ViewScrollChangedEvent {
    let source = ScrollEvent {
        in_smooth_scrolling: false,
        old_width: 0.0,
        old_scroll_width: 0.0,
        old_scroll_left: 0.0,
        width: 0.0,
        scroll_width: 0.0,
        scroll_left: 0.0,
        old_height: 0.0,
        old_scroll_height: 0.0,
        old_scroll_top: 0.0,
        height: 0.0,
        scroll_height: 0.0,
        scroll_top: 0.0,
        width_changed: false,
        scroll_width_changed: false,
        scroll_left_changed,
        height_changed: false,
        scroll_height_changed: false,
        scroll_top_changed,
    };
    ViewScrollChangedEvent::new(&source)
}

/// Dispara todo evento da visão que pode afetar um indicador num overlay que já pintou `text`, e informa quais
/// pediram um novo render.
fn invalidations_after_render(text: &[&str], options: &Dynamic) -> Vec<(&'static str, bool)> {
    let mut invalidations = Vec::new();
    with_overlay(text, options, |context| {
        context.render(RenderOptions::default());
        let overlay = &context.overlay;
        invalidations.push(("onDecorationsChanged", overlay.on_decorations_changed(&ViewDecorationsChangedEvent::new(None))));
        invalidations.push(("onFlushed", overlay.on_flushed(&ViewFlushedEvent)));
        invalidations.push(("onLineMappingChanged", overlay.on_line_mapping_changed(&ViewLineMappingChangedEvent)));
        invalidations.push(("onLinesChanged", overlay.on_lines_changed(&ViewLinesChangedEvent::new(1, 1))));
        invalidations.push(("onLinesDeleted", overlay.on_lines_deleted(&ViewLinesDeletedEvent::new(1, 1))));
        invalidations.push(("onLinesInserted", overlay.on_lines_inserted(&ViewLinesInsertedEvent::new(1, 1))));
        invalidations.push((
            "onTokensChanged",
            overlay.on_tokens_changed(&ViewTokensChangedEvent::new(vec![ViewTokensChangedRange {
                from_line_number: 1,
                to_line_number: 1,
            }])),
        ));
        invalidations.push(("onZonesChanged", overlay.on_zones_changed(&ViewZonesChangedEvent)));
        // A rolagem vertical muda as linhas pintadas. O glifo fica na coluna de quebra durante a rolagem
        // horizontal, então ela não exige um novo render.
        invalidations.push(("onScrolledVertically", overlay.on_scroll_changed(&scroll_event(true, false))));
        invalidations.push(("onScrolledHorizontally", overlay.on_scroll_changed(&scroll_event(false, true))));
    });
    invalidations
}

/// Aplica `new_options` a um overlay e informa se a mudança de configuração pediu um novo render. O overlay é um
/// handler de eventos da visão registrado, então a atualização chega a ele pelo despachante de verdade e deixa a
/// resposta em `shouldRender`.
fn configuration_change_invalidates(options: &Dynamic, new_options: &Dynamic) -> bool {
    configuration_changes_invalidate(options, std::slice::from_ref(new_options))[0]
}

fn configuration_changes_invalidate(options: &Dynamic, new_options: &[Dynamic]) -> Vec<bool> {
    let mut result = Vec::new();
    with_overlay(&["aaaaa bbbbb"], options, |context| {
        for update in new_options {
            context.overlay.on_did_render();
            context.configuration.update_options(update);
            result.push(context.overlay.should_render());
        }
    });
    result
}

const WRAPPED_TEXT: [&str; 3] = ["aaaaa bbbbb ccccc", "short", "ddddd eeeee"];
const INDICATOR_LEFT: f64 = 60.0;

fn viewport_options() -> Dynamic {
    object(&[
        ("folding", Dynamic::from(false)),
        ("glyphMargin", Dynamic::from(false)),
        ("lineDecorationsWidth", Dynamic::from(0)),
        ("lineNumbers", Dynamic::from("off")),
        ("minimap", object(&[("enabled", Dynamic::from(false))])),
        ("scrollbar", object(&[("verticalScrollbarSize", Dynamic::from(10))])),
    ])
}

// Com `wordWrapColumn: 6` isto mapeia para as linhas da visão:
//   1: 'aaaaa ' (quebrada)  2: 'bbbbb ' (quebrada)  3: 'ccccc'
//   4: 'short'
//   5: 'ddddd ' (quebrada)  6: 'eeeee'
fn wrapping_options() -> Dynamic {
    merge(&viewport_options(), &[("wordWrap", Dynamic::from("wordWrapColumn")), ("wordWrapColumn", Dynamic::from(6))])
}

fn with_indicator(options: &Dynamic, enabled: bool) -> Dynamic {
    merge(options, &[("wordWrapIndicator", Dynamic::from(enabled))])
}

fn wrapped_indicators() -> Vec<Option<WordWrapIndicator>> {
    vec![default_indicator(INDICATOR_LEFT), default_indicator(INDICATOR_LEFT), None, None, default_indicator(INDICATOR_LEFT), None]
}

#[test]
fn renders_an_indicator_for_every_soft_wrapped_view_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_indicators(&WRAPPED_TEXT, &with_indicator(&wrapping_options(), true), RenderOptions::default()),
        wrapped_indicators()
    );
}

#[test]
fn does_not_construct_line_rendering_data_to_find_wrapped_lines() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_overlay(&WRAPPED_TEXT, &with_indicator(&wrapping_options(), true), |context| {
        // O espião não existe em Rust (ver as notas de tradução): o resultado vem só da consulta de linha quebrada.
        assert_eq!(context.render(RenderOptions::default()), wrapped_indicators());
    });
}

#[test]
fn renders_nothing_when_the_indicator_is_disabled() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_indicators(&WRAPPED_TEXT, &with_indicator(&wrapping_options(), false), RenderOptions::default()),
        vec![None; 6]
    );
}

#[test]
fn renders_nothing_when_the_setting_is_left_at_its_default() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(render_indicators(&WRAPPED_TEXT, &wrapping_options(), RenderOptions::default()), vec![None; 6]);
}

#[test]
fn renders_nothing_when_word_wrap_is_off() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_indicators(
            &WRAPPED_TEXT,
            &object(&[("wordWrap", Dynamic::from("off")), ("wordWrapIndicator", Dynamic::from(true))]),
            RenderOptions::default()
        ),
        vec![None; 3]
    );
}

#[test]
fn renders_nothing_for_the_last_view_line_of_the_model() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_indicators(&["aaa"], &with_indicator(&wrapping_options(), true), RenderOptions::default()),
        vec![None]
    );
}

fn column_wrapping_options(column: i32) -> Dynamic {
    merge(
        &viewport_options(),
        &[
            ("wordWrap", Dynamic::from("wordWrapColumn")),
            ("wordWrapColumn", Dynamic::from(column)),
            ("wordWrapIndicator", Dynamic::from(true)),
        ],
    )
}

#[test]
fn positions_the_indicator_at_the_wrapping_column() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_indicators(&["aaa bb cccc"], &column_wrapping_options(5), RenderOptions::default()),
        vec![default_indicator(50.0), default_indicator(50.0), None]
    );
}

#[test]
fn renders_an_indicator_when_a_word_is_broken_mid_token() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_indicators(&["aaaaaaaaaa"], &column_wrapping_options(5), RenderOptions::default()),
        vec![default_indicator(50.0), None]
    );
}

#[test]
fn keeps_the_indicator_at_the_wrapping_column_while_horizontally_scrolled() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_overlay(&WRAPPED_TEXT, &with_indicator(&wrapping_options(), true), |context| {
        let view_layout = context.view_model.view_layout();
        view_layout.set_max_line_width(200.0);
        view_layout.set_scroll_position(
            INewScrollPosition { scroll_left: Some(20.0), scroll_top: None },
            ScrollType::Immediate,
        );
        assert_eq!(context.render(RenderOptions::default()), wrapped_indicators());
    });
}

#[test]
fn keeps_the_indicator_past_a_fixed_wrapping_column_wider_than_the_viewport() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let options = column_wrapping_options(12);
    with_overlay(&["aaaaaaaaaaaaaaaaaaaaaaaa"], &options, |context| {
        assert_eq!(context.render(RenderOptions::default()), vec![default_indicator(120.0), None]);

        let view_layout = context.view_model.view_layout();
        view_layout.set_max_line_width(200.0);
        view_layout.set_scroll_position(
            INewScrollPosition { scroll_left: Some(50.0), scroll_top: None },
            ScrollType::Immediate,
        );
        assert_eq!(context.render(RenderOptions::default()), vec![default_indicator(120.0), None]);
    });
}

#[test]
fn renders_only_the_view_lines_inside_the_viewport() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // `prepareRender` só percorre o viewport, e `render` indexa o resultado relativo à primeira linha que a visão
    // está pintando, então tudo fora fica vazio.
    assert_eq!(
        render_indicators(
            &WRAPPED_TEXT,
            &with_indicator(&wrapping_options(), true),
            RenderOptions { viewport: Some((2, 4)), ..RenderOptions::default() }
        ),
        vec![None, default_indicator(INDICATOR_LEFT), None, None, None, None]
    );
}

#[test]
fn renders_nothing_before_prepare_render_has_run() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_indicators(
            &WRAPPED_TEXT,
            &with_indicator(&wrapping_options(), true),
            RenderOptions { skip_prepare_render: true, ..RenderOptions::default() }
        ),
        vec![None; 6]
    );
}

fn add_double_line_height(_view_model: &Rc<TestViewModel>, model: &Rc<TextModel>) {
    // `lineHeight` é um múltiplo da altura padrão, aplicado aqui à primeira linha inteira do modelo, ou seja, às
    // linhas da visão de 1 a 3.
    model.delta_decorations(
        &[],
        &[ModelDeltaDecoration {
            range: Range::new(1, 1, 1, model.get_line_max_column(1)),
            options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                description: "line-height".to_owned(),
                line_height: Some(2.0),
                ..Default::default()
            }),
        }],
        0,
    );
}

#[test]
fn sizes_the_indicator_to_the_height_of_its_view_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_indicators(
            &WRAPPED_TEXT,
            &with_indicator(&wrapping_options(), true),
            RenderOptions { prepare: Some(add_double_line_height), ..RenderOptions::default() }
        ),
        vec![
            indicator(INDICATOR_LEFT, 2.0 * LINE_HEIGHT),
            indicator(INDICATOR_LEFT, 2.0 * LINE_HEIGHT),
            None,
            None,
            default_indicator(INDICATOR_LEFT),
            None
        ]
    );
}

fn all_invalidations(value_of: impl Fn(&str) -> bool) -> Vec<(&'static str, bool)> {
    [
        "onDecorationsChanged",
        "onFlushed",
        "onLineMappingChanged",
        "onLinesChanged",
        "onLinesDeleted",
        "onLinesInserted",
        "onTokensChanged",
        "onZonesChanged",
        "onScrolledVertically",
        "onScrolledHorizontally",
    ]
    .into_iter()
    .map(|name| (name, value_of(name)))
    .collect()
}

#[test]
fn asks_for_a_rerender_on_every_event_that_can_affect_an_indicator() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        invalidations_after_render(&WRAPPED_TEXT, &with_indicator(&wrapping_options(), true)),
        all_invalidations(|name| !matches!(name, "onTokensChanged" | "onScrolledHorizontally"))
    );
}

#[test]
fn asks_for_no_rerender_at_all_while_it_renders_nothing() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let all_false = all_invalidations(|_| false);
    assert_eq!(
        (
            invalidations_after_render(&WRAPPED_TEXT, &with_indicator(&wrapping_options(), false)),
            invalidations_after_render(
                &WRAPPED_TEXT,
                &object(&[("wordWrap", Dynamic::from("off")), ("wordWrapIndicator", Dynamic::from(true))])
            )
        ),
        (all_false.clone(), all_false)
    );
}

#[test]
fn ignores_token_changes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut invalidated = None;
    with_overlay(&WRAPPED_TEXT, &with_indicator(&wrapping_options(), true), |context| {
        context.render(RenderOptions::default());
        invalidated = Some(context.overlay.on_tokens_changed(&ViewTokensChangedEvent::new(vec![ViewTokensChangedRange {
            from_line_number: 1,
            to_line_number: 6,
        }])));
    });

    assert_eq!(invalidated, Some(false));
}

#[test]
fn renders_after_being_re_enabled_on_the_same_overlay() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_overlay(&WRAPPED_TEXT, &with_indicator(&wrapping_options(), false), |context| {
        let update_and_render = |options: &Dynamic| {
            context.overlay.on_did_render();
            context.configuration.update_options(options);
            (context.overlay.should_render(), context.render(RenderOptions::default()))
        };
        let indicator_option = |enabled: bool| object(&[("wordWrapIndicator", Dynamic::from(enabled))]);

        let initially_disabled = context.render(RenderOptions::default());
        let enabled = update_and_render(&indicator_option(true));
        let disabled = update_and_render(&indicator_option(false));
        let re_enabled = update_and_render(&indicator_option(true));

        assert_eq!(
            (initially_disabled, enabled, disabled, re_enabled),
            (
                vec![None; 6],
                (true, wrapped_indicators()),
                (true, vec![None; 6]),
                (true, wrapped_indicators())
            )
        );
    });
}

#[test]
fn asks_for_a_rerender_only_on_configuration_changes_that_matter() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let options = merge(
        &with_indicator(&wrapping_options(), true),
        &[("lineNumbers", Dynamic::from("on"))],
    );
    let indicator_option = |enabled: bool| object(&[("wordWrapIndicator", Dynamic::from(enabled))]);
    let word_wrap_off = object(&[("wordWrap", Dynamic::from("off"))]);

    let indicator_turned_on =
        configuration_change_invalidates(&with_indicator(&wrapping_options(), false), &indicator_option(true));
    let indicator_turned_off = configuration_change_invalidates(&options, &indicator_option(false));
    let wrapping_turned_on = configuration_change_invalidates(
        &object(&[("wordWrap", Dynamic::from("off")), ("wordWrapIndicator", Dynamic::from(true))]),
        &wrapping_options(),
    );
    let wrapping_turned_off = configuration_change_invalidates(&options, &word_wrap_off);
    let indicator_turned_off_and_on =
        configuration_changes_invalidate(&options, &[indicator_option(false), indicator_option(true)]);
    let wrapping_turned_off_and_on = configuration_changes_invalidate(&options, &[word_wrap_off, wrapping_options()]);
    let wrapping_column_changed =
        configuration_change_invalidates(&options, &object(&[("wordWrapColumn", Dynamic::from(7))]));
    let layout_changed = configuration_change_invalidates(&options, &object(&[("lineNumbers", Dynamic::from("off"))]));
    let unrelated_change =
        configuration_change_invalidates(&options, &object(&[("cursorBlinking", Dynamic::from("solid"))]));

    assert_eq!(
        (
            indicator_turned_on,
            indicator_turned_off,
            wrapping_turned_on,
            wrapping_turned_off,
            indicator_turned_off_and_on,
            wrapping_turned_off_and_on,
            wrapping_column_changed,
            layout_changed,
            unrelated_change
        ),
        (true, true, true, true, vec![true, true], vec![true, true], true, false, false)
    );
}
