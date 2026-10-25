//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/utils/utils.ts`: medidas de linhas e
//! posições no editor observável, o recuo comum de um bloco, o retângulo válido para sobreposições, o caminho
//! (`PathBuilder`/`createRectangle`) das molduras das views e auxiliares de observáveis.
//!
//! Notas de tradução:
//! - o texto é UTF-16 ([`JsString`]); `Rect`, `Point` e `Size2D` são os de `core_2d`;
//! - `PathBuilder.build()` do original devolve o texto de um atributo `d` de SVG. A UI é nativa: [`PathBuilder::build`]
//!   devolve os comandos ([`PathCommand`]) que o pintor lê, e o `Display` de cada comando é o trecho de texto que o
//!   original anexaria (`M x y `, `L x y `, `Q cx cy x y `, `C c1x c1y c2x c2y x y `), para quem precisa da forma
//!   textual. `createRectangle` devolve os comandos;
//! - `padding: number | {top,...}` e `borderRadius: number | {topLeft,...}` de `createRectangle` são [`Padding`] e
//!   [`BorderRadius`], ambos com `From<f64>` (o valor único); `options` é [`RectangleOptions`];
//! - `classNames(...classes: (string | false | undefined | null)[])` recebe `&[Option<&str>]` (`false`, `undefined` e `null`
//!   são `None`);
//! - `mapOutFalsy` trata só `None` como falso (o `Option` é o `undefined`/`null`); valores `Some` sempre contam como
//!   verdadeiros, inclusive `Some(false)` e `Some(0)`;
//! - `getPrefixTrim`: o `Math.min(...)` sem argumentos (`Infinity`) é `i32::MAX` em `prefix_trim`;
//! - `indentOfLine` (de `textModel.ts`) é o [`indent_of_line`] local: conta os espaços e tabs do início da linha;
//! - `UniqueUriGenerator` usa os milissegundos de [`date_now`] onde o original usa `new Date().toString()`; o contador
//!   estático garante a unicidade nos dois;
//! - `maxContentWidthInRange` mantém o `console.log('unexpected width')` do original como `eprintln!`.
//!
//! Divergências (fora do escopo, por nome de membro do original):
//! - `StatusBarViewItem` (uma subclasse de `MenuEntryActionViewItem` que monta o rótulo com o `KeybindingLabel` num
//!   elemento DOM e desliga a dica): o `MenuEntryActionViewItem` ainda não tem porte. Entra junto do hints widget, que é
//!   quem o usa; o dado que ele leva (o rótulo da ação mais o atalho resolvido, sem dica) é o que o pintor nativo lê;
//! - `observeElementPosition(element, store)` mede um elemento DOM com `ResizeObserver`. O equivalente é o
//!   [`ElementPosition`]: o dono do nó nativo avisa a posição nova (`update`) quando o nó muda de lugar ou de tamanho,
//!   que é o que o callback do `ResizeObserver` fazia, e os observáveis `top` e `left` seguem na mesma transação;
//! - `observeEditorBoundingClientRect(editor, store)` lê `getContainerDomNode().getBoundingClientRect()`: o contêiner
//!   nativo é quem sabe o retângulo, então ele chega como a função `get_bounding_client_rect`, lida a cada
//!   `onDidLayoutChange`, como no original;
//! - `FirstFnArg<T>` é só um tipo auxiliar do TypeScript.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use je_base::JsString;
use je_base::char_code as cc;
use je_base::errors::{self, Error};
use je_base::lifecycle::DisposableStore;
use je_base::observable::base::{AnyObservable, IObservable, IReader, Observable};
use je_base::observable::debug_name::DebugNameSource;
use je_base::observable::deps::StrictEquals;
use je_base::observable::observables::derived::{DerivedOptions, derived_opts};
use je_base::observable::observables::observable_signal_from_event::observable_signal_from_event;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{SettableExt, derived_observable_with_cache, observable_value, transaction};
use je_base::runtime::date_now;
use je_base::strings::split_lines;
use je_base::uri::{URI, UriComponents};

