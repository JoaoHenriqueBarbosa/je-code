//! Porte de `src/vs/editor/browser/config/editorConfiguration.ts`: a classe `EditorConfiguration` (as opções
//! cruas, as validadas e as calculadas, os eventos de mudança, e o recálculo quando muda o zoom, o modo de Tab, o
//! tamanho do contêiner, a medição de fonte, a razão de pixels, o leitor de tela ou o modo de digitação), com
//! `EditorOptionsUtil` (`validateOptions`, `computeOptions`, `checkEquals`, `applyUpdate`) e `ValidatedEditorOptions`.
//!
//! Decisões de tradução:
//! - `EditorConfiguration extends Disposable` é composição com o [`Disposable`] base (`_register`). Os callbacks que
//!   no original capturam `this` guardam um `Weak` (preenchido por `Rc::new_cyclic`).
//! - As opções calculadas (`options`) são lidas por `&self` e trocadas a cada recálculo, e o contrato de
//!   [`ConfigurationOptions`] devolve `&ComputedEditorOptions`. Para a referência continuar válida depois de uma
//!   troca, cada geração das opções fica numa lista só de acréscimo ([`Generations`], segmentada, sem `unsafe`) e
//!   vive até o `Rc` da configuração ser solto. Uma geração nova reaproveita (`Rc`) o valor antigo de toda opção
//!   que não mudou, então o custo por mudança é o vetor de ponteiros mais os valores que mudaram.
//! - `ComputeOptionsMemory` é o `Rc<RefCell<..>>` que o [`EnvironmentalOptions`] já espera.
//! - `private _onDidChange`/`_onDidChangeFast` são `Emitter` registrados no `Disposable` base.
//! - `IEditorConstructionOptions` (`IEditorOptions` mais `dimension` e `overflowWidgetsDomNode`) é um objeto
//!   [`Dynamic`]: o `dimension` é consumido por quem cria o [`ElementSizeObserver`] (o construtor recebe o
//!   observador pronto), e o `overflowWidgetsDomNode` é do DOM e não tem papel aqui.
//! - `setModelLineCount` e `setViewLineCount` recebem `i32`; `digitCount` usa a divisão com piso do original.
//!
//! Divergências (o je-code mede com parley; aqui só a interface):
//! - `FontMeasurements.readFontInfo`, `FontMeasurements.onDidChange`, `PixelRatio`, `getWindow`/`getWindowById`
//!   (`targetWindowId`), `IAccessibilityService`, `browser.isWebKit`/`isFirefox`/`isSafari`, `platform.isMacintosh`
//!   de `getExtraEditorClassName` e `globalThis.EditContext` são do trait [`EditorEnvironment`], que o hospedeiro
//!   implementa: `_readEnvConfiguration` e `_readFontInfo` (os dois métodos `protected` que `TestConfiguration`
//!   sobrescreve) são os métodos dele, e as três assinaturas de evento (medição de fonte, razão de pixels, leitor de
//!   tela) são os outros. `_readEnvConfiguration` recebe as opções cruas (o `this.getRawOptions()` do teste) e o
//!   observador do contêiner (o `this._containerObserver`).
//! - `EditorOptionsUtil.applyUpdate` do original trabalha nas opções cruas (sem tipo). O `applyUpdate` do registro
//!   do porte trabalha nos valores validados, então o equivalente cru está aqui: [`apply_raw_update`] (a mesclagem
//!   profunda de `applyUpdate`) e a sobrescrita atômica de `allowedCharacters` e `allowedLocales` de
//!   `UnicodeHighlight.applyUpdate`.
//! - `_deepEquals` compara os valores calculados com a igualdade do tipo de cada opção
//!   ([`ErasedEditorOption::values_equal`]), e a de arrays é elemento a elemento com `PartialEq` (o original usa
//!   `===` por elemento, que só difere para elementos que são objetos).

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::objects;
use je_platform::actions::MenuId;

