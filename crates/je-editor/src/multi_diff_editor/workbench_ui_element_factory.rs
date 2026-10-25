//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/workbenchUIElementFactory.ts`: o contrato que deixa a camada do
//! editor pedir à camada do workbench os elementos de interface que ela não pode criar (o rótulo de recurso do cabeçalho,
//! a barra de ações de cada arquivo, o clique do meio e a abertura de um diff avulso).
//!
//! Divergências:
//! - `interface` com métodos opcionais é uma struct de closures opcionais, como o `IVirtualizedItemDelegate` do porte; quem
//!   monta o editor preenche só o que tem, e quem consulta testa o `Option` (o `if (factory.openDiffEditor)` do original).
//! - **Sem nós do DOM.** `createResourceLabel(element, kind, accessoryContainer)` perde o `element` e o
//!   `accessoryContainer`: o rótulo é um objeto nativo que o pintor lê, então a closure recebe só o tipo
//!   ([`MultiDiffEditorItemLabelKind`]) e devolve o [`IResourceLabel`].
//! - **A barra de ações.** O original cria o `MenuWorkbenchToolBar` dentro do template, pelo `IInstantiationService`, sobre
//!   o `div.actions`. A barra mora no `je-workbench`, que o `je-editor` não enxerga, então o template monta a requisição
//!   ([`IFileToolbarRequest`]: o menu, o serviço de chaves de contexto escopado, o `ActionRunnerWithContext` e as opções
//!   fixas) e a closure `create_file_toolbar` cria a barra e devolve o que a descarta. O `createToolbarActionViewItem?` (o
//!   `actionViewItemProvider` da barra) é decisão de quem cria a barra, por isso não existe como membro à parte.

use std::rc::Rc;

use je_base::actions::IActionRunner;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::uri::URI;
use je_platform::actions::MenuId;
use je_platform::contextkey::IContextKeyService;

/// `MultiDiffEditorItemLabelKind`: qual dos dois rótulos de caminho de um arquivo está sendo criado, o principal
/// (modificado) ou o secundário (original, mostrado nas renomeações).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiDiffEditorItemLabelKind {
    Primary,
    Secondary,
}

impl MultiDiffEditorItemLabelKind {
    /// O texto do `const enum` no original.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
        }
    }
}

/// `IResourceLabelOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IResourceLabelOptions {
    pub strikethrough: Option<bool>,
}

/// `IResourceLabel`.
pub trait IResourceLabel: IDisposable {
    /// `setUri(uri, options?)`.
    fn set_uri(&self, uri: Option<URI>, options: Option<IResourceLabelOptions>);
}

/// Tudo o que o `new MenuWorkbenchToolBar(actions, MenuId.MultiDiffEditorFileToolbar, {...})` do template recebe.
#[derive(Clone)]
pub struct IFileToolbarRequest {
    /// `MenuId.MultiDiffEditorFileToolbar`.
    pub menu_id: MenuId,
    /// O `IContextKeyService` escopado ao `div.actions` do arquivo (o que o `createChild(ServiceCollection)` entrega à barra).
    pub context_key_service: Rc<dyn IContextKeyService>,
    /// `actionRunner: new ActionRunnerWithContext(() => modifiedUri ?? originalUri)`.
    pub action_runner: Rc<dyn IActionRunner>,
    /// `highlightToggledItems: true`.
    pub highlight_toggled_items: bool,
    /// `menuOptions: { shouldForwardArgs: true }`.
    pub should_forward_args: bool,
    /// `toolbarOptions: { primaryGroup: g => g.startsWith('navigation') }`.
    pub primary_group_prefix: &'static str,
}

/// `IWorkbenchUIElementFactory`.
#[derive(Clone, Default)]
pub struct IWorkbenchUIElementFactory {
    /// `createResourceLabel?`.
    pub create_resource_label: Option<Rc<dyn Fn(MultiDiffEditorItemLabelKind) -> Rc<dyn IResourceLabel>>>,
    /// `handleHeaderMiddleClick?(resource)`: devolve se o clique foi tratado.
    pub handle_header_middle_click: Option<Rc<dyn Fn(&URI) -> bool>>,
    /// `openDiffEditor?(original, modified)`.
    pub open_diff_editor: Option<Rc<dyn Fn(&URI, &URI)>>,
    /// A criação da barra de ações de cada arquivo (ver a nota do módulo).
    pub create_file_toolbar: Option<Rc<dyn Fn(IFileToolbarRequest) -> DisposableRef>>,
}
