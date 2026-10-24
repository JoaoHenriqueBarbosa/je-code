//! Porte de `src/vs/workbench/contrib/debug/browser/debugANSIHandling.ts`: a conversão do texto com sequências de escape
//! ANSI do console de depuração em trechos estilizados (`handleANSIOutput`, `appendStylizedStringToContainer`,
//! `calcANSI8bitColor`).
//!
//! Notas de tradução:
//! - **DOM.** O `HTMLSpanElement` é o [`Element`] de `link_detector.rs`. `container.className = ...` grava o atributo
//!   `class` (mesmo vazio) e `container.style.color`, `backgroundColor` e `textDecorationColor` gravam as declarações no
//!   atributo `style` (`color: ...; background-color: ...; text-decoration-color: ...;`, na ordem em que foram gravadas);
//!   [`style_property`] lê uma declaração de volta.
//! - **UTF-16.** O texto é percorrido em unidades UTF-16 (`charAt`, `charCodeAt`, `length`), como no original; o
//!   deslocamento passado ao `appendStylizedStringToContainer` e os realces (`IHighlight`) também.
//! - **Realces.** `h.start - offset` pode ficar negativo em JavaScript; o `IHighlight` do porte é `usize`, então a
//!   subtração satura em zero (um realce inteiro antes do trecho fica vazio, e o `linkify` ignora o vazio).
//! - **Cores.** `RGBA` é o [`Srgba`] do je-theme e `Color.Format.CSS.formatRGB(new Color(rgba))` o `format_rgb`. A cor do
//!   tema (`string`) é o nome da variável CSS, e [`AnsiColor`] guarda as duas formas. `ansiColorIdentifiers` são os 16 ids
//!   `terminal.ansi*` na ordem do índice.
//! - O `registerThemingParticipant` do final do arquivo (regras CSS por área da janela) NÃO foi portado: é estilo de UI
//!   web, e a pintura nativa lê as variáveis de cor pelo tema.

use std::rc::Rc;

use je_platform::workspace::WorkspaceFolder;
use je_theme::color::{Color, Srgba, format::css as css_format};
use je_widgets::highlighted_label::IHighlight;
use zyarr::RegExp;

use super::link_detector::{DebugLinkHoverBehaviorTypeData, Element, ILinkDetector, LinkNode, LinkifyOptions};

/// `RGBA | string`: a cor RGBA ou o nome da variável CSS do tema.
#[derive(Clone, Debug, PartialEq)]
pub enum AnsiColor {
    Rgba(Srgba),
    Var(String),
}

/// `ansiColorIdentifiers`, na ordem do índice da cor (o de `terminalColorRegistry.ts`).
pub use crate::contrib::terminal::common::terminal_color_registry::ANSI_COLOR_IDENTIFIERS;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorType {
    Foreground,
    Background,
    Underline,
}

impl ColorType {
    fn style_name(self) -> &'static str {
        match self {
            ColorType::Foreground => "code-foreground-colored",
            ColorType::Background => "code-background-colored",
            ColorType::Underline => "code-underline-colored",
        }
    }
}

/// O estado de `handleANSIOutput` (as variáveis locais e as funções internas do original).
struct AnsiState {
    style_names: Vec<String>,
    custom_fg_color: Option<AnsiColor>,
    custom_bg_color: Option<AnsiColor>,
    custom_underline_color: Option<AnsiColor>,
    colors_inverted: bool,
}

impl AnsiState {
    fn remove(&mut self, names: &[&str]) {
        self.style_names.retain(|style| !names.contains(&style.as_str()));
    }

    /// `styleNames = styleNames.filter(style => style !== name); styleNames.push(name)`.
    fn set(&mut self, name: &str) {
        self.remove(&[name]);
        self.style_names.push(name.to_owned());
    }

    /// Change the foreground or background color by clearing the current color and adding the new one. `None` clears the
    /// current color without adding a new one.
    fn change_color(&mut self, color_type: ColorType, color: Option<AnsiColor>) {
        match color_type {
            ColorType::Foreground => self.custom_fg_color = color.clone(),
            ColorType::Background => self.custom_bg_color = color.clone(),
            ColorType::Underline => self.custom_underline_color = color.clone(),
        }
        self.remove(&[color_type.style_name()]);
        if color.is_some() {
            self.style_names.push(color_type.style_name().to_owned());
        }
    }

