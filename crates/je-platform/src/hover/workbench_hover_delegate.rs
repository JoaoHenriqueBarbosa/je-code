//! Porte do `WorkbenchHoverDelegate` de `src/vs/platform/hover/browser/hover.ts`: o delegate que mostra o hover pelo
//! [`HoverService`] com o `hideOnKeyDown`, o `id` pelo texto, o `compact`, o atraso de `workbench.hover.delay`, o modo de
//! hover instantâneo e o que o chamador sobrescreve.
//!
//! Divergências de tradução:
//! - **`Partial<IHoverOptions>`** é [`HoverOverrides`] (cada campo `Option`; `Some` é a propriedade presente).
//! - **`overrideOptions` função ou objeto** é [`HoverOverrideOptions`].
//! - **`IHoverService`** é o [`HoverService`] concreto, e o `addStandardDisposableListener(target, 'keydown')` passa pelo
//!   [`IHoverDom`] que o serviço usa.
//! - **`Date.now()`** é um [`Instant`]: só a diferença entre duas leituras importa, e ela não depende do relógio de parede.
//! - **`delay`** é o [`HoverDelay`]: número fixo, ou a função de `dynamicDelay`.
//! - **`nativeHoverDelegate`** é [`NativeHoverDelegate`] (o `showNativeHover: true` com atraso 0); o `showHover` dele lança no original
//!   e aqui o trait [`IHoverShower`] não é implementado para ele.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use je_base::dynamic::Dynamic;
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};

use super::hover_service::{
    HoverContent, HoverDelay, HoverDomEventKind, HoverDomTarget, HoverPlacement, HoverService, HoverStyle, HoverTarget, IHoverAction,
    IHoverAppearanceOptions, IHoverDelegate, IHoverDom, IHoverOptions, IHoverPersistenceOptions, IHoverPositionOptions, IHoverWidget,
    ManagedHoverContentOrFactory,
};
use super::updatable_hover_widget::IHoverShower;
use crate::configuration::service::IConfigurationService;
use crate::contextview::view_element::ViewElement;

const HOVER_DELAY_SETTING: &str = "workbench.hover.delay";

/// `IHoverDelayOptions`.
#[derive(Clone, Default)]
pub struct IHoverDelayOptions {
    pub instant_hover: bool,
    pub dynamic_delay: Option<Rc<dyn Fn(Option<&ManagedHoverContentOrFactory>) -> Option<f64>>>,
}

/// `Partial<IHoverOptions>`.
#[derive(Clone, Default)]
pub struct HoverOverrides {
    pub content: Option<HoverContent>,
    pub target: Option<HoverTarget>,
    pub container: Option<Rc<ViewElement>>,
    pub id: Option<String>,
    pub actions: Option<Vec<IHoverAction>>,
    pub additional_classes: Option<Vec<String>>,
    pub link_handler: Option<Rc<dyn Fn(&str)>>,
    pub trap_focus: Option<bool>,
    pub style: Option<HoverStyle>,
    pub position: Option<IHoverPositionOptions>,
    pub persistence: Option<IHoverPersistenceOptions>,
    pub appearance: Option<IHoverAppearanceOptions>,
    pub on_did_show: Option<Rc<dyn Fn()>>,
    pub on_did_hide: Option<Rc<dyn Fn()>>,
}

/// `Partial<IHoverOptions> | ((options: IHoverDelegateOptions, focus?: boolean) => Partial<IHoverOptions>)`.
#[derive(Clone)]
pub enum HoverOverrideOptions {
    Fixed(HoverOverrides),
    Dynamic(Rc<dyn Fn(&IHoverOptions, bool) -> HoverOverrides>),
}

impl Default for HoverOverrideOptions {
    fn default() -> Self {
        HoverOverrideOptions::Fixed(HoverOverrides::default())
    }
}

/// `WorkbenchHoverDelegate`.
pub struct WorkbenchHoverDelegate {
    base: Rc<Disposable>,
    hover_service: Rc<HoverService>,
    dom: Rc<dyn IHoverDom>,
    placement: HoverPlacement,
    hover_options: Option<IHoverDelayOptions>,
    override_options: HoverOverrideOptions,
    last_hover_hide_time: Cell<Option<Instant>>,
    time_limit: Cell<Duration>,
    delay_setting: Rc<Cell<f64>>,
    hover_disposables: Rc<DisposableStore>,
}

/// `getValue<number>('workbench.hover.delay')`.
fn read_delay(configuration_service: &dyn IConfigurationService) -> f64 {
    match configuration_service.get_value(Some(HOVER_DELAY_SETTING), None) {
        Dynamic::Number(value) => value,
        _ => 0.0,
    }
}

impl WorkbenchHoverDelegate {
    pub fn new(
        placement: HoverPlacement,
        hover_options: Option<IHoverDelayOptions>,
        override_options: HoverOverrideOptions,
        configuration_service: Rc<dyn IConfigurationService>,
        hover_service: Rc<HoverService>,
        dom: Rc<dyn IHoverDom>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let hover_disposables = DisposableStore::new();
        base.register(hover_disposables.clone());
        let delay_setting = Rc::new(Cell::new(read_delay(&*configuration_service)));
        let this = Rc::new(Self {
            base: base.clone(),
            hover_service,
            dom,
            placement,
            hover_options,
            override_options,
            last_hover_hide_time: Cell::new(None),
            time_limit: Cell::new(Duration::from_millis(200)),
            delay_setting: delay_setting.clone(),
            hover_disposables,
        });
        let service = configuration_service.clone();
        base.register(configuration_service.on_did_change_configuration().listen(move |event| {
            if event.affects_configuration(HOVER_DELAY_SETTING, None) {
                delay_setting.set(read_delay(&*service));
            }
        }));
        this
    }

