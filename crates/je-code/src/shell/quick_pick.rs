//! O Quick Open (Ctrl+P) e a paleta de comandos (Ctrl+Shift+P), o `quickInput` do VS Code
//! (`platform/quickinput/browser/quickInputController.ts` e `quickInputList.ts`) com a pontuação fuzzy do
//! `je-base` (a de `fuzzyScorer.ts`, a mesma do original) e as medidas do DOM do oráculo: painel de 600 px
//! centrado a 6 px do topo, campo de 25 px, linhas de 22 px com o raio de 4 px, a linha em foco em
//! `quickInputList.focusBackground`, os trechos casados em negrito e `list.highlightForeground`, a descrição ao
//! lado do nome e o rótulo do grupo (`recently opened`, `file results`) à direita da primeira linha dele.
//!
//! O valor começando em `>` é a paleta, como no original; sem ele, é a busca de arquivos do workspace (mais os
//! editores abertos como `recently opened`). O arquivo `.gitignore` da raiz entra só como nomes simples de
//! pastas; `node_modules` e `.git` ficam de fora como no `search.exclude` e `files.exclude` padrão.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_base::JsString;
use je_base::fuzzy_scorer::{FuzzyScorerCache, ItemAccessor, ItemScore, prepare_query, score_item_fuzzy};
use je_base::platform::Platform;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, NodeId, Role, Tree};
use je_widgets::text::{measure_text, ui_font};

use super::Command;
use super::explorer::LanguageOf;
use super::file_icons::FileIconTheme;

const WIDTH: f64 = 602.0;
const TOP: f64 = 6.0;
const ROW_HEIGHT: f64 = 22.0;
const MAX_VISIBLE_ROWS: usize = 11;
const PLACEHOLDER_FILES: &str = "Search files by name (append : to go to line or @ to go to symbol), or paste a link";
const PLACEHOLDER_COMMANDS: &str = "Type the name of a command to run.";

/// Uma entrada da lista.
#[derive(Clone)]
pub struct PickItem {
    pub label: String,
    pub description: Option<String>,
    /// O rótulo de atalho à direita (paleta).
    pub keybinding: Option<String>,
    /// O que roda ao aceitar.
    pub command: Command,
    /// Aparece como `recently opened` quando o valor é de busca de arquivos.
    pub recent: bool,
}

/// O que o `scoreItemFuzzy` lê de uma [`PickItem`]: rótulo e descrição (o caminho relativo dos arquivos).
struct Accessor;

impl ItemAccessor<PickItem> for Accessor {
    fn get_item_label(&self, item: &PickItem) -> Option<JsString> {
        Some(JsString::from(item.label.as_str()))
    }

    fn get_item_description(&self, item: &PickItem) -> Option<JsString> {
        item.description.as_deref().map(JsString::from)
    }

    fn get_item_path(&self, _: &PickItem) -> Option<JsString> {
        None
    }
}

pub struct QuickPick {
    files: Vec<PickItem>,
    recents: Vec<PickItem>,
    commands: Vec<PickItem>,
    value: Vec<char>,
    cursor: usize,
    /// Os resultados na ordem mostrada: índice na fonte (`recents`, `files` ou `commands`), pontuação.
    results: Vec<(PickItem, Rc<ItemScore>)>,
    focus: usize,
    scroll: usize,
    /// Uma lista fixa de itens (o seletor de tema): o valor só filtra, sem prefixo `>` e sem ícones de arquivo.
    list_placeholder: Option<String>,
    /// Um prompt de nome: o Enter devolve esta ação com o texto digitado.
    prompt: Option<super::PromptAction>,
}

/// Os arquivos do workspace (caminho relativo como descrição, nome como rótulo), até um limite.
pub fn scan_workspace(root: &Path) -> Vec<PickItem> {
    const LIMIT: usize = 50_000;
    let ignored: Vec<String> = std::fs::read_to_string(root.join(".gitignore"))
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('!') && !l.contains(['*', '?', '[', '/']) || l.ends_with('/') && !l.trim_end_matches('/').contains(['*', '?', '[', '/']))
                .map(|l| l.trim_end_matches('/').to_owned())
                .collect()
        })
        .unwrap_or_default();
    let mut items = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(name.as_str(), ".git" | "node_modules" | ".svn" | ".hg" | "CVS" | ".DS_Store" | "Thumbs.db") || ignored.contains(&name) {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if items.len() < LIMIT {
                let relative = path.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).filter(|p| !p.is_empty());
                items.push(PickItem { label: name, description: relative, keybinding: None, command: Command::OpenFile(path), recent: false });
            }
        }
    }
    items.sort_by(|a, b| a.description.cmp(&b.description).then_with(|| a.label.cmp(&b.label)));
    items
}

