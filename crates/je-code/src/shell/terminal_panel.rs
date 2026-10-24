//! O terminal do painel inferior (`vs/workbench/contrib/terminal`): a grade do `je-terminal` pintada na superfície
//! `Content::Host(1)`, a codificação das teclas para o pty e as cores padrão do tema escuro do VS Code
//! (`terminal.ansi*` sem definição no tema 2026 caem nos valores de `terminalColorRegistry.ts`).

use je_editor_view::layout::{DEFAULT_FONT_FAMILY, FontMetrics};
use je_editor_view::EditorKey;
use je_render::kurbo::Rect;
use je_render::{DisplayList, Rgba};
use je_terminal::{CursorKind, Palette, Rgb, Snapshot, Terminal};
use je_text::{TextLine, with_text_system};
use je_theme::ColorTheme;
use je_ui::Modifiers;

/// `terminal.integrated.fontSize` padrão e a altura de linha (`lineHeight` 1 com o `Droid Sans Mono` de 14 px,
/// que o xterm.js arredonda para 17 px no oráculo).
pub const FONT_SIZE: f32 = 14.0;
pub const ROW_HEIGHT: f64 = 17.0;
/// O recuo do xterm dentro do painel (`.xterm` com `padding: 8px 0 8px 20px`) e a barra de rolagem de 13 px.
pub const PADDING_LEFT: f64 = 20.0;
pub const PADDING_TOP: f64 = 8.0;
pub const SCROLLBAR: f64 = 13.0;

/// As cores ANSI padrão do tema escuro (`terminalColorRegistry.ts`).
const ANSI_DARK: [Rgb; 16] = [
    (0x00, 0x00, 0x00),
    (0xcd, 0x31, 0x31),
    (0x0d, 0xbc, 0x79),
    (0xe5, 0xe5, 0x10),
    (0x24, 0x72, 0xc8),
    (0xbc, 0x3f, 0xbc),
    (0x11, 0xa8, 0xcd),
    (0xe5, 0xe5, 0xe5),
    (0x66, 0x66, 0x66),
    (0xf1, 0x4c, 0x4c),
    (0x23, 0xd1, 0x8b),
    (0xf5, 0xf5, 0x43),
    (0x3b, 0x8e, 0xea),
    (0xd6, 0x70, 0xd6),
    (0x29, 0xb8, 0xdb),
    (0xe5, 0xe5, 0xe5),
];
const ANSI_IDS: [&str; 16] = [
    "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White", "BrightBlack", "BrightRed", "BrightGreen", "BrightYellow", "BrightBlue", "BrightMagenta", "BrightCyan", "BrightWhite",
];

fn rgb(color: Rgba) -> Rgb {
    (color.r, color.g, color.b)
}

/// A paleta do tema (`terminal.background`, `terminal.foreground`, `terminal.ansi*`).
pub fn palette(theme: &ColorTheme) -> Palette {
    let get = |id: &str, fallback: Rgb| theme.get_color(id).map_or(fallback, rgb);
    let mut ansi = ANSI_DARK;
    for (slot, name) in ansi.iter_mut().zip(ANSI_IDS) {
        *slot = get(&format!("terminal.ansi{name}"), *slot);
    }
    Palette { foreground: get("terminal.foreground", (0xcc, 0xcc, 0xcc)), background: get("terminal.background", (0x18, 0x18, 0x18)), cursor: get("terminalCursor.foreground", (0xcc, 0xcc, 0xcc)), ansi }
}

/// A largura de uma célula em px lógicos (o avanço do `Droid Sans Mono` de 14 px).
pub fn cell_width() -> f64 {
    FontMetrics::measure(DEFAULT_FONT_FAMILY, FONT_SIZE, ROW_HEIGHT).char_width
}

/// Quantas colunas e linhas cabem numa área de `width` x `height` px do painel.
pub fn grid_size(width: f64, height: f64) -> (usize, usize) {
    let columns = ((width - PADDING_LEFT - SCROLLBAR) / cell_width()).floor().max(2.0) as usize;
    let rows = ((height - 2.0 * PADDING_TOP) / ROW_HEIGHT).floor().max(1.0) as usize;
    (columns, rows)
}

fn color(c: Rgb) -> Rgba {
    Rgba::opaque(c.0, c.1, c.2)
}