use crate::config::editor_configuration::IEditorConfiguration;
use crate::config::editor_options::registry::{AUTOMATIC_LAYOUT, TAB_FOCUS_MODE, editor_options_registry};
use crate::config::editor_options::{
    AccessibilitySupport, ComputeOptionsMemory, ComputedEditorOptions, ConfigurationChangedEvent, EditorOption,
    EnvironmentalOptions, Opt,
};
use crate::config::element_size_observer::ElementSizeObserver;
use crate::config::font_info::{BareFontInfo, FontInfo};
use crate::config::font_info_from_settings::create_bare_font_info_from_validated_settings;
use crate::config::migrate_options::migrate_options;
use crate::config::{editor_zoom, tab_focus};
use crate::core_2d::dimension::Dimension;
use crate::cursor_common::EditorConfiguration as ConfigurationOptions;
use crate::input_mode;

/// `IEnvConfiguration`: o que o ambiente (o DOM, no original) informa a cada cálculo das opções.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvConfiguration {
    pub extra_editor_class_name: String,
    pub outer_width: f64,
    pub outer_height: f64,
    pub empty_selection_clipboard: bool,
    pub pixel_ratio: f64,
    pub accessibility_support: AccessibilitySupport,
    pub edit_context_supported: bool,
}

/// O ambiente de uma [`EditorConfiguration`] (ver "Divergências" no topo do módulo).
pub trait EditorEnvironment {
    /// `_readEnvConfiguration()`.
    fn read_env_configuration(&self, raw_options: &Dynamic, container_observer: &ElementSizeObserver) -> EnvConfiguration;

    /// `_readFontInfo(bareFontInfo)`.
    fn read_font_info(&self, bare_font_info: &BareFontInfo) -> FontInfo;

    /// `FontMeasurements.onDidChange`.
    fn on_did_change_font_measurements(&self) -> Event<()>;

    /// `PixelRatio.getInstance(getWindow(container)).onDidChange`.
    fn on_did_change_pixel_ratio(&self) -> Event<()>;

    /// `IAccessibilityService.onDidChangeScreenReaderOptimized`.
    fn on_did_change_screen_reader_optimized(&self) -> Event<()>;
}

/// `ValidatedEditorOptions` (que implementa `IValidatedEditorOptions`): o valor validado de cada opção.
pub struct ValidatedEditorOptions {
    values: Vec<Rc<dyn std::any::Any>>,
}

impl ValidatedEditorOptions {
    /// `_read(option)`, sem tipo.
    pub fn read(&self, id: EditorOption) -> &Rc<dyn std::any::Any> {
        &self.values[id as usize]
    }

    /// `get(id)`.
    pub fn get<T: 'static>(&self, key: Opt<T>) -> &T {
        self.values[key.id() as usize]
            .downcast_ref::<T>()
            .unwrap_or_else(|| panic!("tipo errado para a opção {}", key.id().name()))
    }
}

const FIRST_SEGMENT_LEN: usize = 16;
const SEGMENT_COUNT: usize = 28;

/// Lista só de acréscimo com acesso por `&self` e referências estáveis: segmentos que dobram de tamanho, cada um
/// criado uma vez. Só `OnceCell`, sem `unsafe`.
struct Generations<T> {
    segments: [OnceCell<Box<[OnceCell<T>]>>; SEGMENT_COUNT],
    len: Cell<usize>,
}

impl<T> Generations<T> {
    fn new() -> Self {
        Self { segments: std::array::from_fn(|_| OnceCell::new()), len: Cell::new(0) }
    }

    /// O segmento e a posição dentro dele do item `index`.
    fn locate(index: usize) -> (usize, usize) {
        let group = index / FIRST_SEGMENT_LEN + 1;
        let segment = (usize::BITS - 1 - group.leading_zeros()) as usize;
        let start = FIRST_SEGMENT_LEN * ((1usize << segment) - 1);
        (segment, index - start)
    }

    fn push(&self, value: T) {
        let index = self.len.get();
        let (segment, offset) = Self::locate(index);
        let slots = self.segments[segment].get_or_init(|| (0..FIRST_SEGMENT_LEN << segment).map(|_| OnceCell::new()).collect());
        if slots[offset].set(value).is_err() {
            unreachable!("cada posição é escrita uma vez");
        }
        self.len.set(index + 1);
    }

    fn last(&self) -> Option<&T> {
        let index = self.len.get().checked_sub(1)?;
        let (segment, offset) = Self::locate(index);
        self.segments[segment].get()?[offset].get()
    }
}

