//! Porte de `src/vs/platform/accessibility/browser/accessibilityService.ts`: o `AccessibilityService` do navegador, que
//! resolve leitor de tela, movimento e transparência reduzidos e sublinhado de links a partir da configuração e do sistema.
//!
//! Divergências:
//! - **`mainWindow.matchMedia(query)`** e o `addDisposableListener(matcher, 'change', ...)` são o [`IAccessibilityHost`]
//!   e o [`IMediaQueryList`]: quem hospeda o serviço diz se o sistema pede movimento ou transparência reduzidos e avisa
//!   quando isso muda. `alert` e `status` (`aria.alert` e `aria.status`) também passam pelo hospedeiro.
//! - **`classList.toggle(name, force)`** é `class_list_add` ou `class_list_remove` sobre o `ViewElement`.
//! - **Subclasses.** Os campos `protected` (`_accessibilitySupport`, os emissores) ficam privados: nenhuma subclasse
//!   portada os estende.
//! - O `getValue<T>` tipado é lido como texto (`on`, `off` ou `auto`); qualquer outro valor se comporta como o original
//!   com um valor fora dos três (nenhum teste de igualdade casa).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;

use super::{AccessibilitySupport, IAccessibilityService, context_accessibility_mode_enabled};
use crate::configuration::ConfigurationOverrides;
use crate::configuration::service::IConfigurationService;
use crate::contextkey::contextkey::{IContextKey, IContextKeyService};
use crate::contextview::view_element::ViewElement;
use crate::layout::layout_service::ILayoutService;

/// `MediaQueryList`: o resultado de `matchMedia`.
pub trait IMediaQueryList {
    /// `matches`.
    fn matches(&self) -> bool;
    /// O evento `change`.
    fn on_change(&self) -> Event<()>;
}

/// O que o navegador dá ao serviço: `matchMedia`, `aria.alert` e `aria.status`.
pub trait IAccessibilityHost {
    /// `mainWindow.matchMedia(query)`.
    fn match_media(&self, query: &str) -> Rc<dyn IMediaQueryList>;
    /// `aria.alert(message)`.
    fn alert(&self, message: &str);
    /// `aria.status(message)`.
    fn status(&self, message: &str);
}

/// `'auto' | 'on' | 'off'`, lido como o original lê (o valor cru).
type Tristate = Option<String>;

fn read_tristate(configuration_service: &dyn IConfigurationService, key: &str) -> Tristate {
    match configuration_service.get_value(Some(key), None) {
        Dynamic::String(value) => Some(value.to_string_lossy()),
        _ => None,
    }
}

fn is_reduced(config: &Tristate, system: bool) -> bool {
    config.as_deref() == Some("on") || (config.as_deref() == Some("auto") && system)
}

/// `AccessibilityService`.
pub struct AccessibilityService {
    base: Rc<Disposable>,
    configuration_service: Rc<dyn IConfigurationService>,
    layout_service: Rc<dyn ILayoutService>,
    host: Rc<dyn IAccessibilityHost>,
    accessibility_mode_enabled_context: Rc<dyn IContextKey>,
    accessibility_support: Cell<AccessibilitySupport>,
    on_did_change_screen_reader_optimized: Rc<Emitter<()>>,
    config_motion_reduced: RefCell<Tristate>,
    system_motion_reduced: Cell<bool>,
    on_did_change_reduced_motion: Rc<Emitter<()>>,
    config_transparency_reduced: RefCell<Tristate>,
    system_transparency_reduced: Cell<bool>,
    on_did_change_reduced_transparency: Rc<Emitter<()>>,
    link_underlines_enabled: Cell<bool>,
    on_did_change_link_underline: Rc<Emitter<()>>,
}

