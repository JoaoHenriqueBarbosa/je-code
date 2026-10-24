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
use je_widgets::text::ui_font;
use je_widgets::text_field::TextField;

use super::input::InputColors;

use super::Command;

pub const WIDTH: f64 = 419.0;
pub const HEIGHT: f64 = 33.0;
/// A altura com a linha de substituição.
pub const HEIGHT_WITH_REPLACE: f64 = 62.0;
/// A distância do topo da área do editor.
pub const TOP: f64 = 4.0;
/// O que fica à direita do widget: a barra de rolagem (14), o minimapa e mais 14 px de folga.
const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";
const MATCH_LIMIT: usize = 20_000;

pub struct FindWidget {
    /// O campo da busca.
    pub field: TextField,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub matches: Vec<Range>,
    pub current: Option<usize>,
    /// A versão do modelo para a qual `matches` vale.
    pub version: i64,
    /// A posição de onde a busca parte ao digitar (o cursor do editor ao abrir).
    pub origin: Position,
    /// A linha de substituição aberta, o texto dela e se o teclado está nela (e não no campo da busca).
    pub replace_visible: bool,
    pub replace: TextField,
    pub in_replace: bool,
}

impl FindWidget {
    pub fn new(seed: &str, origin: Position) -> Self {
        Self { field: super::input::seeded(seed), match_case: false, whole_word: false, regex: false, matches: Vec::new(), current: None, version: -1, origin, replace_visible: false, replace: TextField::new(), in_replace: false }
    }

    pub fn text(&self) -> String {
        self.field.value().to_owned()
    }

    /// Recalcula as ocorrências no modelo.
    pub fn search(&mut self, model: &TextModel) {
        self.version = model.get_version_id();
        self.matches = if self.field.is_empty() {
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

    /// O texto que troca a ocorrência `matched`.
    pub fn replacement_for(&self, matched: &str) -> String {
        expand_replacement(&self.text(), self.regex, self.match_case, self.replace.value(), matched)
    }

    /// O texto do contador.
    pub fn count_label(&self) -> String {
        if self.field.is_empty() {
            return "No results".to_owned();
        }
        match (self.matches.len(), self.current) {
            (0, _) => "No results".to_owned(),
            (n, Some(i)) => format!("{} of {n}", i + 1),
            (n, None) => format!("? of {n}"),
        }
    }

    /// O retângulo do widget numa área de editor de `editor` (coordenadas da janela) e do minimapa de `minimap_width`.
    pub fn rect(&self, editor: Rect, minimap_width: f64) -> Rect {
        let right = editor.x1 - 14.0 - minimap_width - 14.0;
        let height = if self.replace_visible { HEIGHT_WITH_REPLACE } else { HEIGHT };
        Rect::new(right - WIDTH, editor.y0 + TOP, right, editor.y0 + TOP + height)
    }

    /// Monta o widget como filho de `parent` na posição `rect` e devolve os nós clicáveis.
    pub fn render(&mut self, tree: &mut Tree, parent: NodeId, rect: Rect, focused: bool, theme: &ColorTheme) -> Vec<(NodeId, Command)> {
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
        let toggle_bottom = if self.replace_visible { 56.0 } else { 27.0 };
        button(tree, &mut hits, Rect::new(5.0, 4.0, 23.0, toggle_bottom), if self.replace_visible { "chevron-down" } else { "chevron-right" }, 16.0, color("icon.foreground"), Command::FindToggleReplace, 4.0);
        // O campo.
        let input = tree.create(widget, Role::TextInput);
        tree.set_bounds(input, Rect::new(26.0, 3.0, 242.0, 28.0));
        let style = tree.style_mut(input);
        style.background = Some(color("input.background"));
        let no_results = !self.field.is_empty() && self.matches.is_empty();
        let find_focused = focused && !self.in_replace;
        let border = if no_results { color("inputValidation.errorBorder") } else if find_focused { color("focusBorder") } else { color("input.border") };
        style.border = Some(Border { width: 1.0, color: border });
        style.radius = 4.0;
        let colors = InputColors { foreground, placeholder: color("input.placeholderForeground"), selection: color("editor.selectionBackground") };
        super::input::paint(tree, input, Rect::new(6.0, 0.0, 144.0, 23.0), &mut self.field, "Find", find_focused, &font, &colors);
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
        if self.replace_visible {
            // A linha de substituição (`.replace-part`): o campo e os botões Replace e Replace All.
            let input = tree.create(widget, Role::TextInput);
            tree.set_bounds(input, Rect::new(26.0, 32.0, 242.0, 57.0));
            hits.push((input, Command::FindFocusReplace));
            let style = tree.style_mut(input);
            style.background = Some(color("input.background"));
            style.border = Some(Border { width: 1.0, color: if focused && self.in_replace { color("focusBorder") } else { color("input.border") } });
            style.radius = 4.0;
            super::input::paint(tree, input, Rect::new(6.0, 0.0, 208.0, 23.0), &mut self.replace, "Replace", focused && self.in_replace, &ui_font(12.0, 400), &colors);
            button(tree, &mut hits, Rect::new(246.0, 33.5, 268.0, 55.5), "replace", 16.0, enabled, Command::FindReplace, 5.0);
            button(tree, &mut hits, Rect::new(271.0, 33.5, 293.0, 55.5), "replace-all", 16.0, enabled, Command::FindReplaceAll, 5.0);
        }
        hits
    }
}

/// O texto de substituição para a ocorrência `matched` da busca `pattern`: com regex, `$&`, `$1`...`$9` e `$$` se
/// expandem com os grupos do casamento (o `buildReplaceString` do VS Code); sem regex, o texto como está.
pub fn expand_replacement(pattern: &str, regex: bool, match_case: bool, template: &str, matched: &str) -> String {
    if !regex {
        return template.to_owned();
    }
    let flags = if match_case { "u" } else { "iu" };
    let Ok(regexp) = je_base::js_regexp::JsRegExp::from_str(pattern, flags) else { return template.to_owned() };
    let units: Vec<u16> = matched.encode_utf16().collect();
    let Some(found) = regexp.exec(&units) else { return template.to_owned() };
    let group = |n: usize| found.captures.get(n).cloned().flatten().map(|r| String::from_utf16_lossy(&units[r])).unwrap_or_default();
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('$', Some('$')) => {
                chars.next();
                out.push('$');
            }
            ('$', Some('&')) => {
                chars.next();
                out.push_str(&group(0));
            }
            ('$', Some(d)) if d.is_ascii_digit() && d != '0' => {
                chars.next();
                out.push_str(&group(d as usize - '0' as usize));
            }
            _ => out.push(c),
        }
    }
    out
}