/// `EditorConfiguration`.
pub struct EditorConfiguration {
    base: Rc<Disposable>,
    me: Weak<EditorConfiguration>,
    on_did_change: Rc<Emitter<ConfigurationChangedEvent>>,
    on_did_change_fast: Rc<Emitter<ConfigurationChangedEvent>>,
    is_simple_widget: bool,
    context_menu_id: MenuId,
    container_observer: Rc<ElementSizeObserver>,
    environment: Rc<dyn EditorEnvironment>,

    is_dominated_by_long_lines: Cell<bool>,
    view_line_count: Cell<i32>,
    line_numbers_digit_count: Cell<i32>,
    reserved_height: Cell<f64>,
    glyph_margin_decoration_lane_count: Cell<f64>,

    compute_options_memory: Rc<RefCell<ComputeOptionsMemory>>,
    /// Opções cruas como foram passadas e mescladas com todas as chamadas de `updateOptions`.
    raw_options: Dynamic,
    /// Versão validada de `raw_options`.
    validated_options: RefCell<Rc<ValidatedEditorOptions>>,
    /// Opções completas: as passadas combinadas com os valores do ambiente. A última geração é a vigente.
    options: Generations<ComputedEditorOptions>,
}

impl EditorConfiguration {
    /// `new EditorConfiguration(isSimpleWidget, contextMenuId, options, container, accessibilityService)`. O
    /// `container` e o `options.dimension` entram no `container_observer` (já criado), e o serviço de
    /// acessibilidade e a medição de fonte no `environment`.
    pub fn new(
        is_simple_widget: bool,
        context_menu_id: MenuId,
        options: &Dynamic,
        container_observer: Rc<ElementSizeObserver>,
        environment: Rc<dyn EditorEnvironment>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        let on_did_change_fast = Emitter::new();
        base.register(on_did_change.clone());
        base.register(on_did_change_fast.clone());
        base.register(container_observer.clone());

        let raw_options = deep_clone_and_migrate_options(options);
        let validated_options = Rc::new(EditorOptionsUtil::validate_options(&raw_options));
        let this = Rc::new_cyclic(|me| Self {
            base,
            me: me.clone(),
            on_did_change,
            on_did_change_fast,
            is_simple_widget,
            context_menu_id,
            container_observer,
            environment,
            is_dominated_by_long_lines: Cell::new(false),
            view_line_count: Cell::new(1),
            line_numbers_digit_count: Cell::new(1),
            reserved_height: Cell::new(0.0),
            glyph_margin_decoration_lane_count: Cell::new(1.0),
            compute_options_memory: Rc::new(RefCell::new(ComputeOptionsMemory::new())),
            raw_options,
            validated_options: RefCell::new(validated_options),
            options: Generations::new(),
        });
        this.options.push(this.compute_options());

        if *this.options().get(AUTOMATIC_LAYOUT) {
            this.container_observer.start_observing();
        }

        this.recompute_on(editor_zoom::on_did_change_zoom_level());
        this.recompute_on(tab_focus::on_did_change_tab_focus());
        this.recompute_on(this.container_observer.on_did_change());
        this.recompute_on(this.environment.on_did_change_font_measurements());
        this.recompute_on(this.environment.on_did_change_pixel_ratio());
        this.recompute_on(this.environment.on_did_change_screen_reader_optimized());
        this.recompute_on(input_mode::on_did_change_input_mode());
        this
    }

