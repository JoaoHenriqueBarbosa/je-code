//! O que as partes do workbench (`part.ts`, `component.ts`, `themable.ts`) compartilham e que a barra de status e a barra de
//! título leem do mesmo jeito: a janela alvo, o tema corrente e a decisão de "janela inativa".
//!
//! Divergências:
//! - **`CodeWindow`.** O original lê `targetWindow.vscodeWindowId` e `targetWindow.document.hasFocus()`; o porte leva só isso,
//!   no [`TargetWindow`] (o identificador e a leitura do foco do documento).
//! - **`Themable`.** [`PartTheme`] guarda o `theme` que o `Themable` atualiza em `onThemeChange` e entrega o `getColor(id, modify)`,
//!   que devolve a cor (o chamador a escreve com `toString()` ou, para o `style.backgroundColor`, na forma normalizada do CSSOM).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use je_editor::services::theme_service::IColorTheme;
use je_platform::contextkey::contextkey::IContextKeyServiceTarget;
use je_platform::contextview::view_element::ViewElement;
use je_theme::ThemeType;
use je_theme::color::Color;

use crate::contrib::modern_ui::resolved_style::css_rgb;
use crate::services::host::host::IHostService;
use crate::services::layout::layout_service::{LayoutContainerId, WindowId};

/// O que as partes escrevem no estilo do contêiner de uma janela (`layoutService.getContainer(window).style.setProperty(...)`).
/// O contêiner é o identificador opaco do layout; quem o desenha resolve o nó e guarda a propriedade (valor vazio a remove).
pub trait ContainerStyleHost {
    /// `container.style.setProperty(name, value)`.
    fn set_property(&self, container: LayoutContainerId, name: &str, value: &str);
}

/// `WORKBENCH_BACKGROUND(theme)` de `workbench/common/theme.ts`.
pub fn workbench_background(theme: &dyn IColorTheme) -> Color {
    match theme.theme_type() {
        ThemeType::Light => Color::from_hex("#F3F3F3"),
        ThemeType::HighContrastLight => Color::from_hex("#FFFFFF"),
        ThemeType::HighContrastDark => Color::from_hex("#000000"),
        _ => Color::from_hex("#252526"),
    }
}

/// O `HTMLElement` que `contextKeyService.createScoped(element)` recebe: os atributos que o serviço escopado lê e escreve. O
/// elemento nativo não tem ancestrais com contexto próprio, então `parentElement` é `None` (o serviço escopado fica sob o raiz).
pub struct ViewElementTarget {
    pub element: Rc<ViewElement>,
    attributes: RefCell<BTreeMap<String, String>>,
}

impl ViewElementTarget {
    pub fn new(element: Rc<ViewElement>) -> Rc<Self> {
        Rc::new(Self { element, attributes: RefCell::new(BTreeMap::new()) })
    }
}

impl IContextKeyServiceTarget for ViewElementTarget {
    fn parent_element(&self) -> Option<Rc<dyn IContextKeyServiceTarget>> {
        None
    }

    fn set_attribute(&self, attr: &str, value: &str) {
        self.attributes.borrow_mut().insert(attr.to_owned(), value.to_owned());
    }

    fn remove_attribute(&self, attr: &str) {
        self.attributes.borrow_mut().remove(attr);
    }

    fn has_attribute(&self, attr: &str) -> bool {
        self.attributes.borrow().contains_key(attr)
    }

    fn get_attribute(&self, attr: &str) -> Option<String> {
        self.attributes.borrow().get(attr).cloned()
    }
}

/// `element.style.<propriedade> = valor`: o valor vazio remove a propriedade, como o CSSOM.
pub fn set_style_or_remove(element: &ViewElement, property: &str, value: Option<&str>) {
    match value.filter(|value| !value.is_empty()) {
        Some(value) => element.style_set_property(property, value),
        None => element.style_remove_property(property),
    }
}

/// O que `Themable.getColor(id)` devolve (`color.toString()`): o texto cru ([`ColorValue::raw`], o `#rrggbb` que entra em
/// propriedades personalizadas e em folhas de estilo) e o texto que o CSSOM devolve depois de uma atribuição a `style.<cor>`
/// ([`ColorValue::css`], o `rgb(...)`).
#[derive(Clone, Debug)]
pub struct ColorValue(Color);

impl ColorValue {
    pub fn new(color: Color) -> Self {
        Self(color)
    }

    /// `color.toString()`.
    pub fn raw(&self) -> String {
        self.0.to_string()
    }

    /// O valor normalizado do CSSOM para `style.backgroundColor = color`.
    pub fn css(&self) -> String {
        css_rgb(&self.0)
    }

    pub fn color(&self) -> &Color {
        &self.0
    }
}

/// O `CodeWindow` no que as partes leem: `vscodeWindowId` e `document.hasFocus()`.
#[derive(Clone)]
pub struct TargetWindow {
    /// `vscodeWindowId`.
    pub id: WindowId,
    /// `document.hasFocus()`.
    pub has_focus: Rc<dyn Fn() -> bool>,
}

/// O `modify` de `Themable.getColor(id, modify)`.
pub type ColorModifier<'a> = &'a dyn Fn(Color, &dyn IColorTheme) -> Color;

/// O `theme` do `Themable`.
pub struct PartTheme {
    theme: RefCell<Rc<dyn IColorTheme>>,
}

impl PartTheme {
    pub fn new(theme: Rc<dyn IColorTheme>) -> Self {
        Self { theme: RefCell::new(theme) }
    }

    /// `this.theme = theme`.
    pub fn set(&self, theme: Rc<dyn IColorTheme>) {
        *self.theme.borrow_mut() = theme;
    }

    /// `this.theme`.
    pub fn current(&self) -> Rc<dyn IColorTheme> {
        self.theme.borrow().clone()
    }

    /// `getColor(id, modify)`: a cor do tema, depois do `modify`, ou `None` (o `null` do original).
    pub fn get_color(&self, id: &str, modify: Option<ColorModifier<'_>>) -> Option<Color> {
        let theme = self.current();
        let color = theme.get_color(id, None)?;
        Some(match modify {
            Some(modify) => modify(color, theme.as_ref()),
            None => color,
        })
    }
}

/// O par `isWindowActive` / `isInactive` do `updateStyles` das partes: com a janela ativa ainda desconhecida vale o foco do
/// documento da janela alvo, senão a igualdade com a janela ativa; a parte está inativa quando o host perdeu o foco ou a janela
/// alvo não é a ativa.
pub fn is_part_inactive(active_window_id: Option<u32>, window: &TargetWindow, host_service: &dyn IHostService) -> bool {
    let is_window_active = match active_window_id {
        None => (window.has_focus)(),
        Some(active_window_id) => active_window_id == window.id.0,
    };
    !host_service.has_focus() || !is_window_active
}
