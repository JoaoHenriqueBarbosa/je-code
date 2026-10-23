//! Galeria do `InputBox` e do `HistoryInputBox`: cada estado numa linha, nos dois temas.
//!
//! Só um campo tem o foco por vez (como numa janela), e o cursor, a seleção, o anel de foco e a rolagem só
//! aparecem no campo focado. Por isso há uma imagem por foco, em `wip/scratch/gallery/`:
//! `inputbox-<tema>.png` (foco no campo de cursor), `inputbox-selection-<tema>.png` (foco no campo com
//! seleção), `inputbox-scroll-<tema>.png` (foco no texto longo, rolado até o cursor) e
//! `inputbox-history-<tema>.png` (foco no campo de histórico, com a dica no placeholder). Os campos com
//! mensagem usam o `InlineContextViewProvider` (o bloco logo abaixo do campo).
#[path = "support/mod.rs"]
mod support;

use std::path::PathBuf;
use std::rc::Rc;

use je_base::history::HistorySet;
use je_base::lifecycle::IDisposable;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Role, Ui};
use je_widgets::context_view_api::IContextViewProvider;
use je_widgets::input_box::{
    HistoryInputBox, HistoryInputBoxOptions, InlineContextViewProvider, InputBox, InputBoxOptions, InputType, Message, MessageType, Range, default_input_box_styles,
};
use je_widgets::text::{set_node_text, ui_font};

const WIDTH: f64 = 360.0;
const BOX_WIDTH: f64 = 320.0;
/// A altura que o bloco de uma mensagem de provedor reserva abaixo do campo.
const MESSAGE_SPACE: f64 = 44.0;

/// Qual campo recebe o foco numa imagem.
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Caret,
    Selection,
    Scroll,
    History,
}

impl Focus {
    const ALL: [Focus; 4] = [Focus::Caret, Focus::Selection, Focus::Scroll, Focus::History];

    fn file_name(self, theme: &str) -> String {
        match self {
            Focus::Caret => format!("inputbox-{theme}"),
            Focus::Selection => format!("inputbox-selection-{theme}"),
            Focus::Scroll => format!("inputbox-scroll-{theme}"),
            Focus::History => format!("inputbox-history-{theme}"),
        }
    }
}

enum Kind {
    Plain(fn(&InputBox)),
    Password,
    Placeholder,
    Message { message_type: MessageType, provider: bool },
    History,
}

struct Row {
    label: &'static str,
    kind: Kind,
    /// O foco que leva a imagem a focar este campo.
    focus: Option<Focus>,
}

fn rows() -> Vec<Row> {
    let row = |label, kind, focus| Row { label, kind, focus };
    vec![
        row("vazio, com placeholder", Kind::Placeholder, None),
        row("com texto", Kind::Plain(|b| b.set_value("src/vs/base/browser/ui")), None),
        row("com seleção (pintada só com foco)", Kind::Plain(|b| {
            b.set_value("hello wonderful world");
            b.select(Some(Range { start: 6, end: 15 }));
        }), Some(Focus::Selection)),
        row("com foco (cursor e anel de foco)", Kind::Plain(|b| b.set_value("typing here")), Some(Focus::Caret)),
        row("desabilitado", Kind::Plain(|b| {
            b.set_value("read only");
            b.disable();
        }), None),
        row("senha", Kind::Password, None),
        row("texto longo, sem foco (reticências)", Kind::Plain(|b| b.set_value("a very long value that does not fit in the box at all")), None),
        row("texto longo, com foco (rolado até o cursor)", Kind::Plain(|b| b.set_value("a very long value that does not fit in the box at all")), Some(Focus::Scroll)),
        row("info, só a borda (sem provedor)", Kind::Message { message_type: MessageType::Info, provider: false }, None),
        row("info (provedor)", Kind::Message { message_type: MessageType::Info, provider: true }, None),
        row("aviso (provedor)", Kind::Message { message_type: MessageType::Warning, provider: true }, None),
        row("erro (provedor)", Kind::Message { message_type: MessageType::Error, provider: true }, None),
        row("histórico (a dica ⇅ aparece com foco)", Kind::History, Some(Focus::History)),
    ]
}

fn message_text(message_type: MessageType) -> &'static str {
    match message_type {
        MessageType::Info => "This value is optional and can be changed later.",
        MessageType::Warning => "This name is already used by another **setting** in the workspace.",
        MessageType::Error => "The value must be a valid `glob` pattern, for example `**/*.rs`.",
    }
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
    let mut scene = Scene { disposables: Vec::new() };
    let mut focused: Option<Rc<dyn Fn()>> = None;
    let mut y = 12.0;
    for row in rows() {
        let container = {
            let mut tree = ui.tree_mut();
            let root = tree.root();
            tree.set_bounds(root, Rect::new(0.0, 0.0, WIDTH, 800.0));
            let label = tree.create(root, Role::Generic);
            tree.set_bounds(label, Rect::new(20.0, y, WIDTH - 20.0, y + 14.0));
            set_node_text(&mut tree, label, row.label, &ui_font(11.0, 400), foreground, 14.0);
            let container = tree.create(root, Role::Generic);
            tree.set_bounds(container, Rect::new(20.0, y + 16.0, 20.0 + BOX_WIDTH, y + 46.0));
            container
        };
        let mut options = InputBoxOptions::new(default_input_box_styles(theme));
        options.caret_blink = false;
        let mut extra = 0.0;
        let wants_focus = row.focus == Some(focus);
        match row.kind {
            Kind::Placeholder => {
                options.placeholder = Some("Search".to_owned());
                scene.disposables.push(InputBox::new(&ui, container, None, options));
            }
            Kind::Plain(setup) => {
                let input_box = InputBox::new(&ui, container, None, options);
                setup(&input_box);
                if wants_focus {
                    let target = input_box.clone();
                    focused = Some(Rc::new(move || {
                        target.focus();
                        if focus == Focus::Scroll {
                            let end = target.value().encode_utf16().count();
                            target.text_field().set_caret(end);
                            target.layout();
                        }
                    }));
                }
                scene.disposables.push(input_box);
            }
            Kind::Password => {
                options.input_type = InputType::Password;
                let input_box = InputBox::new(&ui, container, None, options);
                input_box.set_value("hunter2");
                scene.disposables.push(input_box);
            }
            Kind::Message { message_type, provider } => {
                let provider = provider.then(|| InlineContextViewProvider::new(&ui));
                let as_provider = provider.clone().map(|p| p as Rc<dyn IContextViewProvider>);
                let input_box = InputBox::new(&ui, container, as_provider, options);
                input_box.set_value("**/*.{rs,toml");
                input_box.show_message(Message { content: Some(message_text(message_type).to_owned()), format_content: true, r#type: Some(message_type) }, true);
                if provider.is_some() {
                    extra = MESSAGE_SPACE;
                }
                scene.disposables.push(input_box);
            }
            Kind::History => {
                options.placeholder = Some("Find".to_owned());
                options.history = Some(Rc::new(HistorySet::new(["one".to_owned()])));
                let input_box = HistoryInputBox::new(&ui, container, None, HistoryInputBoxOptions { input: options, show_history_hint: Some(Rc::new(|| true)) });
                if wants_focus {
                    let target = input_box.clone();
                    focused = Some(Rc::new(move || target.focus()));
                }
                scene.disposables.push(input_box);
            }
        }
        y += 46.0 + extra + 6.0;
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
