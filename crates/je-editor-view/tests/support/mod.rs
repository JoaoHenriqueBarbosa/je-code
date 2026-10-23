//! O ambiente comum dos testes: o editor aberto sobre um arquivo TypeScript de exemplo com o tema 2026 Dark.

#![allow(dead_code)]

use je_editor::cursor_common::CursorSimpleModel;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_editor::text_model::{TextModel, TextSource};
use je_editor::undo_redo_service::UndoRedoService;
use je_editor_view::clipboard::Clipboard;
use je_editor_view::environment::find_builtin_extensions;
use je_editor_view::{EditorColors, EditorEnvironment, EditorView};
use je_theme::ColorTheme;

pub const SAMPLE: &str = include_str!("../fixtures/sample.ts");
/// A área de código do oráculo (1200x800, sem a minimapa e a barra de rolagem): 1009 x 677 a partir de (53, 90).
pub const VIEW_WIDTH: f64 = 1009.0;
pub const VIEW_HEIGHT: f64 = 677.0;

pub fn theme_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../je-theme/tests/golden/themes/2026-dark.json")
}

pub fn colors() -> EditorColors {
    EditorColors::from_theme(&ColorTheme::load(&theme_path()).expect("tema"))
}

/// Uma view sobre texto simples (sem linguagem), para os testes de entrada.
pub fn plain_view(text: &str) -> EditorView {
    let model = TextModel::new(TextSource::from(text), "plaintext", TextModel::default_creation_options(), None, UndoRedoService::for_tests());
    let mut view = EditorView::new(model, VIEW_WIDTH, VIEW_HEIGHT, colors());
    view.set_clipboard(Clipboard::internal_only());
    view
}

/// O ambiente de linguagens, ou `None` quando as extensões embutidas não estão no disco (`ref/` fica fora do git).
pub fn environment() -> Option<EditorEnvironment> {
    let dir = find_builtin_extensions()?;
    Some(EditorEnvironment::load(&dir, &theme_path()).expect("ambiente"))
}

/// A view do arquivo de exemplo, com os tokens já calculados.
pub fn sample_view(env: &EditorEnvironment) -> EditorView {
    let model: Rc<TextModel> = env.open("sample.ts", SAMPLE);
    model.tokenization().force_tokenization(model.get_line_count());
    let mut view = EditorView::new(model, VIEW_WIDTH, VIEW_HEIGHT, colors());
    view.set_token_colors(&env.color_map());
    view.set_clipboard(Clipboard::internal_only());
    view
}

/// A view do arquivo de exemplo sem forçar a tokenização: os tokens chegam pelo loop do runtime, em fatias.
pub fn lazy_sample_view(env: &EditorEnvironment, copies: usize) -> EditorView {
    let model: Rc<TextModel> = env.open("sample.ts", &SAMPLE.repeat(copies));
    let mut view = EditorView::new(model, VIEW_WIDTH, VIEW_HEIGHT, colors());
    view.set_token_colors(&env.color_map());
    view.set_clipboard(Clipboard::internal_only());
    view
}