use crate::code_editor_widget::CodeEditorWidget;
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::core_2d::point::Point;
use crate::core_2d::rect::Rect;
use crate::core_2d::size::Size2D;
use crate::diff::range_mapping::RangeMapping;
use crate::edits::text_edit::{TextEdit, TextReplacement};
use crate::line_range::LineRange;
use crate::observable_code_editor::{ObservableCodeEditor, observable_code_editor};
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `derivedOpts({ name }, fn)`: um derivado com o nome de depuração dado.
fn derived_named<T: Clone + StrictEquals + 'static>(name: &str, compute: impl Fn(&dyn IReader) -> T + 'static) -> IObservable<T> {
    derived_opts(DerivedOptions { debug_name: Some(DebugNameSource::from(name)), ..DerivedOptions::default() }, compute)
}

/// `indentOfLine(line)` (`textModel.ts`): quantos espaços e tabs há no início da linha.
fn indent_of_line(line: &JsString) -> i32 {
    line.units().iter().take_while(|&&unit| unit == cc::Space || unit == cc::Tab).count() as i32
}

/// Warning: might return 0.
pub fn max_content_width_in_range(editor: &ObservableCodeEditor, range: LineRange, reader: Option<&dyn IReader>) -> f64 {
    let Some(model) = editor.model.read_opt(reader) else {
        return 0.0;
    };
    let mut max_content_width = 0.0_f64;

    for i in range.start_line_number..range.end_line_number_exclusive {
        let line_content_width = editor.get_width_of_line(i, reader);
        max_content_width = max_content_width.max(line_content_width);
    }
    let lines = range.map_to_line_array(|l| model.get_line_content(l));

    if max_content_width < 5.0 && lines.iter().any(|l| !l.is_empty()) && model.uri().scheme() != "file" {
        eprintln!("unexpected width");
    }
    max_content_width
}

/// `getContentSizeOfLines(editor, range, reader)`: a largura e a altura de cada linha do intervalo.
pub fn get_content_size_of_lines(editor: &ObservableCodeEditor, range: LineRange, reader: Option<&dyn IReader>) -> Vec<Size2D> {
    observable_signal_from_event("onDidChangeLineHeight", editor.editor.on_did_change_line_height()).read_opt(reader);

    let Some(model) = editor.model.read_opt(reader) else {
        errors::throw(Error::bug_indicating(Some("Model is required")));
    };

    let mut sizes: Vec<Size2D> = Vec::new();

    for i in range.start_line_number..range.end_line_number_exclusive {
        let mut line_content_width = editor.get_width_of_line(i, reader);
        if line_content_width == -1.0 {
            // approximation
            let column = model.get_line_length(i) as i32 + 1;
            let typical_halfwidth_character_width = editor.editor.get_option(FONT_INFO).typical_halfwidth_character_width;
            let approximation = f64::from(column) * typical_halfwidth_character_width;
            line_content_width = approximation;
        }

        let height = editor.editor.get_line_height_for_position(Position::new(i, 1));
        sizes.push(Size2D::new(line_content_width, height));
    }

    sizes
}

/// `getOffsetForPos(editor, pos, reader)`.
pub fn get_offset_for_pos(editor: &ObservableCodeEditor, pos: Position, reader: &dyn IReader) -> f64 {
    editor.layout_info.read(reader);
    editor.value.read(reader);

    if editor.model.read(reader).is_none() {
        return 0.0;
    }

    editor.scroll_top.read(reader);

    editor.editor.get_offset_for_column(pos.line_number, pos.column)
}

/// O resultado de `getPrefixTrim`: `{ prefixTrim, prefixLeftOffset }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrefixTrim {
    pub prefix_trim: i32,
    pub prefix_left_offset: f64,
}

