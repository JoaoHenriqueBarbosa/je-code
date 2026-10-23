//! O editor de texto de linha única que o `<input>` do navegador dá de graça ao `InputBox` do VS Code.
//!
//! O `inputBox.ts` não edita nada: a edição (cursor, seleção, palavra, desfazer, área de transferência,
//! rolagem horizontal) é do `<input type="text">` do Chromium. Como a árvore do `je-ui` não tem controle
//! nativo, este módulo é esse controle, escrito para se comportar como o do Chromium, e não depende do
//! `je-editor` nem do `je-editor-view` (o editor do Monaco é outra coisa: modelo de linhas, decorações).
//!
//! O que existe:
//!
//! - [`TextField`]: o controlador, sobre o texto e uma seleção com `anchor` e `focus`. Os deslocamentos são
//!   em unidades UTF-16, como o `selectionStart`/`selectionEnd` do DOM, e as bordas são tratadas com cuidado
//!   de pares substitutos: uma seleção nunca para no meio de um par, e o cursor anda de grafema em grafema
//!   (emoji com ZWJ, letra com acento combinante) como o do Chromium. Operações: inserir (substitui a
//!   seleção), apagar para trás e para a frente por grafema, por palavra e até a borda da linha, mover com ou
//!   sem Shift, selecionar tudo, selecionar palavra, ponteiro (clique, duplo, triplo, arrasto), desfazer e
//!   refazer com edições agrupadas, recortar, copiar e colar por um [`ClipboardProvider`].
//! - [`TextField::handle_key`]: a tabela de atalhos, que depende do sistema como no Chromium (macOS usa
//!   Cmd e Option, Windows e Linux usam Ctrl). Teclas que só produzem texto não são tratadas aqui: o texto
//!   chega como `TextInput` e entra por [`TextField::insert_text`], como o `input`/`beforeinput` do DOM.
//! - A geometria: [`TextField::offset_at_x`] (cursor por clique, medindo prefixos com
//!   [`measure_text`](crate::text::measure_text)), [`TextField::x_at_offset`] e a rolagem horizontal que
//!   mantém o cursor visível ([`TextField::reveal_caret`]).
//!
//! Fica de fora: composição de IME, correção ortográfica, texto bidirecional (a medida é de uma direção só)
//! e o `maxlength`.
//!
//! Divergências do Chromium que não puderam ser medidas aqui (o oráculo ainda não cobre o `<input>`):
//! a regra de palavra é "letras, dígitos e `_` formam palavra, o resto separa" (o ICU do Chromium tem mais
//! casos, como o apóstrofo no meio de uma palavra e o CJK); quebra de linha colada vira um espaço (o
//! Chromium também as tira do valor de um `<input>`, aqui viram espaço); `set_value` as remove, como o
//! algoritmo de sanitização do valor.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::strings::{is_high_surrogate, is_low_surrogate, next_char_length, prev_char_length};
use je_base::key_codes::KeyCode;
use je_text::TextStyle;

use crate::common::is_windows;
use crate::dom::StandardKeyboardEvent;
use crate::sash::is_macintosh;
use crate::text::measure_text;

/// O ponto de código que mascara cada caractere de um campo de senha.
pub const PASSWORD_MASK: char = '\u{2022}';

/// A área de transferência que o campo usa. O `je-editor-view` usa o `arboard` em `clipboard.rs`; a
/// integração liga os dois.
// TODO(integração): implementar sobre o `arboard` do `je-editor-view` (`clipboard.rs`).
pub trait ClipboardProvider {
    /// O texto da área de transferência, se houver.
    fn read_text(&self) -> Option<String>;
    fn write_text(&self, text: &str);
}

/// A área de transferência em memória, para os testes e para quem não tem a do sistema.
#[derive(Default)]
pub struct MemoryClipboard(RefCell<Option<String>>);

impl MemoryClipboard {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn with_text(text: &str) -> Rc<Self> {
        Rc::new(Self(RefCell::new(Some(text.to_owned()))))
    }
}

impl ClipboardProvider for MemoryClipboard {
    fn read_text(&self) -> Option<String> {
        self.0.borrow().clone()
    }

    fn write_text(&self, text: &str) {
        *self.0.borrow_mut() = Some(text.to_owned());
    }
}

impl<T: ClipboardProvider + ?Sized> ClipboardProvider for Rc<T> {
    fn read_text(&self) -> Option<String> {
        (**self).read_text()
    }

