//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViewInterface.ts`:
//! `InlineEditTabAction`, `InlineEditClickEvent` e `IInlineEditsView`.
//!
//! Notas de tradução:
//! - `InlineCompletionViewKind` e `InlineCompletionViewData` (que o original declara neste arquivo) já vivem em
//!   `inline_completion_view_data` e são reexportados daqui, para quem importa do arquivo do original;
//! - `IMouseEvent` (`vs/base/browser/mouseEvent.ts`) é o [`MouseEvent`] de `editor_browser`. O
//!   `InlineEditClickEvent.create(event, alternativeAction)` só embrulha o `PointerEvent` do navegador num
//!   `StandardMouseEvent`; o evento nativo já chega como [`MouseEvent`], então a criação é [`InlineEditClickEvent::new`];
//! - `IInlineEditsView` é o trait [`IInlineEditsView`]; o `minEditorScrollHeight?` opcional é um método com valor
//!   padrão (`None`).

use je_base::event::Event;
use je_base::observable::base::IObservable;
use je_base::observable::deps::StrictEquals;

use crate::editor_browser::MouseEvent;

pub use crate::inline_completions::inline_completion_view_data::{InlineCompletionViewData, InlineCompletionViewKind};

/// `InlineEditTabAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InlineEditTabAction {
    Jump,
    Accept,
    Inactive,
}

impl InlineEditTabAction {
    /// O valor textual do membro (`'jump'`, `'accept'`, `'inactive'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jump => "jump",
            Self::Accept => "accept",
            Self::Inactive => "inactive",
        }
    }
}

impl StrictEquals for InlineEditTabAction {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// `InlineEditClickEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineEditClickEvent {
    pub event: MouseEvent,
    pub alternative_action: bool,
}

impl InlineEditClickEvent {
    /// O construtor (e o `create`, ver as notas do módulo); `alternative_action` é falso por padrão no original.
    pub fn new(event: MouseEvent, alternative_action: bool) -> Self {
        Self { event, alternative_action }
    }
}

/// `IInlineEditsView`.
pub trait IInlineEditsView {
    /// `isHovered`.
    fn is_hovered(&self) -> IObservable<bool>;

    /// `minEditorScrollHeight` (opcional).
    fn min_editor_scroll_height(&self) -> Option<IObservable<f64>> {
        None
    }

    /// `onDidClick`.
    fn on_did_click(&self) -> Event<InlineEditClickEvent>;
}