/// A linha de base de uma linha de texto numa caixa de `box_height` que começa em `top`, no pixel (como o editor).
fn baseline(top: f64, box_height: f64, line: &TextLine) -> f64 {
    (top.round() + ((box_height - f64::from(line.line_height)) / 2.0).floor() + f64::from(line.baseline)).round()
}

/// Pinta o terminal em `area` (px lógicos da janela): fundo, células, cursor.
pub fn paint(list: &mut DisplayList, area: Rect, scale: f64, snapshot: &Snapshot, palette: &Palette, focused: bool, cursor_on: bool) {
    let metrics = FontMetrics::measure(DEFAULT_FONT_FAMILY, FONT_SIZE, ROW_HEIGHT);
    let cw = metrics.char_width;
    let dev = |v: f64| (v * scale).round();
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| Rect::new(dev(x0), dev(y0), dev(x1), dev(y1));
    list.fill_rect(rect(area.x0, area.y0, area.x1, area.y1), color(palette.background));
    let (left, top) = (area.x0 + PADDING_LEFT, area.y0 + PADDING_TOP);
    for (row, cells) in snapshot.rows.iter().enumerate() {
        let y = top + row as f64 * ROW_HEIGHT;
        // Fundos: um retângulo por trecho contíguo da mesma cor.
        let mut col = 0;
        while col < cells.len() {
            let Some(bg) = cells[col].bg else {
                col += 1;
                continue;
            };
            let start = col;
            while col < cells.len() && cells[col].bg == Some(bg) {
                col += 1;
            }
            list.fill_rect(rect(left + start as f64 * cw, y, left + col as f64 * cw, y + ROW_HEIGHT), color(bg));
        }
        // Texto: trechos contíguos com o mesmo estilo e cor, desenhados juntos.
        let mut col = 0;
        while col < cells.len() {
            let first = cells[col];
            if first.ch == ' ' || first.ch == '\0' {
                col += 1;
                continue;
            }
            let start = col;
            let mut text = String::new();
            while col < cells.len() && cells[col].ch != ' ' && cells[col].ch != '\0' && (cells[col].fg, cells[col].bold, cells[col].italic) == (first.fg, first.bold, first.italic) {
                text.push(cells[col].ch);
                col += if cells[col].wide { 2 } else { 1 };
            }
            let style = metrics.style(scale, first.bold, first.italic);
            let line = with_text_system(|t| t.line(&text, &style));
            let x_dev = dev(left + start as f64 * cw) as f32;
            let base = baseline(y * scale, ROW_HEIGHT * scale, &line);
            line.paint(list, x_dev, (base - f64::from(line.baseline)) as f32, color(first.fg));
            if first.underline || first.strikeout {
                let width = f64::from(line.width);
                if first.underline {
                    list.fill_rect(Rect::new(f64::from(x_dev), base + scale, f64::from(x_dev) + width, base + 2.0 * scale), color(first.fg));
                }
                if first.strikeout {
                    list.fill_rect(Rect::new(f64::from(x_dev), base - 4.0 * scale, f64::from(x_dev) + width, base - 3.0 * scale), color(first.fg));
                }
            }
        }
    }
    if let Some((col, row, kind)) = snapshot.cursor
        && (!focused || cursor_on)
    {
        let x = left + col as f64 * cw;
        let y = top + row as f64 * ROW_HEIGHT;
        let cursor_color = color(palette.cursor);
        match (focused, kind) {
            (false, _) | (_, CursorKind::Hollow) => {
                list.stroke(je_render::Shape::Rect(rect(x, y, x + cw, y + ROW_HEIGHT).inset(-0.5 * scale)), scale, cursor_color);
            }
            (_, CursorKind::Block) => {
                list.fill_rect(rect(x, y, x + cw, y + ROW_HEIGHT), cursor_color);
                // O caractere sob o bloco sai na cor do fundo.
                if let Some(cell) = snapshot.rows.get(row).and_then(|r| r.get(col))
                    && cell.ch != ' '
                {
                    let style = metrics.style(scale, cell.bold, cell.italic);
                    let line = with_text_system(|t| t.line(&cell.ch.to_string(), &style));
                    let base = baseline(y * scale, ROW_HEIGHT * scale, &line);
                    line.paint(list, dev(x) as f32, (base - f64::from(line.baseline)) as f32, color(palette.background));
                }
            }
            (_, CursorKind::Underline) => list.fill_rect(rect(x, y + ROW_HEIGHT - 2.0, x + cw, y + ROW_HEIGHT), cursor_color),
            (_, CursorKind::Beam) => list.fill_rect(rect(x, y, x + 2.0, y + ROW_HEIGHT), cursor_color),
        }
    }
}

