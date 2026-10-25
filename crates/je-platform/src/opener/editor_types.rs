//! Porte dos tipos de `src/vs/platform/editor/common/editor.ts` que o `opener.ts` usa: `ViewColumn`,
//! `IEditorOptions` (com `EditorActivation`, `EditorResolution`, `EditorOpenSource` e as opções do modal)
//! e `ITextEditorSelection`.
//!
//! Os números do original são `f64` (o `number` do JavaScript) e o texto é [`JsString`]. `undefined` é
//! `None`. As uniões do original viram `enum`: `override: string | EditorResolution` é [`EditorOverride`].
//! O `IRectangle` de `platform/window/common/window.ts` entra só na forma `Partial<IRectangle>` que o
//! `auxiliary.bounds` pede ([`PartialRectangle`]).

use std::any::Any;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;

use crate::contextkey::IContextKeyService;

/// `EditorActivation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorActivation {
    /// Ativa o editor depois de aberto. Restaura o editor se ele estiver minimizado.
    Activate = 1,
    /// Só restaura o editor se ele estiver minimizado, sem ativar (só vale com `preserve_focus: true`).
    Restore = 2,
    /// Preserva o editor ativo atual (só vale com `preserve_focus: true`).
    Preserve = 3,
}

/// `EditorResolution`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorResolution {
    /// Só os editores exclusivos são considerados. O valor não é zero para que `override` seja truthy.
    ExclusiveOnly = 1,
}

/// `EditorOpenSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorOpenSource {
    /// Padrão: a abertura veio de uma chamada programática à API do serviço de editores.
    Api = 0,
    /// A abertura foi disparada por uma ação do usuário, como mouse ou teclado.
    User = 1,
}

/// `ViewColumn`: uma coluna de editor ou um posicionamento simbólico relativo ao editor ativo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewColumn {
    Active = -1,
    Beside = -2,
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
}

/// `string | EditorResolution` (`IEditorOptions.override`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorOverride {
    /// Sobrescrita específica por id.
    Id(JsString),
    /// Tratamento específico de sobrescrita.
    Resolution(EditorResolution),
}

/// `Partial<IRectangle>`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartialRectangle {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

/// `IEditorOptions.auxiliary`: opções que só valem quando `AUX_WINDOW_GROUP` é usado para abrir.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AuxiliaryEditorOptions {
    /// Define os limites da janela do editor.
    pub bounds: Option<PartialRectangle>,
    /// Mostra o editor compacto, escondendo os elementos desnecessários.
    pub compact: Option<bool>,
    /// Mostra o editor sempre acima das outras janelas.
    pub always_on_top: Option<bool>,
}

/// `{ readonly width: number; readonly height: number }` (`size`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorSize {
    pub width: f64,
    pub height: f64,
}

/// `{ readonly left: number; readonly top: number }` (`position`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorPosition {
    pub left: f64,
    pub top: f64,
}

/// `IModalEditorNavigation`: o contexto para navegar entre itens dentro de um editor modal.
#[derive(Clone)]
pub struct ModalEditorNavigation {
    /// Número total de itens da lista de navegação.
    pub total: f64,
    /// Índice atual (base 0) na lista de navegação.
    pub current: f64,
    /// Navega para o item no índice dado (base 0).
    pub navigate: Rc<dyn Fn(f64)>,
}

/// O `{ readonly height: number; readonly width: number }` do evento `onDidLayout` da barra lateral.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutDimension {
    pub height: f64,
    pub width: f64,
}

/// `IModalEditorSidebar`: conteúdo próprio ao lado do conteúdo principal do editor modal.
#[derive(Clone, Default)]
pub struct ModalEditorSidebar {
    /// Largura da barra lateral definida pelo usuário ao redimensionar, se houver.
    pub sidebar_width: Option<f64>,
    /// Se a barra lateral está escondida.
    pub sidebar_hidden: Option<bool>,
    /// `render(container, onDidLayout, contextKeyService)`: desenha o conteúdo no contêiner dado (o
    /// `HTMLElement` do original é opaco aqui) e devolve o disposable a limpar quando o modal fecha.
    pub render: Option<Rc<dyn Fn(Rc<dyn Any>, Event<LayoutDimension>, Rc<dyn IContextKeyService>) -> DisposableRef>>,
}

/// `IModalEditorPartOptions`.
#[derive(Clone, Default)]
pub struct ModalEditorPartOptions {
    /// Se o editor modal deve abrir maximizado.
    pub maximized: Option<bool>,
    /// Tamanho do modal quando não está maximizado.
    pub size: Option<EditorSize>,
    /// Posição do modal quando não está maximizado.
    pub position: Option<EditorPosition>,
    /// O contexto de navegação entre itens deste editor modal.
    pub navigation: Option<ModalEditorNavigation>,
    /// Conteúdo opcional da barra lateral à esquerda do editor modal. Só aparece quando informado na
    /// abertura e não pode ser adicionado, removido ou atualizado depois.
    pub sidebar: Option<ModalEditorSidebar>,
}

