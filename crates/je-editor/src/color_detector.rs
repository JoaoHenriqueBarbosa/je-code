//! Porte de `src/vs/editor/contrib/colorPicker/browser/colorDetector.ts`: o `ColorDetector` (a contribuição que pede as
//! cores aos provedores, marca os trechos de cor e desenha o quadrado de cor antes de cada um) e o
//! `DecoratorLimitReporter`.
//!
//! Divergências:
//! - `extends Disposable` vira um [`Disposable`] por dentro; o construtor devolve `Rc<Self>`. Os serviços injetados
//!   (`IConfigurationService`, `ILanguageFeaturesService`, `ILanguageFeatureDebounceService`) chegam em
//!   [`ColorDetectorServices`], como no `FoldingController`.
//! - A pintura é nativa, sem DOM nem CSS: `DynamicCssRules` e as propriedades `--vscode-colorPicker-colorDecorator*` do
//!   `getContainerDomNode()` não existem. O `inlineClassName` da decoração leva `colorpicker-color-decoration` e uma classe
//!   `color-decorator-<r>-<g>-<b>-<a>` com o `rgba` que o original punha no `background-color`; quem pinta o quadrado lê a
//!   cor dali. A largura em `em` (`widthInEm`) é a mesma do original.
//! - `ColorDecorationInjectedTextMarker` (o `attachedData` do texto injetado) não tem lugar em `InjectedTextOptions`: a
//!   pergunta "esta decoração é de cor?" ([`ColorDetector::is_color_decoration`]) é respondida pela coleção de decorações.
//! - `createCancelablePromise(async token => ...)` é um [`create_cancelable_promise`] sobre `get_colors`; o `await` de
//!   `beginCompute` é um `then_result`, e o erro (inclusive o `CancellationError` do cancelamento) vai a `on_unexpected_error`.
//! - `limited: number | false` do relator é `Option<usize>` (`None` é `false`), como no `RangesLimitReporter`.
//! - O registro (`registerEditorContribution` em `colorPickerContribution.ts`, `AfterFirstRender`) é [`register_color_detector`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::{TimeoutTimer, create_cancelable_promise};
use je_base::dynamic::Dynamic;
use je_base::errors::on_unexpected_error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::{CancelablePromise, Promise};
use je_base::stopwatch::StopWatch;
use je_platform::configuration::service::IConfigurationService;

use crate::code_editor_widget::{CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution};
use crate::color::{DefaultColorDecoratorsEnablement, IColorData, get_colors};
use crate::config::editor_options::registry::{COLOR_DECORATORS, COLOR_DECORATORS_LIMIT, DEFAULT_COLOR_DECORATORS};
use crate::config::editor_options::{ConfigurationChangedEvent, EditorOption};
use crate::editor_decorations_collection::EditorDecorationsCollection;
use crate::editor_extensions::register_editor_contribution;
use crate::language_feature_debounce::{DebounceConfig, IFeatureDebounceInformation, ILanguageFeatureDebounceService};
use crate::language_feature_registry::AnyLanguageFeatureRegistry;
use crate::language_features_service::LanguageFeaturesService;
use crate::model::{InjectedTextOptions, ModelDecoration, ModelDecorationOptionsInit, ModelDeltaDecoration};
use crate::multicursor::listen_weak;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, TextModel};

