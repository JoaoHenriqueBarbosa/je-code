//! O widget de busca do editor (`editor/contrib/find/browser/findWidget.ts`): campo com as chaves de maiúsculas,
//! palavra inteira e regex, contador `N of M` ("No results" sem ocorrência), anterior, próxima e fechar, no canto
//! superior direito do editor, com as medidas do DOM do oráculo. As ocorrências vêm do `TextModel::find_matches`
//! (a busca portada do Monaco) e o editor as pinta (`EditorView::set_find_matches`).
//!
//! Fica de fora por ora: a linha de substituição (o botão de expandir aparece, mas não abre), a busca na seleção
//! e o histórico do campo.

use je_base::JsString;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::{FindScope, TextModel};
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, NodeId, Role, Tree};
use je_widgets::text::{measure_text, ui_font};

use super::Command;

pub const WIDTH: f64 = 419.0;
pub const HEIGHT: f64 = 33.0;
/// A distância do topo da área do editor.
pub const TOP: f64 = 4.0;
/// O que fica à direita do widget: a barra de rolagem (14), o minimapa e mais 14 px de folga.
const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";
const MATCH_LIMIT: usize = 20_000;

pub struct FindWidget {
    pub value: Vec<char>,
    pub cursor: usize,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub matches: Vec<Range>,
    pub current: Option<usize>,
    /// A versão do modelo para a qual `matches` vale.
    pub version: i64,
    /// A posição de onde a busca parte ao digitar (o cursor do editor ao abrir).
    pub origin: Position,
    /// O valor inteiro está selecionado (a semente ao abrir): digitar o substitui.
    pub select_all: bool,
}

impl FindWidget {
    pub fn new(seed: &str, origin: Position) -> Self {
        let value: Vec<char> = seed.chars().collect();
        Self { cursor: value.len(), value, match_case: false, whole_word: false, regex: false, matches: Vec::new(), current: None, version: -1, origin, select_all: !seed.is_empty() }
    }

    pub fn text(&self) -> String {
        self.value.iter().collect()
    }

    /// Recalcula as ocorrências no modelo.
    pub fn search(&mut self, model: &TextModel) {
        self.version = model.get_version_id();
        self.matches = if self.value.is_empty() {
            Vec::new()
        } else {
            let separators = self.whole_word.then_some(WORD_SEPARATORS);
            model.find_matches(JsString::from(self.text().as_str()), FindScope::Whole, self.regex, self.match_case, separators, false, MATCH_LIMIT).into_iter().map(|m| m.range).collect()
        };
        self.current = None;
    }

    /// Escolhe a primeira ocorrência a partir de `from` (dando a volta no documento) e a devolve.
    pub fn select_from(&mut self, from: Position) -> Option<Range> {
        let index = self.matches.iter().position(|m| !m.get_start_position().is_before(&from)).or(if self.matches.is_empty() { None } else { Some(0) })?;
        self.current = Some(index);
        Some(self.matches[index])
    }

    /// A ocorrência seguinte (`delta` 1) ou anterior (-1) à atual, dando a volta.
    pub fn step(&mut self, delta: isize) -> Option<Range> {
        let n = self.matches.len() as isize;
        if n == 0 {
            return None;
        }
        let next = match self.current {
            Some(i) => (i as isize + delta).rem_euclid(n),
            None if delta > 0 => 0,
            None => n - 1,
        } as usize;
        self.current = Some(next);
        Some(self.matches[next])
    }

    /// O texto do contador.
    pub fn count_label(&self) -> String {
        if self.value.is_empty() {
            return "No results".to_owned();
        }
        match (self.matches.len(), self.current) {
            (0, _) => "No results".to_owned(),
            (n, Some(i)) => format!("{} of {n}", i + 1),
            (n, None) => format!("? of {n}"),
        }
    }

    /// O retângulo do widget numa área de editor de `editor` (coordenadas da janela) e do minimapa de `minimap_width`.
    pub fn rect(editor: Rect, minimap_width: f64) -> Rect {
        let right = editor.x1 - 14.0 - minimap_width - 14.0;
        Rect::new(right - WIDTH, editor.y0 + TOP, right, editor.y0 + TOP + HEIGHT)
    }

