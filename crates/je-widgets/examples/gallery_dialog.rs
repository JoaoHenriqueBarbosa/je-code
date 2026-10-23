//! Galeria visual do `Dialog`: cada diálogo numa janela própria de 640x400, em grade de duas colunas, nos dois
//! temas. Grava `wip/scratch/gallery/dialog-dark.png` e `dialog-light.png`. Ordem (esquerda para direita,
//! cima para baixo): info com detalhe, erro, aviso, pergunta com caixa de seleção, campo de texto, campo de
//! senha com texto-guia, botão em hover, botão com foco pelo Tab, alinhamento vertical, detalhe longo, tipo
//! `none` e `pending`, rodapé com link.
#[path = "support/mod.rs"]
mod support;

use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_render::kurbo::Rect;
use je_render::{DisplayList, Rgba, render_rgba};
use je_theme::ColorTheme;
use je_ui::{Content, KeyData, Modifiers, PointerInput, PointerInputKind, Role, Ui};
use je_widgets::button::default_button_styles;
use je_widgets::dialog::{Dialog, DialogContentsAlignment, DialogInputOptions, DialogOptions, DialogType, default_dialog_styles};
use je_widgets::dom::create_el;
use je_widgets::input_box::{InputType, default_input_box_styles};
use je_widgets::text::ui_font;
use je_widgets::toggle::default_checkbox_styles;

const TILE_WIDTH: u16 = 640;
const TILE_HEIGHT: u16 = 400;
const COLUMNS: usize = 2;

/// O que muda de um diálogo para o outro.
struct Spec {
    message: &'static str,
    buttons: &'static [&'static str],
    configure: fn(&mut DialogOptions),
    /// O que fazer na janela depois de aberto (hover, Tab).
    after_show: fn(&Ui, &Dialog),
}

fn nothing(_: &Ui, _: &Dialog) {}

fn hover_second_button(ui: &Ui, dialog: &Dialog) {
    let buttons = dialog.buttons();
    let rect = ui.tree().absolute_bounds(buttons[1].element());
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, (rect.x0 + rect.x1) / 2.0, (rect.y0 + rect.y1) / 2.0));
}

fn tab_to_next_button(ui: &Ui, _: &Dialog) {
    ui.key_input(true, KeyData { key_code: KeyCode::Tab, modifiers: Modifiers::default() });
}

fn specs() -> Vec<Spec> {
    vec![
        Spec {
            message: "Salvar as alterações?",
            buttons: &["&&Salvar", "&&Não salvar", "Cancelar"],
            configure: |o| {
                o.r#type = Some(DialogType::Info);
                o.cancel_id = Some(2);
                o.detail = Some("Se você não salvar, as alterações serão perdidas.".to_owned());
            },
            after_show: nothing,
        },
        Spec {
            message: "Não foi possível abrir o arquivo",
            buttons: &["OK"],
            configure: |o| {
                o.r#type = Some(DialogType::Error);
                o.detail = Some("O arquivo está em uso por outro processo.\nFeche o outro programa e tente de novo.".to_owned());
            },
            after_show: nothing,
        },
        Spec {
            message: "Este arquivo é muito grande",
            buttons: &["&&Abrir mesmo assim", "Cancelar"],
            configure: |o| {
                o.r#type = Some(DialogType::Warning);
                o.cancel_id = Some(1);
                o.detail = Some("Abrir arquivos de mais de 50 MB pode deixar o editor lento.".to_owned());
            },
            after_show: nothing,
        },
        Spec {
            message: "Excluir a pasta \"node_modules\"?",
            buttons: &["&&Excluir", "Cancelar"],
            configure: |o| {
                o.r#type = Some(DialogType::Question);
                o.cancel_id = Some(1);
                o.detail = Some("Você pode restaurá-la da lixeira.".to_owned());
                o.checkbox_label = Some("Não perguntar de novo".to_owned());
                o.checkbox_checked = true;
            },
            after_show: nothing,
        },
        Spec {
            message: "Nome do novo ramo",
            buttons: &["&&Criar", "Cancelar"],
            configure: |o| {
                o.r#type = Some(DialogType::Question);
                o.cancel_id = Some(1);
                o.inputs = Some(vec![DialogInputOptions { placeholder: Some("nome-do-ramo".to_owned()), input_type: None, value: Some("feature/dialogos".to_owned()) }]);
            },
            after_show: nothing,
        },
        Spec {
            message: "Autenticação necessária",
            buttons: &["&&Entrar", "Cancelar"],
            configure: |o| {
                o.r#type = Some(DialogType::Info);
                o.cancel_id = Some(1);
                o.detail = Some("Informe o usuário e a senha do repositório.".to_owned());
                o.inputs = Some(vec![
                    DialogInputOptions { placeholder: Some("Usuário".to_owned()), input_type: None, value: Some("joao".to_owned()) },
                    DialogInputOptions { placeholder: Some("Senha".to_owned()), input_type: Some(InputType::Password), value: Some("segredo123".to_owned()) },
                ]);
            },
            after_show: nothing,
        },
        Spec {
            message: "Botão do meio em hover",
            buttons: &["&&Salvar", "&&Não salvar", "Cancelar"],
            configure: |o| {
                o.r#type = Some(DialogType::Info);
                o.cancel_id = Some(2);
            },
            after_show: hover_second_button,
        },
        Spec {
            message: "Foco no segundo botão (Tab)",
            buttons: &["&&Salvar", "&&Não salvar", "Cancelar"],
            configure: |o| {
                o.r#type = Some(DialogType::Info);
                o.cancel_id = Some(2);
            },
            after_show: tab_to_next_button,
        },
        Spec {
            message: "Alinhamento vertical",
            buttons: &["&&Reiniciar agora", "Mais tarde"],
            configure: |o| {
                o.r#type = Some(DialogType::Info);
                o.cancel_id = Some(1);
                o.alignment = DialogContentsAlignment::Vertical;
                o.detail = Some("A atualização foi instalada e será aplicada na próxima inicialização.".to_owned());
            },
            after_show: nothing,
        },
        Spec {
            message: "Detalhe longo, com quebra por largura",
            buttons: &["OK"],
            configure: |o| {
                o.r#type = Some(DialogType::Warning);
                o.detail = Some("Este texto é comprido de propósito para mostrar a quebra de linha por largura medida: o diálogo tem 480px de conteúdo e o texto fica ao lado do ícone, com 12px de recuo, quebrando em palavras inteiras. Palavras_gigantes_sem_espaço_também_são_cortadas_para_não_estourar_a_caixa_de_diálogo.".to_owned());
            },
            after_show: nothing,
        },
        Spec {
            message: "Sem ícone (tipo none)",
            buttons: &["OK"],
            configure: |o| {
                o.r#type = Some(DialogType::None);
                o.detail = Some("O tipo none não mostra o ícone.".to_owned());
            },
            after_show: nothing,
        },
        Spec {
            message: "Rodapé com link e corpo",
            buttons: &["OK"],
            configure: |o| {
                o.r#type = Some(DialogType::Info);
                o.detail = Some("Termos de uso no rodapé, com o estilo de link do tema.".to_owned());
                o.render_footer = Some(Rc::new(|ui, footer| {
                    let mut tree = ui.tree_mut();
                    let link = create_el(&mut tree, footer, Role::Link, "");
                    tree.set_bounds(link, Rect::new(0.0, 0.0, 120.0, 18.0));
                    tree.style_mut(link).content = Content::Text { text: "Termos de uso".to_owned(), style: ui_font(13.0, 400), color: Rgba::opaque(255, 255, 255), line_height: 18.0 };
                }));
            },
            after_show: nothing,
        },
    ]
}

