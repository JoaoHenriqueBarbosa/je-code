//! Porte de `src/vs/editor/test/browser/config/fontMeasurements.test.ts` (suíte `FontMeasurements`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (em [`Fixture::new`]), e o
//!   `store` da suíte é o próprio guarda.
//! - O DOM e o canvas viram o [`TestEnvironment`]: `registerWindow`/`onDidUnregisterWindow` são [`TestEnvironment::register_window`]
//!   (o descarte do registro dispara o evento), `PixelRatio` vale 1, e `readCharWidths` é dublado como o original dubla o
//!   `CharWidthRequest.prototype.fulfill`: cada pedido recebe `8 * chr.length` mais o desvio que o caso define. O caso
//!   `serializes empty current-session failed font measurements` usa o `readCharWidths` real no original, que não mede nada
//!   com `fontSize: 0`; a dublê equivalente devolve largura 0 para essa fonte.
//! - `window` é o [`WindowId`]: `mainWindow` é 1 e a janela auxiliar (`ensureCodeWindow(auxiliaryWindow, 999)`) é 999.
//! - `sinon.useFakeTimers()` é `with_faked_timers` (o `setTimeout` do original em cada janela é o do loop do thread), e
//!   `clock.tick(5000)` é `advance_fake_time(5000)`.
//! - `assert.deepStrictEqual` é `assert_eq!` sobre tuplas; `FontInfo` compartilhado é `Rc<FontInfo>` e `===` é `Rc::ptr_eq`.
//! - `allows closed-window font measurements to be garbage collected` roda sempre (o original pula sem `gc` exposto): sem
//!   coletor, a leitura solta é um `Weak` que deixa de subir quando o último `Rc` some com o cache da janela.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::runtime::{advance_fake_time, with_faked_timers};
use je_editor::config::char_width_reader::CharWidthRequest;
use je_editor::config::editor_options::font_ligatures::EditorFontLigatures;
use je_editor::config::font_info::{BareFontInfo, FontInfo, FontInfoOptions, SERIALIZED_FONT_INFO_VERSION};
use je_editor::config::font_measurements::{
    FontMeasurementsEnvironment, FontMeasurementsImpl, SerializedFontInfo, WindowId,
};

const MAIN_WINDOW: WindowId = 1;

type Reader = Box<dyn Fn(&BareFontInfo, &CharWidthRequest)>;

struct TestEnvironment {
    unregistered: Rc<Emitter<WindowId>>,
    reader: Reader,
}

impl TestEnvironment {
    /// `registerWindow(window)`: descartar o registro tira a janela (`onDidUnregisterWindow`).
    fn register_window(&self, window: WindowId) -> DisposableRef {
        let unregistered = self.unregistered.clone();
        to_disposable(move || unregistered.fire(window))
    }
}

impl FontMeasurementsEnvironment for TestEnvironment {
    fn pixel_ratio(&self, _target_window: WindowId) -> f64 {
        1.0
    }

    fn on_did_unregister_window(&self) -> Event<WindowId> {
        self.unregistered.event()
    }

    fn read_char_widths(&self, _target_window: WindowId, bare_font_info: &BareFontInfo, requests: &[Rc<CharWidthRequest>]) {
        for request in requests {
            (self.reader)(bare_font_info, request);
        }
    }
}

/// O leitor que mede `8 * chr.length`, e largura 0 para uma fonte de tamanho 0 (que o canvas real não consegue medir).
fn plain_reader() -> Reader {
    Box::new(|bare, request| {
        request.fulfill(if bare.font_size == 0.0 { 0.0 } else { 8.0 * request.chr_length() as f64 });
    })
}

struct Fixture {
    store: LeakGuard,
    environment: Rc<TestEnvironment>,
}

impl Fixture {
    fn new(reader: Reader) -> Fixture {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let unregistered = store.add_rc(Emitter::new());
        Fixture { store, environment: Rc::new(TestEnvironment { unregistered, reader }) }
    }

    fn measurements(&self) -> Rc<FontMeasurementsImpl> {
        self.store.add_rc(FontMeasurementsImpl::new(self.environment.clone()))
    }

    /// `createAuxiliaryWindow()`: o registro da janela, descartado ao fim do caso se ainda não foi.
    fn create_auxiliary_window(&self) -> (WindowId, DisposableRef) {
        let registration = self.store.add(self.environment.register_window(AUXILIARY_WINDOW));
        (AUXILIARY_WINDOW, registration)
    }
}

const AUXILIARY_WINDOW: WindowId = 999;