/// `getPrefixTrim(diffRanges, originalLinesRange, modifiedLines, editor, reader)`.
pub fn get_prefix_trim(
    diff_ranges: &[Range],
    original_lines_range: LineRange,
    modified_lines: &[JsString],
    editor: &Rc<CodeEditorWidget>,
    reader: Option<&dyn IReader>,
) -> PrefixTrim {
    let Some(text_model) = editor.get_model() else {
        return PrefixTrim { prefix_trim: 0, prefix_left_offset: 0.0 };
    };

    let replacement_start = diff_ranges.iter().map(|r| if r.is_single_line() { r.start_column - 1 } else { 0 });
    let original_indents = original_lines_range.map_to_line_array(|line| indent_of_line(&text_model.get_line_content(line)));
    let modified_indents = modified_lines.iter().filter(|line| !line.is_empty()).map(indent_of_line);
    let prefix_trim = replacement_start.chain(original_indents).chain(modified_indents).min().unwrap_or(i32::MAX);

    let prefix_left_offset;
    let start_line_indent = text_model.get_line_indent_column(original_lines_range.start_line_number);
    if start_line_indent >= prefix_trim.saturating_add(1) {
        // We can use the editor to get the offset
        // go through other usages of getOffsetForColumn and come up with a robust reactive solution to read it
        // getOffsetForColumn requires the line number to be visible. This might change on scroll top.
        observable_code_editor(editor).scroll_top.read_opt(reader);
        prefix_left_offset = editor.get_offset_for_column(original_lines_range.start_line_number, prefix_trim + 1);
    } else if let Some(first_modified_line) = modified_lines.first() {
        // Content is not in the editor, we can use the content width to calculate the offset
        let prefix_length = usize::try_from(prefix_trim).unwrap_or(0).min(first_modified_line.len());
        prefix_left_offset = get_content_render_width(&JsString::from(&first_modified_line.units()[..prefix_length]), editor, &text_model);
    } else {
        // unable to approximate the offset
        return PrefixTrim { prefix_trim: 0, prefix_left_offset: 0.0 };
    }

    PrefixTrim { prefix_trim, prefix_left_offset }
}

/// `getContentRenderWidth(content, editor, textModel)`.
pub fn get_content_render_width(content: &JsString, editor: &Rc<CodeEditorWidget>, text_model: &Rc<TextModel>) -> f64 {
    let w = editor.get_option(FONT_INFO).typical_halfwidth_character_width;
    let tab_size = f64::from(text_model.get_options().tab_size as i32) * w;

    let num_tabs = content.units().iter().filter(|&&unit| unit == cc::Tab).count();
    let num_none_tabs = content.len() - num_tabs;
    num_none_tabs as f64 * w + num_tabs as f64 * tab_size
}

/// `getEditorValidOverlayRect(editor)`: a área do editor onde uma sobreposição é válida (sem a margem, sem o minimapa
/// nem a barra de rolagem).
pub fn get_editor_valid_overlay_rect(editor: &ObservableCodeEditor) -> IObservable<Rect> {
    let content_left = Rc::clone(&editor.layout_info_content_left);

    let width = {
        let (layout_info_minimap, layout_info_width, layout_info_vertical_scrollbar_width, content_left) = (
            Rc::clone(&editor.layout_info_minimap),
            Rc::clone(&editor.layout_info_width),
            Rc::clone(&editor.layout_info_vertical_scrollbar_width),
            Rc::clone(&content_left),
        );
        derived_named("editor.validOverlay.width", move |r| {
            let minimap = layout_info_minimap.read(r);
            let has_minimap_on_the_right = minimap.minimap_left != 0.0;
            let editor_width = (layout_info_width.read(r) - content_left.read(r)).max(0.0);

            if has_minimap_on_the_right {
                let minimap_and_scrollbar_width = minimap.minimap_width + layout_info_vertical_scrollbar_width.read(r);
                return (editor_width - minimap_and_scrollbar_width).max(0.0);
            }

            editor_width
        })
    };

    let height = {
        let (layout_info_height, content_height) = (Rc::clone(&editor.layout_info_height), Rc::clone(&editor.content_height));
        derived_named("editor.validOverlay.height", move |r| layout_info_height.read(r) + content_height.read(r))
    };

    derived_named("editor.validOverlay", move |r| Rect::from_left_top_width_height(content_left.read(r), 0.0, width.read(r), height.read(r)))
}