    /// `this._register(event(() => this._recomputeOptions()))`.
    fn recompute_on<E: 'static>(&self, event: Event<E>) {
        let me = self.me.clone();
        self.base.register(event.listen(move |_| {
            if let Some(configuration) = me.upgrade() {
                configuration.recompute_options();
            }
        }));
    }

    fn recompute_options(&self) {
        let new_options = self.compute_options();
        let Some(change_event) = EditorOptionsUtil::check_equals(self.options(), &new_options) else {
            // nothing changed!
            return;
        };

        // O valor antigo de uma opção que não mudou continua a ser o mesmo `Rc`.
        let current = self.options();
        let merged = ComputedEditorOptions::new(
            EditorOption::ALL
                .iter()
                .map(|id| {
                    let source = if change_event.has_changed(*id) { &new_options } else { current };
                    source.get_any(*id).clone()
                })
                .collect(),
        );
        self.options.push(merged);
        self.on_did_change_fast.fire(change_event.clone());
        self.on_did_change.fire(change_event);
    }

    fn compute_options(&self) -> ComputedEditorOptions {
        let partial_env = self.environment.read_env_configuration(&self.raw_options, &self.container_observer);
        let validated_options = self.validated_options.borrow().clone();
        let bare_font_info =
            create_bare_font_info_from_validated_settings(&validated_options, partial_env.pixel_ratio, self.is_simple_widget);
        let font_info = self.environment.read_font_info(&bare_font_info);
        let env = EnvironmentalOptions {
            memory: Some(self.compute_options_memory.clone()),
            outer_width: partial_env.outer_width,
            outer_height: partial_env.outer_height - self.reserved_height.get(),
            font_info,
            extra_editor_class_name: partial_env.extra_editor_class_name,
            is_dominated_by_long_lines: self.is_dominated_by_long_lines.get(),
            view_line_count: f64::from(self.view_line_count.get()),
            line_numbers_digit_count: f64::from(self.line_numbers_digit_count.get()),
            empty_selection_clipboard: partial_env.empty_selection_clipboard,
            pixel_ratio: partial_env.pixel_ratio,
            tab_focus_mode: *validated_options.get(TAB_FOCUS_MODE) || tab_focus::get_tab_focus_mode(),
            input_mode: input_mode::get_input_mode(),
            accessibility_support: partial_env.accessibility_support,
            glyph_margin_decoration_lane_count: self.glyph_margin_decoration_lane_count.get(),
            edit_context_supported: partial_env.edit_context_supported,
        };
        EditorOptionsUtil::compute_options(&validated_options, &env)
    }
}

impl ConfigurationOptions for EditorConfiguration {
    /// `options`: a geração vigente das opções calculadas.
    fn options(&self) -> &ComputedEditorOptions {
        self.options.last().expect("o construtor escreve a primeira geração das opções")
    }
}

impl IDisposable for EditorConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorConfiguration for EditorConfiguration {
    fn is_simple_widget(&self) -> bool {
        self.is_simple_widget
    }

    fn context_menu_id(&self) -> MenuId {
        self.context_menu_id.clone()
    }

    fn on_did_change_fast(&self) -> Event<ConfigurationChangedEvent> {
        self.on_did_change_fast.event()
    }

    fn on_did_change(&self) -> Event<ConfigurationChangedEvent> {
        self.on_did_change.event()
    }

    fn get_raw_options(&self) -> Dynamic {
        self.raw_options.clone()
    }

    fn update_options(&self, new_options: &Dynamic) {
        let new_options = deep_clone_and_migrate_options(new_options);

        let did_change = EditorOptionsUtil::apply_update(&self.raw_options, &new_options);
        if !did_change {
            return;
        }

        *self.validated_options.borrow_mut() = Rc::new(EditorOptionsUtil::validate_options(&self.raw_options));
        self.recompute_options();
    }

    fn observe_container(&self, dimension: Option<Dimension>) {
        self.container_observer.observe(dimension);
    }

    fn set_is_dominated_by_long_lines(&self, is_dominated_by_long_lines: bool) {
        if self.is_dominated_by_long_lines.get() == is_dominated_by_long_lines {
            return;
        }
        self.is_dominated_by_long_lines.set(is_dominated_by_long_lines);
        self.recompute_options();
    }

    fn set_model_line_count(&self, model_line_count: i32) {
        let line_numbers_digit_count = digit_count(model_line_count);
        if self.line_numbers_digit_count.get() == line_numbers_digit_count {
            return;
        }
        self.line_numbers_digit_count.set(line_numbers_digit_count);
        self.recompute_options();
    }

    fn set_view_line_count(&self, view_line_count: i32) {
        if self.view_line_count.get() == view_line_count {
            return;
        }
        self.view_line_count.set(view_line_count);
        self.recompute_options();
    }

    fn set_reserved_height(&self, reserved_height: f64) {
        if self.reserved_height.get() == reserved_height {
            return;
        }
        self.reserved_height.set(reserved_height);
        self.recompute_options();
    }

