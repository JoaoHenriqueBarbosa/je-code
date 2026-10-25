//! Porte de `src/vs/base/browser/ui/list/list.ts`: os contratos da lista (delegate de alturas, renderer,
//! eventos) e os tipos de erro e de delegate com cache.
//!
//! Do TypeScript para o Rust: o `TTemplateData` genérico do renderer é um [`TemplateData`] (um `Rc<dyn Any>`
//! que o renderer converte de volta com `downcast_ref`), porque a lista guarda renderers de vários tipos de
//! template ao mesmo tempo (o `IListRenderer<any, any>` do original). O `HTMLElement` do contêiner é o
//! [`NodeId`] do `je-ui`. O `UIEvent` do `browserEvent` é o [`BrowserEvent`].
//! `IListDragAndDrop` e `IListDragOverReaction` estão em [`crate::list::list_dnd`]. `IListTouchEvent`,
//! `IListGestureEvent` e `IListContextMenuEvent` ficam de fora até o `je-ui` ter toque e menu de contexto.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_ui::{KeyData, Modifiers, NodeId, PointerButton, Role};

/// Os renderers de uma lista por `templateId`, compartilhados com o cache de linhas.
pub type RendererMap<T> = Rc<RefCell<HashMap<String, Rc<dyn IListRenderer<T>>>>>;

/// O filtro de elementos que `focusNextPage` e companhia aceitam.
pub type ElementFilter<T> = Rc<dyn Fn(&T) -> bool>;

/// O dado que um renderer guarda por linha (o `templateData`).
pub type TemplateData = Rc<dyn Any>;

/// `IListVirtualDelegate<T>`.
pub trait IListVirtualDelegate<T> {
    fn get_height(&self, element: &T) -> f64;
    fn get_template_id(&self, element: &T) -> String;
    /// `hasDynamicHeight?`: `None` quando o delegate não o define (o original distingue "ausente" de
    /// "devolve falso").
    fn has_dynamic_height(&self, _element: &T) -> Option<bool> {
        None
    }
    /// `getDynamicHeight?`.
    fn get_dynamic_height(&self, _element: &T) -> Option<f64> {
        None
    }
    /// `setDynamicHeight?`.
    fn set_dynamic_height(&self, _element: &T, _height: f64) {}
}

/// `IListElementRenderDetails`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ListElementRenderDetails {
    pub height: Option<f64>,
    pub on_scroll: Option<bool>,
}

/// `IListRenderer<T, TTemplateData>`.
pub trait IListRenderer<T> {
    fn template_id(&self) -> String;
    fn render_template(&self, container: NodeId) -> TemplateData;
    fn render_element(&self, element: &T, index: usize, template_data: &TemplateData, details: Option<&ListElementRenderDetails>);
    fn dispose_element(&self, _element: &T, _index: i64, _template_data: &TemplateData, _details: Option<&ListElementRenderDetails>) {}
    fn dispose_template(&self, template_data: &TemplateData);
}

/// O evento de origem de uma mudança (o `browserEvent?: UIEvent`): o que a lista precisa dele.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrowserEvent {
    pub modifiers: Modifiers,
    pub button: PointerButton,
    pub target: Option<NodeId>,
    /// O `detail` do evento de ponteiro (1 no primeiro clique, 2 no duplo clique); 0 no teclado.
    pub click_count: u32,
    /// `isKeyboardEvent(browserEvent)`.
    pub is_keyboard: bool,
    /// O `__forceEvent` do `Trait.set` da árvore: dispara o evento mesmo sem mudança.
    pub force_event: bool,
}

/// `IListEvent<T>`.
#[derive(Clone, Debug)]
pub struct ListEvent<T> {
    pub elements: Vec<T>,
    pub indexes: Vec<usize>,
    pub browser_event: Option<BrowserEvent>,
}

/// `IListMouseEvent<T>`: `is_handled_by_list` é o `browserEvent.isHandledByList`, compartilhado entre
/// quem recebe o mesmo evento.
#[derive(Clone)]
pub struct ListMouseEvent<T> {
    pub browser_event: BrowserEvent,
    pub is_handled_by_list: Rc<Cell<bool>>,
    pub element: Option<T>,
    pub index: Option<usize>,
}

/// `NotSelectableGroupId`.
pub const NOT_SELECTABLE_GROUP_ID: &str = "notSelectable";

/// O `number | NotSelectableGroupIdType` do `getGroupId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupId {
    Group(i64),
    NotSelectable,
}