/// `IEditorOptions`.
#[derive(Clone, Default)]
pub struct EditorOptions {
    /// Faz o editor não receber o foco do teclado ao ser aberto. Também não ativa o grupo, a menos que
    /// ele já seja o ativo (a opção `activation` pode mudar isso).
    pub preserve_focus: Option<bool>,
    /// Só relevante quando o editor abre num grupo que ainda não é o ativo: controla se o grupo
    /// inativo passa a ativo, é restaurado ou é preservado.
    pub activation: Option<EditorActivation>,
    /// Recarrega a entrada no editor mesmo que seja idêntica à que já está aparecendo.
    pub force_reload: Option<bool>,
    /// Revela o editor se ele já estiver aberto e visível em algum grupo.
    pub reveal_if_visible: Option<bool>,
    /// Revela o editor se ele já estiver aberto (mesmo sem estar visível) em algum grupo.
    pub reveal_if_opened: Option<bool>,
    /// Um editor fixado permanece na pilha quando outro é aberto.
    pub pinned: Option<bool>,
    /// Um editor fixo vai para o início da lista do grupo e fica lá até ser fechado explicitamente.
    pub sticky: Option<bool>,
    /// O índice na pilha de documentos onde inserir o editor ao abrir.
    pub index: Option<f64>,
    /// Abre o editor em segundo plano, sem carregar o conteúdo.
    pub inactive: Option<bool>,
    /// Em caso de erro ao abrir, não apresenta o erro ao usuário: cabe a quem chamou informar.
    pub ignore_error: Option<bool>,
    /// Permite sobrescrever o editor usado para exibir a entrada.
    pub override_: Option<EditorOverride>,
    /// Dica de em que contexto o editor abre.
    pub source: Option<EditorOpenSource>,
    /// Indica se o editor abre por uma ação explícita do usuário (`true`) ou automaticamente (`false`).
    /// Quando ausente, quem chamou é tratado como explícito.
    pub is_explicit: Option<bool>,
    /// Estado de visão a aplicar ao abrir o editor (o `object` do original).
    pub view_state: Option<Dynamic>,
    /// Um editor transitório tenta aparecer como prévia, e componentes como o histórico podem ignorá-lo.
    pub transient: Option<bool>,
    /// Opções que só valem quando `AUX_WINDOW_GROUP` é usado para abrir.
    pub auxiliary: Option<AuxiliaryEditorOptions>,
    /// Opções que só valem quando `MODAL_GROUP` é usado para abrir.
    pub modal: Option<ModalEditorPartOptions>,
}

/// `ITextEditorSelection`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextEditorSelection {
    pub start_line_number: f64,
    pub start_column: f64,
    pub end_line_number: Option<f64>,
    pub end_column: Option<f64>,
}

/// `TextEditorSelectionRevealType`: como revelar a seleção de `ITextEditorOptions.selectionRevealType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEditorSelectionRevealType {
    /// Option to scroll vertically or horizontally as necessary and reveal a range centered vertically.
    Center = 0,
    /// Option to scroll vertically or horizontally as necessary and reveal a range centered vertically only if it lies
    /// outside the viewport.
    CenterIfOutsideViewport = 1,
    /// Option to scroll vertically or horizontally as necessary and reveal a range close to the top of the viewport, but
    /// not quite at the top.
    NearTop = 2,
    /// Same as [`Self::NearTop`], only if it lies outside the viewport.
    NearTopIfOutsideViewport = 3,
}

/// `IModalEditorOptions`: opções do editor modal por editor, dadas por uma entrada de editor que quer influenciar como
/// é desenhada dentro da parte de editor modal. Ao contrário de [`ModalEditorPartOptions`], valem para um editor só e
/// saem do editor ativo (não da API de opções da parte).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModalEditorOptions {
    /// Quando verdadeiro, o editor modal desenha um cabeçalho simplificado: usa o fundo do editor, esconde o ícone do
    /// título, remove a borda de baixo e usa uma altura fixa um pouco maior. Útil para editores que trazem o próprio
    /// cabeçalho.
    pub compact_header: Option<bool>,
    /// Se o editor modal pode ser movido para uma área de editores.
    pub can_move_to_editor_area: Option<bool>,
}

/// `IModalEditorOptionsProvider`: interface marcadora das entradas de editor que querem personalizar como são
/// desenhadas quando abertas na parte de editor modal (ver [`ModalEditorOptions`]).
pub trait ModalEditorOptionsProvider {
    fn get_modal_editor_options(&self) -> Option<ModalEditorOptions>;
}

/// `isModalEditorOptionsProvider(obj)`: o `obj: unknown` do original (`undefined`, `null`, um objeto sem o método ou com
/// ele que não é função) é o `None` aqui, porque a checagem estrutural é resolvida pelo tipo: só um valor que
/// implementa [`ModalEditorOptionsProvider`] chega como `Some`.
pub fn is_modal_editor_options_provider(candidate: Option<&dyn ModalEditorOptionsProvider>) -> bool {
    candidate.is_some()
}

/// `canMoveModalEditorToEditorArea(obj)`: o padrão é poder mover, e o editor desliga com `canMoveToEditorArea: false`.
pub fn can_move_modal_editor_to_editor_area(candidate: Option<&dyn ModalEditorOptionsProvider>) -> bool {
    match candidate {
        None => true,
        Some(provider) => provider.get_modal_editor_options().and_then(|options| options.can_move_to_editor_area) != Some(false),
    }
}
