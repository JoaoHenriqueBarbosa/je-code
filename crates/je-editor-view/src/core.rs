//! O núcleo de edição da view: o `CodeEditorWidget` do `je-editor` (modelo, `ViewModel`, vários cursores, comandos
//! do núcleo, ações de linha, multicursor e desfazer) montado com a configuração real do editor e os serviços
//! mínimos do [`crate::services`].
//!
//! No VS Code o widget é o dono do estado de edição e a `View` só o pinta e traduz a entrada em `trigger`. Aqui
//! é igual: a [`crate::EditorView`] guarda um [`EditorCore`] e lê dele as seleções, o texto e o desfazer.

use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{IdleDeadline, RunWhenIdle, run_when_global_idle};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, plain_disposable};
use je_base::runtime;
use je_base::scrollable::{INewScrollPosition, ScheduleAtNextAnimationFrame};
use je_editor::editor_common::ScrollType;
use je_editor::view_model_types::IViewModel;
use je_editor::code_editor_widget::{
    CodeEditorWidget, CodeEditorWidgetOptions, CodeEditorWidgetServices, EditorContributionDescription, EditorContributionInstantiation, EditorViewModel, ICodeEditor,
    IEditorContribution,
};
use je_editor::config::editor_configuration::IEditorConfiguration;
use je_editor::config::editor_configuration_impl::{EditorConfiguration, EditorEnvironment as ConfigurationEnvironment, EnvConfiguration};
use je_editor::config::editor_options::AccessibilitySupport;
use je_editor::config::editor_options::font_ligatures::EditorFontLigatures;
use je_editor::config::editor_options::font_variations::EditorFontVariations;
use je_editor::config::element_size_observer::ElementSizeObserver;
use je_editor::config::font_info::{BareFontInfo, BareFontInfoOptions, FontInfo, FontInfoOptions};
use je_editor::core_2d::dimension::Dimension;
use je_editor::cursor_common::EditorConfiguration as ConfigurationOptions;
use je_editor::editor_extensions::EditorExtensionsRegistry;
use je_editor::model_line_projection_data::LineBreaksComputerFactory;
use je_editor::monospace_line_breaks_computer::MonospaceLineBreaksComputerFactory;
use je_editor::multicursor::MultiCursorSelectionController;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::actions::menu_ids::editor_context;

use crate::layout::FontMetrics;
use crate::services::{FindController, FindControllers, NoCommands, NoHighlightProviders, SilentNotifications, StaticTheme};

/// As medidas da fonte da view como o `FontInfo` do editor (no VS Code o `FontMeasurements` as mede no DOM).
struct ViewEnvironment {
    /// Compartilhadas com o [`EditorCore`]: a troca de fonte (`editor.fontSize`...) atualiza antes do `updateOptions`.
    metrics: Rc<std::cell::RefCell<FontMetrics>>,
}

impl ConfigurationEnvironment for ViewEnvironment {
    fn read_env_configuration(&self, _raw_options: &Dynamic, container_observer: &ElementSizeObserver) -> EnvConfiguration {
        EnvConfiguration {
            extra_editor_class_name: String::new(),
            outer_width: container_observer.get_width(),
            outer_height: container_observer.get_height(),
            empty_selection_clipboard: true,
            pixel_ratio: 1.0,
            accessibility_support: AccessibilitySupport::Unknown,
            edit_context_supported: true,
        }
    }

    fn read_font_info(&self, styling: &BareFontInfo) -> FontInfo {
        let m = self.metrics.borrow();
        FontInfo::new(
            FontInfoOptions {
                bare: BareFontInfoOptions {
                    pixel_ratio: styling.pixel_ratio,
                    font_family: styling.font_family.clone(),
                    font_weight: styling.font_weight.clone(),
                    font_size: styling.font_size,
                    font_feature_settings: EditorFontLigatures::OFF.to_owned(),
                    font_variation_settings: EditorFontVariations::OFF.to_owned(),
                    line_height: styling.line_height,
                    letter_spacing: styling.letter_spacing,
                },
                is_monospace: true,
                typical_halfwidth_character_width: m.char_width,
                typical_fullwidth_character_width: m.char_width * 2.0,
                can_use_halfwidth_rightwards_arrow: true,
                space_width: m.char_width,
                middot_width: m.char_width,
                wsmiddot_width: m.char_width,
                max_digit_width: m.digit_width,
            },
            true,
        )
    }