thread_local! {
    /// `UniqueUriGenerator._modelId`.
    static MODEL_ID: Cell<u32> = const { Cell::new(0) };
}

/// `UniqueUriGenerator`.
pub struct UniqueUriGenerator {
    pub scheme: String,
}

impl UniqueUriGenerator {
    pub fn new(scheme: impl Into<String>) -> Self {
        Self { scheme: scheme.into() }
    }

    pub fn get_unique_uri(&self) -> URI {
        let model_id = MODEL_ID.with(|counter| {
            let current = counter.get();
            counter.set(current + 1);
            current
        });
        URI::from(
            &UriComponents {
                scheme: Some(JsString::from(self.scheme.as_str())),
                authority: None,
                path: Some(JsString::from(format!("{}{}", date_now(), model_id).as_str())),
                query: None,
                fragment: None,
            },
            false,
        )
    }
}

/// `applyEditToModifiedRangeMappings(rangeMapping, edit)`: leva a edição para o lado modificado dos mapeamentos.
pub fn apply_edit_to_modified_range_mappings(range_mapping: &[RangeMapping], edit: &TextEdit) -> Vec<RangeMapping> {
    range_mapping.iter().map(|m| RangeMapping::new(m.original_range, edit.map_range(&m.modified_range))).collect()
}

/// `classNames(...classes)`.
pub fn class_names(classes: &[Option<&str>]) -> String {
    classes.iter().flatten().copied().collect::<Vec<&str>>().join(" ")
}

/// `offsetRangeToRange(columnOffsetRange, startPos)`.
fn offset_range_to_range(column_offset_range: OffsetRange, start_pos: Position) -> Range {
    Range::new(
        start_pos.line_number,
        start_pos.column + column_offset_range.start,
        start_pos.line_number,
        start_pos.column + column_offset_range.end_exclusive,
    )
}

/// Calculates the indentation size (in spaces) of a given line,
/// interpreting tabs as the specified tab size.
fn get_indentation_size(line: &JsString, tab_size: i32) -> i32 {
    let mut current_size = 0;
    for &unit in line.units() {
        if unit == cc::Tab {
            current_size += tab_size;
        } else if unit == cc::Space {
            current_size += 1;
        } else {
            break;
        }
    }
    // if currentSize % tabSize !== 0,
    // then there are spaces which are not part of the indentation
    current_size - (current_size % tab_size)
}

/// Calculates the number of characters at the start of a line that correspond to a given indentation size,
/// taking into account both tabs and spaces.
fn indent_size_to_indent_length(line: &JsString, indent_size: i32, tab_size: i32) -> i32 {
    let mut remaining_size = indent_size - (indent_size % tab_size);
    let mut i = 0;
    for &unit in line.units() {
        if remaining_size == 0 {
            break;
        }
        if unit == cc::Tab {
            remaining_size -= tab_size;
        } else if unit == cc::Space {
            remaining_size -= 1;
        } else {
            errors::throw(Error::bug_indicating(Some("Unexpected character found while calculating indent length")));
        }
        i += 1;
    }
    i
}

