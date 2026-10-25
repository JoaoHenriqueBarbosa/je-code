//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellDragRenderer.ts`: `CodeCellDragImageRenderer`, que monta a
//! imagem mostrada ao arrastar uma célula (a própria célula, com o texto do editor já colorido pelos tokens), e o
//! `EditorTextRenderer` que ela usa para montar esse texto.
//!
//! Divergências:
//! - **HTML vira árvore de nós.** O `EditorTextRenderer.getRichText` devolve um `div` com `innerHTML`; aqui devolve um nó solto da
//!   árvore do `je-ui` com um filho por linha e um trecho de texto por token (`tokenizeLineToHTML`, de `textToHtmlTokenizer.ts`, sem
//!   porte, vira [`tokenize_line_to_runs`], que faz a mesma contagem de largura, tabulações e substituição de caracteres, devolvendo
//!   trechos em vez de `<span>`). O escape de `<`, `>` e `&` não existe (não há HTML), e as entidades `&#00;`, `&#8203` e `&#160;`
//!   viram os caracteres que elas produzem. O `createTrustedTypesPolicy` não tem equivalente.
//! - **Estilo do `div`.** O `style` com `color`, `background-color`, `font-size`, `font-weight`, `line-height` e `white-space: pre` é a
//!   cor de fundo do nó, o [`InlineStyle`] dos trechos e a altura de linha. As variáveis `--notebook-editor-font-family` e companhia
//!   não têm equivalente: o texto usa a fonte da interface (`ui_font`), porque o `InlineStyle` do `je-widgets` só tem tamanho e peso;
//!   a família do editor (`fontInfo.fontFamily`) não é aplicada.
//! - `getInlineStyle(tokenIndex, colorMap)` é a cor de primeiro plano do token (índice no mapa de cores) mais o negrito e o itálico do
//!   [`TokenPresentation`]; sublinhado e tachado não são desenhados.
//! - **`cloneNode(true)`** é [`clone_node`], uma cópia profunda da subárvore (papel, geometria, estilo, rótulo e classes). Os atributos
//!   e as dimensões relativas (`set_fill`, `set_right`, `set_bottom`) da árvore não são legíveis e não são copiados.
//! - **`querySelector('.cell-editor-container')`** é a busca em profundidade do primeiro descendente com a classe.
//! - `BaseCellRenderTemplate` (de `notebookRenderingCommon.ts`, que entra com o `cellRenderer.ts`) é o contrato estreito
//!   [`ICellDragImageTemplate`], só com `container`; `ICodeEditor` é [`ICellDragImageEditor`], só com o modelo, o `fontInfo` e o
//!   tamanho de tabulação. `TokenizationRegistry.getColorMap()` é o registro global do `je-editor`.
//! - `document.createElement('div')` com `textContent = '1 cell'` é um nó solto com o texto; o estilo do texto
//!   (`label_styles`) chega no construtor, de quem tem o tema. `type: 'code' | 'markdown'` é [`CellDragImageType`].

use std::rc::Rc;

use je_base::js_string::JsString;
use je_base::platform::Platform;
use je_base::strings::is_full_width_character;
use je_editor::config::font_info::FontInfo;
use je_editor::encoded_token_attributes::{TokenPresentation, color_id};
use je_editor::line_tokens::{IViewLineTokens, LineTokens};
use je_editor::range::Range;
use je_editor::tokenization_registry::tokenization_registry;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Tree, Ui};
use je_widgets::dom::{clear_node, create_detached_el, create_el};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::icon_labels::{InlineStyle, LabelPart, build_parts, layout_parts};
use je_widgets::text::{WORKBENCH_LINE_HEIGHT, set_node_text, ui_font};

/// `'code' | 'markdown'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellDragImageType {
    Code,
    Markdown,
}

impl CellDragImageType {
    fn as_str(self) -> &'static str {
        match self {
            CellDragImageType::Code => "code",
            CellDragImageType::Markdown => "markdown",
        }
    }
}