    /// Swap foreground and background colors. Used for color inversion.
    fn reverse_foreground_and_background_colors(&mut self) {
        let old_fg_color = self.custom_fg_color.clone();
        let bg = self.custom_bg_color.clone();
        self.change_color(ColorType::Foreground, bg);
        self.change_color(ColorType::Background, old_fg_color);
    }

    /// Calculate and set basic ANSI formatting. Supports ON/OFF of bold, italic, underline, double underline,
    /// crossed-out/strikethrough, overline, dim, blink, rapid blink, reverse/invert video, hidden, superscript, subscript
    /// and alternate font codes, clearing/resetting of foreground, background and underline colors, setting normal
    /// foreground and background colors, and bright foreground and background colors. Not to be used for codes containing
    /// advanced colors. Will ignore invalid codes.
    fn set_basic_formatters(&mut self, style_codes: &[i64]) {
        for &code in style_codes {
            match code {
                0 => {
                    // reset (everything)
                    self.style_names.clear();
                    self.custom_fg_color = None;
                    self.custom_bg_color = None;
                }
                1 => self.set("code-bold"),
                2 => self.set("code-dim"),
                3 => self.set("code-italic"),
                4 => {
                    // underline
                    self.remove(&["code-underline", "code-double-underline"]);
                    self.style_names.push("code-underline".to_owned());
                }
                5 => self.set("code-blink"),
                6 => self.set("code-rapid-blink"),
                7 => {
                    // invert foreground and background
                    if !self.colors_inverted {
                        self.colors_inverted = true;
                        self.reverse_foreground_and_background_colors();
                    }
                }
                8 => self.set("code-hidden"),
                9 => self.set("code-strike-through"),
                10 => {
                    // normal default font
                    self.style_names.retain(|style| !style.starts_with("code-font"));
                }
                11..=20 => {
                    // font codes (and 20 is 'blackletter' font code)
                    self.style_names.retain(|style| !style.starts_with("code-font"));
                    self.style_names.push(format!("code-font-{}", code - 10));
                }
                21 => {
                    // double underline
                    self.remove(&["code-underline", "code-double-underline"]);
                    self.style_names.push("code-double-underline".to_owned());
                }
                22 => self.remove(&["code-bold", "code-dim"]), // normal intensity (bold off and dim off)
                23 => self.remove(&["code-italic", "code-font-10"]), // Neither italic or blackletter (font 10)
                24 => self.remove(&["code-underline", "code-double-underline"]), // not underlined (Neither singly nor doubly underlined)
                25 => self.remove(&["code-blink", "code-rapid-blink"]), // not blinking
                27 => {
                    // not reversed/inverted
                    if self.colors_inverted {
                        self.colors_inverted = false;
                        self.reverse_foreground_and_background_colors();
                    }
                }
                28 => self.remove(&["code-hidden"]),         // not hidden (reveal)
                29 => self.remove(&["code-strike-through"]), // not crossed-out
                53 => self.set("code-overline"),             // overlined
                55 => self.remove(&["code-overline"]),       // not overlined
                39 => self.change_color(ColorType::Foreground, None), // default foreground color
                49 => self.change_color(ColorType::Background, None), // default background color
                59 => self.change_color(ColorType::Underline, None), // default underline color
                73 => {
                    // superscript
                    self.remove(&["code-superscript", "code-subscript"]);
                    self.style_names.push("code-superscript".to_owned());
                }
                74 => {
                    // subscript
                    self.remove(&["code-superscript", "code-subscript"]);
                    self.style_names.push("code-subscript".to_owned());
                }
                75 => self.remove(&["code-superscript", "code-subscript"]), // neither superscript or subscript
                _ => self.set_basic_color(code),
            }
        }
    }