    fn write_text(&self, text: &str) {
        (**self).write_text(text);
    }
}

/// O sistema de que os atalhos e a regra de palavra dependem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Mac,
    Linux,
}

impl Platform {
    pub fn current() -> Self {
        if is_macintosh() {
            Self::Mac
        } else if is_windows() {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// A direção de uma seleção (o `selectionDirection` do DOM).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionDirection {
    None,
    Forward,
    Backward,
}

/// Para onde um movimento de cursor vai.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Movement {
    Left,
    Right,
    WordLeft,
    WordRight,
    Start,
    End,
}

/// O que um apagamento apaga, a partir do cursor (sem seleção).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteUnit {
    Grapheme,
    Word,
    /// Até a borda da linha (o início para trás, o fim para a frente).
    Line,
}

/// O resultado de uma tecla: se o campo a tratou (o `preventDefault` do navegador) e se o texto mudou.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyOutcome {
    pub handled: bool,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKind {
    Typing,
    DeleteBackward,
    DeleteForward,
    Other,
}

/// Uma edição desfazível: o trecho `[start, start + inserted.len())` do texto novo era `removed`.
struct Edit {
    start: usize,
    removed: Vec<u16>,
    inserted: Vec<u16>,
    before: (usize, usize),
    after: (usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragMode {
    Char,
    /// O arrasto de um duplo clique cresce por palavras, a partir da palavra inicial.
    Word { start: usize, end: usize },
    /// O arrasto de um triplo clique mantém tudo selecionado.
    All,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Other,
}

fn char_class(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Other
    }
}

/// O ponto de código que começa em `offset` (o par substituto inteiro) e quantas unidades ocupa.
fn char_at(units: &[u16], offset: usize) -> (char, usize) {
    let width = if is_high_surrogate(units[offset]) && units.get(offset + 1).is_some_and(|u| is_low_surrogate(*u)) { 2 } else { 1 };
    (char::decode_utf16(units[offset..offset + width].iter().copied()).next().and_then(Result::ok).unwrap_or('\u{fffd}'), width)
}

/// O ponto de código que termina em `offset`.
fn char_before(units: &[u16], offset: usize) -> (char, usize) {
    let width = if offset >= 2 && is_low_surrogate(units[offset - 1]) && is_high_surrogate(units[offset - 2]) { 2 } else { 1 };
    (char::decode_utf16(units[offset - width..offset].iter().copied()).next().and_then(Result::ok).unwrap_or('\u{fffd}'), width)
}

fn to_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn from_units(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// O texto de uma linha só: o que o `<input>` guarda no valor (sem quebras de linha).
fn single_line(text: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str(replacement);
            }
            '\n' => out.push_str(replacement),
            c => out.push(c),
        }
    }
    out
}

/// O campo de texto de linha única. Ver a documentação do módulo.
pub struct TextField {
    units: Vec<u16>,
    text: String,
    anchor: usize,
    focus: usize,
    password: bool,
    platform: Platform,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// O tipo da edição que ainda aceita fusão com a próxima (digitar em seguida, apagar em seguida);
    /// qualquer mudança de seleção que não seja uma edição o fecha.
    open_group: Option<EditKind>,
    scroll_left: f64,
    drag: Option<DragMode>,
}

impl Default for TextField {
    fn default() -> Self {
        Self::new()
    }
}

impl TextField {
    pub fn new() -> Self {
        Self { units: Vec::new(), text: String::new(), anchor: 0, focus: 0, password: false, platform: Platform::current(), undo: Vec::new(), redo: Vec::new(), open_group: None, scroll_left: 0.0, drag: None }
    }