/// O modelo de texto do editor, só com o que `getRichText` lê.
pub trait IDragImageTextModel {
    /// `model.getOptions().tabSize`.
    fn tab_size(&self) -> i32;
    /// `model.tokenization.getLineTokens(lineNumber)`.
    fn get_line_tokens(&self, line_number: i32) -> LineTokens;
}

/// O que `CodeCellDragImageRenderer` e o `EditorTextRenderer` chamam do `ICodeEditor`.
pub trait ICellDragImageEditor {
    /// `editor.getModel()`.
    fn model(&self) -> Option<Rc<dyn IDragImageTextModel>>;
    /// `editor.getOptions().get(EditorOption.fontInfo)`.
    fn font_info(&self) -> Rc<FontInfo>;
}

/// O que `CodeCellDragImageRenderer` lê do `BaseCellRenderTemplate`.
pub trait ICellDragImageTemplate {
    /// `container`.
    fn container(&self) -> NodeId;
}

/// Um trecho de texto com a apresentação do token de onde veio.
struct RichTextRun {
    text: String,
    presentation: TokenPresentation,
}

/// `tokenizeLineToHTML(text, viewLineTokens, colorMap, startOffset, endOffset, tabSize, useNbsp)`, em trechos de texto em vez de HTML.
fn tokenize_line_to_runs(text: &JsString, view_line_tokens: &dyn IViewLineTokens, start_offset: i32, end_offset: i32, tab_size: i32, use_nbsp: bool) -> Vec<RichTextRun> {
    const TAB: u16 = 9;
    const CARRIAGE_RETURN: u16 = 13;
    const SPACE: u16 = 32;
    const NULL: u16 = 0;
    const UTF8_BOM: u16 = 0xFEFF;
    const LINE_SEPARATOR: u16 = 0x2028;
    const PARAGRAPH_SEPARATOR: u16 = 0x2029;
    const NEXT_LINE: u16 = 0x0085;
    const NBSP: u16 = 0x00A0;
    const REPLACEMENT: u16 = 0xFFFD;
    const ZERO_WIDTH_SPACE: u16 = 0x200B;

    let units = text.units();
    let tab_size = tab_size.max(1);
    let mut runs = Vec::new();
    let mut char_index: i32 = 0;
    let mut width: i32 = 0;
    let mut prev_is_space = true;

    for token_index in 0..view_line_tokens.get_count() {
        let token_end_index = view_line_tokens.get_end_offset(token_index);
        let mut part_content: Vec<u16> = Vec::new();

        while char_index < token_end_index && char_index < end_offset {
            let char_code = units.get(char_index as usize).copied().unwrap_or(0);
            let is_tab = char_code == TAB;

            width += if is_full_width_character(char_code) {
                2
            } else if is_tab {
                0
            } else {
                1
            };

            if char_index < start_offset {
                if is_tab {
                    let remainder = width % tab_size;
                    width += if remainder == 0 { tab_size } else { tab_size - remainder };
                }
                char_index += 1;
                continue;
            }

            // Um espaço, ou um espaço que vira NBSP quando o anterior já era espaço (`useNbsp`).
            let push_space = |part_content: &mut Vec<u16>, prev_is_space: &mut bool| {
                if use_nbsp && *prev_is_space {
                    part_content.push(NBSP);
                    *prev_is_space = false;
                } else {
                    part_content.push(SPACE);
                    *prev_is_space = true;
                }
            };

            match char_code {
                TAB => {
                    let remainder = width % tab_size;
                    let insert_spaces_count = if remainder == 0 { tab_size } else { tab_size - remainder };
                    width += insert_spaces_count;
                    for _ in 0..insert_spaces_count {
                        push_space(&mut part_content, &mut prev_is_space);
                    }
                }
                NULL | UTF8_BOM | LINE_SEPARATOR | PARAGRAPH_SEPARATOR | NEXT_LINE => {
                    part_content.push(REPLACEMENT);
                    prev_is_space = false;
                }
                CARRIAGE_RETURN => {
                    // zero width space, because carriage return would introduce a line break
                    part_content.push(ZERO_WIDTH_SPACE);
                    prev_is_space = false;
                }
                SPACE => push_space(&mut part_content, &mut prev_is_space),
                _ => {
                    part_content.push(char_code);
                    prev_is_space = false;
                }
            }
            char_index += 1;
        }

        if token_end_index <= start_offset {
            continue;
        }

        let text = String::from_utf16_lossy(&part_content);
        if !text.is_empty() {
            runs.push(RichTextRun { text, presentation: view_line_tokens.get_presentation(token_index) });
        }

        if token_end_index > end_offset || char_index >= end_offset || start_offset >= end_offset {
            break;
        }
    }

    runs
}