fn restored_font_info() -> SerializedFontInfo {
    SerializedFontInfo {
        version: SERIALIZED_FONT_INFO_VERSION,
        pixel_ratio: 1.0,
        font_family: "monospace".into(),
        font_weight: "normal".into(),
        font_size: 14.0,
        font_feature_settings: "\"liga\" off, \"calt\" off".into(),
        font_variation_settings: "normal".into(),
        line_height: 19.0,
        letter_spacing: 0.0,
        is_monospace: true,
        typical_halfwidth_character_width: 8.0,
        typical_fullwidth_character_width: 14.0,
        can_use_halfwidth_rightwards_arrow: true,
        space_width: 8.0,
        middot_width: 8.0,
        wsmiddot_width: 14.0,
        max_digit_width: 8.0,
    }
}

/// `new FontInfo(serialized, false)`.
fn untrusted_font_info(serialized: &SerializedFontInfo) -> FontInfo {
    FontInfo::new(FontInfoOptions::from(serialized), false)
}

fn assert_ligature_classification(probe: &'static str, width_difference: f64, is_monospace: bool) {
    let measured_ligatures: Rc<RefCell<Vec<String>>> = Rc::default();
    let measured = measured_ligatures.clone();
    let fixture = Fixture::new(Box::new(move |_bare, request| {
        request.fulfill(8.0 * request.chr_length() as f64 + if request.chr == probe { width_difference } else { 0.0 });
        if request.chr_length() > 1 {
            measured.borrow_mut().push(request.chr.clone());
        }
    }));
    let font_measurements = fixture.measurements();
    let options = untrusted_font_info(&SerializedFontInfo {
        font_feature_settings: EditorFontLigatures::ON.into(),
        ..restored_font_info()
    });
    let font_info = font_measurements.read_font_info(MAIN_WINDOW, &options);

    assert_eq!(
        (font_info.is_monospace, measured_ligatures.borrow().clone()),
        (is_monospace, vec!["fi".to_owned(), "<=".to_owned(), "=>".to_owned(), "===".to_owned()])
    );
}

#[test]
fn classifies_width_preserving_fi_ligatures_as_monospace() {
    assert_ligature_classification("fi", 0.0, true);
}

#[test]
fn classifies_fi_ligatures_narrower_just_inside_the_tolerance_as_monospace() {
    assert_ligature_classification("fi", -0.004, true);
}

#[test]
fn classifies_fi_ligatures_wider_just_inside_the_tolerance_as_monospace() {
    assert_ligature_classification("fi", 0.004, true);
}

#[test]
fn classifies_fi_ligatures_narrower_just_outside_the_tolerance_as_non_monospace() {
    assert_ligature_classification("fi", -0.006, false);
}

#[test]
fn classifies_fi_ligatures_wider_just_outside_the_tolerance_as_non_monospace() {
    assert_ligature_classification("fi", 0.006, false);
}

#[test]
fn classifies_width_preserving_le_ligatures_as_monospace() {
    assert_ligature_classification("<=", 0.0, true);
}

#[test]
fn classifies_le_ligatures_narrower_just_inside_the_tolerance_as_monospace() {
    assert_ligature_classification("<=", -0.004, true);
}

#[test]
fn classifies_le_ligatures_wider_just_inside_the_tolerance_as_monospace() {
    assert_ligature_classification("<=", 0.004, true);
}

#[test]
fn classifies_le_ligatures_narrower_just_outside_the_tolerance_as_non_monospace() {
    assert_ligature_classification("<=", -0.006, false);
}

#[test]
fn classifies_le_ligatures_wider_just_outside_the_tolerance_as_non_monospace() {
    assert_ligature_classification("<=", 0.006, false);
}

#[test]
fn classifies_width_preserving_arrow_ligatures_as_monospace() {
    assert_ligature_classification("=>", 0.0, true);
}

#[test]
fn classifies_arrow_ligatures_narrower_just_inside_the_tolerance_as_monospace() {
    assert_ligature_classification("=>", -0.004, true);
}

#[test]
fn classifies_arrow_ligatures_wider_just_inside_the_tolerance_as_monospace() {
    assert_ligature_classification("=>", 0.004, true);
}

#[test]
fn classifies_arrow_ligatures_narrower_just_outside_the_tolerance_as_non_monospace() {
    assert_ligature_classification("=>", -0.006, false);
}

#[test]
fn classifies_arrow_ligatures_wider_just_outside_the_tolerance_as_non_monospace() {
    assert_ligature_classification("=>", 0.006, false);
}

#[test]
fn classifies_width_preserving_triple_equals_ligatures_as_monospace() {
    assert_ligature_classification("===", 0.0, true);
}

#[test]
fn classifies_triple_equals_ligatures_narrower_just_inside_the_tolerance_as_monospace() {
    assert_ligature_classification("===", -0.004, true);
}