    fn on_did_change_font_measurements(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_pixel_ratio(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
}

thread_local! {
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Os `registerEditorCommand` e `registerEditorAction` dos módulos do editor que a view usa, uma vez por thread: o
/// que no VS Code o carregamento dos módulos de `editor.all.ts` faz.
fn register_editor_modules() {
    if REGISTERED.replace(true) {
        return;
    }
    je_editor::core_commands::register_core_commands();
    je_editor::word_operations::register_word_operations();
    je_editor::lines_operations::register_lines_operations();
    je_editor::line_selection::register_line_selection();
    je_editor::comment::comment_actions::register_comment();
    je_editor::multicursor::register_multi_cursor(Rc::new(FindControllers), Rc::new(NoHighlightProviders));
    register_view_keybindings();
}

/// Os atalhos das ações que a view trata por conta própria (a dobra do `FoldingController` e o `toggleWordWrap`
/// do workbench), registrados como os `kbOpts` do original para o resolvedor de atalhos achá-los.
fn register_view_keybindings() {
    use je_base::key_codes::{KeyCode, KeyMod, key_chord};
    use je_platform::keybinding::keybindings_registry::{KeybindingRule, KeybindingWeight, Keybindings, PlatformKeybindings, keybindings_registry};
    let rule = |id: &str, primary: u32, mac: Option<u32>, weight: i64| KeybindingRule {
        keybindings: Keybindings { primary: Some(primary), mac: mac.map(|p| PlatformKeybindings { primary: Some(p), ..Default::default() }), ..Default::default() },
        id: id.to_owned(),
        weight,
        args: Dynamic::Undefined,
        when: Some(je_editor::core_commands::editor_text_focus()),
    };
    let code = |k: KeyCode| k.0 as u32;
    let registry = keybindings_registry();
    for r in [
        rule("editor.fold", KeyMod::CTRL_CMD | KeyMod::SHIFT | code(KeyCode::BracketLeft), Some(KeyMod::CTRL_CMD | KeyMod::ALT | code(KeyCode::BracketLeft)), KeybindingWeight::EDITOR_CONTRIB),
        rule("editor.unfold", KeyMod::CTRL_CMD | KeyMod::SHIFT | code(KeyCode::BracketRight), Some(KeyMod::CTRL_CMD | KeyMod::ALT | code(KeyCode::BracketRight)), KeybindingWeight::EDITOR_CONTRIB),
        rule("editor.foldAll", key_chord(KeyMod::CTRL_CMD | code(KeyCode::KeyK), KeyMod::CTRL_CMD | code(KeyCode::Digit0)), None, KeybindingWeight::EDITOR_CONTRIB),
        rule("editor.unfoldAll", key_chord(KeyMod::CTRL_CMD | code(KeyCode::KeyK), KeyMod::CTRL_CMD | code(KeyCode::KeyJ)), None, KeybindingWeight::EDITOR_CONTRIB),
        rule("editor.action.toggleWordWrap", KeyMod::ALT | code(KeyCode::KeyZ), None, KeybindingWeight::WORKBENCH_CONTRIB),
    ] {
        // Registrados uma vez por thread, junto com os módulos: ficam vivos enquanto o programa roda.
        std::mem::forget(registry.register_keybinding_rule(r));
    }
}

fn no_extensions_idle() -> RunWhenIdle {
    Rc::new(|runner: Box<dyn FnOnce(IdleDeadline)>, timeout| run_when_global_idle(runner, timeout))
}

fn schedule_at_next_animation_frame() -> ScheduleAtNextAnimationFrame {
    Rc::new(|callback| {
        let cancelled = Rc::new(Cell::new(false));
        let flag = cancelled.clone();
        runtime::set_timeout(
            move || {
                if !flag.get() {
                    callback();
                }
            },
            16,
        );
        plain_disposable(move || cancelled.set(true))
    })
}

/// As opções do editor que a view fixa (o resto vem do padrão do VS Code): a fonte da view, sem quebra de linha.
fn editor_options(metrics: &FontMetrics) -> Dynamic {
    Dynamic::object([
        ("fontFamily", Dynamic::from(metrics.families.as_str())),
        ("fontSize", Dynamic::from(f64::from(metrics.size))),
        ("lineHeight", Dynamic::from(metrics.line_height)),
        ("wordWrap", Dynamic::from("off")),
        // O do editor de texto do workbench (o padrão do Monaco é 5): a largura dos números de linha que a view mede.
        ("lineNumbersMinChars", Dynamic::from(3.0)),
        ("automaticLayout", Dynamic::from(false)),
    ])
}

/// O widget de edição de uma view.
pub(crate) struct EditorCore {
    pub widget: Rc<CodeEditorWidget>,
    pub view_model: Rc<EditorViewModel>,
    configuration: Rc<EditorConfiguration>,
    metrics: Rc<std::cell::RefCell<FontMetrics>>,
}

impl EditorCore {
    pub fn new(model: Rc<TextModel>, metrics: &FontMetrics, width: f64, height: f64) -> Self {
        register_editor_modules();
        let options = editor_options(metrics);
        let observer = ElementSizeObserver::new(None, Some(Dimension { width, height }));
        let shared_metrics = Rc::new(std::cell::RefCell::new(metrics.clone()));
        let configuration = EditorConfiguration::new(false, editor_context(), &options, observer, Rc::new(ViewEnvironment { metrics: shared_metrics.clone() }));
        let dom_line_breaks: Rc<dyn LineBreaksComputerFactory> = Rc::new(MonospaceLineBreaksComputerFactory::create(ConfigurationOptions::options(&*configuration)));
        let registry = EditorExtensionsRegistry::instance();
        let mut contributions = registry.get_some_editor_contributions(&[MultiCursorSelectionController::ID]);
        contributions.push(EditorContributionDescription {
            id: FindController::ID.to_owned(),
            ctor: Rc::new(|_editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> { FindController::new() }),
            instantiation: EditorContributionInstantiation::Eager,
        });
        let widget = CodeEditorWidget::new(
            configuration.clone(),
            CodeEditorWidgetOptions { contributions: Some(contributions), telemetry_data: None },
            CodeEditorWidgetServices {
                language_configuration_service: model.language_configuration_service().clone(),
                theme_service: StaticTheme::new(),
                notification_service: Rc::new(SilentNotifications),
                command_service: Rc::new(NoCommands),
                extensions_registry: registry,
                dom_line_breaks_computer_factory: dom_line_breaks,
                schedule_at_next_animation_frame: schedule_at_next_animation_frame(),
                run_when_idle: no_extensions_idle(),
                code_editor_service: None,
            },
        );
        widget.set_instantiation_service(InstantiationService::new(ServiceCollection::new()));
        widget.set_model(Some(model));
        widget.set_has_text_focus(true);
        let view_model = widget.get_view_model().expect("o widget tem o modelo ligado");
        view_model.set_has_focus(true);
        Self { widget, view_model, configuration, metrics: shared_metrics }
    }

    /// Aplica as opções do usuário (`updateOptions`), com as medidas da fonte nova já valendo para o `FontInfo`.
    pub fn update_options(&self, metrics: &FontMetrics, options: &Dynamic) {
        *self.metrics.borrow_mut() = metrics.clone();
        self.widget.update_options(Some(options));
    }

    /// O tamanho da área de edição mudou (a opção de layout, de onde saem o tamanho da página e a quebra de linha).
    pub fn resize(&self, width: f64, height: f64) {
        self.configuration.observe_container(Some(Dimension { width, height }));
    }

    /// Todas as seleções, a primária em primeiro lugar da ordem de criação (`getSelections()`).
    pub fn selections(&self) -> Vec<Selection> {
        self.widget.get_selections().unwrap_or_default()
    }

    /// A seleção do cursor primário (o último criado).
    pub fn primary(&self) -> Selection {
        self.widget.get_selection().unwrap_or_else(|| Selection::new(1, 1, 1, 1))
    }

    /// A rolagem da view no `ViewModel`, para os comandos que revelam ou rolam partirem da posição certa.
    pub fn set_scroll(&self, top: f64, left: f64) {
        if (self.widget.get_scroll_top() - top).abs() > f64::EPSILON || (self.widget.get_scroll_left() - left).abs() > f64::EPSILON {
            self.widget.set_scroll_position(INewScrollPosition { scroll_top: Some(top), scroll_left: Some(left) }, Some(ScrollType::Immediate));
        }
    }

    pub fn set_focus(&self, focused: bool) {
        self.widget.set_has_text_focus(focused);
        self.view_model.set_has_focus(focused);
    }

    /// `editor.trigger(source, handlerId, payload)` com a origem `keyboard`.
    pub fn trigger(&self, handler_id: &str, payload: Option<Dynamic>) {
        self.widget.trigger(Some("keyboard"), handler_id, payload);
    }

    /// O `trigger` do widget sabe rodar `id`: um handler embutido, uma ação do editor ou um comando de editor.
    pub fn knows_command(&self, id: &str) -> bool {
        use je_editor::code_editor_widget::{Handler, IEditorExtensionsRegistry};
        matches!(id, Handler::COMPOSITION_START | Handler::COMPOSITION_END | Handler::TYPE | Handler::REPLACE_PREVIOUS_CHAR | Handler::COMPOSITION_TYPE | Handler::PASTE | Handler::CUT)
            || self.widget.get_action(id).is_some()
            || EditorExtensionsRegistry::instance().get_editor_command(id).is_some()
    }

    /// Digita `text` nos cursores (o handler `type`).
    pub fn type_text(&self, text: &str) {
        self.trigger("type", Some(Dynamic::object([("text", Dynamic::from(JsString::from(text)))])));
    }
}

impl Drop for EditorCore {
    fn drop(&mut self) {
        self.widget.dispose();
    }
}