thread_local! {
    /// O registro da contribuição acontece uma vez por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `noBreakWhitespace`.
const NO_BREAK_WHITESPACE: &str = "\u{00a0}";

/// Os serviços que o construtor recebe por injeção (ver o topo do módulo).
pub struct ColorDetectorServices {
    /// `@IConfigurationService`.
    pub configuration_service: Rc<dyn IConfigurationService>,
    /// `@ILanguageFeaturesService`.
    pub language_features_service: Rc<LanguageFeaturesService>,
    /// `@ILanguageFeatureDebounceService`.
    pub language_feature_debounce_service: Rc<dyn ILanguageFeatureDebounceService>,
}

/// `'auto' | 'always' | 'never'` do valor de `editor.defaultColorDecorators`.
fn enablement_of(value: &str) -> DefaultColorDecoratorsEnablement {
    match value {
        "always" => DefaultColorDecoratorsEnablement::Always,
        "never" => DefaultColorDecoratorsEnablement::Never,
        _ => DefaultColorDecoratorsEnablement::Auto,
    }
}

/// `ColorDetector`.
pub struct ColorDetector {
    me: Weak<ColorDetector>,
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    services: ColorDetectorServices,

    local_to_dispose: Rc<DisposableStore>,
    compute_promise: RefCell<Option<CancelablePromise<Vec<IColorData>>>>,
    timeout_timer: RefCell<Option<Rc<TimeoutTimer>>>,
    debounce_information: Rc<dyn IFeatureDebounceInformation>,

    decorations_ids: RefCell<Vec<String>>,
    color_datas: RefCell<HashMap<String, IColorData>>,

    color_decorator_ids: EditorDecorationsCollection,

    is_color_decorators_enabled: Cell<bool>,
    default_color_decorators_enablement: Cell<DefaultColorDecoratorsEnablement>,

    decorator_limit_reporter: Rc<DecoratorLimitReporter>,
}

impl ColorDetector {
    pub const ID: &'static str = "editor.contrib.colorDetector";

    /// `ColorDetector.RECOMPUTE_TIME` (ms).
    pub const RECOMPUTE_TIME: f64 = 1000.0;

    const COLOR_DECORATOR_INNER_WIDTH_IN_EM: f64 = 0.8;
    const COLOR_DECORATOR_MARGIN_IN_EM: f64 = 0.2;

    /// `ColorDetector.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<ColorDetector>> {
        editor.get_contribution_as::<ColorDetector>(Self::ID)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, services: ColorDetectorServices) -> Rc<Self> {
        let base = Disposable::new();
        let local_to_dispose = DisposableStore::new();
        base.register(local_to_dispose.clone());
        let decorator_limit_reporter = DecoratorLimitReporter::new();
        base.register(decorator_limit_reporter.clone());

        let debounce_registry: Rc<dyn AnyLanguageFeatureRegistry> = services.language_features_service.color_provider.clone();
        let debounce_information = services.language_feature_debounce_service.for_feature(
            debounce_registry,
            "Document Colors",
            Some(DebounceConfig { min: Some(Self::RECOMPUTE_TIME), ..DebounceConfig::default() }),
        );

        let this = Rc::new_cyclic(|me: &Weak<ColorDetector>| {
            base.register(listen_weak(me, editor.on_did_change_model(), |this: &ColorDetector, _| {
                this.is_color_decorators_enabled.set(this.is_enabled());
                this.update_colors();
            }));
            base.register(listen_weak(me, editor.on_did_change_model_language(), |this: &ColorDetector, _| this.update_colors()));
            base.register(listen_weak(me, services.language_features_service.color_provider.on_did_change(), |this: &ColorDetector, _| {
                this.update_colors();
            }));
            base.register(listen_weak(me, editor.on_did_change_configuration(), |this: &ColorDetector, e| {
                this.on_configuration_changed(e);
            }));

            ColorDetector {
                me: me.clone(),
                base: base.clone(),
                editor: Rc::downgrade(editor),
                services,
                local_to_dispose,
                compute_promise: RefCell::new(None),
                timeout_timer: RefCell::new(None),
                debounce_information,
                decorations_ids: RefCell::new(Vec::new()),
                color_datas: RefCell::new(HashMap::new()),
                color_decorator_ids: editor.create_decorations_collection(None),
                is_color_decorators_enabled: Cell::new(false),
                default_color_decorators_enablement: Cell::new(DefaultColorDecoratorsEnablement::Auto),
                decorator_limit_reporter,
            }
        });
        this.is_color_decorators_enabled.set(this.is_enabled());
        this.default_color_decorators_enablement.set(enablement_of(editor.get_options().get(DEFAULT_COLOR_DECORATORS)));
        this.update_colors();
        this
    }

    fn on_configuration_changed(&self, e: &ConfigurationChangedEvent) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let prev_is_enabled = self.is_color_decorators_enabled.get();
        let is_enabled = self.is_enabled();
        self.is_color_decorators_enabled.set(is_enabled);
        self.default_color_decorators_enablement.set(enablement_of(editor.get_options().get(DEFAULT_COLOR_DECORATORS)));
        let updated_color_decorators_setting = prev_is_enabled != is_enabled || e.has_changed(EditorOption::ColorDecoratorsLimit);
        let updated_default_color_decorators_setting = e.has_changed(EditorOption::DefaultColorDecorators);
        if updated_color_decorators_setting || updated_default_color_decorators_setting {
            if is_enabled {
                self.update_colors();
            } else {
                self.remove_all_decorations();
            }
        }
    }

    /// `isEnabled()`.
    pub fn is_enabled(&self) -> bool {
        let Some(editor) = self.editor.upgrade() else {
            return false;
        };
        let Some(model) = editor.get_model() else {
            return false;
        };
        let language_id = model.get_language_id();
        // handle deprecated settings. [languageId].colorDecorators.enable
        let deprecated_config = self.services.configuration_service.get_value(Some(&language_id), None);
        if let Dynamic::Object(_) = deprecated_config {
            let color_decorators = deprecated_config.get_str("colorDecorators");
            if let Dynamic::Object(_) = color_decorators {
                let enable = color_decorators.get_str("enable");
                if !matches!(enable, Dynamic::Undefined) && !je_base::dynamic::Truthy::is_truthy(&enable) {
                    return false;
                }
            }
        }

        *editor.get_options().get(COLOR_DECORATORS)
    }

    /// `limitReporter`.
    pub fn limit_reporter(&self) -> &Rc<DecoratorLimitReporter> {
        &self.decorator_limit_reporter
    }

    fn update_colors(&self) {
        self.stop();

        if !self.is_color_decorators_enabled.get() {
            self.remove_all_decorations();
            return;
        }
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let Some(model) = editor.get_model() else {
            return;
        };
        if !self.services.language_features_service.color_provider.has(&model) {
            return;
        }

        let me = self.me.clone();
        let model_for_timer = Rc::clone(&model);
        self.local_to_dispose.add(editor.on_did_change_model_content().listen(move |_| {
            let Some(this) = me.upgrade() else {
                return;
            };
            if this.timeout_timer.borrow().is_none() {
                let timer = TimeoutTimer::new();
                *this.timeout_timer.borrow_mut() = Some(Rc::clone(&timer));
                let again = Rc::downgrade(&this);
                let delay = this.debounce_information.get(&model_for_timer);
                timer.cancel_and_set(
                    move || {
                        if let Some(this) = again.upgrade() {
                            *this.timeout_timer.borrow_mut() = None;
                            this.begin_compute();
                        }
                    },
                    delay as u64,
                );
            }
        }));
        self.begin_compute();
    }

    fn begin_compute(&self) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let color_provider = self.services.language_features_service.color_provider.clone();
        let enablement = self.default_color_decorators_enablement.get();
        let debounce_information = Rc::clone(&self.debounce_information);
        let compute_editor = Rc::downgrade(&editor);
        let compute_promise = create_cancelable_promise(move |token| {
            let Some(model) = compute_editor.upgrade().and_then(|editor| editor.get_model()) else {
                return Promise::resolved(Vec::new());
            };
            let stop_watch = StopWatch::new(Some(false));
            get_colors(&color_provider, &model, &token, enablement).then(move |colors| {
                debounce_information.update(&model, stop_watch.elapsed());
                colors
            })
        });
        *self.compute_promise.borrow_mut() = Some(compute_promise.clone());

        let me = self.me.clone();
        compute_promise.promise.then_result(move |result| {
            match result {
                Ok(colors) => {
                    if let Some(this) = me.upgrade() {
                        this.update_decorations(&colors);
                        this.update_color_decorators(&colors);
                        *this.compute_promise.borrow_mut() = None;
                    }
                }
                Err(error) => on_unexpected_error(&error),
            }
            Ok(())
        });
    }

    fn stop(&self) {
        if let Some(timer) = self.timeout_timer.borrow_mut().take() {
            timer.cancel();
        }
        if let Some(compute_promise) = self.compute_promise.borrow_mut().take() {
            compute_promise.cancel();
        }
        self.local_to_dispose.clear();
    }

    fn update_decorations(&self, color_datas: &[IColorData]) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let decorations: Vec<ModelDeltaDecoration> = color_datas
            .iter()
            .map(|c| ModelDeltaDecoration {
                range: c.color_info.range,
                options: ModelDecorationOptions::empty(),
            })
            .collect();

        editor.change_decorations(|change_accessor| {
            let old_ids = self.decorations_ids.borrow().clone();
            let new_ids = change_accessor.delta_decorations(&old_ids, &decorations);

            let mut map = HashMap::new();
            for (id, data) in new_ids.iter().zip(color_datas) {
                map.insert(id.clone(), data.clone());
            }
            *self.color_datas.borrow_mut() = map;
            *self.decorations_ids.borrow_mut() = new_ids;
        });
    }

    fn update_color_decorators(&self, color_data: &[IColorData]) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let mut decorations: Vec<ModelDeltaDecoration> = Vec::new();
        let width_in_em = Self::COLOR_DECORATOR_INNER_WIDTH_IN_EM + 2.0 * Self::COLOR_DECORATOR_MARGIN_IN_EM;

        let limit = *editor.get_options().get(COLOR_DECORATORS_LIMIT);

        let mut i = 0;
        while i < color_data.len() && (decorations.len() as f64) < limit {
            let color = &color_data[i].color_info.color;
            let (red, green, blue) = ((color.red * 255.0).round(), (color.green * 255.0).round(), (color.blue * 255.0).round());
            let class_name = format!("color-decorator-{red}-{green}-{blue}-{}", color.alpha);

            decorations.push(ModelDeltaDecoration {
                range: color_data[i].color_info.range,
                options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                    description: "colorDetector".to_string(),
                    before: Some(InjectedTextOptions {
                        content: NO_BREAK_WHITESPACE.to_string(),
                        inline_class_name: Some(format!("{class_name} colorpicker-color-decoration")),
                        inline_class_name_affects_letter_spacing: true,
                        width_in_em: Some(width_in_em),
                        ..InjectedTextOptions::default()
                    }),
                    ..ModelDecorationOptionsInit::default()
                }),
            });
            i += 1;
        }
        let limited = if limit < color_data.len() as f64 { Some(limit as usize) } else { None };
        self.decorator_limit_reporter.update(color_data.len(), limited);

        self.color_decorator_ids.set(&decorations);
    }

    fn remove_all_decorations(&self) {
        if let Some(editor) = self.editor.upgrade() {
            let ids = std::mem::take(&mut *self.decorations_ids.borrow_mut());
            editor.remove_decorations(&ids);
        }
        self.color_decorator_ids.clear();
    }

    /// `getColorData(position)`.
    pub fn get_color_data(&self, position: Position) -> Option<IColorData> {
        let editor = self.editor.upgrade()?;
        let model = editor.get_model()?;

        let color_datas = self.color_datas.borrow();
        let decorations = model
            .get_decorations_in_range(&Range::from_positions(position, position), 0, false, false, false, false)
            .into_iter()
            .filter(|d| color_datas.contains_key(&d.id))
            .collect::<Vec<_>>();

        decorations.first().and_then(|d| color_datas.get(&d.id).cloned())
    }

    /// `isColorDecoration(decoration)`.
    pub fn is_color_decoration(&self, decoration: &ModelDecoration) -> bool {
        self.color_decorator_ids.has(decoration)
    }
}