/// `#rrggbb` ou `#rrggbbaa` (o que o `Color.Format.CSS.formatHex` produz) como [`Rgba`].
fn parse_hex_color(hex: &str) -> Option<Rgba> {
    let digits = hex.strip_prefix('#')?;
    let byte = |index: usize| digits.get(index * 2..index * 2 + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok());
    match digits.len() {
        6 => Some(Rgba::new(byte(0)?, byte(1)?, byte(2)?, 0xff)),
        8 => Some(Rgba::new(byte(0)?, byte(1)?, byte(2)?, byte(3)?)),
        _ => None,
    }
}

/// O `font-weight` do `FontInfo` (`'normal'`, `'bold'` ou um número) como peso numérico.
fn font_weight_value(font_weight: &str) -> u16 {
    match font_weight {
        "normal" => 400,
        "bold" => 700,
        other => other.parse().unwrap_or(400),
    }
}

/// `EditorTextRenderer`.
struct EditorTextRenderer<'a> {
    ui: &'a Ui,
}

impl EditorTextRenderer<'_> {
    fn get_rich_text(&self, editor: &dyn ICellDragImageEditor, model_range: Range) -> Option<NodeId> {
        let model = editor.model()?;

        let color_map = Self::get_default_color_map();
        let font_info = editor.font_info();
        let color_at = |id: u32| color_map.get(id as usize).and_then(|hex| parse_hex_color(hex));
        let default_foreground = color_at(color_id::DEFAULT_FOREGROUND).unwrap_or(Rgba::opaque(0, 0, 0));
        let weight = font_weight_value(&font_info.font_weight);
        let line_height = font_info.line_height;

        let mut tree = self.ui.tree_mut();
        let element = create_detached_el(&mut tree, Role::Generic, "");
        tree.style_mut(element).background = color_at(color_id::DEFAULT_BACKGROUND);

        let start_line_number = model_range.start_line_number;
        let start_column = model_range.start_column;
        let end_line_number = model_range.end_line_number;
        let end_column = model_range.end_column;
        let tab_size = model.tab_size();

        let mut y = 0.0;
        let mut max_width: f64 = 0.0;
        for line_number in start_line_number..=end_line_number {
            let line_tokens = model.get_line_tokens(line_number);
            let line_content = line_tokens.get_line_content();
            let start_offset = if line_number == start_line_number { start_column - 1 } else { 0 };
            let end_offset = if line_number == end_line_number { end_column - 1 } else { line_content.len() as i32 };

            let line = create_el(&mut tree, element, Role::Generic, "");
            let mut width = 0.0;
            // `<br>` para a linha vazia: a linha fica só com a altura.
            if !line_content.units().is_empty() {
                let mut rendered = Vec::new();
                for run in tokenize_line_to_runs(&line_content, &line_tokens.inflate(), start_offset, end_offset, tab_size, Platform::detect().is_windows) {
                    let style = InlineStyle {
                        size_px: font_info.font_size as f32,
                        weight: if run.presentation.bold { 700 } else { weight },
                        color: color_at(run.presentation.foreground).unwrap_or(default_foreground),
                        line_height,
                        italic: run.presentation.italic,
                        strikethrough: false,
                    };
                    rendered.extend(build_parts(&mut tree, line, &[LabelPart::Text(run.text)], &style));
                }
                width = layout_parts(&mut tree, &rendered, line_height, None);
            }
            tree.set_bounds(line, Rect::new(0.0, y, width, y + line_height));
            y += line_height;
            max_width = max_width.max(width);
        }
        tree.set_bounds(element, Rect::new(0.0, 0.0, max_width, y));

        Some(element)
    }

    /// `getDefaultColorMap()`: `#000000` no índice 0 e o mapa do registro de tokenização dali em diante.
    fn get_default_color_map() -> Vec<String> {
        let mut result = vec!["#000000".to_owned()];
        if let Some(color_map) = tokenization_registry().get_color_map() {
            result.extend(color_map.into_iter().skip(1));
        }
        result
    }
}