    /// Calculate and set styling for complicated 24-bit ANSI color codes.
    fn set_24_bit_color(&mut self, style_codes: &[i64], color_type: ColorType) {
        if style_codes.len() >= 5 && style_codes[2..5].iter().all(|c| (0..=255).contains(c)) {
            let custom_color = Srgba::opaque(style_codes[2] as f64, style_codes[3] as f64, style_codes[4] as f64);
            self.change_color(color_type, Some(AnsiColor::Rgba(custom_color)));
        }
    }

    /// Calculate and set styling for advanced 8-bit ANSI color codes.
    fn set_8_bit_color(&mut self, style_codes: &[i64], color_type: ColorType) {
        let Some(&color_number) = style_codes.get(2) else {
            return;
        };
        if let Some(color) = calc_ansi_8bit_color(color_number as f64) {
            self.change_color(color_type, Some(AnsiColor::Rgba(color)));
        } else if (0..=15).contains(&color_number) {
            if color_type == ColorType::Underline {
                // for underline colors we just decode the 0-15 color number to theme color, set and return
                let color_name = ANSI_COLOR_IDENTIFIERS[color_number as usize];
                self.change_color(color_type, Some(AnsiColor::Var(format!("--vscode-debug-ansi-{color_name}"))));
                return;
            }

            // Need to map to one of the four basic color ranges (30-37, 90-97, 40-47, 100-107)
            let mut color_number = color_number + 30;
            if color_number >= 38 {
                // Bright colors
                color_number += 52;
            }
            if color_type == ColorType::Background {
                color_number += 10;
            }
            self.set_basic_color(color_number);
        }
    }