/// `createReindentEdit(text, range, tabSize)`: a edição que tira das linhas do intervalo o recuo comum a todas.
pub fn create_reindent_edit(text: &JsString, range: LineRange, tab_size: i32) -> TextEdit {
    let new_lines: Vec<JsString> = split_lines(text.units()).into_iter().map(JsString::from).collect();
    let mut edits: Vec<TextReplacement> = Vec::new();
    let min_indent_size = range
        .map_to_line_array(|l| get_indentation_size(&new_lines[(l - 1) as usize], tab_size))
        .into_iter()
        .min()
        .unwrap_or(0);
    range.for_each(|line_number| {
        let indent_length = indent_size_to_indent_length(&new_lines[(line_number - 1) as usize], min_indent_size, tab_size);
        edits.push(TextReplacement::new(offset_range_to_range(OffsetRange::new(0, indent_length), Position::new(line_number, 1)), ""));
    });
    TextEdit::new(edits)
}

/// Um comando do caminho: o que o `PathBuilder` do original anexava ao texto do atributo `d` do SVG.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCommand {
    /// `M x y`.
    MoveTo(Point),
    /// `L x y`.
    LineTo(Point),
    /// `Q cx cy x y`: curva quadrática com o ponto de controle e o destino.
    CurveTo { control: Point, to: Point },
    /// `C c1x c1y c2x c2y x y`: curva cúbica com os dois pontos de controle e o destino.
    CurveTo2 { control1: Point, control2: Point, to: Point },
}

impl fmt::Display for PathCommand {
    /// O trecho de texto do original, com o espaço no fim.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathCommand::MoveTo(point) => write!(f, "M {} {} ", point.x, point.y),
            PathCommand::LineTo(point) => write!(f, "L {} {} ", point.x, point.y),
            PathCommand::CurveTo { control, to } => write!(f, "Q {} {} {} {} ", control.x, control.y, to.x, to.y),
            PathCommand::CurveTo2 { control1, control2, to } => {
                write!(f, "C {} {} {} {} {} {} ", control1.x, control1.y, control2.x, control2.y, to.x, to.y)
            }
        }
    }
}

/// `PathBuilder`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathBuilder {
    data: Vec<PathCommand>,
}

impl PathBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(&mut self, point: Point) -> &mut Self {
        self.data.push(PathCommand::MoveTo(point));
        self
    }

    pub fn line_to(&mut self, point: Point) -> &mut Self {
        self.data.push(PathCommand::LineTo(point));
        self
    }

    pub fn curve_to(&mut self, cp: Point, to: Point) -> &mut Self {
        self.data.push(PathCommand::CurveTo { control: cp, to });
        self
    }

    pub fn curve_to2(&mut self, cp1: Point, cp2: Point, to: Point) -> &mut Self {
        self.data.push(PathCommand::CurveTo2 { control1: cp1, control2: cp2, to });
        self
    }

    /// `build()`: os comandos na ordem em que foram anexados.
    pub fn build(&self) -> Vec<PathCommand> {
        self.data.clone()
    }
}

/// `layout` de `createRectangle`: `{ topLeft, width, height }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectangleLayout {
    pub top_left: Point,
    pub width: f64,
    pub height: f64,
}

/// `padding` de `createRectangle`: `number | { top, right, bottom, left }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Padding {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl From<f64> for Padding {
    fn from(padding: f64) -> Self {
        Padding { top: padding, right: padding, bottom: padding, left: padding }
    }
}

/// `borderRadius` de `createRectangle`: `number | { topLeft, topRight, bottomLeft, bottomRight }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderRadius {
    pub top_left: f64,
    pub top_right: f64,
    pub bottom_left: f64,
    pub bottom_right: f64,
}

impl From<f64> for BorderRadius {
    fn from(border_radius: f64) -> Self {
        BorderRadius { top_left: border_radius, top_right: border_radius, bottom_left: border_radius, bottom_right: border_radius }
    }
}

/// `options` de `createRectangle`: `{ hideLeft?, hideRight?, hideTop?, hideBottom? }`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RectangleOptions {
    pub hide_left: bool,
    pub hide_right: bool,
    pub hide_top: bool,
    pub hide_bottom: bool,
}