    /// `private isInstantlyHovering()`.
    fn is_instantly_hovering(&self) -> bool {
        self.hover_options.as_ref().is_some_and(|options| options.instant_hover)
            && self.last_hover_hide_time.get().is_some_and(|time| time.elapsed() < self.time_limit.get())
    }

    /// `setInstantHoverTimeLimit(timeLimit)`: lança (devolve o erro) quando o hover instantâneo não está ligado.
    pub fn set_instant_hover_time_limit(&self, time_limit: Duration) -> Result<(), je_base::errors::Error> {
        if !self.hover_options.as_ref().is_some_and(|options| options.instant_hover) {
            return Err(je_base::errors::Error::new("Instant hover is not enabled"));
        }
        self.time_limit.set(time_limit);
        Ok(())
    }

    /// `{ ...options, ...overrideOptions, persistence, id, appearance }` de `showHover`.
    fn merged_options(&self, options: IHoverOptions, focus: bool) -> IHoverOptions {
        let overrides = match &self.override_options {
            HoverOverrideOptions::Fixed(overrides) => overrides.clone(),
            HoverOverrideOptions::Dynamic(overrides) => overrides(&options, focus),
        };
        let id = match &options.content {
            HoverContent::Element(_) => None,
            HoverContent::Text(text) => Some(text.clone()),
            HoverContent::Markdown(markdown) => Some(markdown.value.to_string()),
        };
        let appearance_base = options.appearance.unwrap_or_default();
        let persistence = overrides.persistence.unwrap_or_default();
        let appearance = overrides.appearance.unwrap_or_default();
        let skip_fade_in_animation = self.is_instantly_hovering();

        let mut merged = options.respread();
        if let Some(content) = overrides.content {
            merged.content = content;
        }
        if let Some(target) = overrides.target {
            merged.target = target;
        }
        if overrides.container.is_some() {
            merged.container = overrides.container;
        }
        if let Some(actions) = overrides.actions {
            merged.actions = actions;
        }
        if let Some(additional_classes) = overrides.additional_classes {
            merged.additional_classes = additional_classes;
        }
        if overrides.link_handler.is_some() {
            merged.link_handler = overrides.link_handler;
        }
        if let Some(trap_focus) = overrides.trap_focus {
            merged.trap_focus = trap_focus;
        }
        if overrides.style.is_some() {
            merged.style = overrides.style;
        }
        if overrides.position.is_some() {
            merged.position = overrides.position;
        }
        if overrides.on_did_show.is_some() {
            merged.on_did_show = overrides.on_did_show;
        }
        if overrides.on_did_hide.is_some() {
            merged.on_did_hide = overrides.on_did_hide;
        }
        merged.persistence = Some(IHoverPersistenceOptions {
            hide_on_hover: persistence.hide_on_hover,
            hide_on_key_down: persistence.hide_on_key_down.or(Some(true)),
            sticky: persistence.sticky,
        });
        merged.id = id;
        merged.appearance = Some(IHoverAppearanceOptions {
            show_pointer: appearance.show_pointer.or(appearance_base.show_pointer),
            compact: appearance.compact.or(Some(true)),
            show_hover_hint: appearance.show_hover_hint.or(appearance_base.show_hover_hint),
            skip_fade_in_animation: appearance.skip_fade_in_animation.or(Some(skip_fade_in_animation)),
            max_height_ratio: appearance.max_height_ratio.or(appearance_base.max_height_ratio),
        });
        merged
    }
}

impl IHoverDelegate for WorkbenchHoverDelegate {
    fn delay(&self) -> HoverDelay {
        if self.is_instantly_hovering() {
            return HoverDelay::Fixed(0.0); // show instantly when a hover was recently shown
        }
        if let Some(dynamic_delay) = self.hover_options.as_ref().and_then(|options| options.dynamic_delay.clone()) {
            let delay = self.delay_setting.clone();
            return HoverDelay::Dynamic(Rc::new(move |content| dynamic_delay(Some(content)).unwrap_or_else(|| delay.get())));
        }
        HoverDelay::Fixed(self.delay_setting.get())
    }

    fn placement(&self) -> Option<HoverPlacement> {
        Some(self.placement)
    }

    fn on_did_hide_hover(&self) {
        self.hover_disposables.clear();
        if self.hover_options.as_ref().is_some_and(|options| options.instant_hover) {
            self.last_hover_hide_time.set(Some(Instant::now()));
        }
    }
}

impl IHoverShower for WorkbenchHoverDelegate {
    fn show_hover(&self, options: IHoverOptions, focus: bool) -> Option<Rc<dyn IHoverWidget>> {
        // close hover on escape
        self.hover_disposables.clear();
        for target in options.target.target_elements() {
            let service = self.hover_service.clone();
            self.hover_disposables.add(self.dom.add_listener(
                HoverDomTarget::Element(target),
                HoverDomEventKind::KeyDown,
                false,
                Rc::new(move |event| {
                    if event.key_code == KeyCode::Escape {
                        service.hide_hover(false);
                    }
                }),
            ));
        }
        let merged = self.merged_options(options, focus);
        let hover = self.hover_service.show_instant_hover(merged, focus, false)?;
        Some(hover as Rc<dyn IHoverWidget>)
    }
}

impl IDisposable for WorkbenchHoverDelegate {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "WorkbenchHoverDelegate"
    }
}

/// `nativeHoverDelegate`: `delay: 0` e `showNativeHover: true`.
pub struct NativeHoverDelegate;

impl IHoverDelegate for NativeHoverDelegate {
    fn delay(&self) -> HoverDelay {
        HoverDelay::Fixed(0.0)
    }

    fn show_native_hover(&self) -> bool {
        true
    }
}