/// `IIdentityProvider<T>`.
pub trait IIdentityProvider<T> {
    fn get_id(&self, element: &T) -> String;
    fn get_group_id(&self, _element: &T) -> Option<GroupId> {
        None
    }
}

/// `IListAccessibilityProvider<T>`: o rótulo e o papel ARIA da lista e de cada linha. O `getAriaLevel`, o
/// `isChecked` e o `onDidChangeActiveDescendant` não existem: o `je-ui` guarda só `role` e `label` por nó.
pub trait IListAccessibilityProvider<T> {
    /// `getAriaLabel(element)`: `null` ou vazio tira o rótulo da linha.
    fn get_aria_label(&self, element: &T) -> Option<String>;
    /// `getWidgetAriaLabel()`.
    fn get_widget_aria_label(&self) -> String;
    /// `getWidgetRole?()`: sem ele, `list`.
    fn get_widget_role(&self) -> Option<Role> {
        None
    }
    /// `getRole?(element)`: sem ele, `listitem`.
    fn get_role(&self, _element: &T) -> Option<Role> {
        None
    }
}

/// `ListError`.
pub fn list_error(user: &str, message: &str) -> je_base::errors::Error {
    je_base::errors::Error::new(format!("ListError [{user}] {message}"))
}

/// `CachedListVirtualDelegate<T>`: o cache é por chave de identidade do elemento (o `WeakMap` do original
/// guarda por referência; aqui a chave é dada pelo `key`).
pub struct CachedListVirtualDelegate<T, K: std::hash::Hash + Eq> {
    cache: RefCell<HashMap<K, f64>>,
    key: Box<dyn Fn(&T) -> K>,
    estimate_height: Box<dyn Fn(&T) -> f64>,
    template_id: Box<dyn Fn(&T) -> String>,
}

impl<T, K: std::hash::Hash + Eq> CachedListVirtualDelegate<T, K> {
    pub fn new(key: impl Fn(&T) -> K + 'static, estimate_height: impl Fn(&T) -> f64 + 'static, template_id: impl Fn(&T) -> String + 'static) -> Self {
        Self { cache: RefCell::new(HashMap::new()), key: Box::new(key), estimate_height: Box::new(estimate_height), template_id: Box::new(template_id) }
    }

    pub fn get_cached_height(&self, element: &T) -> Option<f64> {
        self.cache.borrow().get(&(self.key)(element)).copied()
    }
}

impl<T, K: std::hash::Hash + Eq> IListVirtualDelegate<T> for CachedListVirtualDelegate<T, K> {
    fn get_height(&self, element: &T) -> f64 {
        self.get_cached_height(element).unwrap_or_else(|| (self.estimate_height)(element))
    }

    fn get_template_id(&self, element: &T) -> String {
        (self.template_id)(element)
    }

    fn set_dynamic_height(&self, element: &T, height: f64) {
        if height > 0.0 {
            self.cache.borrow_mut().insert((self.key)(element), height);
        }
    }
}

/// `IKeyboardNavigationLabelProvider<T>.getKeyboardNavigationLabel`: um rótulo (ou vários) por elemento.
/// Cada `None` é o `undefined` do original (o elemento não tem rótulo).
pub trait IKeyboardNavigationLabelProvider<T> {
    fn get_keyboard_navigation_label(&self, element: &T) -> Vec<Option<String>>;
}

/// `IKeyboardNavigationDelegate`: diz à lista se uma tecla pode produzir um caractere imprimível (o que liga a navegação
/// por tipo). O `mightProducePrintableCharacter` não é do `IKeyboardNavigationLabelProvider`: o original o põe aqui, e o
/// `StringRepresentationProvider` do peek o declara por conta própria (`referencesTree.ts`). O padrão da lista é o
/// [`DefaultKeyboardNavigationDelegate`](crate::list::type_navigation::DefaultKeyboardNavigationDelegate); o workbench
/// injeta o do `IKeybindingService` (`listService.ts`).
pub trait IKeyboardNavigationDelegate {
    fn might_produce_printable_character(&self, event: &KeyData) -> bool;
}

/// `IKeyboardNavigationEventFilter`: `(e: StandardKeyboardEvent) => boolean`, a condição para uma tecla começar a navegação
/// por tipo (a lista aceita tudo quando não há filtro).
pub type IKeyboardNavigationEventFilter = Rc<dyn Fn(&KeyData) -> bool>;
