//! Os campos de texto de uma linha do shell (busca, substituição, Quick Open, mensagem do commit, filtro das
//! extensões) sobre o [`TextField`] do `je-widgets` (o `<input>` do Chromium: seleção, palavra, desfazer, área de
//! transferência), com a pintura do `InputBox` do VS Code: texto rolado, seleção e cursor.

use std::cell::RefCell;

use je_base::key_codes::KeyCode;
use je_editor_view::EditorKey;
use je_editor_view::clipboard::Clipboard;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_ui::{Content, Modifiers, NodeId, Role, Tree};
use je_widgets::dom::StandardKeyboardEvent;
use je_widgets::text_field::{ClipboardProvider, TextField};

/// A área de transferência do sistema (a mesma do editor) para os campos.
pub struct SystemClipboard(RefCell<Clipboard>);

impl SystemClipboard {
    pub fn new() -> Self {
        Self(RefCell::new(Clipboard::new()))
    }
}

impl ClipboardProvider for SystemClipboard {
    fn read_text(&self) -> Option<String> {
        Some(self.0.borrow_mut().text()).filter(|t| !t.is_empty())
    }

    fn write_text(&self, text: &str) {
        self.0.borrow_mut().set_text(text);
    }
}

/// O `KeyCode` da tecla: o da janela, ou o da tecla de edição quando a janela não o deu (os testes).
fn key_code_of(key_code: KeyCode, key: Option<EditorKey>) -> KeyCode {
    if key_code != KeyCode::Unknown {
        return key_code;
    }
    match key {
        Some(EditorKey::Left) => KeyCode::LeftArrow,
        Some(EditorKey::Right) => KeyCode::RightArrow,
        Some(EditorKey::Up) => KeyCode::UpArrow,
        Some(EditorKey::Down) => KeyCode::DownArrow,
        Some(EditorKey::Home) => KeyCode::Home,
        Some(EditorKey::End) => KeyCode::End,
        Some(EditorKey::Backspace) => KeyCode::Backspace,
        Some(EditorKey::Delete) => KeyCode::Delete,
        Some(EditorKey::Char(c)) => je_base::key_codes::KeyCodeUtils::from_string(&c.to_ascii_uppercase().to_string()),
        _ => KeyCode::Unknown,
    }
}

/// Entrega uma tecla ao campo: os atalhos de edição do `<input>` e, sem atalho, o texto digitado. Devolve se o
/// valor mudou; `None` quando a tecla não é do campo (Enter, Escape, Tab, setas verticais...).
pub fn edit(field: &mut TextField, key_code: KeyCode, key: Option<EditorKey>, mods: Modifiers, text: Option<&str>, clipboard: &dyn ClipboardProvider) -> Option<bool> {
    let code = key_code_of(key_code, key);
    let event = StandardKeyboardEvent { key_code: code, ctrl_key: mods.ctrl, shift_key: mods.shift, alt_key: mods.alt, meta_key: mods.meta };
    let outcome = field.handle_key(&event, clipboard);
    if outcome.handled {
        return Some(outcome.changed);
    }
    let ctrl = mods.ctrl || mods.meta;
    match (key, text) {
        (None | Some(EditorKey::Char(_)), Some(text)) if (!ctrl || mods.alt) && !text.chars().all(char::is_control) => Some(field.insert_text(text)),
        _ => None,
    }
}

/// As cores de um campo.
pub struct InputColors {
    pub foreground: Rgba,
    pub placeholder: Rgba,
    pub selection: Rgba,
}

/// Pinta o texto do campo (ou o `placeholder` vazio) em `area` (relativa a `parent`), com a seleção e o cursor
/// quando focado, rolando para o cursor ficar à vista.
#[allow(clippy::too_many_arguments)]
pub fn paint(tree: &mut Tree, parent: NodeId, area: Rect, field: &mut TextField, placeholder: &str, focused: bool, style: &TextStyle, colors: &InputColors) {
    let line_height = area.height();
    let clip = tree.create(parent, Role::Generic);
    tree.set_bounds(clip, area);
    tree.style_mut(clip).clip = true;
    if field.is_empty() {
        if !placeholder.is_empty() {
            let node = tree.create(clip, Role::Generic);
            tree.set_bounds(node, Rect::new(0.0, 0.0, area.width(), line_height));
            tree.style_mut(node).content = Content::Text { text: placeholder.to_owned(), style: style.clone(), color: colors.placeholder, line_height };
        }
    } else {
        field.reveal_caret(area.width() - 1.0, style);
        let scroll = field.scroll_left();
        if focused && let Some((x0, x1)) = field.selection_x_range(style) {
            let selection = tree.create(clip, Role::Generic);
            tree.set_bounds(selection, Rect::new(x0 - scroll, 3.0, x1 - scroll, line_height - 3.0));
            tree.style_mut(selection).background = Some(colors.selection);
        }
        let width = field.content_width(style);
        let node = tree.create(clip, Role::Generic);
        tree.set_bounds(node, Rect::new(-scroll, 0.0, -scroll + width + 1.0, line_height));
        tree.style_mut(node).content = Content::Text { text: field.display_text().into_owned(), style: style.clone(), color: colors.foreground, line_height };
    }
    if focused && !field.has_selection() {
        let x = field.x_at_offset(field.caret(), style) - field.scroll_left();
        let caret = tree.create(clip, Role::Generic);
        tree.set_bounds(caret, Rect::new(x, 4.0, x + 1.0, line_height - 4.0));
        tree.style_mut(caret).background = Some(colors.foreground);
    }
}

/// Um campo com `text`, todo selecionado (a semente da busca).
pub fn seeded(text: &str) -> TextField {
    let mut field = TextField::new();
    field.set_value(text);
    field.select_all();
    field
}