    /// Monta o widget como filho de `parent` na posição `rect` e devolve os nós clicáveis.
    pub fn render(&self, tree: &mut Tree, parent: NodeId, rect: Rect, focused: bool, theme: &ColorTheme) -> Vec<(NodeId, Command)> {
        let color = |id: &str| theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0, 0xff));
        let foreground = color("editorWidget.foreground");
        let icon_color = color("icon.foreground");
        let font = ui_font(12.0, 400);
        let mut hits = Vec::new();
        let widget = tree.create(parent, Role::Dialog);
        tree.set_bounds(widget, rect);
        let style = tree.style_mut(widget);
        style.background = Some(color("editorWidget.background"));
        style.border = Some(Border { width: 1.0, color: color("widget.border") });
        style.radius = 8.0;
        let button = |tree: &mut Tree, hits: &mut Vec<(NodeId, Command)>, bounds: Rect, icon: &str, size: f32, tint: Rgba, command: Command, radius: f64| {
            let node = tree.create(widget, Role::Button);
            tree.set_bounds(node, bounds);
            let style = tree.style_mut(node);
            style.content = Content::Codicon { id: icon.into(), size, color: tint };
            style.radius = radius;
            style.cursor = Some(Cursor::Pointer);
            hits.push((node, command));
        };
        // O botão de expandir a substituição.
        button(tree, &mut hits, Rect::new(5.0, 4.0, 23.0, 27.0), "chevron-right", 16.0, color("icon.foreground"), Command::FindToggleReplace, 4.0);
        // O campo.
        let input = tree.create(widget, Role::TextInput);
        tree.set_bounds(input, Rect::new(26.0, 3.0, 242.0, 28.0));
        let style = tree.style_mut(input);
        style.background = Some(color("input.background"));
        let no_results = !self.value.is_empty() && self.matches.is_empty();
        let border = if no_results { color("inputValidation.errorBorder") } else if focused { color("focusBorder") } else { color("input.border") };
        style.border = Some(Border { width: 1.0, color: border });
        style.radius = 4.0;
        let text = tree.create(input, Role::Generic);
        tree.set_bounds(text, Rect::new(6.0, 0.0, 140.0, 23.0));
        let shown = self.text();
        if shown.is_empty() {
            tree.style_mut(text).content = Content::Text { text: "Find".to_owned(), style: font.clone(), color: color("input.placeholderForeground"), line_height: 23.0 };
        } else {
            tree.style_mut(text).content = Content::Text { text: shown, style: font.clone(), color: foreground, line_height: 23.0 };
        }
        if focused && self.select_all && !self.value.is_empty() {
            let width = measure_text(&self.text(), &font);
            let selection = tree.create(input, Role::Generic);
            tree.set_bounds(selection, Rect::new(6.0, 3.0, 6.0 + width, 20.0));
            tree.style_mut(selection).background = Some(color("editor.selectionBackground"));
            // O texto fica por cima da seleção.
            tree.insert_before(input, text, None);
        }
        if focused {
            let before: String = self.value[..self.cursor].iter().collect();
            let x = 6.0 + measure_text(&before, &font);
            let caret = tree.create(input, Role::Generic);
            tree.set_bounds(caret, Rect::new(x, 4.0, x + 1.0, 18.0));
            tree.style_mut(caret).background = Some(foreground);
        }
        // As chaves dentro do campo (`Aa`, `ab`, `.*`), 20 x 20 a cada 22 px.
        let toggles = [("case-sensitive", self.match_case, Command::FindToggleCase), ("whole-word", self.whole_word, Command::FindToggleWord), ("regex", self.regex, Command::FindToggleRegex)];
        for (i, (icon, on, command)) in toggles.into_iter().enumerate() {
            let x = 150.0 + 22.0 * i as f64;
            let node = tree.create(input, Role::Checkbox);
            tree.set_bounds(node, Rect::new(x, 2.0, x + 20.0, 22.0));
            let style = tree.style_mut(node);
            style.content = Content::Codicon { id: icon.into(), size: 16.0, color: if on { color("inputOption.activeForeground") } else { foreground } };
            style.radius = 3.0;
            style.cursor = Some(Cursor::Pointer);
            if on {
                style.background = Some(color("inputOption.activeBackground"));
                style.border = Some(Border { width: 1.0, color: color("inputOption.activeBorder") });
            }
            hits.push((node, command));
        }
        // O contador.
        let count = tree.create(widget, Role::Status);
        tree.set_bounds(count, Rect::new(246.0, 3.0, 315.0, 28.0));
        let count_color = if no_results { color("errorForeground") } else { foreground };
        tree.style_mut(count).content = Content::Text { text: self.count_label(), style: font, color: count_color, line_height: 25.0 };
        // Anterior, próxima, na seleção e fechar.
        let enabled = if self.matches.is_empty() { Rgba { a: 0x66, ..icon_color } } else { icon_color };
        button(tree, &mut hits, Rect::new(317.0, 4.5, 339.0, 26.5), "arrow-up", 16.0, enabled, Command::FindPrevious, 5.0);
        button(tree, &mut hits, Rect::new(342.0, 4.5, 364.0, 26.5), "arrow-down", 16.0, enabled, Command::FindNext, 5.0);
        button(tree, &mut hits, Rect::new(366.0, 4.5, 388.0, 26.5), "selection", 16.0, icon_color, Command::FindInSelection, 5.0);
        button(tree, &mut hits, Rect::new(391.0, 5.0, 413.0, 27.0), "close", 16.0, icon_color, Command::FindClose, 5.0);
        hits
    }
}