    /// Troca o sistema que decide atalhos e regra de palavra (para os testes e para a pré-visualização).
    pub fn set_platform(&mut self, platform: Platform) {
        self.platform = platform;
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Um campo de senha mostra um [`PASSWORD_MASK`] por caractere e não copia nem recorta.
    pub fn set_password(&mut self, password: bool) {
        self.password = password;
    }

    pub fn is_password(&self) -> bool {
        self.password
    }

    // ---- valor e seleção -------------------------------------------------------------------------------

    pub fn value(&self) -> &str {
        &self.text
    }

    /// O tamanho do valor em unidades UTF-16 (o `value.length` do DOM).
    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// `input.value = text`: troca o valor, leva o cursor ao fim e esquece o histórico de desfazer (como o
    /// DOM). Devolve falso se o valor não mudou, caso em que nada acontece.
    pub fn set_value(&mut self, value: &str) -> bool {
        let value = single_line(value, "");
        if value == self.text {
            return false;
        }
        self.units = to_units(&value);
        self.text = value;
        self.anchor = self.units.len();
        self.focus = self.units.len();
        self.undo.clear();
        self.redo.clear();
        self.open_group = None;
        true
    }

    /// `[selectionStart, selectionEnd]`.
    pub fn selection(&self) -> (usize, usize) {
        (self.anchor.min(self.focus), self.anchor.max(self.focus))
    }

    pub fn selection_direction(&self) -> SelectionDirection {
        match self.anchor.cmp(&self.focus) {
            std::cmp::Ordering::Less => SelectionDirection::Forward,
            std::cmp::Ordering::Greater => SelectionDirection::Backward,
            std::cmp::Ordering::Equal => SelectionDirection::None,
        }
    }

    /// O cursor (o lado móvel da seleção).
    pub fn caret(&self) -> usize {
        self.focus
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.focus
    }

    pub fn selected_text(&self) -> String {
        let (start, end) = self.selection();
        from_units(&self.units[start..end])
    }

    /// `setSelectionRange(start, end, direction)`: os valores são limitados ao texto e levados para fora de
    /// um par substituto.
    pub fn set_selection_range(&mut self, start: usize, end: usize, direction: SelectionDirection) {
        let (low, high) = (start.min(end), start.max(end));
        // Um cursor no meio de um par vai para antes dele; uma seleção o inclui inteiro.
        let (start, end) = if low == high { (self.snap(low, false), self.snap(low, false)) } else { (self.snap(low, false), self.snap(high, true)) };
        (self.anchor, self.focus) = if direction == SelectionDirection::Backward { (end, start) } else { (start, end) };
        self.open_group = None;
    }

    /// `select()`.
    pub fn select_all(&mut self) {
        self.set_selection_range(0, self.units.len(), SelectionDirection::Forward);
    }

    /// Leva o cursor para `offset` (limitado ao texto), sem seleção.
    pub fn set_caret(&mut self, offset: usize) {
        let offset = self.snap(offset, false);
        (self.anchor, self.focus) = (offset, offset);
        self.open_group = None;
    }

    /// Limita `offset` ao texto e o tira do meio de um par substituto (para cima ou para baixo).
    fn snap(&self, offset: usize, up: bool) -> usize {
        let offset = offset.min(self.units.len());
        if offset > 0 && offset < self.units.len() && is_low_surrogate(self.units[offset]) && is_high_surrogate(self.units[offset - 1]) {
            return if up { offset + 1 } else { offset - 1 };
        }
        offset
    }

    // ---- edição ----------------------------------------------------------------------------------------

    /// Substitui `[start, end)` por `with` e deixa o cursor depois do que entrou. Registra a edição, fundindo-a
    /// com a anterior quando o tipo permite. Devolve falso se nada mudou.
    fn replace(&mut self, start: usize, end: usize, with: &[u16], kind: EditKind) -> bool {
        if start == end && with.is_empty() {
            return false;
        }
        let before = (self.anchor, self.focus);
        let removed: Vec<u16> = self.units.splice(start..end, with.iter().copied()).collect();
        self.text = from_units(&self.units);
        let caret = start + with.len();
        (self.anchor, self.focus) = (caret, caret);
        self.redo.clear();
        let merged = self.open_group == Some(kind) && self.merge_into_last(start, &removed, with, kind);
        if !merged {
            self.undo.push(Edit { start, removed, inserted: with.to_vec(), before, after: (caret, caret) });
        } else if let Some(last) = self.undo.last_mut() {
            last.after = (caret, caret);
        }
        self.open_group = (kind != EditKind::Other).then_some(kind);
        true
    }

    fn merge_into_last(&mut self, start: usize, removed: &[u16], inserted: &[u16], kind: EditKind) -> bool {
        let Some(last) = self.undo.last_mut() else { return false };
        match kind {
            EditKind::Typing if removed.is_empty() && last.removed.is_empty() && start == last.start + last.inserted.len() => {
                last.inserted.extend_from_slice(inserted);
                true
            }
            EditKind::DeleteBackward if last.inserted.is_empty() && start + removed.len() == last.start => {
                last.start = start;
                let mut joined = removed.to_vec();
                joined.append(&mut last.removed);
                last.removed = joined;
                true
            }
            EditKind::DeleteForward if last.inserted.is_empty() && start == last.start => {
                last.removed.extend_from_slice(removed);
                true
            }
            _ => false,
        }
    }

    /// Insere `text` no lugar da seleção (o `input` de um `<input>`): quebras de linha viram espaço.
    pub fn insert_text(&mut self, text: &str) -> bool {
        self.insert_with(text, EditKind::Typing)
    }

    fn insert_with(&mut self, text: &str, kind: EditKind) -> bool {
        let (start, end) = self.selection();
        self.replace(start, end, &to_units(&single_line(text, " ")), kind)
    }

    /// Apaga a seleção, ou, sem seleção, a `unit` antes (`forward` falso) ou depois do cursor.
    pub fn delete(&mut self, unit: DeleteUnit, forward: bool) -> bool {
        let (start, end) = self.selection();
        let kind = if forward { EditKind::DeleteForward } else { EditKind::DeleteBackward };
        if start != end {
            return self.replace(start, end, &[], EditKind::Other);
        }
        let target = match (unit, forward) {
            (DeleteUnit::Grapheme, false) => self.grapheme_before(start),
            (DeleteUnit::Grapheme, true) => self.grapheme_after(start),
            (DeleteUnit::Word, forward) => self.word_boundary(start, forward),
            (DeleteUnit::Line, false) => 0,
            (DeleteUnit::Line, true) => self.units.len(),
        };
        let (from, to) = (start.min(target), start.max(target));
        let kind = if unit == DeleteUnit::Grapheme { kind } else { EditKind::Other };
        self.replace(from, to, &[], kind)
    }

    fn grapheme_before(&self, offset: usize) -> usize {
        if offset == 0 { 0 } else { offset - prev_char_length(&self.units, offset) }
    }

    fn grapheme_after(&self, offset: usize) -> usize {
        if offset >= self.units.len() { self.units.len() } else { offset + next_char_length(&self.units, offset) }
    }

    /// A próxima fronteira de palavra a partir de `offset`.
    ///
    /// Para trás (e para a frente fora do Windows): pula o que não é palavra e depois a palavra, parando no
    /// começo dela (ou no fim dela, para a frente). No Windows, para a frente, pula a palavra (ou a sequência
    /// do mesmo tipo) e depois os espaços, parando no começo da palavra seguinte.
    fn word_boundary(&self, offset: usize, forward: bool) -> usize {
        let units = &self.units;
        let mut pos = offset;
        let class_at = |p: usize| char_class(char_at(units, p).0);
        let class_before = |p: usize| char_class(char_before(units, p).0);
        if forward {
            if self.platform == Platform::Windows {
                if pos < units.len() && class_at(pos) != CharClass::Space {
                    let class = class_at(pos);
                    while pos < units.len() && class_at(pos) == class {
                        pos += char_at(units, pos).1;
                    }
                }
                while pos < units.len() && class_at(pos) == CharClass::Space {
                    pos += char_at(units, pos).1;
                }
            } else {
                while pos < units.len() && class_at(pos) != CharClass::Word {
                    pos += char_at(units, pos).1;
                }
                while pos < units.len() && class_at(pos) == CharClass::Word {
                    pos += char_at(units, pos).1;
                }
            }
        } else if self.platform == Platform::Windows {
            while pos > 0 && class_before(pos) == CharClass::Space {
                pos -= char_before(units, pos).1;
            }
            if pos > 0 {
                let class = class_before(pos);
                while pos > 0 && class_before(pos) == class {
                    pos -= char_before(units, pos).1;
                }
            }
        } else {
            while pos > 0 && class_before(pos) != CharClass::Word {
                pos -= char_before(units, pos).1;
            }
            while pos > 0 && class_before(pos) == CharClass::Word {
                pos -= char_before(units, pos).1;
            }
        }
        pos
    }

    // ---- movimento e seleção ----------------------------------------------------------------------------

    /// Move o cursor. Com `extend` (Shift) só o `focus` anda; sem ele, uma seleção existente colapsa na
    /// borda do lado do movimento (Esquerda e Direita) e o cursor anda a partir dela, como no Chromium.
    pub fn move_caret(&mut self, movement: Movement, extend: bool) {
        let (start, end) = self.selection();
        let target = match movement {
            Movement::Left if !extend && start != end => start,
            Movement::Right if !extend && start != end => end,
            Movement::Left => self.grapheme_before(self.focus),
            Movement::Right => self.grapheme_after(self.focus),
            Movement::WordLeft => self.word_boundary(self.focus, false),
            Movement::WordRight => self.word_boundary(self.focus, true),
            Movement::Start => 0,
            Movement::End => self.units.len(),
        };
        self.focus = target;
        if !extend {
            self.anchor = target;
        }
        self.open_group = None;
    }

    /// O intervalo da palavra em `offset` (o duplo clique). Letras e dígitos formam uma palavra, uma
    /// sequência de espaços forma um trecho e cada outro caractere é um trecho só. No Windows a palavra
    /// leva os espaços que a seguem, como no Chromium.
    pub fn word_range_at(&self, offset: usize) -> (usize, usize) {
        let units = &self.units;
        if units.is_empty() {
            return (0, 0);
        }
        let offset = self.snap(offset, false);
        let mut start = if offset >= units.len() { units.len() - char_before(units, units.len()).1 } else { offset };
        let class = char_class(char_at(units, start).0);
        let mut end = start + char_at(units, start).1;
        if class != CharClass::Other {
            while start > 0 && char_class(char_before(units, start).0) == class {
                start -= char_before(units, start).1;
            }
            while end < units.len() && char_class(char_at(units, end).0) == class {
                end += char_at(units, end).1;
            }
        }
        if self.platform == Platform::Windows && class == CharClass::Word {
            while end < units.len() && char_class(char_at(units, end).0) == CharClass::Space {
                end += char_at(units, end).1;
            }
        }
        (start, end)
    }

    /// O botão do mouse desceu no deslocamento `offset` (de [`TextField::offset_at_x`]). Um clique coloca o
    /// cursor (com `extend`, estende a seleção), dois selecionam a palavra e três ou mais selecionam tudo. O
    /// arrasto seguinte é de [`TextField::pointer_drag`].
    pub fn pointer_down(&mut self, offset: usize, click_count: u32, extend: bool) {
        let offset = self.snap(offset, false);
        match click_count {
            0 | 1 => {
                self.focus = offset;
                if !extend {
                    self.anchor = offset;
                }
                self.drag = Some(DragMode::Char);
            }
            2 => {
                let (start, end) = self.word_range_at(offset);
                (self.anchor, self.focus) = (start, end);
                self.drag = Some(DragMode::Word { start, end });
            }
            _ => {
                (self.anchor, self.focus) = (0, self.units.len());
                self.drag = Some(DragMode::All);
            }
        }
        self.open_group = None;
    }

    /// O mouse andou até `offset` com o botão baixo.
    pub fn pointer_drag(&mut self, offset: usize) {
        let offset = self.snap(offset, false);
        match self.drag {
            Some(DragMode::Char) => self.focus = offset,
            Some(DragMode::Word { start, end }) => {
                let (word_start, word_end) = self.word_range_at(offset);
                if offset < start {
                    (self.anchor, self.focus) = (end, word_start);
                } else {
                    (self.anchor, self.focus) = (start, word_end.max(end));
                }
            }
            Some(DragMode::All) | None => {}
        }
        self.open_group = None;
    }

    /// O botão subiu: o arrasto acabou.
    pub fn pointer_up(&mut self) {
        self.drag = None;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    // ---- área de transferência ---------------------------------------------------------------------------

    /// Copia a seleção. Um campo de senha não copia. Devolve se copiou.
    pub fn copy(&self, clipboard: &dyn ClipboardProvider) -> bool {
        if self.password || !self.has_selection() {
            return false;
        }
        clipboard.write_text(&self.selected_text());
        true
    }

    /// Recorta a seleção: devolve se o texto mudou.
    pub fn cut(&mut self, clipboard: &dyn ClipboardProvider) -> bool {
        self.copy(clipboard) && self.delete(DeleteUnit::Grapheme, false)
    }

    /// Cola no lugar da seleção: devolve se o texto mudou.
    pub fn paste(&mut self, clipboard: &dyn ClipboardProvider) -> bool {
        clipboard.read_text().is_some_and(|text| self.insert_with(&text, EditKind::Other))
    }

    // ---- desfazer e refazer --------------------------------------------------------------------------------

    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else { return false };
        self.units.splice(edit.start..edit.start + edit.inserted.len(), edit.removed.iter().copied());
        self.text = from_units(&self.units);
        (self.anchor, self.focus) = edit.before;
        self.redo.push(edit);
        self.open_group = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else { return false };
        self.units.splice(edit.start..edit.start + edit.removed.len(), edit.inserted.iter().copied());
        self.text = from_units(&self.units);
        (self.anchor, self.focus) = edit.after;
        self.undo.push(edit);
        self.open_group = None;
        true
    }

    // ---- teclado ----------------------------------------------------------------------------------------------

    /// A escala de um movimento ou apagamento pelos modificadores (sem contar o Shift): grafema, palavra
    /// (Ctrl, ou Option no macOS) ou linha (Cmd, só no macOS). Outras combinações não são do campo.
    fn scale(&self, key: &StandardKeyboardEvent) -> Option<DeleteUnit> {
        match (self.platform == Platform::Mac, key.ctrl_key, key.alt_key, key.meta_key) {
            (_, false, false, false) => Some(DeleteUnit::Grapheme),
            (false, true, false, false) | (true, false, true, false) => Some(DeleteUnit::Word),
            (true, false, false, true) => Some(DeleteUnit::Line),
            _ => None,
        }
    }

    /// A tabela de atalhos do `<input>` do Chromium (a ação padrão de um `keydown`). Teclas que só digitam
    /// não são tratadas (o texto vem como `TextInput`), nem Enter, Escape, Tab e as de função.
    pub fn handle_key(&mut self, key: &StandardKeyboardEvent, clipboard: &dyn ClipboardProvider) -> KeyOutcome {
        let mac = self.platform == Platform::Mac;
        let shift = key.shift_key;
        let code = key.key_code;
        let handled = |changed: bool| KeyOutcome { handled: true, changed };
        let scale = self.scale(key);
        if matches!(code, KeyCode::LeftArrow | KeyCode::RightArrow | KeyCode::Home | KeyCode::End | KeyCode::UpArrow | KeyCode::DownArrow) {
            let forward = matches!(code, KeyCode::RightArrow | KeyCode::End | KeyCode::DownArrow);
            // Cima e baixo só andam sem modificador (ou com Cmd no macOS); as outras teclas aceitam qualquer escala.
            let movement = match (code, scale) {
                (KeyCode::LeftArrow | KeyCode::RightArrow, Some(DeleteUnit::Grapheme)) => if forward { Movement::Right } else { Movement::Left },
                (KeyCode::LeftArrow | KeyCode::RightArrow, Some(DeleteUnit::Word)) => if forward { Movement::WordRight } else { Movement::WordLeft },
                (KeyCode::UpArrow | KeyCode::DownArrow, Some(DeleteUnit::Word)) => return KeyOutcome::default(),
                (_, Some(_)) => if forward { Movement::End } else { Movement::Start },
                (_, None) => return KeyOutcome::default(),
            };
            self.move_caret(movement, shift);
            return handled(false);
        }
        if matches!(code, KeyCode::Backspace | KeyCode::Delete) {
            let forward = code == KeyCode::Delete;
            if !mac && forward && shift && scale == Some(DeleteUnit::Grapheme) {
                return handled(self.cut(clipboard));
            }
            return scale.map_or_else(KeyOutcome::default, |unit| handled(self.delete(unit, forward)));
        }
        if code == KeyCode::Insert && !mac {
            return match (key.ctrl_key, shift, key.alt_key || key.meta_key) {
                (true, false, false) => {
                    self.copy(clipboard);
                    handled(false)
                }
                (false, true, false) => handled(self.paste(clipboard)),
                _ => KeyOutcome::default(),
            };
        }
        let primary_only = if mac { key.meta_key && !key.ctrl_key && !key.alt_key } else { key.ctrl_key && !key.alt_key && !key.meta_key };
        if primary_only {
            match (code, shift) {
                (KeyCode::KeyA, false) => {
                    self.select_all();
                    return handled(false);
                }
                (KeyCode::KeyC, false) => {
                    self.copy(clipboard);
                    return handled(false);
                }
                (KeyCode::KeyX, false) => return handled(self.cut(clipboard)),
                (KeyCode::KeyV, false) => return handled(self.paste(clipboard)),
                (KeyCode::KeyZ, false) => return handled(self.undo()),
                (KeyCode::KeyZ, true) => return handled(self.redo()),
                (KeyCode::KeyY, false) if !mac => return handled(self.redo()),
                _ => {}
            }
        }
        if mac && key.ctrl_key && !key.alt_key && !key.meta_key && !shift {
            // Os atalhos de Emacs do campo de texto do macOS.
            let movement = match code {
                KeyCode::KeyA => Some(Movement::Start),
                KeyCode::KeyE => Some(Movement::End),
                KeyCode::KeyB => Some(Movement::Left),
                KeyCode::KeyF => Some(Movement::Right),
                _ => None,
            };
            if let Some(movement) = movement {
                self.move_caret(movement, false);
                return handled(false);
            }
            return match code {
                KeyCode::KeyH => handled(self.delete(DeleteUnit::Grapheme, false)),
                KeyCode::KeyD => handled(self.delete(DeleteUnit::Grapheme, true)),
                KeyCode::KeyK => handled(self.delete(DeleteUnit::Line, true)),
                _ => KeyOutcome::default(),
            };
        }
        KeyOutcome::default()
    }

    // ---- geometria -----------------------------------------------------------------------------------------------

    /// O texto como é pintado: igual ao valor, ou uma máscara por caractere num campo de senha.
    pub fn display_text(&self) -> Cow<'_, str> {
        if self.password { Cow::Owned(self.text.chars().map(|_| PASSWORD_MASK).collect()) } else { Cow::Borrowed(&self.text) }
    }

