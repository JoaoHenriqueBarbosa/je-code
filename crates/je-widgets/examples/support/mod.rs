//! Ajudantes das galerias visuais (H26/H11): carrega o tema 2026 Dark ou Light, pinta a árvore do `Ui`
//! pelo `je-render` (caminho de CPU) e grava o PNG em `wip/scratch/gallery/`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use je_render::{DisplayList, Rgba, render_rgba};
use je_theme::ColorTheme;
use je_ui::Ui;

/// A raiz do repositório (o `CARGO_MANIFEST_DIR` é `crates/je-widgets`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("raiz do repositório")
}

/// `dark` ou `light`: o tema 2026 que o oráculo usa.
pub fn load_theme(name: &str) -> ColorTheme {
    let file = format!("2026-{name}.json");
    ColorTheme::load(&repo_root().join("ref/vscode/extensions/theme-defaults/themes").join(file)).expect("tema")
}

/// Os dois temas da galeria, com o nome.
pub fn themes() -> [(&'static str, ColorTheme); 2] {
    [("dark", load_theme("dark")), ("light", load_theme("light"))]
}

/// Pinta o `Ui` (a árvore inteira, escala 1) sobre `background` e grava `wip/scratch/gallery/<name>.png`.
pub fn save_png(ui: &Ui, background: Rgba, width: u16, height: u16, name: &str) -> PathBuf {
    save_png_layers(std::slice::from_ref(ui), background, width, height, name)
}

/// Como [`save_png`], com várias árvores empilhadas na mesma imagem (a última por cima): serve para widgets
/// que não podem coexistir num `Ui` só (duas listas suspensas abertas, onde a segunda tira o foco da primeira).
pub fn save_png_layers(uis: &[Ui], background: Rgba, width: u16, height: u16, name: &str) -> PathBuf {
    let mut list = DisplayList::new();
    for ui in uis {
        ui.tree().paint(&mut list, 1.0);
    }
    let rgba = render_rgba(&list, background, width, height);
    let dir = repo_root().join("wip/scratch/gallery");
    std::fs::create_dir_all(&dir).expect("pasta da galeria");
    let path = dir.join(format!("{name}.png"));
    let file = std::fs::File::create(&path).expect("saída");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("png").write_image_data(&rgba).expect("png");
    path
}
