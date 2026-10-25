//! O `DataTransfer` do arrastar e soltar: a carga que viaja com os eventos `dragstart`, `drag`, `dragenter`,
//! `dragover`, `dragleave`, `drop` e `dragend` (ver [`crate::events`]).
//!
//! Modela o "drag data store" do HTML, que o Chromium do Electron implementa e de que os widgets do VS Code dependem
//! (`dataTransfer.setData`/`getData`, `effectAllowed`, `dropEffect`, `setDragImage`, `types`, `files`):
//!
//! - **Modo do repositório.** No `dragstart` o repositório é de leitura e escrita (o ouvinte escolhe o que
//!   carregar); no `drop` é só de leitura (`getData` devolve o dado); nos demais eventos é protegido (`types`
//!   aparece, `getData` devolve vazio e `setData` não faz nada). É o que impede um alvo de espiar o conteúdo antes
//!   de o usuário soltar.
//! - **Tipos.** `setData` guarda uma string por tipo (em minúsculas); `"text"` vale `"text/plain"` e `"url"` vale
//!   `"text/uri-list"`, como no navegador. Arquivos arrastados de fora entram como o tipo `"Files"` ao fim de
//!   [`DataTransfer::types`] e só são legíveis em [`DataTransfer::files`] durante o `drop`.
//! - **Efeitos.** [`EffectAllowed`] é o que a origem permite (escrito no `dragstart`); [`DropEffect`] é o que o
//!   alvo escolhe no `dragover`. O efeito só vale se a origem o permitir ([`EffectAllowed::permits`]).
//!
//! O [`DataTransfer`] é um identificador compartilhado (clonar é barato): os eventos de um mesmo arrasto carregam o
//! mesmo objeto, como no navegador.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::tree::NodeId;

/// O `dropEffect`: o que o alvo diz que vai fazer com a carga.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DropEffect {
    #[default]
    None,
    Copy,
    Link,
    Move,
}

impl DropEffect {
    /// O texto do DOM (`"none"`, `"copy"`, `"link"`, `"move"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Copy => "copy",
            Self::Link => "link",
            Self::Move => "move",
        }
    }

    /// O inverso de [`DropEffect::as_str`]; valor desconhecido dá `None` (o setter do DOM ignora).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "copy" => Some(Self::Copy),
            "link" => Some(Self::Link),
            "move" => Some(Self::Move),
            _ => None,
        }
    }
}

/// O `effectAllowed`: o que a origem permite fazer com a carga.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EffectAllowed {
    None,
    Copy,
    CopyLink,
    CopyMove,
    Link,
    LinkMove,
    Move,
    All,
    /// O valor inicial: nada foi escolhido, vale como `all`.
    #[default]
    Uninitialized,
}

impl EffectAllowed {
    /// O texto do DOM (`"copyLink"`, `"uninitialized"`...).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Copy => "copy",
            Self::CopyLink => "copyLink",
            Self::CopyMove => "copyMove",
            Self::Link => "link",
            Self::LinkMove => "linkMove",
            Self::Move => "move",
            Self::All => "all",
            Self::Uninitialized => "uninitialized",
        }
    }

    /// O inverso de [`EffectAllowed::as_str`]; valor desconhecido dá `None` (o setter do DOM ignora).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "copy" => Some(Self::Copy),
            "copyLink" => Some(Self::CopyLink),
            "copyMove" => Some(Self::CopyMove),
            "link" => Some(Self::Link),
            "linkMove" => Some(Self::LinkMove),
            "move" => Some(Self::Move),
            "all" => Some(Self::All),
            "uninitialized" => Some(Self::Uninitialized),
            _ => None,
        }
    }

    /// A origem permite este efeito de soltura? `DropEffect::None` é sempre permitido (é a recusa).
    pub fn permits(self, effect: DropEffect) -> bool {
        match effect {
            DropEffect::None => true,
            DropEffect::Copy => matches!(self, Self::Copy | Self::CopyLink | Self::CopyMove | Self::All | Self::Uninitialized),
            DropEffect::Link => matches!(self, Self::Link | Self::CopyLink | Self::LinkMove | Self::All | Self::Uninitialized),
            DropEffect::Move => matches!(self, Self::Move | Self::CopyMove | Self::LinkMove | Self::All | Self::Uninitialized),
        }
    }

    /// O `dropEffect` com que `dragenter` e `dragover` chegam ao ouvinte, derivado do que a origem permite (a tabela
    /// do HTML: copiar quando copiar é permitido, senão vincular, senão mover).
    pub fn initial_drop_effect(self) -> DropEffect {
        match self {
            Self::None => DropEffect::None,
            Self::Copy | Self::CopyLink | Self::CopyMove | Self::All | Self::Uninitialized => DropEffect::Copy,
            Self::Link | Self::LinkMove => DropEffect::Link,
            Self::Move => DropEffect::Move,
        }
    }
}

/// O modo do repositório de dados do arrasto (ver o `//!` do módulo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DataStoreMode {
    ReadWrite,
    ReadOnly,
    Protected,
}

/// A imagem de arrasto escolhida com `setDragImage(element, x, y)`: o nó que o shell pinta junto do ponteiro, e o
/// deslocamento do ponteiro dentro dele.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragImage {
    pub node: NodeId,
    pub x: f64,
    pub y: f64,
}

struct Inner {
    mode: DataStoreMode,
    /// Tipo (normalizado) e dado, na ordem em que foram escritos.
    items: Vec<(String, String)>,
    files: Vec<PathBuf>,
    effect_allowed: EffectAllowed,
    drop_effect: DropEffect,
    drag_image: Option<DragImage>,
}