    /// Calculate and set styling for basic bright and dark ANSI color codes. Uses theme colors if available.
    /// Automatically distinguishes between foreground and background colors; does not support color-clearing codes 39
    /// and 49.
    fn set_basic_color(&mut self, style_code: i64) {
        let (color_type, color_index) = if (30..=37).contains(&style_code) {
            (ColorType::Foreground, style_code - 30)
        } else if (90..=97).contains(&style_code) {
            (ColorType::Foreground, (style_code - 90) + 8) // High-intensity (bright)
        } else if (40..=47).contains(&style_code) {
            (ColorType::Background, style_code - 40)
        } else if (100..=107).contains(&style_code) {
            (ColorType::Background, (style_code - 100) + 8) // High-intensity (bright)
        } else {
            return;
        };

        let color_name = ANSI_COLOR_IDENTIFIERS[color_index as usize];
        self.change_color(color_type, Some(AnsiColor::Var(format!("--vscode-debug-ansi-{}", color_name.replace('.', "-")))));
    }
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn from_units(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

fn is_sequence_terminator(unit: u16) -> bool {
    matches!(char::from_u32(unit as u32), Some('A' | 'B' | 'C' | 'D' | 'H' | 'I' | 'J' | 'K' | 'f' | 'h' | 'm' | 'p' | 's' | 'u'))
}

/// `handleANSIOutput(text, linkDetector, workspaceFolder, highlights, hoverBehavior)`: the {@link HTMLSpanElement} that
/// contains the potentially stylized text.
pub fn handle_ansi_output(
    text: &str,
    link_detector: &dyn ILinkDetector,
    workspace_folder: Option<&WorkspaceFolder>,
    highlights: Option<&[IHighlight]>,
    hover_behavior: &DebugLinkHoverBehaviorTypeData,
) -> Rc<Element> {
    let root = Element::new("span");
    let text = units(text);
    let text_length = text.len();

    let mut state = AnsiState { style_names: Vec::new(), custom_fg_color: None, custom_bg_color: None, custom_underline_color: None, colors_inverted: false };

    let sequence_regex = RegExp::from_str(
        r"^(?:[34][0-8]|9[0-7]|10[0-7]|[0-9]|2[1-5,7-9]|[34]9|5[8,9]|1[0-9])(?:;[349][0-7]|10[0-7]|[013]|[245]|[34]9)?(?:;[012]?[0-9]?[0-9])*;?m$",
        "",
    )
    .expect("a expressão das sequências de estilo é válida");

    let mut current_pos: usize = 0;
    let mut unprinted_chars: usize = 0;
    let mut buffer: Vec<u16> = Vec::new();

    while current_pos < text_length {
        let mut sequence_found = false;

        // Potentially an ANSI escape sequence.
        // See http://ascii-table.com/ansi-escape-sequences.php & https://en.wikipedia.org/wiki/ANSI_escape_code
        if text[current_pos] == 27 && text.get(current_pos + 1) == Some(&(b'[' as u16)) {
            let start_pos = current_pos;
            current_pos += 2; // Ignore 'Esc[' as it's in every sequence.

            let mut ansi_sequence: Vec<u16> = Vec::new();

            while current_pos < text_length {
                let unit = text[current_pos];
                ansi_sequence.push(unit);
                current_pos += 1;

                // Look for a known sequence terminating character.
                if is_sequence_terminator(unit) {
                    sequence_found = true;
                    break;
                }
            }

            if sequence_found {
                unprinted_chars += 2 + ansi_sequence.len();

                // Flush buffer with previous styles.
                append_stylized_string_to_container(
                    &root,
                    &from_units(&buffer),
                    &state.style_names,
                    link_detector,
                    workspace_folder,
                    state.custom_fg_color.as_ref(),
                    state.custom_bg_color.as_ref(),
                    state.custom_underline_color.as_ref(),
                    highlights,
                    (current_pos - buffer.len() - unprinted_chars) as i64,
                    hover_behavior,
                );

                buffer.clear();

                // Certain ranges that are matched here do not contain real graphics rendition sequences. For the sake of
                // having a simpler expression, they have been included anyway.
                if sequence_regex.exec(&ansi_sequence, 0).is_some() {
                    let sequence = from_units(&ansi_sequence);
                    let style_codes: Vec<i64> = sequence[..sequence.len() - 1] // Remove final 'm' character.
                        .split(';') // Separate style codes.
                        .filter(|elem| !elem.is_empty()) // Filter empty elems as '34;m' -> ['34', ''].
                        .map(|elem| elem.parse::<i64>().unwrap_or(0)) // Convert to numbers.
                        .collect();

                    if matches!(style_codes.first(), Some(38) | Some(48) | Some(58)) {
                        // Advanced color code - can't be combined with formatting codes like simple colors can
                        // Ignores invalid colors and additional info beyond what is necessary
                        let color_type = match style_codes[0] {
                            38 => ColorType::Foreground,
                            48 => ColorType::Background,
                            _ => ColorType::Underline,
                        };

                        match style_codes.get(1) {
                            Some(5) => state.set_8_bit_color(&style_codes, color_type),
                            Some(2) => state.set_24_bit_color(&style_codes, color_type),
                            _ => {}
                        }
                    } else {
                        state.set_basic_formatters(&style_codes);
                    }
                }
                // else: Unsupported sequence so simply hide it.
            } else {
                current_pos = start_pos;
            }
        }

        if !sequence_found {
            buffer.push(text[current_pos]);
            current_pos += 1;
        }
    }

    // Flush remaining text buffer if not empty.
    if !buffer.is_empty() {
        append_stylized_string_to_container(
            &root,
            &from_units(&buffer),
            &state.style_names,
            link_detector,
            workspace_folder,
            state.custom_fg_color.as_ref(),
            state.custom_bg_color.as_ref(),
            state.custom_underline_color.as_ref(),
            highlights,
            (current_pos - buffer.len()) as i64,
            hover_behavior,
        );
    }

    root
}

/// `container.style[property] = value`: grava a declaração no atributo `style`, trocando a que já existe (um valor vazio
/// remove a declaração, como o DOM).
pub fn set_style_property(element: &Element, property: &str, value: &str) {
    let mut declarations: Vec<(String, String)> = element
        .get_attribute("style")
        .map(|style| {
            style
                .split(';')
                .filter_map(|declaration| declaration.split_once(':').map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned())))
                .collect()
        })
        .unwrap_or_default();
    if value.is_empty() {
        declarations.retain(|(name, _)| name != property);
    } else {
        match declarations.iter_mut().find(|(name, _)| name == property) {
            Some(entry) => entry.1 = value.to_owned(),
            None => declarations.push((property.to_owned(), value.to_owned())),
        }
    }
    let style: String = declarations.iter().map(|(name, value)| format!("{name}: {value}; ")).collect();
    element.set_attribute("style", style.trim_end());
}

