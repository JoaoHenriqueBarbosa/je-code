//! Galeria do `FindInput` e do `ReplaceInput`: cada estado numa linha, nos dois temas.
//!
//! Só um nó tem o foco por vez, e o cursor e o anel de foco só aparecem nele. Por isso há uma imagem por foco,
//! em `wip/scratch/gallery/`: `findinput-<tema>.png` (sem foco), `findinput-focus-<tema>.png` (foco no campo
//! de texto), `findinput-toggle-focus-<tema>.png` (foco no botão de caixa) e `findinput-replace-focus-<tema>.png`
//! (foco no campo de substituição). A mensagem de erro usa o `InlineContextViewProvider` e aparece nas quatro
//! (é forçada com `show_message(.., true)`).
#[path = "support/mod.rs"]
mod support;

use std::path::PathBuf;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Role, Ui};
use je_widgets::context_view_api::IContextViewProvider;
use je_widgets::find_input::{FindInput, FindInputOptions, default_find_input_styles};
use je_widgets::find_input_toggles::{FindInputToggleOpts, regex_toggle};
use je_widgets::input_box::{InlineContextViewProvider, Message, MessageType};
use je_widgets::replace_input::{ReplaceInput, ReplaceInputOptions};
use je_widgets::text::{set_node_text, ui_font};

const WIDTH: f64 = 380.0;
const BOX_WIDTH: f64 = 340.0;
const ROW_HEIGHT: f64 = 30.0;
const MESSAGE_SPACE: f64 = 44.0;

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    None,
    Input,
    Toggle,
    Replace,
}

impl Focus {
    const ALL: [Focus; 4] = [Focus::None, Focus::Input, Focus::Toggle, Focus::Replace];

    fn file_name(self, theme: &str) -> String {
        match self {
            Focus::None => format!("findinput-{theme}"),
            Focus::Input => format!("findinput-focus-{theme}"),
            Focus::Toggle => format!("findinput-toggle-focus-{theme}"),
            Focus::Replace => format!("findinput-replace-focus-{theme}"),
        }
    }
}