/// Os bytes que o pty recebe para uma tecla (`xterm`): `key` é a tecla de edição, `text` o texto digitado.
pub fn encode_key(terminal: &Terminal, key: Option<EditorKey>, key_code: je_base::key_codes::KeyCode, text: Option<&str>, mods: Modifiers) -> Option<Vec<u8>> {
    use je_base::key_codes::KeyCode as K;
    let ctrl = mods.ctrl || mods.meta;
    // Parâmetro de modificador do xterm: 1 + shift(1) + alt(2) + ctrl(4).
    let modifier = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(ctrl);
    let csi = |final_byte: char| -> Vec<u8> {
        if modifier > 1 { format!("\x1b[1;{modifier}{final_byte}").into_bytes() } else { format!("\x1b[{final_byte}").into_bytes() }
    };
    let ss3_or_csi = |final_byte: char| -> Vec<u8> {
        if modifier == 1 && terminal.app_cursor() { format!("\x1bO{final_byte}").into_bytes() } else { csi(final_byte) }
    };
    let tilde = |n: u8| -> Vec<u8> { if modifier > 1 { format!("\x1b[{n};{modifier}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() } };
    let fkey = |n: u8| -> Vec<u8> {
        match n {
            1..=4 => {
                let letter = (b'P' + n - 1) as char;
                if modifier > 1 { format!("\x1b[1;{modifier}{letter}").into_bytes() } else { format!("\x1bO{letter}").into_bytes() }
            }
            5 => tilde(15),
            6 => tilde(17),
            7 => tilde(18),
            8 => tilde(19),
            9 => tilde(20),
            10 => tilde(21),
            11 => tilde(23),
            _ => tilde(24),
        }
    };
    let bytes = match key {
        Some(EditorKey::Enter) => b"\r".to_vec(),
        Some(EditorKey::Backspace) => if ctrl { vec![0x08] } else if mods.alt { vec![0x1b, 0x7f] } else { vec![0x7f] },
        Some(EditorKey::Tab) => if mods.shift { b"\x1b[Z".to_vec() } else { b"\t".to_vec() },
        Some(EditorKey::Escape) => vec![0x1b],
        Some(EditorKey::Up) => ss3_or_csi('A'),
        Some(EditorKey::Down) => ss3_or_csi('B'),
        Some(EditorKey::Right) => ss3_or_csi('C'),
        Some(EditorKey::Left) => ss3_or_csi('D'),
        Some(EditorKey::Home) => ss3_or_csi('H'),
        Some(EditorKey::End) => ss3_or_csi('F'),
        Some(EditorKey::PageUp) => tilde(5),
        Some(EditorKey::PageDown) => tilde(6),
        Some(EditorKey::Delete) => tilde(3),
        Some(EditorKey::Char(c)) if ctrl && !mods.alt => {
            // Ctrl+letra vira o caractere de controle; o resto cai no texto.
            match c {
                'a'..='z' => vec![c as u8 - b'a' + 1],
                '[' => vec![0x1b],
                '\\' => vec![0x1c],
                ']' => vec![0x1d],
                ' ' | '@' => vec![0],
                _ => return None,
            }
        }
        _ => {
            let number = match key_code {
                K::F1 => 1,
                K::F2 => 2,
                K::F3 => 3,
                K::F4 => 4,
                K::F5 => 5,
                K::F6 => 6,
                K::F7 => 7,
                K::F8 => 8,
                K::F9 => 9,
                K::F10 => 10,
                K::F11 => 11,
                K::F12 => 12,
                _ => 0,
            };
            if number > 0 {
                return Some(fkey(number));
            }
            let text = text.filter(|t| !t.is_empty() && !t.chars().all(char::is_control))?;
            if ctrl && !mods.alt {
                return None;
            }
            let mut out = Vec::new();
            if mods.alt && !ctrl {
                out.push(0x1b);
            }
            out.extend_from_slice(text.as_bytes());
            out
        }
    };
    Some(bytes)
}