/// `element.style.<property>`: o valor da declaração (texto vazio se não existir), com o nome em CSS
/// (`color`, `background-color`, `text-decoration-color`).
pub fn style_property(element: &Element, property: &str) -> String {
    element
        .get_attribute("style")
        .and_then(|style| {
            style
                .split(';')
                .filter_map(|declaration| declaration.split_once(':'))
                .find(|(name, _)| name.trim() == property)
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap_or_default()
}

fn css_color(color: &AnsiColor) -> String {
    match color {
        AnsiColor::Var(name) => format!("var({name})"),
        AnsiColor::Rgba(rgba) => css_format::format_rgb(&Color::from(*rgba)),
    }
}

/// `appendStylizedStringToContainer(root, stringContent, cssClasses, linkDetector, workspaceFolder, customTextColor,
/// customBackgroundColor, customUnderlineColor, highlights, offset, hoverBehavior)`.
#[allow(clippy::too_many_arguments)]
pub fn append_stylized_string_to_container(
    root: &Element,
    string_content: &str,
    css_classes: &[String],
    link_detector: &dyn ILinkDetector,
    workspace_folder: Option<&WorkspaceFolder>,
    custom_text_color: Option<&AnsiColor>,
    custom_background_color: Option<&AnsiColor>,
    custom_underline_color: Option<&AnsiColor>,
    highlights: Option<&[IHighlight]>,
    offset: i64,
    hover_behavior: &DebugLinkHoverBehaviorTypeData,
) {
    if string_content.is_empty() {
        return;
    }

    let shift = |value: usize| -> usize { (value as i64 - offset).max(0) as usize };
    let container = link_detector.linkify(
        string_content,
        hover_behavior,
        &LinkifyOptions {
            split_lines: true,
            workspace_folder: workspace_folder.cloned(),
            include_fulltext: false,
            highlights: highlights
                .map(|highlights| highlights.iter().map(|h| IHighlight { start: shift(h.start), end: shift(h.end), extra_classes: h.extra_classes.clone() }).collect())
                .unwrap_or_default(),
        },
    );

    container.set_attribute("class", &css_classes.join(" "));

    if let Some(color) = custom_text_color {
        set_style_property(&container, "color", &css_color(color));
    }
    if let Some(color) = custom_background_color {
        set_style_property(&container, "background-color", &css_color(color));
    }
    if let Some(color) = custom_underline_color {
        set_style_property(&container, "text-decoration-color", &css_color(color));
    }

    root.append([LinkNode::Element(container)]);
}

/// `calcANSI8bitColor(colorNumber)`: calculate the color from the color set defined in the ANSI 8-bit standard. Standard
/// and high intensity colors are not defined in the standard as specific colors, so these and invalid colors return `None`.
pub fn calc_ansi_8bit_color(color_number: f64) -> Option<Srgba> {
    if color_number % 1.0 != 0.0 {
        // Should be integer
        return None;
    }
    if (16.0..=231.0).contains(&color_number) {
        // Converts to one of 216 RGB colors
        let mut color_number = color_number - 16.0;

        let mut blue = color_number % 6.0;
        color_number = (color_number - blue) / 6.0;
        let mut green = color_number % 6.0;
        color_number = (color_number - green) / 6.0;
        let mut red = color_number;

        // red, green, blue now range on [0, 5], need to map to [0,255]
        let conv_factor = 255.0 / 5.0;
        blue = js_round(blue * conv_factor);
        green = js_round(green * conv_factor);
        red = js_round(red * conv_factor);

        Some(Srgba::opaque(red, green, blue))
    } else if (232.0..=255.0).contains(&color_number) {
        // Converts to a grayscale value
        let color_level = js_round((color_number - 232.0) / 23.0 * 255.0);
        Some(Srgba::opaque(color_level, color_level, color_level))
    } else {
        None
    }
}

/// `Math.round`: arredonda o meio para cima.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}