/// O `DataTransfer` de um arrasto. Clonar compartilha o mesmo repositório.
#[derive(Clone)]
pub struct DataTransfer(Rc<RefCell<Inner>>);

impl Default for DataTransfer {
    fn default() -> Self {
        Self::new()
    }
}

/// O tipo como o repositório o guarda: minúsculas, com os apelidos legados `text` e `url` resolvidos.
fn normalize(format: &str) -> String {
    let lower = format.to_ascii_lowercase();
    match lower.as_str() {
        "text" => "text/plain".to_owned(),
        "url" => "text/uri-list".to_owned(),
        _ => lower,
    }
}

impl DataTransfer {
    /// Um repositório vazio, de leitura e escrita (o do início do arrasto).
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Inner {
            mode: DataStoreMode::ReadWrite,
            items: Vec::new(),
            files: Vec::new(),
            effect_allowed: EffectAllowed::Uninitialized,
            drop_effect: DropEffect::None,
            drag_image: None,
        })))
    }

    /// O repositório de um arrasto vindo de fora da janela: arquivos do sistema, qualquer efeito permitido.
    pub(crate) fn external(files: Vec<PathBuf>) -> Self {
        let transfer = Self::new();
        {
            let mut inner = transfer.0.borrow_mut();
            inner.files = files;
            inner.effect_allowed = EffectAllowed::All;
            inner.mode = DataStoreMode::Protected;
        }
        transfer
    }

    /// `setData(format, data)`: só no `dragstart`; em outro momento não faz nada, como no navegador. Um tipo que já
    /// existe é substituído no lugar.
    pub fn set_data(&self, format: &str, data: &str) {
        let mut inner = self.0.borrow_mut();
        if inner.mode != DataStoreMode::ReadWrite {
            return;
        }
        let format = normalize(format);
        match inner.items.iter_mut().find(|(f, _)| *f == format) {
            Some(item) => item.1 = data.to_owned(),
            None => inner.items.push((format, data.to_owned())),
        }
    }

    /// `getData(format)`: só no `drop`; em outro momento devolve vazio. Tipo ausente também devolve vazio.
    pub fn get_data(&self, format: &str) -> String {
        let inner = self.0.borrow();
        if inner.mode != DataStoreMode::ReadOnly {
            return String::new();
        }
        let format = normalize(format);
        inner.items.iter().find(|(f, _)| *f == format).map(|(_, d)| d.clone()).unwrap_or_default()
    }

    /// `clearData(format?)`: sem tipo apaga tudo (os dados, não os arquivos); só no `dragstart`.
    pub fn clear_data(&self, format: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        if inner.mode != DataStoreMode::ReadWrite {
            return;
        }
        match format {
            Some(format) => {
                let format = normalize(format);
                inner.items.retain(|(f, _)| *f != format);
            }
            None => inner.items.clear(),
        }
    }

    /// `types`: os tipos escritos, na ordem, e `"Files"` ao fim quando há arquivos. Visível em todos os eventos.
    pub fn types(&self) -> Vec<String> {
        let inner = self.0.borrow();
        let mut types: Vec<String> = inner.items.iter().map(|(f, _)| f.clone()).collect();
        if !inner.files.is_empty() {
            types.push("Files".to_owned());
        }
        types
    }

    /// `files`: só no `drop`; antes disso a lista é vazia (o tipo `"Files"` já aparece em [`DataTransfer::types`]).
    pub fn files(&self) -> Vec<PathBuf> {
        let inner = self.0.borrow();
        if inner.mode == DataStoreMode::ReadOnly { inner.files.clone() } else { Vec::new() }
    }

    pub fn effect_allowed(&self) -> EffectAllowed {
        self.0.borrow().effect_allowed
    }

    /// Escrever `effectAllowed` só vale no `dragstart`.
    pub fn set_effect_allowed(&self, effect: EffectAllowed) {
        let mut inner = self.0.borrow_mut();
        if inner.mode == DataStoreMode::ReadWrite {
            inner.effect_allowed = effect;
        }
    }

    pub fn drop_effect(&self) -> DropEffect {
        self.0.borrow().drop_effect
    }

    /// `dropEffect = ...`: o alvo escolhe no `dragover` (ou no `dragenter`).
    pub fn set_drop_effect(&self, effect: DropEffect) {
        self.0.borrow_mut().drop_effect = effect;
    }

    /// `setDragImage(element, x, y)`: só no `dragstart`.
    pub fn set_drag_image(&self, node: NodeId, x: f64, y: f64) {
        let mut inner = self.0.borrow_mut();
        if inner.mode == DataStoreMode::ReadWrite {
            inner.drag_image = Some(DragImage { node, x, y });
        }
    }

    pub fn drag_image(&self) -> Option<DragImage> {
        self.0.borrow().drag_image
    }

    pub(crate) fn set_mode(&self, mode: DataStoreMode) {
        self.0.borrow_mut().mode = mode;
    }

    /// Volta o `dropEffect` ao valor com que `dragenter` e `dragover` chegam ao ouvinte.
    pub(crate) fn reset_drop_effect(&self) {
        let mut inner = self.0.borrow_mut();
        inner.drop_effect = inner.effect_allowed.initial_drop_effect();
    }

    /// Acrescenta arquivos de um arrasto externo, sem repetir (o winit entrega um evento por arquivo).
    pub(crate) fn add_files(&self, files: Vec<PathBuf>) {
        let mut inner = self.0.borrow_mut();
        for file in files {
            if !inner.files.contains(&file) {
                inner.files.push(file);
            }
        }
    }
}