/// `createRectangle(layout, padding, borderRadius, options)`: o contorno de um retângulo de cantos arredondados, com os
/// lados escondidos que `options` pedir. Arguments are a bit messy currently, could be improved.
pub fn create_rectangle(
    layout: RectangleLayout,
    padding: impl Into<Padding>,
    border_radius: impl Into<BorderRadius>,
    options: RectangleOptions,
) -> Vec<PathCommand> {
    let top_left_inner = layout.top_left;
    let top_right_inner = top_left_inner.delta_x(layout.width);
    let bottom_left_inner = top_left_inner.delta_y(layout.height);
    let bottom_right_inner = bottom_left_inner.delta_x(layout.width);

    // padding
    let Padding { top: padding_top, right: padding_right, bottom: padding_bottom, left: padding_left } = padding.into();

    // corner radius
    let BorderRadius { top_left: radius_tl, top_right: radius_tr, bottom_left: radius_bl, bottom_right: radius_br } = border_radius.into();

    let total_height = layout.height + padding_top + padding_bottom;
    let total_width = layout.width + padding_left + padding_right;

    // The path is drawn from bottom left at the end of the rounded corner in a clockwise direction
    // Before: before the rounded corner
    // After: after the rounded corner
    let top_left = top_left_inner.delta_x(-padding_left).delta_y(-padding_top);
    let top_right = top_right_inner.delta_x(padding_right).delta_y(-padding_top);
    let top_left_before = top_left.delta_y(radius_tl.min(total_height / 2.0));
    let top_left_after = top_left.delta_x(radius_tl.min(total_width / 2.0));
    let top_right_before = top_right.delta_x(-radius_tr.min(total_width / 2.0));
    let top_right_after = top_right.delta_y(radius_tr.min(total_height / 2.0));

    let bottom_left = bottom_left_inner.delta_x(-padding_left).delta_y(padding_bottom);
    let bottom_right = bottom_right_inner.delta_x(padding_right).delta_y(padding_bottom);
    let bottom_left_before = bottom_left.delta_x(radius_bl.min(total_width / 2.0));
    let bottom_left_after = bottom_left.delta_y(-radius_bl.min(total_height / 2.0));
    let bottom_right_before = bottom_right.delta_y(-radius_br.min(total_height / 2.0));
    let bottom_right_after = bottom_right.delta_x(-radius_br.min(total_width / 2.0));

    let mut path = PathBuilder::new();

    if !options.hide_left {
        path.move_to(bottom_left_after).line_to(top_left_before);
    }

    if !options.hide_left && !options.hide_top {
        path.curve_to(top_left, top_left_after);
    } else {
        path.move_to(top_left_after);
    }

    if !options.hide_top {
        path.line_to(top_right_before);
    }

    if !options.hide_top && !options.hide_right {
        path.curve_to(top_right, top_right_after);
    } else {
        path.move_to(top_right_after);
    }

    if !options.hide_right {
        path.line_to(bottom_right_before);
    }

    if !options.hide_right && !options.hide_bottom {
        path.curve_to(bottom_right, bottom_right_after);
    } else {
        path.move_to(bottom_right_after);
    }

    if !options.hide_bottom {
        path.line_to(bottom_left_before);
    }

    if !options.hide_bottom && !options.hide_left {
        path.curve_to(bottom_left, bottom_left_after);
    } else {
        path.move_to(bottom_left_after);
    }

    path.build()
}