impl AccessibilityService {
    pub fn new(
        context_key_service: Rc<dyn IContextKeyService>,
        layout_service: Rc<dyn ILayoutService>,
        configuration_service: Rc<dyn IConfigurationService>,
        host: Rc<dyn IAccessibilityHost>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_screen_reader_optimized = Emitter::new();
        let on_did_change_reduced_motion = Emitter::new();
        let on_did_change_reduced_transparency = Emitter::new();
        let on_did_change_link_underline = Emitter::new();
        base.register(on_did_change_screen_reader_optimized.clone());
        base.register(on_did_change_reduced_motion.clone());
        base.register(on_did_change_reduced_transparency.clone());
        base.register(on_did_change_link_underline.clone());

        let reduce_motion_matcher = host.match_media("(prefers-reduced-motion: reduce)");
        let reduce_transparency_matcher = host.match_media("(prefers-reduced-transparency: reduce)");

        let this = Rc::new(Self {
            base,
            accessibility_mode_enabled_context: context_accessibility_mode_enabled().bind_to(context_key_service.as_ref()),
            accessibility_support: Cell::new(AccessibilitySupport::Unknown),
            on_did_change_screen_reader_optimized,
            config_motion_reduced: RefCell::new(read_tristate(configuration_service.as_ref(), "workbench.reduceMotion")),
            system_motion_reduced: Cell::new(reduce_motion_matcher.matches()),
            on_did_change_reduced_motion,
            config_transparency_reduced: RefCell::new(read_tristate(configuration_service.as_ref(), "workbench.reduceTransparency")),
            system_transparency_reduced: Cell::new(reduce_transparency_matcher.matches()),
            on_did_change_reduced_transparency,
            link_underlines_enabled: Cell::new(configuration_service.get_value(Some("accessibility.underlineLinks"), None).is_truthy()),
            on_did_change_link_underline,
            configuration_service: configuration_service.clone(),
            layout_service,
            host,
        });

        let weak: Weak<Self> = Rc::downgrade(&this);
        this.base.register(configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |event| {
                let Some(this) = weak.upgrade() else { return };
                if event.affects_configuration("editor.accessibilitySupport", None) {
                    this.update_context_key();
                    this.on_did_change_screen_reader_optimized.fire(());
                }
                if event.affects_configuration("workbench.reduceMotion", None) {
                    *this.config_motion_reduced.borrow_mut() = read_tristate(this.configuration_service.as_ref(), "workbench.reduceMotion");
                    this.on_did_change_reduced_motion.fire(());
                }
                if event.affects_configuration("workbench.reduceTransparency", None) {
                    *this.config_transparency_reduced.borrow_mut() = read_tristate(this.configuration_service.as_ref(), "workbench.reduceTransparency");
                    this.on_did_change_reduced_transparency.fire(());
                }
            }
        }));
        this.update_context_key();
        this.base.register(this.on_did_change_screen_reader_optimized.event().listen({
            let weak = weak.clone();
            move |()| {
                if let Some(this) = weak.upgrade() {
                    this.update_context_key();
                }
            }
        }));

        this.init_reduced_motion_listeners(&weak, reduce_motion_matcher);
        this.init_reduced_transparency_listeners(&weak, reduce_transparency_matcher);
        this.init_link_underline_listeners(&weak);
        this
    }

    fn update_context_key(&self) {
        self.accessibility_mode_enabled_context.set(Dynamic::Bool(self.is_screen_reader_optimized()));
    }

    fn update_motion_classes(&self, container: &ViewElement) {
        let reduce = self.is_motion_reduced();
        set_class(container, "monaco-reduce-motion", reduce);
        set_class(container, "monaco-enable-motion", !reduce);
    }

    fn init_reduced_motion_listeners(self: &Rc<Self>, weak: &Weak<Self>, matcher: Rc<dyn IMediaQueryList>) {
        self.base.register(matcher.on_change().listen({
            let weak = weak.clone();
            let matcher = matcher.clone();
            move |()| {
                let Some(this) = weak.upgrade() else { return };
                this.system_motion_reduced.set(matcher.matches());
                if this.config_motion_reduced.borrow().as_deref() == Some("auto") {
                    this.on_did_change_reduced_motion.fire(());
                }
            }
        }));

        let update_root_classes = {
            let weak = weak.clone();
            move || {
                let Some(this) = weak.upgrade() else { return };
                for container in this.layout_service.containers() {
                    this.update_motion_classes(&container);
                }
            }
        };
        update_root_classes();
        self.base.register(self.on_did_change_reduced_motion.event().listen(move |()| update_root_classes()));
        self.base.register(self.layout_service.on_did_add_container().listen({
            let weak = weak.clone();
            move |event| {
                if let Some(this) = weak.upgrade() {
                    this.update_motion_classes(&event.container);
                }
            }
        }));
    }

    fn init_reduced_transparency_listeners(self: &Rc<Self>, weak: &Weak<Self>, matcher: Rc<dyn IMediaQueryList>) {
        self.base.register(matcher.on_change().listen({
            let weak = weak.clone();
            let matcher = matcher.clone();
            move |()| {
                let Some(this) = weak.upgrade() else { return };
                this.system_transparency_reduced.set(matcher.matches());
                if this.config_transparency_reduced.borrow().as_deref() == Some("auto") {
                    this.on_did_change_reduced_transparency.fire(());
                }
            }
        }));

        let update_root_classes = {
            let weak = weak.clone();
            move || {
                let Some(this) = weak.upgrade() else { return };
                let reduce = this.is_transparency_reduced();
                set_class(&this.layout_service.main_container(), "monaco-reduce-transparency", reduce);
            }
        };
        update_root_classes();
        self.base.register(self.on_did_change_reduced_transparency.event().listen(move |()| update_root_classes()));
    }

    fn init_link_underline_listeners(self: &Rc<Self>, weak: &Weak<Self>) {
        self.base.register(self.configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |event| {
                let Some(this) = weak.upgrade() else { return };
                if event.affects_configuration("accessibility.underlineLinks", None) {
                    let enabled = this.configuration_service.get_value(Some("accessibility.underlineLinks"), None).is_truthy();
                    this.link_underlines_enabled.set(enabled);
                    this.on_did_change_link_underline.fire(());
                }
            }
        }));

        let update_link_underline_classes = {
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.upgrade() {
                    set_class(&this.layout_service.main_container(), "underline-links", this.link_underlines_enabled.get());
                }
            }
        };
        update_link_underline_classes();
        self.base.register(self.on_did_change_link_underlines(move || update_link_underline_classes()));
    }

    /// `onDidChangeLinkUnderlines(listener)`.
    pub fn on_did_change_link_underlines(&self, listener: impl Fn() + 'static) -> Rc<dyn IDisposable> {
        self.on_did_change_link_underline.event().listen(move |()| listener())
    }

    /// `getAccessibilitySupportConfigurationValue()`: resolve o escopo na ordem de precedência, sem depender da
    /// resolução por recurso deste serviço global.
    fn get_accessibility_support_configuration_value(&self) -> String {
        let inspected = self.configuration_service.inspect("editor.accessibilitySupport", &ConfigurationOverrides::default());
        [
            inspected.policy_value(),
            inspected.memory_value(),
            inspected.workspace_folder_value(),
            inspected.workspace_value(),
            inspected.user_value(),
            inspected.application_value(),
            inspected.default_value(),
        ]
        .into_iter()
        .find(|value| !value.is_nullish())
        .and_then(|value| match value {
            Dynamic::String(text) => Some(text.to_string_lossy()),
            _ => None,
        })
        .unwrap_or_else(|| "auto".to_owned())
    }
}