    fn set_glyph_margin_decoration_lane_count(&self, decoration_lane_count: f64) {
        if self.glyph_margin_decoration_lane_count.get() == decoration_lane_count {
            return;
        }
        self.glyph_margin_decoration_lane_count.set(decoration_lane_count);
        self.recompute_options();
    }
}

/// `digitCount(n)`: o número de dígitos decimais (no mínimo 1).
fn digit_count(mut n: i32) -> i32 {
    let mut r = 0;
    while n != 0 {
        n = n.div_euclid(10);
        r += 1;
    }
    if r != 0 { r } else { 1 }
}

/// `deepCloneAndMigrateOptions`.
fn deep_clone_and_migrate_options(options: &Dynamic) -> Dynamic {
    let options = objects::deep_clone(options);
    migrate_options(&options);
    options
}

/// `options.hasOwnProperty(name)`.
fn has_own_property(value: &Dynamic, name: &str) -> bool {
    matches!(value, Dynamic::Object(object) if object.has_own(&JsString::from(name)))
}

/// `typeof x === 'object' && x` (o par de testes de `applyUpdate`).
fn is_non_null_object(value: &Dynamic) -> bool {
    value.type_of() == "object" && value.is_truthy()
}

/// `applyUpdate<T>(value, update)` de `editorOptions.ts`, sobre valores crus: mescla `update` em `value` (que é
/// modificado) e devolve o valor novo com o aviso de mudança.
pub fn apply_raw_update(value: &Dynamic, update: &Dynamic) -> (Dynamic, bool) {
    if !is_non_null_object(value) || !is_non_null_object(update) {
        return (update.clone(), !value.strict_equals(update));
    }
    if value.is_array() || update.is_array() {
        let array_equals = match (value, update) {
            (Dynamic::Array(a), Dynamic::Array(b)) => {
                let (a, b) = (a.to_vec(), b.to_vec());
                a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.strict_equals(y))
            }
            _ => false,
        };
        return (update.clone(), !array_equals);
    }
    let mut did_change = false;
    for (key, update_value) in update.own_entries() {
        let (new_value, changed) = apply_raw_update(&value.get(&key), &update_value);
        if changed {
            if let Dynamic::Object(object) = value {
                object.set(&key, new_value);
            }
            did_change = true;
        }
    }
    (value.clone(), did_change)
}

/// `UnicodeHighlight.applyUpdate`: `allowedCharacters` e `allowedLocales` são tratados de forma atômica.
fn apply_unicode_highlight_raw_update(value: &Dynamic, update: &Dynamic) -> (Dynamic, bool) {
    let mut value = value.clone();
    let mut did_change = false;
    for key in ["allowedCharacters", "allowedLocales"] {
        let update_part = update.get_str(key);
        if update_part.is_truthy() && value.is_truthy() && !objects::equals(&value.get_str(key), &update_part) {
            // `value = { ...value, [key]: update[key] }`.
            let js_key = JsString::from(key);
            let mut entries = value.own_entries();
            match entries.iter_mut().find(|(k, _)| *k == js_key) {
                Some(entry) => entry.1 = update_part,
                None => entries.push((js_key, update_part)),
            }
            value = Dynamic::object(entries);
            did_change = true;
        }
    }
    let (new_value, changed) = apply_raw_update(&value, update);
    (new_value, did_change || changed)
}

/// `EditorOptionsUtil`.
pub struct EditorOptionsUtil;

impl EditorOptionsUtil {
    pub fn validate_options(options: &Dynamic) -> ValidatedEditorOptions {
        let values = editor_options_registry()
            .iter()
            .map(|editor_option| {
                let value = if editor_option.name() == "_never_" { Dynamic::Undefined } else { options.get_str(editor_option.name()) };
                editor_option.validate(&value)
            })
            .collect();
        ValidatedEditorOptions { values }
    }

    pub fn compute_options(options: &ValidatedEditorOptions, env: &EnvironmentalOptions) -> ComputedEditorOptions {
        let mut result = ComputedEditorOptions::uninitialized();
        for editor_option in editor_options_registry().iter() {
            let id = editor_option.id();
            let value = editor_option.compute(env, &result, &**options.read(id));
            result.write(id, value);
        }
        result
    }