/// `cloneNode(true)`: copia a subárvore (ver as divergências do módulo).
fn clone_node(tree: &mut Tree, source: NodeId) -> NodeId {
    let clone = tree.create_detached(tree.role(source));
    tree.set_bounds(clone, tree.bounds(source));
    *tree.style_mut(clone) = tree.style(source).clone();
    if let Some(label) = tree.label(source).map(str::to_owned) {
        tree.set_label(clone, label);
    }
    let class_name = tree.classes(source).join(" ");
    tree.set_class_name(clone, &class_name);
    for child in tree.children(source).to_vec() {
        let child_clone = clone_node(tree, child);
        tree.append_child(clone, child_clone);
    }
    clone
}

/// `querySelector('.' + class_name)`: o primeiro descendente com a classe, em profundidade.
fn find_descendant_with_class(tree: &Tree, node: NodeId, class_name: &str) -> Option<NodeId> {
    tree.children(node).iter().find_map(|child| if tree.has_class(*child, class_name) { Some(*child) } else { find_descendant_with_class(tree, *child, class_name) })
}

/// `CodeCellDragImageRenderer`.
pub struct CodeCellDragImageRenderer {
    ui: Ui,
    label_styles: HighlightedLabelStyles,
}

impl CodeCellDragImageRenderer {
    pub fn new(ui: &Ui, label_styles: HighlightedLabelStyles) -> Self {
        Self { ui: ui.clone(), label_styles }
    }

    pub fn get_drag_image(&self, template_data: &dyn ICellDragImageTemplate, editor: &dyn ICellDragImageEditor, type_: CellDragImageType) -> NodeId {
        match self.get_drag_image_impl(template_data, editor, type_) {
            Some(drag_image) => drag_image,
            None => {
                // TODO@roblourens I don't think this can happen
                let mut tree = self.ui.tree_mut();
                let drag_image = create_detached_el(&mut tree, Role::Generic, "");
                let line_height = self.label_styles.line_height.unwrap_or(WORKBENCH_LINE_HEIGHT);
                let width = set_node_text(&mut tree, drag_image, "1 cell", &ui_font(self.label_styles.font_size, self.label_styles.font_weight), self.label_styles.foreground, line_height);
                tree.set_bounds(drag_image, Rect::new(0.0, 0.0, width, line_height));
                drag_image
            }
        }
    }

    fn get_drag_image_impl(&self, template_data: &dyn ICellDragImageTemplate, editor: &dyn ICellDragImageEditor, type_: CellDragImageType) -> Option<NodeId> {
        let drag_image_container = {
            let mut tree = self.ui.tree_mut();
            let drag_image_container = clone_node(&mut tree, template_data.container());
            tree.set_class_name(drag_image_container, "");
            for class_name in ["cell-drag-image", "monaco-list-row", "focused"] {
                tree.add_class(drag_image_container, class_name);
            }
            tree.add_class(drag_image_container, &format!("{}-cell-row", type_.as_str()));
            drag_image_container
        };

        let editor_container = find_descendant_with_class(&self.ui.tree(), drag_image_container, "cell-editor-container")?;

        let rich_editor_text = EditorTextRenderer { ui: &self.ui }.get_rich_text(editor, Range::new(1, 1, 1, 1000))?;
        let mut tree = self.ui.tree_mut();
        clear_node(&mut tree, editor_container);
        tree.append_child(editor_container, rich_editor_text);

        Some(drag_image_container)
    }
}
