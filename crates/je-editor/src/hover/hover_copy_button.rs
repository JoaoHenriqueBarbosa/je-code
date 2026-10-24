//! Porte de `src/vs/editor/contrib/hover/browser/hoverCopyButton.ts`: o botão que aparece nas partes do hover
//! para copiar o conteúdo, e o `SimpleButton` de `src/vs/editor/contrib/find/browser/findWidget.ts` de que ele
//! depende (o comportamento do botão: classes, papel, `tabindex`, `aria-label`, disparo por clique, Enter e
//! Espaço).
//!
//! Diferenças do original, nenhuma de comportamento:
//! - Não há DOM: o `HTMLElement` que recebe o botão é o [`HoverContainer`] (a lista de classes e os botões
//!   filhos), e o `div` do botão é o próprio [`SimpleButton`], com os atributos que o original grava nele.
//!   `container.querySelector('.hover-copy-button')` é [`HoverContainer::query_selector`] e o clique e o
//!   `keydown` são [`SimpleButton::click`] e [`SimpleButton::key_down`] (o `StandardKeyboardEvent` do original
//!   já traduziu o `keyCode` do navegador para o [`KeyCode`]).
//! - O `IHoverService` só liga a dica atrasada (`setupDelayedHover`) ao `div`: a UI nativa mostra a dica no
//!   widget, então o serviço não é parâmetro.
//! - O `status(...)` global de `aria.ts` (anúncio para leitor de tela) é o parâmetro `status`, chamado com o
//!   texto "Copied to clipboard" depois da cópia.
//! - `localize('hover.copy', "Copy")` e o `hover.copied` são os textos em inglês do original (a camada de
//!   tradução não existe).
//! - `onKeyDown` opcional do `ISimpleButtonOpts` é `Option<Box<dyn Fn(KeyCode)>>`; `hoverLifecycleOptions`
//!   não existe (só a dica atrasada o lê).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_base::themables::ThemeIcon;

use crate::services::clipboard_service::IClipboardService;

/// `ISimpleButtonOpts`.
pub struct SimpleButtonOptions {
    pub label: String,
    pub icon: Option<ThemeIcon>,
    pub on_trigger: Box<dyn Fn()>,
    pub on_key_down: Option<Box<dyn Fn(KeyCode)>>,
    pub class_name: Option<String>,
}

/// `SimpleButton`.
pub struct SimpleButton {
    opts: SimpleButtonOptions,
    class_name: RefCell<String>,
    tab_index: Cell<i32>,
    aria_disabled: Cell<Option<bool>>,
}

impl SimpleButton {
    pub fn new(opts: SimpleButtonOptions) -> Rc<Self> {
        let mut class_name = String::from("button");
        if let Some(extra) = &opts.class_name {
            class_name = format!("{class_name} {extra}");
        }
        if let Some(icon) = &opts.icon {
            class_name = format!("{class_name} {}", icon.as_class_name());
        }
        Rc::new(Self { opts, class_name: RefCell::new(class_name), tab_index: Cell::new(0), aria_disabled: Cell::new(None) })
    }

    /// `className`.
    pub fn class_name(&self) -> String {
        self.class_name.borrow().clone()
    }

    /// `classList.contains(name)`.
    pub fn has_class(&self, name: &str) -> bool {
        self.class_name.borrow().split_whitespace().any(|class| class == name)
    }

    /// `getAttribute('role')`.
    pub fn role(&self) -> &'static str {
        "button"
    }

    /// `getAttribute('aria-label')`.
    pub fn aria_label(&self) -> &str {
        &self.opts.label
    }

    /// `getAttribute('aria-disabled')`.
    pub fn aria_disabled(&self) -> Option<bool> {
        self.aria_disabled.get()
    }

    /// `tabIndex`.
    pub fn tab_index(&self) -> i32 {
        self.tab_index.get()
    }

    /// `isEnabled()`.
    pub fn is_enabled(&self) -> bool {
        self.tab_index.get() >= 0
    }

    /// `setEnabled(enabled)`.
    pub fn set_enabled(&self, enabled: bool) {
        let mut class_name = self.class_name.borrow_mut();
        let without_disabled: Vec<&str> = class_name.split_whitespace().filter(|class| *class != "disabled").collect();
        let updated = if enabled { without_disabled.join(" ") } else { [without_disabled, vec!["disabled"]].concat().join(" ") };
        *class_name = updated;
        self.aria_disabled.set(Some(!enabled));
        self.tab_index.set(if enabled { 0 } else { -1 });
    }

    /// O clique: dispara `onTrigger`.
    pub fn click(&self) {
        (self.opts.on_trigger)();
    }

    /// O `keydown`: Espaço e Enter disparam `onTrigger`; as demais teclas vão para `onKeyDown`.
    pub fn key_down(&self, key_code: KeyCode) {
        if key_code == KeyCode::Space || key_code == KeyCode::Enter {
            (self.opts.on_trigger)();
            return;
        }
        if let Some(on_key_down) = &self.opts.on_key_down {
            on_key_down(key_code);
        }
    }
}

/// O elemento que recebe o botão do hover: a lista de classes e os filhos (ver o topo do módulo).
#[derive(Default)]
pub struct HoverContainer {
    class_list: RefCell<Vec<String>>,
    children: RefCell<Vec<Rc<SimpleButton>>>,
}

impl HoverContainer {
    pub fn new() -> Self {
        Self::default()
    }

    /// `classList.add(name)`.
    pub fn add_class(&self, name: &str) {
        let mut class_list = self.class_list.borrow_mut();
        if !class_list.iter().any(|class| class == name) {
            class_list.push(name.to_string());
        }
    }

    /// `classList.contains(name)`.
    pub fn has_class(&self, name: &str) -> bool {
        self.class_list.borrow().iter().any(|class| class == name)
    }

    /// `appendChild(button.domNode)`.
    pub fn append_child(&self, button: Rc<SimpleButton>) {
        self.children.borrow_mut().push(button);
    }

    /// `querySelector('.class_name')`: o primeiro botão filho com a classe.
    pub fn query_selector(&self, class_name: &str) -> Option<Rc<SimpleButton>> {
        self.children.borrow().iter().find(|child| child.has_class(class_name)).cloned()
    }
}

/// `HoverCopyButton`: um botão que aparece nas partes do hover para copiar o conteúdo para a área de
/// transferência.
pub struct HoverCopyButton {
    button: Rc<SimpleButton>,
}

impl HoverCopyButton {
    pub fn new(
        container: &HoverContainer,
        get_content: impl Fn() -> String + 'static,
        clipboard_service: Rc<dyn IClipboardService>,
        status: impl Fn(&str) + 'static,
    ) -> Self {
        container.add_class("hover-row-with-copy");

        let status = Rc::new(status);
        let button = SimpleButton::new(SimpleButtonOptions {
            label: "Copy".to_string(),
            icon: Some(ThemeIcon::from_id("copy")),
            on_trigger: Box::new(move || {
                // `_copyContent`: só copia quando há conteúdo
                let content = get_content();
                if !content.is_empty() {
                    let status = status.clone();
                    clipboard_service.write_text(&content).then(move |()| status("Copied to clipboard"));
                }
            }),
            on_key_down: None,
            class_name: Some("hover-copy-button".to_string()),
        });

        container.append_child(button.clone());
        Self { button }
    }

    /// O botão (o `_button` do original).
    pub fn button(&self) -> &Rc<SimpleButton> {
        &self.button
    }
}

impl IDisposable for HoverCopyButton {
    fn dispose(&self) {}
}