#[test]
fn classifies_triple_equals_ligatures_wider_just_inside_the_tolerance_as_monospace() {
    assert_ligature_classification("===", 0.004, true);
}

#[test]
fn classifies_triple_equals_ligatures_narrower_just_outside_the_tolerance_as_non_monospace() {
    assert_ligature_classification("===", -0.006, false);
}

#[test]
fn classifies_triple_equals_ligatures_wider_just_outside_the_tolerance_as_non_monospace() {
    assert_ligature_classification("===", 0.006, false);
}

#[test]
fn releases_readings_for_unregistered_windows_without_invalidating_the_main_window() {
    let fixture = Fixture::new(plain_reader());
    let font_measurements = fixture.measurements();
    let (auxiliary_window, registration) = fixture.create_auxiliary_window();
    let options = untrusted_font_info(&restored_font_info());
    let main_font = font_measurements.read_font_info(MAIN_WINDOW, &options);
    let auxiliary_font = font_measurements.read_font_info(auxiliary_window, &options);
    assert!(auxiliary_font.is_trusted);

    registration.dispose();

    assert_eq!(
        (
            font_measurements.has_window_cache(auxiliary_window),
            font_measurements.serialize_font_info(MAIN_WINDOW),
            Rc::ptr_eq(&font_measurements.read_font_info(MAIN_WINDOW, &options), &main_font),
        ),
        (false, Some(vec![main_font.clone()]), true)
    );
}

#[test]
fn does_not_emit_a_delayed_font_change_for_an_unregistered_window() {
    with_faked_timers(|| {
        let fixture = Fixture::new(plain_reader());
        let font_measurements = fixture.measurements();
        let (auxiliary_window, registration) = fixture.create_auxiliary_window();
        let changes = Rc::new(Cell::new(0));
        let counter = changes.clone();
        fixture.store.add(font_measurements.on_did_change().listen(move |_| counter.set(counter.get() + 1)));
        font_measurements.restore_font_info(auxiliary_window, &[restored_font_info()]);

        registration.dispose();
        advance_fake_time(5000);

        assert_eq!((changes.get(), font_measurements.has_window_cache(auxiliary_window)), (0, false));
    });
}

#[test]
fn allows_closed_window_font_measurements_to_be_garbage_collected() {
    let fixture = Fixture::new(plain_reader());
    let font_measurements = fixture.measurements();
    let (auxiliary_window, registration) = fixture.create_auxiliary_window();
    let reading = Rc::downgrade(&font_measurements.read_font_info(auxiliary_window, &untrusted_font_info(&restored_font_info())));
    registration.dispose();

    assert!(reading.upgrade().is_none(), "The closed window font reading is still retained");
}

#[test]
fn preserves_restored_untrusted_font_information_through_eviction() {
    with_faked_timers(|| {
        let fixture = Fixture::new(plain_reader());
        let font_measurements = fixture.measurements();
        let initially_serialized = font_measurements.serialize_font_info(MAIN_WINDOW);
        let change_count = Rc::new(Cell::new(0));
        let counter = change_count.clone();
        fixture.store.add(font_measurements.on_did_change().listen(move |_| counter.set(counter.get() + 1)));

        font_measurements.restore_font_info(MAIN_WINDOW, &[restored_font_info()]);
        let is_trusted = font_measurements.read_font_info(MAIN_WINDOW, &untrusted_font_info(&restored_font_info())).is_trusted;
        let serialized_before_eviction = font_measurements.serialize_font_info(MAIN_WINDOW);

        advance_fake_time(5000);

        let serialized_after_eviction = font_measurements.serialize_font_info(MAIN_WINDOW);
        let change_count_after_eviction = change_count.get();

        font_measurements.clear_all_font_infos();

        assert_eq!(
            (
                initially_serialized,
                is_trusted,
                serialized_before_eviction,
                serialized_after_eviction,
                change_count_after_eviction,
                font_measurements.serialize_font_info(MAIN_WINDOW),
            ),
            (Some(vec![]), false, None, None, 1, Some(vec![]))
        );
    });
}

#[test]
fn serializes_empty_current_session_failed_font_measurements() {
    let fixture = Fixture::new(plain_reader());
    let font_measurements = fixture.measurements();
    let font_info = font_measurements.read_font_info(
        MAIN_WINDOW,
        &untrusted_font_info(&SerializedFontInfo { font_size: 0.0, ..restored_font_info() }),
    );

    assert_eq!(
        (font_info.is_trusted, font_measurements.serialize_font_info(MAIN_WINDOW)),
        (false, Some(vec![]))
    );
}