/// `mapOutFalsy(obs)`: um observável do valor que só é `Some` enquanto o original é `Some`, e que guarda o último valor
/// verdadeiro (o `obs.read(reader) || lastValue`). O observável interno é sempre o mesmo, então quem o lê não recalcula
/// enquanto o valor externo continua `Some`.
pub fn map_out_falsy<T: Clone + StrictEquals + 'static>(obs: &IObservable<Option<T>>) -> IObservable<Option<IObservable<T>>> {
    let non_undefined_obs: IObservable<Option<T>> = {
        let obs = Rc::clone(obs);
        derived_observable_with_cache(None, move |reader, last_value: Option<&Option<T>>| {
            obs.read(reader).or_else(|| last_value.and_then(Clone::clone))
        })
    };
    let stable_obs: IObservable<T> = {
        let non_undefined_obs = Rc::clone(&non_undefined_obs);
        derived_opts(DerivedOptions::default(), move |reader| non_undefined_obs.read(reader).expect("só é lido enquanto o valor externo é verdadeiro"))
    };

    let obs = Rc::clone(obs);
    let name_source = Rc::clone(&obs);
    derived_opts(
        DerivedOptions {
            debug_name: Some(DebugNameSource::Lazy(Rc::new(move || Some(format!("{}.mapOutFalsy", name_source.debug_name()))))),
            ..DerivedOptions::default()
        },
        move |reader| {
            non_undefined_obs.read(reader);
            obs.read(reader).map(|_| Rc::clone(&stable_obs))
        },
    )
}

/// `observeElementPosition(element, store)`: a posição de um elemento na página, como observáveis que o dono do nó
/// nativo atualiza (ver as divergências do módulo).
pub struct ElementPosition {
    pub top: IObservable<f64>,
    pub left: IObservable<f64>,
    top_value: Rc<ObservableValue<f64, ()>>,
    left_value: Rc<ObservableValue<f64, ()>>,
}

impl ElementPosition {
    /// Começa na posição lida do elemento.
    pub fn new(top: f64, left: f64) -> Self {
        let top_value = observable_value::<f64, ()>("top", top);
        let left_value = observable_value::<f64, ()>("left", left);
        Self { top: top_value.to_observable(), left: left_value.to_observable(), top_value, left_value }
    }

    /// O que o callback do `ResizeObserver` fazia: atualiza `top` e `left` na mesma transação.
    pub fn update(&self, top: f64, left: f64) {
        transaction(|tx| {
            self.top_value.set(top, Some(tx));
            self.left_value.set(left, Some(tx));
        });
    }
}

/// `rectToProps(fn)`: os quatro lados do retângulo como observáveis (`None` quando a função devolve `None`).
pub struct RectProps {
    pub left: IObservable<Option<f64>>,
    pub top: IObservable<Option<f64>>,
    pub width: IObservable<Option<f64>>,
    pub height: IObservable<Option<f64>>,
}

/// `rectToProps(fn)`.
pub fn rect_to_props(compute_rect: impl Fn(&dyn IReader) -> Option<Rect> + 'static) -> RectProps {
    let compute_rect: Rc<dyn Fn(&dyn IReader) -> Option<Rect>> = Rc::new(compute_rect);
    let prop = |name: &str, get: fn(&Rect) -> f64| {
        let compute_rect = Rc::clone(&compute_rect);
        derived_named(name, move |reader| compute_rect(reader).map(|rect| get(&rect)))
    };
    RectProps {
        left: prop("editor.validOverlay.left", |rect| rect.left),
        top: prop("editor.validOverlay.top", |rect| rect.top),
        width: prop("editor.validOverlay.width", |rect| rect.width()),
        height: prop("editor.validOverlay.height", |rect| rect.height()),
    }
}

/// `observeEditorBoundingClientRect(editor, store)`: o retângulo do contêiner do editor, relido a cada
/// `onDidLayoutChange`. O `DOMRect` do original é um objeto novo a cada leitura, então o observável avisa a cada leitura.
pub fn observe_editor_bounding_client_rect(
    editor: &Rc<CodeEditorWidget>,
    get_bounding_client_rect: impl Fn() -> Rect + 'static,
    store: &DisposableStore,
) -> IObservable<Rc<Rect>> {
    let dom_rect = observable_value::<Rc<Rect>, ()>("domRect", Rc::new(get_bounding_client_rect()));
    let target = Rc::clone(&dom_rect);
    store.add(editor.on_did_layout_change().listen(move |_| {
        target.set(Rc::new(get_bounding_client_rect()), None);
    }));
    dom_rect.to_observable()
}