/// O que cada linha monta.
enum Kind {
    Find { value: &'static str, case: bool, words: bool, regex: bool, common: bool },
    Disabled,
    Error,
    Additional,
    Replace { value: &'static str, preserve: bool, buttons: bool },
}

struct Row {
    label: &'static str,
    kind: Kind,
    focus: Option<Focus>,
}

fn rows() -> Vec<Row> {
    let row = |label, kind, focus| Row { label, kind, focus };
    let find = |value, case, words, regex| Kind::Find { value, case, words, regex, common: true };
    vec![
        row("vazio, com placeholder", find("", false, false, false), None),
        row("com texto, botões desligados", find("src/vs/base/browser", false, false, false), None),
        row("com texto, caixa ligada", find("TextModel", true, false, false), None),
        row("palavra inteira e regex ligadas", find("^import .* from", false, true, true), None),
        row("os três ligados", find("(foo|bar)+", true, true, true), None),
        row("com foco no campo (cursor e anel)", find("typing here", false, false, true), Some(Focus::Input)),
        row("com foco no botão de caixa", find("TextModel", true, false, false), Some(Focus::Toggle)),
        row("sem botões comuns", Kind::Find { value: "plain input", case: false, words: false, regex: false, common: false }, None),
        row("botão adicional", Kind::Additional, None),
        row("desabilitado", Kind::Disabled, None),
        row("erro de validação (provedor)", Kind::Error, None),
        row("ReplaceInput vazio, sem botões", Kind::Replace { value: "", preserve: false, buttons: false }, None),
        row("ReplaceInput com texto, preservar caixa", Kind::Replace { value: "Replacement", preserve: false, buttons: true }, None),
        row("ReplaceInput com preservar caixa ligado", Kind::Replace { value: "Replacement", preserve: true, buttons: true }, None),
        row("ReplaceInput com foco", Kind::Replace { value: "typing here", preserve: false, buttons: true }, Some(Focus::Replace)),
    ]
}

/// Mantém vivo (e descarta ao fim) o que a imagem usa.
struct Scene {
    disposables: Vec<Rc<dyn IDisposable>>,
}

impl Drop for Scene {
    fn drop(&mut self) {
        for d in &self.disposables {
            d.dispose();
        }
    }
}

fn build(theme_name: &str, theme: &ColorTheme, focus: Focus) -> PathBuf {
    let ui = Ui::new();
    let background = theme.get_color("sideBar.background").or_else(|| theme.get_color("editor.background")).expect("fundo");
    let foreground = theme.get_color("foreground").expect("foreground");
    let styles = default_find_input_styles(theme);
    let mut scene = Scene { disposables: Vec::new() };
    let mut focused: Option<Box<dyn Fn()>> = None;
    let mut y = 12.0;
    for row in rows() {
        let container = {
            let mut tree = ui.tree_mut();
            let root = tree.root();
            tree.set_bounds(root, Rect::new(0.0, 0.0, WIDTH, 900.0));
            let label = tree.create(root, Role::Generic);
            tree.set_bounds(label, Rect::new(20.0, y, WIDTH - 20.0, y + 14.0));
            set_node_text(&mut tree, label, row.label, &ui_font(11.0, 400), foreground, 14.0);
            let container = tree.create(root, Role::Generic);
            tree.set_bounds(container, Rect::new(20.0, y + 16.0, 20.0 + BOX_WIDTH, y + 16.0 + ROW_HEIGHT));
            container
        };
        let wants_focus = row.focus == Some(focus);
        let mut extra = 0.0;
        let find_options = |common: bool| FindInputOptions {
            placeholder: Some("Find".to_owned()),
            show_common_find_toggles: common,
            append_case_sensitive_label: " (Alt+C)".to_owned(),
            append_whole_words_label: " (Alt+W)".to_owned(),
            append_regex_label: " (Alt+R)".to_owned(),
            caret_blink: false,
            ..FindInputOptions::new("Find", styles.clone())
        };
        match row.kind {
            Kind::Find { value, case, words, regex, common } => {
                let find_input = FindInput::new(&ui, Some(container), None, find_options(common));
                find_input.set_value(value);
                find_input.set_case_sensitive(case);
                find_input.set_whole_words(words);
                find_input.set_regex(regex);
                if wants_focus {
                    let target = find_input.clone();
                    focused = Some(if focus == Focus::Toggle { Box::new(move || target.focus_on_case_sensitive()) } else { Box::new(move || target.focus()) });
                }
                scene.disposables.push(find_input);
            }
            Kind::Additional => {
                let extra_toggle = regex_toggle(&ui, FindInputToggleOpts { is_checked: true, ..FindInputToggleOpts::new(styles.toggle_styles) });
                let find_input = FindInput::new(&ui, Some(container), None, FindInputOptions { additional_toggles: vec![extra_toggle], ..find_options(true) });
                find_input.set_value("with an extra toggle");
                scene.disposables.push(find_input);
            }
            Kind::Disabled => {
                let find_input = FindInput::new(&ui, Some(container), None, find_options(true));
                find_input.set_value("read only");
                find_input.set_case_sensitive(true);
                find_input.disable();
                scene.disposables.push(find_input);
            }
            Kind::Error => {
                let provider = InlineContextViewProvider::new(&ui);
                let as_provider: Rc<dyn IContextViewProvider> = provider;
                let find_input = FindInput::new(&ui, Some(container), Some(as_provider), find_options(true));
                find_input.set_value("(unclosed");
                find_input.set_regex(true);
                let message = Message { content: Some("Invalid regular expression: missing `)`".to_owned()), format_content: true, r#type: Some(MessageType::Error) };
                find_input.input_box.show_message(message, true);
                extra = MESSAGE_SPACE;
                scene.disposables.push(find_input);
            }
            Kind::Replace { value, preserve, buttons } => {
                let mut options = ReplaceInputOptions::new("Replace", styles.clone());
                options.placeholder = Some("Replace".to_owned());
                options.caret_blink = false;
                options.append_preserve_case_label = " (Alt+P)".to_owned();
                let replace_input = ReplaceInput::new(&ui, Some(container), None, buttons, options);
                replace_input.set_value(value);
                replace_input.set_preserve_case(preserve);
                if wants_focus {
                    let target = replace_input.clone();
                    focused = Some(Box::new(move || target.focus()));
                }
                scene.disposables.push(replace_input);
            }
        }
        y += 16.0 + ROW_HEIGHT + extra + 8.0;
    }
    if let Some(focus_it) = focused {
        focus_it();
    }
    support::save_png(&ui, background, WIDTH as u16, y as u16 + 6, &focus.file_name(theme_name))
}

fn main() {
    for (name, theme) in support::themes() {
        for focus in Focus::ALL {
            println!("{}", build(name, &theme, focus).display());
        }
    }
}