    /// O texto pintado até `offset` (a máscara conta um caractere por ponto de código).
    fn display_prefix(&self, offset: usize) -> String {
        let prefix = from_units(&self.units[..offset.min(self.units.len())]);
        if self.password { prefix.chars().map(|_| PASSWORD_MASK).collect() } else { prefix }
    }

    /// A largura do texto inteiro.
    pub fn content_width(&self, style: &TextStyle) -> f64 {
        measure_text(&self.display_text(), style)
    }

    /// A posição x, a partir do início do texto, do deslocamento `offset`.
    pub fn x_at_offset(&self, offset: usize, style: &TextStyle) -> f64 {
        measure_text(&self.display_prefix(offset), style)
    }

    /// O deslocamento do cursor mais próximo de `x`, a partir do início do texto (o clique): o cursor vai
    /// para a fronteira de grafema mais próxima, e `x` antes do texto ou depois dele vale o início ou o fim.
    pub fn offset_at_x(&self, x: f64, style: &TextStyle) -> usize {
        let mut boundaries = vec![0usize];
        let mut pos = 0;
        while pos < self.units.len() {
            pos += next_char_length(&self.units, pos).max(1);
            boundaries.push(pos.min(self.units.len()));
        }
        if x <= 0.0 {
            return 0;
        }
        // O primeiro índice da lista cujo x passa de `x`; depois escolhe o mais próximo dos dois lados.
        let (mut lo, mut hi) = (0usize, boundaries.len() - 1);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.x_at_offset(boundaries[mid], style) < x {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return 0;
        }
        let (left, right) = (boundaries[lo - 1], boundaries[lo]);
        if x - self.x_at_offset(left, style) < self.x_at_offset(right, style) - x { left } else { right }
    }

