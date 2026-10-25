//! Porte de `src/vs/workbench/contrib/modernUI/electron-browser/frostedGlass.contribution.ts`: a contribuição que liga o
//! vidro fosco nos contêineres do workbench (a classe `modern-ui-frosted-glass` e a variável `--modern-ui-glass-opacity`),
//! conforme a configuração, a transparência reduzida, o tema de alto contraste e a composição por GPU.
//!
//! Recortes:
//! - O `import '../browser/media/frostedGlass.css'` é a especificação do material, portada em [`super::frosted_glass`].
//! - O `registerWorkbenchContribution2(FrostedGlassContribution.ID, ..., WorkbenchPhase.AfterRestored)` fica com quem monta
//!   o workbench; o `ID` é [`FrostedGlassContribution::ID`].
//!
//! Divergências:
//! - **O DOM.** `container.classList.toggle`, `container.style.setProperty` e `container.style.removeProperty` não têm
//!   equivalente: os contêineres são [`LayoutContainerId`] opacos, e [`FrostedGlassHost`] reúne o que a contribuição pede
//!   de `IWorkbenchLayoutService` (`containers` e `onDidAddContainer`) e o que ela faz em cada contêiner (a classe e a
//!   variável de estilo).
//! - **`INativeHostService`.** O serviço de host nativo não foi portado por inteiro: [`GpuCompositingHost`] é o recorte que
//!   a contribuição lê (`isGPUCompositingEnabled` e `onDidChangeGPUCompositing`).
//! - **`IThemeService`.** A contribuição só lê `getColorTheme().type` e `onDidColorThemeChange`; [`FrostedGlassThemeService`]
//!   é esse recorte, com o tipo do tema como [`ThemeType`] (o `ColorScheme`). `isHighContrast` é [`is_high_contrast`].
//! - `async initializeGPUCompositing` com `try`/`catch` é o `then_result` da promessa de `isGPUCompositingEnabled`: o
//!   `Err` é o `catch`. `this._store.isDisposed` é o `disposed` da contribuição.
//! - `logService.warn(message, error)` é `warn_args` com o erro como argumento; `warn(message)` é `warn`.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::numbers::clamp;
use je_base::promise::Promise;
use je_platform::accessibility::IAccessibilityService;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_theme::ThemeType;

use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::layout::layout_service::{AddedContainer, LayoutContainerId, LayoutSettings, ModernUiFrostedGlassOpacity};

/// `FROSTED_GLASS_CLASS`.
pub const FROSTED_GLASS_CLASS: &str = "modern-ui-frosted-glass";

/// `FROSTED_GLASS_OPACITY_PROPERTY`.
pub const FROSTED_GLASS_OPACITY_PROPERTY: &str = "--modern-ui-glass-opacity";

/// `isHighContrast(scheme)` de `platform/theme/common/theme.ts`.
pub fn is_high_contrast(scheme: ThemeType) -> bool {
    matches!(scheme, ThemeType::HighContrastDark | ThemeType::HighContrastLight)
}

/// O que a contribuição pede ao workbench (ver as divergências do módulo).
pub trait FrostedGlassHost {
    /// `layoutService.onDidAddContainer`.
    fn on_did_add_container(&self) -> Event<AddedContainer>;

    /// `layoutService.containers`.
    fn containers(&self) -> Vec<LayoutContainerId>;

    /// `container.classList.toggle(className, force)`.
    fn toggle_class(&self, container: LayoutContainerId, class_name: &str, force: bool);

    /// `container.style.setProperty(name, value)`.
    fn set_style_property(&self, container: LayoutContainerId, name: &str, value: &str);

    /// `container.style.removeProperty(name)`.
    fn remove_style_property(&self, container: LayoutContainerId, name: &str);
}

/// O recorte de `INativeHostService` que a contribuição lê.
pub trait GpuCompositingHost {
    /// `isGPUCompositingEnabled()`.
    fn is_gpu_compositing_enabled(&self) -> Promise<bool>;

    /// `onDidChangeGPUCompositing`.
    fn on_did_change_gpu_compositing(&self) -> Event<bool>;
}

/// O recorte de `IThemeService` que a contribuição lê.
pub trait FrostedGlassThemeService {
    /// `getColorTheme().type`.
    fn color_theme_type(&self) -> ThemeType;

    /// `onDidColorThemeChange`.
    fn on_did_color_theme_change(&self) -> Event<()>;
}

pub struct FrostedGlassContribution {
    base: Rc<Disposable>,
    configuration_service: Rc<dyn IConfigurationService>,
    layout_service: Rc<dyn FrostedGlassHost>,
    accessibility_service: Rc<dyn IAccessibilityService>,
    theme_service: Rc<dyn FrostedGlassThemeService>,
    native_host_service: Rc<dyn GpuCompositingHost>,
    log_service: LogServiceRef,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    gpu_compositing_enabled: Cell<Option<bool>>,
    requested: Cell<bool>,
    opacity: Cell<f64>,
    disposed: Cell<bool>,
}

impl FrostedGlassContribution {
    pub const ID: &'static str = "workbench.contrib.frostedGlass";