    /// `checkEquals(a, b)`: `None` quando nenhuma opção mudou.
    pub fn check_equals(a: &ComputedEditorOptions, b: &ComputedEditorOptions) -> Option<ConfigurationChangedEvent> {
        let mut result = vec![false; EditorOption::COUNT];
        let mut something_changed = false;
        for editor_option in editor_options_registry().iter() {
            let id = editor_option.id();
            let changed = !editor_option.values_equal(&**a.get_any(id), &**b.get_any(id));
            result[id as usize] = changed;
            something_changed |= changed;
        }
        something_changed.then(|| ConfigurationChangedEvent::new(result))
    }

    /// `applyUpdate(options, update)`: devolve `true` se algo mudou. Modifica `options`.
    pub fn apply_update(options: &Dynamic, update: &Dynamic) -> bool {
        let mut changed = false;
        for editor_option in editor_options_registry().iter() {
            let name = editor_option.name();
            if !has_own_property(update, name) {
                continue;
            }
            let (old_value, new_value) = (options.get_str(name), update.get_str(name));
            let (result_value, did_change) = if editor_option.id() == EditorOption::UnicodeHighlighting {
                apply_unicode_highlight_raw_update(&old_value, &new_value)
            } else {
                apply_raw_update(&old_value, &new_value)
            };
            if let Dynamic::Object(object) = options {
                object.set(&JsString::from(name), result_value);
            }
            changed = changed || did_change;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(value: f64) -> Dynamic {
        Dynamic::Number(value)
    }

    #[test]
    fn digit_count_counts_decimal_digits() {
        assert_eq!([digit_count(0), digit_count(9), digit_count(10), digit_count(12345)], [1, 1, 2, 5]);
    }

    #[test]
    fn generations_keep_every_reference_across_segments() {
        let generations = Generations::new();
        assert!(generations.last().is_none());
        let mut seen = Vec::new();
        for index in 0..200_usize {
            generations.push(index);
            seen.push(generations.last().expect("acabou de escrever"));
        }
        assert!(seen.iter().enumerate().all(|(index, value)| **value == index));
    }

    #[test]
    fn raw_update_merges_objects_in_place_and_replaces_the_rest() {
        let minimap = Dynamic::object([("enabled", Dynamic::Bool(true)), ("scale", number(1.0))]);
        let options = Dynamic::object([("minimap", minimap.clone()), ("fontSize", number(14.0))]);

        let same = Dynamic::object([("minimap", Dynamic::object([("enabled", Dynamic::Bool(true))]))]);
        assert!(!EditorOptionsUtil::apply_update(&options, &same));

        let update = Dynamic::object([("minimap", Dynamic::object([("scale", number(2.0))])), ("fontSize", number(16.0))]);
        assert!(EditorOptionsUtil::apply_update(&options, &update));
        assert_eq!(minimap.get_str("scale"), number(2.0));
        assert_eq!(minimap.get_str("enabled"), Dynamic::Bool(true));
        assert_eq!(options.get_str("fontSize"), number(16.0));
    }

    #[test]
    fn raw_update_compares_arrays_by_element() {
        let (_, changed) = apply_raw_update(
            &Dynamic::array([number(1.0), number(2.0)]),
            &Dynamic::array([number(1.0), number(2.0)]),
        );
        assert!(!changed);
        let (_, changed) = apply_raw_update(&Dynamic::array([number(1.0)]), &Dynamic::array([number(2.0)]));
        assert!(changed);
    }

    #[test]
    fn unicode_highlight_allowed_sets_are_replaced_atomically() {
        let value = Dynamic::object([("allowedCharacters", Dynamic::object([("a", Dynamic::Bool(true))]))]);
        let update = Dynamic::object([("allowedCharacters", Dynamic::object([("b", Dynamic::Bool(true))]))]);
        let (new_value, changed) = apply_unicode_highlight_raw_update(&value, &update);
        assert!(changed);
        let characters = new_value.get_str("allowedCharacters");
        assert!(characters.get_str("a").is_undefined());
        assert_eq!(characters.get_str("b"), Dynamic::Bool(true));
    }
}