/// `classList.toggle(name, force)`.
fn set_class(element: &ViewElement, name: &str, present: bool) {
    if present {
        element.class_list_add(name);
    } else {
        element.class_list_remove(name);
    }
}

impl IDisposable for AccessibilityService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IAccessibilityService for AccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        self.on_did_change_screen_reader_optimized.event()
    }

    fn on_did_change_reduced_motion(&self) -> Event<()> {
        self.on_did_change_reduced_motion.event()
    }

    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        self.on_did_change_reduced_transparency.event()
    }

    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }

    fn is_screen_reader_optimized(&self) -> bool {
        let config = self.get_accessibility_support_configuration_value();
        config == "on" || (config == "auto" && self.accessibility_support.get() == AccessibilitySupport::Enabled)
    }

    fn is_motion_reduced(&self) -> bool {
        is_reduced(&self.config_motion_reduced.borrow(), self.system_motion_reduced.get())
    }

    fn is_transparency_reduced(&self) -> bool {
        is_reduced(&self.config_transparency_reduced.borrow(), self.system_transparency_reduced.get())
    }

    fn get_accessibility_support(&self) -> AccessibilitySupport {
        self.accessibility_support.get()
    }

    fn set_accessibility_support(&self, accessibility_support: AccessibilitySupport) {
        if self.accessibility_support.get() == accessibility_support {
            return;
        }
        self.accessibility_support.set(accessibility_support);
        self.on_did_change_screen_reader_optimized.fire(());
    }

    fn alert(&self, message: &str) {
        self.host.alert(message);
    }

    fn status(&self, message: &str) {
        self.host.status(message);
    }
}