    pub fn new(
        configuration_service: Rc<dyn IConfigurationService>,
        layout_service: Rc<dyn FrostedGlassHost>,
        accessibility_service: Rc<dyn IAccessibilityService>,
        theme_service: Rc<dyn FrostedGlassThemeService>,
        native_host_service: Rc<dyn GpuCompositingHost>,
        log_service: LogServiceRef,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    ) -> Rc<Self> {
        let this = Rc::new(Self {
            base: Disposable::new(),
            configuration_service,
            layout_service,
            accessibility_service,
            theme_service,
            native_host_service,
            log_service,
            environment_service,
            gpu_compositing_enabled: Cell::new(None),
            requested: Cell::new(false),
            opacity: Cell::new(f64::from(ModernUiFrostedGlassOpacity::Default as i32)),
            disposed: Cell::new(false),
        });

        Self::listen(&this, this.configuration_service.on_did_change_configuration(), |this, e: &Rc<ConfigurationChangeEvent>| {
            if [LayoutSettings::ModernUi, LayoutSettings::ModernUiFrostedGlass, LayoutSettings::ModernUiFrostedGlassOpacity]
                .iter()
                .any(|setting| e.affects_configuration(setting.as_str(), None))
            {
                this.update();
            }
        });
        Self::listen(&this, this.accessibility_service.on_did_change_reduced_transparency(), |this, _: &()| this.update());
        Self::listen(&this, this.theme_service.on_did_color_theme_change(), |this, _: &()| this.update());
        Self::listen(&this, this.native_host_service.on_did_change_gpu_compositing(), |this, enabled: &bool| {
            this.gpu_compositing_enabled.set(Some(*enabled));
            this.apply();
        });
        Self::listen(&this, this.layout_service.on_did_add_container(), |this, added: &AddedContainer| {
            this.apply_to(added.container);
            let layout_service = this.layout_service.clone();
            let container = added.container;
            added.disposables.add(to_disposable(move || {
                layout_service.toggle_class(container, FROSTED_GLASS_CLASS, false);
                layout_service.remove_style_property(container, FROSTED_GLASS_OPACITY_PROPERTY);
            }));
        });

        this.update();
        this.initialize_gpu_compositing();
        this
    }

    /// `this._register(event(handler))`, com o `this` da contribuição como referência fraca.
    fn listen<T: 'static>(this: &Rc<Self>, event: Event<T>, handler: impl Fn(&Rc<Self>, &T) + 'static) {
        let weak: Weak<Self> = Rc::downgrade(this);
        this.base.register(event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        }));
    }

    fn get_opacity(&self) -> f64 {
        let default_opacity = f64::from(ModernUiFrostedGlassOpacity::Default as i32);
        let key = LayoutSettings::ModernUiFrostedGlassOpacity.as_str();
        let value = self.configuration_service.get_value(Some(key), None);
        if value.is_undefined() {
            return default_opacity;
        }
        let Dynamic::Number(value) = value else {
            return self.invalid_opacity(key, default_opacity);
        };
        if !value.is_finite() {
            return self.invalid_opacity(key, default_opacity);
        }
        let opacity = clamp(value, f64::from(ModernUiFrostedGlassOpacity::Minimum as i32), f64::from(ModernUiFrostedGlassOpacity::Maximum as i32));
        if opacity != value {
            self.log_service.warn(&format!("{key} is out of range. Clamping background opacity to {}%.", number_to_js_string(opacity)));
        }
        opacity
    }

    fn invalid_opacity(&self, key: &str, default_opacity: f64) -> f64 {
        self.log_service.warn(&format!("Invalid {key}: expected a finite number. Using the default background opacity."));
        default_opacity
    }

    fn is_true(&self, setting: LayoutSettings) -> bool {
        matches!(self.configuration_service.get_value(Some(setting.as_str()), None), Dynamic::Bool(true))
    }

    fn update(&self) {
        self.opacity.set(self.get_opacity());
        self.requested.set(
            (self.environment_service.is_sessions_window() || self.is_true(LayoutSettings::ModernUi))
                && self.is_true(LayoutSettings::ModernUiFrostedGlass)
                && !self.accessibility_service.is_transparency_reduced()
                && !is_high_contrast(self.theme_service.color_theme_type()),
        );
        self.apply();
    }

    fn initialize_gpu_compositing(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.native_host_service.is_gpu_compositing_enabled().then_result(move |result| {
            if let Some(this) = weak.upgrade() {
                match result {
                    Ok(enabled) => {
                        if !this.disposed.get() && this.gpu_compositing_enabled.get().is_none() {
                            this.gpu_compositing_enabled.set(Some(enabled));
                            this.apply();
                        }
                    }
                    Err(error) => {
                        this.log_service.warn_args(&LogMessage::from("Unable to read initial GPU compositing state for frosted glass."), &[LogArg::Error(error)]);
                    }
                }
            }
            Ok(())
        });
    }

    fn apply(&self) {
        for container in self.layout_service.containers() {
            self.apply_to(container);
        }
    }

    fn apply_to(&self, container: LayoutContainerId) {
        let enabled = self.requested.get() && self.gpu_compositing_enabled.get() == Some(true);
        if enabled {
            self.layout_service.set_style_property(container, FROSTED_GLASS_OPACITY_PROPERTY, &format!("{}%", number_to_js_string(self.opacity.get())));
        } else {
            self.layout_service.remove_style_property(container, FROSTED_GLASS_OPACITY_PROPERTY);
        }
        self.layout_service.toggle_class(container, FROSTED_GLASS_CLASS, enabled);
    }
}

impl IDisposable for FrostedGlassContribution {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.requested.set(false);
        self.apply();
        self.base.dispose();
    }
}