impl QuickPick {
    pub fn new(files: Vec<PickItem>, recents: Vec<PickItem>, commands: Vec<PickItem>, initial: &str) -> Self {
        let value: Vec<char> = initial.chars().collect();
        let mut pick = Self { files, recents, commands, cursor: value.len(), value, results: Vec::new(), focus: 0, scroll: 0, list_placeholder: None, prompt: None };
        pick.refresh();
        pick
    }

    /// Uma lista fixa de `items` com `placeholder` no campo (o seletor de tema e afins).
    pub fn list(items: Vec<PickItem>, placeholder: &str) -> Self {
        let mut pick = Self::new(Vec::new(), Vec::new(), items, "");
        pick.list_placeholder = Some(placeholder.to_owned());
        pick.refresh();
        pick
    }

    /// Um prompt de uma linha (`InputBox` do `quickInput`): sem lista, o Enter entrega `action` com o texto.
    pub fn prompt(placeholder: &str, initial: &str, action: super::PromptAction) -> Self {
        let mut pick = Self::list(Vec::new(), placeholder);
        pick.value = initial.chars().collect();
        pick.cursor = pick.value.len();
        pick.prompt = Some(action);
        pick
    }

    /// A ação e o texto do prompt, se este é um.
    pub fn prompt_result(&self) -> Option<(super::PromptAction, String)> {
        self.prompt.clone().map(|action| (action, self.value.iter().collect()))
    }

    pub fn is_commands(&self) -> bool {
        self.list_placeholder.is_some() || self.value.first() == Some(&'>')
    }

    fn query_start(&self) -> usize {
        usize::from(self.list_placeholder.is_none() && self.is_commands())
    }

    /// O comando do item em foco, para a pré-visualização.
    pub fn focused_command(&self) -> Option<Command> {
        self.accept(None)
    }

    fn query(&self) -> String {
        let start = self.query_start();
        self.value[start..].iter().collect::<String>().trim().to_owned()
    }

    /// Recalcula os resultados para o valor atual.
    fn refresh(&mut self) {
        let query = self.query();
        let source: Vec<&PickItem> = if self.is_commands() { self.commands.iter().collect() } else { self.recents.iter().chain(self.files.iter()).collect() };
        self.results.clear();
        if query.is_empty() {
            if self.is_commands() {
                self.results = self.commands.iter().map(|i| (i.clone(), Rc::new(ItemScore::default()))).collect();
            } else {
                self.results = self.recents.iter().map(|i| (i.clone(), Rc::new(ItemScore::default()))).collect();
            }
        } else {
            let platform = Platform::detect();
            let prepared = prepare_query(&query.encode_utf16().collect::<Vec<u16>>(), &platform);
            let mut cache: FuzzyScorerCache = HashMap::new();
            let mut scored: Vec<(PickItem, Rc<ItemScore>)> = source
                .into_iter()
                .filter_map(|item| {
                    let score = score_item_fuzzy(Some(item), &prepared, true, &Accessor, &mut cache, &platform);
                    (score.score > 0).then(|| (item.clone(), score))
                })
                .collect();
            // Os editores abertos primeiro (`recently opened`), cada grupo por pontuação e, no empate, por nome.
            scored.sort_by(|a, b| b.0.recent.cmp(&a.0.recent).then_with(|| b.1.score.cmp(&a.1.score)).then_with(|| a.0.label.cmp(&b.0.label)));
            // Um arquivo aberto aparece só como `recently opened`.
            let recent_paths: Vec<Command> = scored.iter().filter(|(i, _)| i.recent).map(|(i, _)| i.command.clone()).collect();
            scored.retain(|(i, _)| i.recent || !recent_paths.contains(&i.command));
            scored.truncate(200);
            self.results = scored;
        }
        self.focus = 0;
        self.scroll = 0;
    }

    fn set_value(&mut self, value: Vec<char>, cursor: usize) {
        self.value = value;
        self.cursor = cursor;
        self.refresh();
    }

    pub fn type_text(&mut self, text: &str) {
        let mut value = self.value.clone();
        let inserted: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
        let cursor = self.cursor + inserted.len();
        value.splice(self.cursor..self.cursor, inserted);
        self.set_value(value, cursor);
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let mut value = self.value.clone();
            value.remove(self.cursor - 1);
            self.set_value(value, self.cursor - 1);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.value.len() {
            let mut value = self.value.clone();
            value.remove(self.cursor);
            self.set_value(value, self.cursor);
        }
    }