fn render_tile(theme: &ColorTheme, background: Rgba, spec: &Spec) -> Vec<u8> {
    let ui = Ui::new();
    let root = ui.root();
    ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, f64::from(TILE_WIDTH), f64::from(TILE_HEIGHT)));
    // O que há por trás do bloco modal: um painel com a cor do editor.
    let mut options = DialogOptions::new(default_button_styles(theme), default_checkbox_styles(theme), default_input_box_styles(theme), default_dialog_styles(theme));
    (spec.configure)(&mut options);
    let buttons = spec.buttons.iter().map(|b| (*b).to_owned()).collect();
    let dialog = Dialog::new(&ui, root, spec.message, Some(buttons), options);
    let _result = dialog.show();
    (spec.after_show)(&ui, &dialog);
    let mut list = DisplayList::new();
    ui.tree().paint(&mut list, 1.0);
    let rgba = render_rgba(&list, background, TILE_WIDTH, TILE_HEIGHT);
    dialog.dispose();
    rgba
}

fn main() {
    let specs = specs();
    let rows = specs.len().div_ceil(COLUMNS);
    let (width, height) = (usize::from(TILE_WIDTH) * COLUMNS, usize::from(TILE_HEIGHT) * rows);
    for (name, theme) in support::themes() {
        let background = theme.get_color("editor.background").expect("editor.background");
        let mut canvas = vec![0u8; width * height * 4];
        for (index, spec) in specs.iter().enumerate() {
            let tile = render_tile(&theme, background, spec);
            let (column, row) = (index % COLUMNS, index / COLUMNS);
            for y in 0..usize::from(TILE_HEIGHT) {
                let from = y * usize::from(TILE_WIDTH) * 4;
                let to = ((row * usize::from(TILE_HEIGHT) + y) * width + column * usize::from(TILE_WIDTH)) * 4;
                canvas[to..to + usize::from(TILE_WIDTH) * 4].copy_from_slice(&tile[from..from + usize::from(TILE_WIDTH) * 4]);
            }
        }
        let dir = support::repo_root().join("wip/scratch/gallery");
        std::fs::create_dir_all(&dir).expect("pasta da galeria");
        let path = dir.join(format!("dialog-{name}.png"));
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&path).expect("saída")), width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().expect("png").write_image_data(&canvas).expect("png");
        println!("{}", path.display());
    }
}