impl IDisposable for ColorDetector {
    fn dispose(&self) {
        self.stop();
        self.remove_all_decorations();
        self.base.dispose();
    }
}

impl IEditorContribution for ColorDetector {}

/// `DecoratorLimitReporter`.
pub struct DecoratorLimitReporter {
    on_did_change: Rc<Emitter<()>>,
    computed: Cell<usize>,
    limited: Cell<Option<usize>>,
}

impl DecoratorLimitReporter {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { on_did_change: Emitter::new(), computed: Cell::new(0), limited: Cell::new(None) })
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `computed`.
    pub fn computed(&self) -> usize {
        self.computed.get()
    }

    /// `limited` (`false` é `None`).
    pub fn limited(&self) -> Option<usize> {
        self.limited.get()
    }

    /// `update(computed, limited)`.
    pub fn update(&self, computed: usize, limited: Option<usize>) {
        if computed != self.computed.get() || limited != self.limited.get() {
            self.computed.set(computed);
            self.limited.set(limited);
            self.on_did_change.fire(());
        }
    }
}

impl IDisposable for DecoratorLimitReporter {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}

/// O `registerEditorContribution(ColorDetector.ID, ColorDetector, EditorContributionInstantiation.AfterFirstRender)` de
/// `colorPickerContribution.ts`. `services` entrega os [`ColorDetectorServices`] de cada editor (o `createInstance` do
/// original). É idempotente: o registro acontece uma vez por thread.
pub fn register_color_detector(services: Rc<dyn Fn(&Rc<CodeEditorWidget>) -> ColorDetectorServices>) {
    if REGISTERED.replace(true) {
        return;
    }
    register_editor_contribution(
        ColorDetector::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
            ColorDetector::new(editor, services(editor))
        }),
        EditorContributionInstantiation::AfterFirstRender,
    );
}