    pub fn move_cursor(&mut self, to: usize) {
        self.cursor = to.min(self.value.len());
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn len(&self) -> usize {
        self.value.len()
    }

    pub fn move_focus(&mut self, delta: isize) {
        let n = self.results.len();
        if n == 0 {
            return;
        }
        // Dá a volta nas pontas (`quickInput` com `wrap around`).
        self.focus = (self.focus as isize + delta).rem_euclid(n as isize) as usize;
        if self.focus < self.scroll {
            self.scroll = self.focus;
        } else if self.focus >= self.scroll + MAX_VISIBLE_ROWS {
            self.scroll = self.focus + 1 - MAX_VISIBLE_ROWS;
        }
    }

    /// O comando da linha `index` (o focado se `None`).
    pub fn accept(&self, index: Option<usize>) -> Option<Command> {
        self.results.get(index.unwrap_or(self.focus)).map(|(item, _)| item.command.clone())
    }

    /// O retângulo do painel (sem a sombra) para uma janela de `width` de largura.
    pub fn widget_rect(&self, width: f64) -> Rect {
        let rows = self.results.len().min(MAX_VISIBLE_ROWS);
        let list = if rows == 0 { 0.0 } else { rows as f64 * ROW_HEIGHT + 7.0 };
        let left = ((width - (WIDTH - 2.0)) / 2.0).floor();
        Rect::new(left, TOP, left + WIDTH, TOP + 1.0 + 35.0 + if rows == 0 { 0.0 } else { 2.0 } + list + 1.0)
    }

    /// Monta o painel como filho de `parent` e devolve os nós das linhas com o índice do resultado.
    pub fn render(&self, tree: &mut Tree, parent: NodeId, window_width: f64, theme: &ColorTheme, icons: Option<&FileIconTheme>, language_of: &LanguageOf) -> Vec<(NodeId, usize)> {
        let color = |id: &str| theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0, 0xff));
        let (foreground, background) = (color("quickInput.foreground"), color("quickInput.background"));
        let font = ui_font(13.0, 400);
        let bold = ui_font(13.0, 700);
        let rect = self.widget_rect(window_width);
        let widget = tree.create(parent, Role::Dialog);
        tree.set_bounds(widget, rect);
        let style = tree.style_mut(widget);
        style.background = Some(background);
        style.border = Some(Border { width: 1.0, color: color("widget.border") });
        style.radius = 8.0;
        style.clip = true;
        // O campo (`.quick-input-box`): fundo do `input.background`, borda de foco.
        let input = tree.create(widget, Role::TextInput);
        tree.set_bounds(input, Rect::new(7.0, 7.0, 595.0, 32.0));
        let style = tree.style_mut(input);
        style.background = Some(color("input.background"));
        style.border = Some(Border { width: 1.0, color: color("focusBorder") });
        style.radius = 4.0;
        let text_node = tree.create(input, Role::Generic);
        let shown: String = self.value.iter().collect();
        if shown.is_empty() {
            let placeholder = self.list_placeholder.as_deref().unwrap_or(if self.is_commands() { PLACEHOLDER_COMMANDS } else { PLACEHOLDER_FILES });
            tree.set_bounds(text_node, Rect::new(7.0, 1.0, 586.0, 24.0));
            tree.style_mut(text_node).content = Content::Text { text: placeholder.to_owned(), style: font.clone(), color: color("input.placeholderForeground"), line_height: 23.0 };
        } else {
            tree.set_bounds(text_node, Rect::new(7.0, 1.0, 586.0, 24.0));
            tree.style_mut(text_node).content = Content::Text { text: shown.clone(), style: font.clone(), color: foreground, line_height: 23.0 };
        }
        // O cursor de texto: 1 px na posição do cursor.
        let before: String = self.value[..self.cursor].iter().collect();
        let caret_x = 7.0 + measure_text(&before, &font);
        let caret = tree.create(input, Role::Generic);
        tree.set_bounds(caret, Rect::new(caret_x, 5.0, caret_x + 1.0, 20.0));
        tree.style_mut(caret).background = Some(foreground);
        // A lista.
        let mut rows = Vec::new();
        if self.results.is_empty() {
            return rows;
        }
        let visible = self.results.len().min(MAX_VISIBLE_ROWS);
        let list = tree.create(widget, Role::ListBox);
        tree.set_bounds(list, Rect::new(1.0, 38.0, WIDTH - 1.0, 38.0 + visible as f64 * ROW_HEIGHT + 7.0));
        let (focus_bg, focus_fg) = (color("quickInputList.focusBackground"), color("quickInputList.focusForeground"));
        let muted = color("descriptionForeground");
        let mut previous_recent = None;
        for (slot, index) in (self.scroll..self.scroll + visible).enumerate() {
            let (item, score) = &self.results[index];
            let focused = index == self.focus;
            let y = slot as f64 * ROW_HEIGHT;
            let row = tree.create(list, Role::ListItem);
            tree.set_bounds(row, Rect::new(6.0, y, WIDTH - 8.0, y + ROW_HEIGHT));
            let style = tree.style_mut(row);
            style.radius = 4.0;
            style.cursor = Some(Cursor::Pointer);
            if focused {
                style.background = Some(focus_bg);
            }
            // A borda do grupo: a primeira linha do segundo grupo leva 1 px de `pickerGroup.border` em cima.
            let group_start = previous_recent != Some(item.recent) && !self.is_commands() && !self.value.is_empty();
            if group_start && slot > 0 {
                let line = tree.create(row, Role::Separator);
                tree.set_bounds(line, Rect::new(0.0, 0.0, WIDTH - 14.0, 1.0));
                tree.style_mut(line).background = Some(color("pickerGroup.border"));
            }
            previous_recent = Some(item.recent);
            let (name_color, highlight) = if focused { (focus_fg, color("quickInputList.focusHighlightForeground")) } else { (foreground, color("list.highlightForeground")) };
            let mut x = 4.0;
            if !self.is_commands() {
                if let Some(icons) = icons {
                    icons.decorate(tree, row, Rect::new(x + 3.0, 0.0, x + 19.0, ROW_HEIGHT), &item.label, (language_of)(&item.label).as_deref());
                }
                x += 27.0;
            }
            // O nome com os trechos casados em negrito e na cor de destaque.
            let label_units: Vec<u16> = item.label.encode_utf16().collect();
            let mut spans = score.label_match.clone().unwrap_or_default();
            spans.sort_by_key(|m| m.start);
            let mut cursor = 0usize;
            let piece = |tree: &mut Tree, x: &mut f64, text: String, style: &TextStyle, color: Rgba| {
                if text.is_empty() {
                    return;
                }
                let width = measure_text(&text, style);
                let node = tree.create(row, Role::Generic);
                tree.set_bounds(node, Rect::new(*x, 3.0, *x + width, ROW_HEIGHT));
                tree.style_mut(node).content = Content::Text { text, style: style.clone(), color, line_height: 18.0 };
                *x += width;
            };
            let slice = |from: usize, to: usize| String::from_utf16_lossy(&label_units[from.min(label_units.len())..to.min(label_units.len())]);
            for m in &spans {
                if m.start < cursor {
                    continue;
                }
                piece(tree, &mut x, slice(cursor, m.start), &font, name_color);
                piece(tree, &mut x, slice(m.start, m.end), &bold, highlight);
                cursor = m.end;
            }
            piece(tree, &mut x, slice(cursor, label_units.len()), &font, name_color);
            if let Some(description) = &item.description {
                x += 6.0;
                piece(tree, &mut x, description.clone(), &font, if focused { focus_fg } else { muted });
            }
            // À direita: o atalho do comando ou o rótulo do grupo na primeira linha dele.
            let right_text = if let Some(keybinding) = &item.keybinding {
                Some(keybinding.clone())
            } else if !self.is_commands() && (slot == 0 || group_start) {
                Some(if item.recent { "recently opened" } else { "file results" }.to_owned())
            } else {
                None
            };
            if let Some(text) = right_text {
                // O rótulo do grupo e o atalho saem em 12 px.
                let small = ui_font(12.0, 400);
                let width = measure_text(&text, &small);
                let mut right = WIDTH - 14.0 - 4.0;
                // O item aberto em foco mostra os botões de dividir e fechar.
                if item.recent && focused {
                    for icon in ["close", "split-horizontal"] {
                        let button = tree.create(row, Role::Button);
                        tree.set_bounds(button, Rect::new(right - 22.0, 0.0, right, ROW_HEIGHT));
                        tree.style_mut(button).content = Content::Codicon { id: icon.into(), size: 16.0, color: focus_fg };
                        right -= 24.0;
                    }
                    right -= 2.0;
                }
                let node = tree.create(row, Role::Generic);
                tree.set_bounds(node, Rect::new(right - width, 3.0, right, ROW_HEIGHT));
                let text_color = if focused { focus_fg } else { muted };
                tree.style_mut(node).content = Content::Text { text, style: small, color: text_color, line_height: 18.0 };
            }
            rows.push((row, index));
        }
        rows
    }
}

/// Os arquivos abertos como itens `recently opened`, do mais recente para o mais antigo.
pub fn recent_items(open: &[PathBuf], workspace: Option<&Path>) -> Vec<PickItem> {
    open.iter()
        .map(|path| {
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            let description = path.parent().and_then(|p| workspace.and_then(|w| p.strip_prefix(w).ok()).map(|p| p.to_string_lossy().replace('\\', "/"))).filter(|p| !p.is_empty());
            PickItem { label: name, description, keybinding: None, command: Command::OpenFile(path.clone()), recent: true }
        })
        .collect()
}