    /// O trecho `[x0, x1]` da seleção, a partir do início do texto.
    pub fn selection_x_range(&self, style: &TextStyle) -> Option<(f64, f64)> {
        let (start, end) = self.selection();
        (start != end).then(|| (self.x_at_offset(start, style), self.x_at_offset(end, style)))
    }

    pub fn scroll_left(&self) -> f64 {
        self.scroll_left
    }

    /// `input.scrollLeft = x`, limitado ao que existe de texto fora da janela de largura `viewport_width`.
    pub fn set_scroll_left(&mut self, x: f64, viewport_width: f64, style: &TextStyle) {
        let max = (self.content_width(style) - viewport_width).max(0.0);
        self.scroll_left = x.clamp(0.0, max);
    }

    /// Rola o mínimo para o cursor ficar dentro da janela de largura `viewport_width`. Com o texto menor
    /// que a janela não há rolagem.
    pub fn reveal_caret(&mut self, viewport_width: f64, style: &TextStyle) {
        let caret_x = self.x_at_offset(self.focus, style);
        let mut scroll = self.scroll_left;
        if caret_x < scroll {
            scroll = caret_x;
        } else if caret_x > scroll + viewport_width {
            scroll = caret_x - viewport_width;
        }
        self.set_scroll_left(scroll, viewport_width, style);
    }
}
