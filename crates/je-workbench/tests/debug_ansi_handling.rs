//! Porte de `src/vs/workbench/contrib/debug/test/browser/debugANSIHandling.test.ts` (suíte `Debug - ANSI Handling`), caso a
//! caso e na mesma ordem.
//!
//! Os 8 casos, todos portados: `appendStylizedStringToContainer`, `Expected single sequence operation`, `Expected single
//! 8-bit color sequence operation`, `Expected single 24-bit color sequence operation`, `Expected multiple sequence
//! operation`, `Invalid codes treated as regular text`, `Empty sequence output`, `calcANSI8bitColor`.
//!
//! Notas de tradução:
//! - **DOM.** O `HTMLSpanElement` é o [`Element`] de `link_detector.rs`; `isHTMLSpanElement(child)` é a conferência do
//!   `tagName` `SPAN`; `child.classList.length`, `classList.contains` e `style.<propriedade>` leem o atributo `class` e as
//!   declarações do atributo `style` (`style_property`).
//! - **`assertInlineColor`.** O original atribui o CSS esperado a `element.style.<cor>` e compara com o valor que havia
//!   antes (o navegador normaliza o texto); aqui o valor esperado já sai normalizado do `format_rgb`, então o caso compara
//!   o valor gravado com o CSS esperado, com a mesma lógica de `colorShouldMatch`.
//! - **`setup`.** `createMockDebugModel`, `createTestSession` e o `LinkDetector` (os serviços de teste do workbench, como em
//!   `link_detector.rs`) são montados antes do `LeakGuard` do caso, como o `setup` do original (o
//!   `ensureNoDisposablesAreLeakedInTestSuite()` é registrado depois do `setup`); o `teardown` é o `dispose` do store.
//!   `registerColors()` não existe no porte (a cor do tema é só o nome da variável CSS).
//! - `new RGBA(r, g, b)` é `Srgba::opaque`; `Color.Format.CSS.formatRGB(new Color(color))` é `format_rgb`.
//! - `generateUuid()` é o do je-base.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::opener::opener::{
    IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions, ResolvedExternalUri,
};
use je_theme::color::format::css as css_format;
use je_theme::color::{Color, Srgba};
use je_workbench::contrib::debug::debug_ansi_handling::{append_stylized_string_to_container, calc_ansi_8bit_color, handle_ansi_output, style_property};
use je_workbench::contrib::debug::debug_session::DebugSession;
use je_workbench::contrib::debug::link_detector::{DebugLinkHoverBehavior, DebugLinkHoverBehaviorTypeData, Element, ITunnelService, LinkDetector};
use je_workbench::test::browser::workbench_test_services::{TestEditorService, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::TestFileService;
use support::mock_debug::{create_mock_debug_model, create_test_session};

/// `{ canTunnel: () => false }`.
struct NoTunnelService;

impl ITunnelService for NoTunnelService {
    fn can_tunnel(&self, _uri: &URI) -> bool {
        false
    }
}

/// O `IOpenerService` do `workbenchInstantiationService`: nenhum caso abre um link.
struct TestOpenerService;

impl IOpenerService for TestOpenerService {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        unreachable!("o linkDetector não registra opener")
    }

    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        unreachable!("o linkDetector não registra validador")
    }

    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        unreachable!("o linkDetector não registra resolvedor")
    }

    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        unreachable!("o linkDetector não troca o opener externo")
    }

    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        unreachable!("o linkDetector não registra opener externo")
    }

    fn open(&self, _resource: &OpenResource, _options: Option<&OpenOptions>) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        unreachable!("o linkDetector não resolve URI externo")
    }
}

/// O `setup`: o modelo, a sessão e o `LinkDetector`.
struct Setup {
    disposables: Rc<DisposableStore>,
    session: Rc<DebugSession>,
    link_detector: LinkDetector,
}

fn setup() -> Setup {
    let disposables = DisposableStore::new();
    let model = create_mock_debug_model(&disposables);
    let session = create_test_session(&model, "mockSession", None, &disposables);
    let link_detector = LinkDetector::new(
        Rc::new(TestEditorService),
        TestFileService::new(),
        Rc::new(TestOpenerService),
        TestPathService::new(None, None),
        Rc::new(NoTunnelService),
        TestEnvironmentService::new(),
        TestConfigurationService::new(None),
    );
    Setup { disposables, session, link_detector }
}

impl Setup {
    /// O `teardown`.
    fn teardown(&self) {
        self.disposables.dispose();
    }
}

/// `{ type: DebugLinkHoverBehavior.None, store: new DisposableStore() }`.
fn none_hover() -> DebugLinkHoverBehaviorTypeData {
    DebugLinkHoverBehaviorTypeData { kind: DebugLinkHoverBehavior::None, store: DisposableStore::new() }
}

fn class_list(element: &Element) -> Vec<String> {
    element.get_attribute("class").map(|value| value.split_whitespace().map(str::to_owned).collect()).unwrap_or_default()
}

/// `isHTMLSpanElement(child)`.
fn assert_span(element: &Element) {
    assert_eq!(element.tag_name(), "SPAN", "Unexpected assertion error");
}

/// Apply an ANSI sequence to {@link #getSequenceOutput}: the span that contains the stylized text.
fn get_sequence_output(setup: &Setup, sequence: &str) -> Rc<Element> {
    let hover_behavior = none_hover();
    let root = handle_ansi_output(sequence, &setup.link_detector, setup.session.root.as_ref(), Some(&[]), &hover_behavior);
    assert_eq!(1, root.children().len());
    let child = root.children().last().cloned().expect("o filho");
    hover_behavior.store.dispose();
    assert_span(&child);
    child
}

/// Assert that a given ANSI sequence maintains added content following the ANSI code, and that the provided assertion
/// passes. The provided sequence should contain ANSI codes only, and should not include actual text content as it is
/// provided by this function.
fn assert_single_sequence_element(setup: &Setup, sequence: &str, assertion: impl FnOnce(&Rc<Element>)) {
    let child = get_sequence_output(setup, &format!("{sequence}content"));
    assert_eq!("content", child.text_content());
    assertion(&child);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorKind {
    Background,
    Foreground,
    Underline,
}

/// Assert that a given DOM element has the custom inline CSS style matching the color value provided; with `None`, assert
/// that no value is set. With `color_should_match` false, assert that the color does NOT match.
fn assert_inline_color(element: &Element, color_type: ColorKind, color: Option<Srgba>, message: &str, color_should_match: bool) {
    let property = match color_type {
        ColorKind::Background => "background-color",
        ColorKind::Foreground => "color",
        ColorKind::Underline => "text-decoration-color",
    };
    match color {
        Some(color) => {
            let css_color = css_format::format_rgb(&Color::from(color));
            let style_before = style_property(element, property);
            assert_eq!(style_before == css_color, color_should_match, "{message} (found color: {style_before}, expected {css_color}).");
        }
        None => assert!(style_property(element, property).is_empty(), "{message}"),
    }
}

fn inline(element: &Element, color_type: ColorKind, color: Option<Srgba>) {
    assert_inline_color(element, color_type, color, "Incorrect color style found", true);
}

type ChildAssertion = Box<dyn Fn(&Rc<Element>)>;

/// Assert that a given ANSI sequence produces the expected number of spans. For each child, run the provided assertion.
fn assert_multiple_sequence_elements(setup: &Setup, sequence: &str, assertions: Vec<ChildAssertion>, elements_expected: Option<usize>) {
    let elements_expected = elements_expected.unwrap_or(assertions.len());
    let hover_behavior = none_hover();
    let root = handle_ansi_output(sequence, &setup.link_detector, setup.session.root.as_ref(), Some(&[]), &hover_behavior);
    let children = root.children();
    assert_eq!(elements_expected, children.len());
    for (i, assertion) in assertions.iter().enumerate().take(elements_expected) {
        assert_span(&children[i]);
        assertion(&children[i]);
    }
    hover_behavior.store.dispose();
}

/// `(child) => { assert.strictEqual(count, child.classList.length); present.forEach(contains); absent.forEach(!contains) }`.
fn expect(count: Option<usize>, present: &'static [&'static str], absent: &'static [&'static str]) -> ChildAssertion {
    Box::new(move |child| {
        if let Some(count) = count {
            assert_eq!(count, class_list(child).len(), "Incorrect number of classes: {:?}", class_list(child));
        }
        for class in present {
            assert!(class_list(child).iter().any(|c| c == class), "Class {class} not found in {:?}", class_list(child));
        }
        for class in absent {
            assert!(!class_list(child).iter().any(|c| c == class), "Class {class} found in {:?}", class_list(child));
        }
    })
}

const FG: &str = "code-foreground-colored";
const BG: &str = "code-background-colored";
const UL_COLORED: &str = "code-underline-colored";

fn has_class(child: &Element, class: &str) -> bool {
    class_list(child).iter().any(|c| c == class)
}

#[test]
fn append_stylized_string_to_container_case() {
    let setup = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root = Element::new("span");

    assert_eq!(0, root.children().len());

    let hover_behavior = none_hover();
    let classes = |names: &[&str]| -> Vec<String> { names.iter().map(|n| (*n).to_owned()).collect() };
    append_stylized_string_to_container(
        &root,
        "content1",
        &classes(&["class1", "class2"]),
        &setup.link_detector,
        setup.session.root.as_ref(),
        None,
        None,
        None,
        None,
        0,
        &hover_behavior,
    );
    append_stylized_string_to_container(
        &root,
        "content2",
        &classes(&["class2", "class3"]),
        &setup.link_detector,
        setup.session.root.as_ref(),
        None,
        None,
        None,
        None,
        0,
        &hover_behavior,
    );

    assert_eq!(2, root.children().len());

    let children = root.children();
    let child = &children[0];
    assert_span(child);
    assert_eq!("content1", child.text_content());
    assert!(has_class(child, "class1"));
    assert!(has_class(child, "class2"));

    let child = children.last().unwrap();
    assert_span(child);
    assert_eq!("content2", child.text_content());
    assert!(has_class(child, "class2"));
    assert!(has_class(child, "class3"));
    hover_behavior.store.dispose();
    setup.teardown();
}

#[test]
fn expected_single_sequence_operation() {
    let setup = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // Bold code
    assert_single_sequence_element(&setup, "\x1b[1m", |child| {
        assert!(has_class(child, "code-bold"), "Bold formatting not detected after bold ANSI code.");
    });

    // Italic code
    assert_single_sequence_element(&setup, "\x1b[3m", |child| {
        assert!(has_class(child, "code-italic"), "Italic formatting not detected after italic ANSI code.");
    });

    // Underline code
    assert_single_sequence_element(&setup, "\x1b[4m", |child| {
        assert!(has_class(child, "code-underline"), "Underline formatting not detected after underline ANSI code.");
    });

    for i in 30..=37 {
        let custom_class_name = "code-foreground-colored";

        // Foreground colour class
        assert_single_sequence_element(&setup, &format!("\x1b[{i}m"), |child| {
            assert!(has_class(child, custom_class_name), "Custom foreground class not found on element after foreground ANSI code #{i}.");
        });

        // Cancellation code removes colour class
        assert_single_sequence_element(&setup, &format!("\x1b[{i};39m"), |child| {
            assert!(!has_class(child, custom_class_name), "Custom foreground class still found after foreground cancellation code.");
            assert_inline_color(child, ColorKind::Foreground, None, "Custom color style still found after foreground cancellation code.", true);
        });
    }

    for i in 40..=47 {
        let custom_class_name = "code-background-colored";

        // Foreground colour class
        assert_single_sequence_element(&setup, &format!("\x1b[{i}m"), |child| {
            assert!(has_class(child, custom_class_name), "Custom background class not found on element after background ANSI code #{i}.");
        });

        // Cancellation code removes colour class
        assert_single_sequence_element(&setup, &format!("\x1b[{i};49m"), |child| {
            assert!(!has_class(child, custom_class_name), "Custom background class still found after background cancellation code.");
            assert_inline_color(child, ColorKind::Foreground, None, "Custom color style still found after background cancellation code.", true);
        });
    }

    // check all basic colors for underlines (full range is checked elsewhere, here we check cancelation)
    for i in 0..=255 {
        let custom_class_name = "code-underline-colored";

        // Underline colour class
        assert_single_sequence_element(&setup, &format!("\x1b[58;5;{i}m"), |child| {
            assert!(has_class(child, custom_class_name), "Custom underline color class not found on element after underline color ANSI code 58;5;{i}m.");
        });

        // Cancellation underline color code removes colour class
        assert_single_sequence_element(&setup, &format!("\x1b[58;5;{i}m\x1b[59m"), |child| {
            assert!(!has_class(child, custom_class_name), "Custom underline color class still found after underline color cancellation code 59m.");
            assert_inline_color(child, ColorKind::Underline, None, "Custom underline color style still found after underline color cancellation code 59m.", true);
        });
    }

    // Different codes do not cancel each other
    assert_single_sequence_element(&setup, "\x1b[1;3;4;30;41m", |child| {
        expect(Some(5), &["code-bold", "code-italic", "code-underline", FG, BG], &[])(child);
    });

    // Different codes do not ACCUMULATE more than one copy of each class
    assert_single_sequence_element(&setup, "\x1b[1;1;2;2;3;3;4;4;5;5;6;6;8;8;9;9;21;21;53;53;73;73;74;74m", |child| {
        expect(
            Some(10),
            &[
                "code-bold",
                "code-italic",
                "code-dim",
                "code-blink",
                "code-rapid-blink",
                "code-double-underline",
                "code-hidden",
                "code-strike-through",
                "code-overline",
                "code-subscript",
            ],
            &["code-underline", "code-superscript"],
        )(child);
    });

    // More Different codes do not cancel each other
    assert_single_sequence_element(&setup, "\x1b[1;2;5;6;21;8;9m", |child| {
        expect(
            Some(7),
            &["code-bold", "code-dim", "code-blink", "code-rapid-blink", "code-double-underline", "code-hidden", "code-strike-through"],
            &[],
        )(child);
    });

    // New foreground codes don't remove old background codes and vice versa
    assert_single_sequence_element(&setup, "\x1b[40;31;42;33m", |child| {
        expect(Some(2), &[BG, FG], &[])(child);
    });

    // Duplicate codes do not change output
    assert_single_sequence_element(&setup, "\x1b[1;1;4;1;4;4;1;4m", |child| {
        expect(None, &["code-bold", "code-underline"], &[])(child);
    });

    // Extra terminating semicolon does not change output
    assert_single_sequence_element(&setup, "\x1b[1;4;m", |child| {
        expect(None, &["code-bold", "code-underline"], &[])(child);
    });

    // Cancellation code removes multiple codes
    assert_single_sequence_element(&setup, "\x1b[1;4;30;41;32;43;34;45;36;47;0m", |child| {
        assert_eq!(0, class_list(child).len(), "Cancellation ANSI code should clear ALL formatting.");
        assert_inline_color(child, ColorKind::Background, None, "Cancellation ANSI code should clear ALL formatting.", true);
        assert_inline_color(child, ColorKind::Foreground, None, "Cancellation ANSI code should clear ALL formatting.", true);
    });
    setup.teardown();
}

#[test]
fn expected_single_8_bit_color_sequence_operation() {
    let setup = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Basic and bright color codes specified with 8-bit color code format
    for i in 0..=15 {
        // As these are controlled by theme, difficult to check actual color value
        // Foreground codes should add standard classes
        assert_single_sequence_element(&setup, &format!("\x1b[38;5;{i}m"), |child| {
            assert!(has_class(child, FG), "Custom color class not found after foreground 8-bit color code 38;5;{i}");
        });

        // Background codes should add standard classes
        assert_single_sequence_element(&setup, &format!("\x1b[48;5;{i}m"), |child| {
            assert!(has_class(child, BG), "Custom color class not found after background 8-bit color code 48;5;{i}");
        });
    }

    // 8-bit advanced colors
    for i in 16..=255 {
        let expected = calc_ansi_8bit_color(f64::from(i));

        // Foreground codes should add custom class and inline style
        assert_single_sequence_element(&setup, &format!("\x1b[38;5;{i}m"), |child| {
            assert!(has_class(child, FG), "Custom color class not found after foreground 8-bit color code 38;5;{i}");
            assert_inline_color(child, ColorKind::Foreground, expected, &format!("Incorrect or no color styling found after foreground 8-bit color code 38;5;{i}"), true);
        });

        // Background codes should add custom class and inline style
        assert_single_sequence_element(&setup, &format!("\x1b[48;5;{i}m"), |child| {
            assert!(has_class(child, BG), "Custom color class not found after background 8-bit color code 48;5;{i}");
            assert_inline_color(child, ColorKind::Background, expected, &format!("Incorrect or no color styling found after background 8-bit color code 48;5;{i}"), true);
        });

        // Color underline codes should add custom class and inline style
        assert_single_sequence_element(&setup, &format!("\x1b[58;5;{i}m"), |child| {
            assert!(has_class(child, UL_COLORED), "Custom color class not found after underline 8-bit color code 58;5;{i}");
            assert_inline_color(child, ColorKind::Underline, expected, &format!("Incorrect or no color styling found after underline 8-bit color code 58;5;{i}"), true);
        });
    }

    // Bad (nonexistent) color should not render
    assert_single_sequence_element(&setup, "\x1b[48;5;300m", |child| {
        assert_eq!(0, class_list(child).len(), "Bad ANSI color codes should have no effect.");
    });

    // Should ignore any codes after the ones needed to determine color
    assert_single_sequence_element(&setup, "\x1b[48;5;100;42;77;99;4;24m", |child| {
        assert!(has_class(child, BG));
        assert_eq!(1, class_list(child).len());
        inline(child, ColorKind::Background, calc_ansi_8bit_color(100.0));
    });
    setup.teardown();
}

#[test]
fn expected_single_24_bit_color_sequence_operation() {
    let setup = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // 24-bit advanced colors
    for r in (0..=255).step_by(64) {
        for g in (0..=255).step_by(64) {
            for b in (0..=255).step_by(64) {
                let color = Srgba::opaque(f64::from(r), f64::from(g), f64::from(b));
                // Foreground codes should add class and inline style
                assert_single_sequence_element(&setup, &format!("\x1b[38;2;{r};{g};{b}m"), |child| {
                    assert!(has_class(child, FG), "DOM should have \"code-foreground-colored\" class for advanced ANSI colors.");
                    inline(child, ColorKind::Foreground, Some(color));
                });

                // Background codes should add class and inline style
                assert_single_sequence_element(&setup, &format!("\x1b[48;2;{r};{g};{b}m"), |child| {
                    assert!(has_class(child, BG), "DOM should have \"code-foreground-colored\" class for advanced ANSI colors.");
                    inline(child, ColorKind::Background, Some(color));
                });

                // Underline color codes should add class and inline style
                assert_single_sequence_element(&setup, &format!("\x1b[58;2;{r};{g};{b}m"), |child| {
                    assert!(has_class(child, UL_COLORED), "DOM should have \"code-underline-colored\" class for advanced ANSI colors.");
                    inline(child, ColorKind::Underline, Some(color));
                });
            }
        }
    }

    // Invalid color should not render
    assert_single_sequence_element(&setup, "\x1b[38;2;4;4m", |child| {
        assert_eq!(0, class_list(child).len(), "Invalid color code \"38;2;4;4\" should not add a class (classes found: {:?}).", class_list(child));
        assert!(style_property(child, "color").is_empty(), "Invalid color code \"38;2;4;4\" should not add a custom color CSS.");
    });

    // Bad (nonexistent) color should not render
    assert_single_sequence_element(&setup, "\x1b[48;2;150;300;5m", |child| {
        assert_eq!(0, class_list(child).len(), "Nonexistent color code \"48;2;150;300;5\" should not add a class (classes found: {:?}).", class_list(child));
    });

    // Should ignore any codes after the ones needed to determine color
    assert_single_sequence_element(&setup, "\x1b[48;2;100;42;77;99;200;75m", |child| {
        assert!(has_class(child, BG), "Color code with extra (valid) items \"48;2;100;42;77;99;200;75\" should still treat initial part as valid code and add class \"code-background-custom\".");
        assert_eq!(1, class_list(child).len(), "Color code with extra items \"48;2;100;42;77;99;200;75\" should add one and only one class. (classes found: {:?}).", class_list(child));
        inline(child, ColorKind::Background, Some(Srgba::opaque(100.0, 42.0, 77.0)));
    });
    setup.teardown();
}

fn rgb(r: f64, g: f64, b: f64) -> Option<Srgba> {
    Some(Srgba::opaque(r, g, b))
}

#[test]
fn expected_multiple_sequence_operation() {
    let setup = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // Multiple codes affect the same text
    assert_single_sequence_element(&setup, "\x1b[1m\x1b[3m\x1b[4m\x1b[32m", |child| {
        expect(None, &["code-bold", "code-italic", "code-underline", FG], &[])(child);
    });

    // Consecutive codes do not affect previous ones
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[1mbold\x1b[32mgreen\x1b[4munderline\x1b[3mitalic\x1b[0mnothing",
        vec![
            expect(Some(1), &["code-bold"], &[]),
            expect(Some(2), &["code-bold", FG], &[]),
            expect(Some(3), &["code-bold", FG, "code-underline"], &[]),
            expect(Some(4), &["code-bold", FG, "code-underline", "code-italic"], &[]),
            expect(Some(0), &[], &[]),
        ],
        Some(5),
    );

    // Consecutive codes with ENDING/OFF codes do not LEAVE affect previous ones
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[1mbold\x1b[22m\x1b[32mgreen\x1b[4munderline\x1b[24m\x1b[3mitalic\x1b[23mjustgreen\x1b[0mnothing",
        vec![
            expect(Some(1), &["code-bold"], &[]),
            expect(Some(1), &[FG], &["code-bold"]),
            expect(Some(2), &[FG, "code-underline"], &[]),
            expect(Some(2), &[FG, "code-italic"], &["code-underline"]),
            expect(Some(1), &[FG], &["code-italic"]),
            expect(Some(0), &[], &[]),
        ],
        Some(6),
    );

    // more Consecutive codes with ENDING/OFF codes do not LEAVE affect previous ones
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[2mdim\x1b[22m\x1b[32mgreen\x1b[5mslowblink\x1b[25m\x1b[6mrapidblink\x1b[25mjustgreen\x1b[0mnothing",
        vec![
            expect(Some(1), &["code-dim"], &[]),
            expect(Some(1), &[FG], &["code-dim"]),
            expect(Some(2), &[FG, "code-blink"], &[]),
            expect(Some(2), &[FG, "code-rapid-blink"], &["code-blink"]),
            expect(Some(1), &[FG], &["code-rapid-blink"]),
            expect(Some(0), &[], &[]),
        ],
        Some(6),
    );

    // more Consecutive codes with ENDING/OFF codes do not LEAVE affect previous ones
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[8mhidden\x1b[28m\x1b[32mgreen\x1b[9mcrossedout\x1b[29m\x1b[21mdoubleunderline\x1b[24mjustgreen\x1b[0mnothing",
        vec![
            expect(Some(1), &["code-hidden"], &[]),
            expect(Some(1), &[FG], &["code-hidden"]),
            expect(Some(2), &[FG, "code-strike-through"], &[]),
            expect(Some(2), &[FG, "code-double-underline"], &["code-strike-through"]),
            expect(Some(1), &[FG], &["code-double-underline"]),
            expect(Some(0), &[], &[]),
        ],
        Some(6),
    );

    // underline, double underline are mutually exclusive, test underline->double underline->off and double underline->underline->off
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[4munderline\x1b[21mdouble underline\x1b[24munderlineOff\x1b[21mdouble underline\x1b[4munderline\x1b[24munderlineOff",
        vec![
            expect(Some(1), &["code-underline"], &[]),
            expect(Some(1), &["code-double-underline"], &["code-underline"]),
            expect(Some(0), &[], &[]),
            expect(Some(1), &["code-double-underline"], &[]),
            expect(Some(1), &["code-underline"], &["code-double-underline"]),
            expect(Some(0), &[], &[]),
        ],
        Some(6),
    );

    // underline and strike-through and overline can exist at the same time and
    // in any combination
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[4munderline\x1b[9mand strikethough\x1b[53mand overline\x1b[24munderlineOff\x1b[55moverlineOff\x1b[29mstriklethoughOff",
        vec![
            expect(Some(1), &["code-underline"], &[]),
            expect(Some(2), &["code-underline", "code-strike-through"], &[]),
            expect(Some(3), &["code-underline", "code-strike-through", "code-overline"], &[]),
            expect(Some(2), &["code-strike-through", "code-overline"], &["code-underline"]),
            expect(Some(1), &["code-strike-through"], &["code-underline", "code-overline"]),
            expect(Some(0), &[], &["code-strike-through"]),
        ],
        Some(6),
    );

    // double underline and strike-through and overline can exist at the same time and
    // in any combination
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[21mdoubleunderline\x1b[9mand strikethough\x1b[53mand overline\x1b[29mstriklethoughOff\x1b[55moverlineOff\x1b[24munderlineOff",
        vec![
            expect(Some(1), &["code-double-underline"], &[]),
            expect(Some(2), &["code-double-underline", "code-strike-through"], &[]),
            expect(Some(3), &["code-double-underline", "code-strike-through", "code-overline"], &[]),
            expect(Some(2), &["code-double-underline", "code-overline"], &["code-strike-through"]),
            expect(Some(1), &["code-double-underline"], &["code-strike-through", "code-overline"]),
            expect(Some(0), &[], &["code-double-underline"]),
        ],
        Some(6),
    );

    // superscript and subscript are mutually exclusive, test superscript->subscript->off and subscript->superscript->off
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[73msuperscript\x1b[74msubscript\x1b[75mneither\x1b[74msubscript\x1b[73msuperscript\x1b[75mneither",
        vec![
            expect(Some(1), &["code-superscript"], &[]),
            expect(Some(1), &["code-subscript"], &["code-superscript"]),
            expect(Some(0), &[], &[]),
            expect(Some(1), &["code-subscript"], &[]),
            expect(Some(1), &["code-superscript"], &["code-subscript"]),
            expect(Some(0), &[], &[]),
        ],
        Some(6),
    );

    // Consecutive font codes switch to new font class and remove previous and then final switch to default font removes class
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[11mFont1\x1b[12mFont2\x1b[13mFont3\x1b[14mFont4\x1b[15mFont5\x1b[10mdefaultFont",
        vec![
            expect(Some(1), &["code-font-1"], &[]),
            expect(Some(1), &["code-font-2"], &["code-font-1"]),
            expect(Some(1), &["code-font-3"], &["code-font-2"]),
            expect(Some(1), &["code-font-4"], &["code-font-3"]),
            expect(Some(1), &["code-font-5"], &["code-font-4"]),
            expect(Some(0), &[], &[]),
        ],
        Some(6),
    );

    // More Consecutive font codes switch to new font class and remove previous and then final switch to default font removes class
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[16mFont6\x1b[17mFont7\x1b[18mFont8\x1b[19mFont9\x1b[20mFont10\x1b[10mdefaultFont",
        vec![
            expect(Some(1), &["code-font-6"], &[]),
            expect(Some(1), &["code-font-7"], &["code-font-6"]),
            expect(Some(1), &["code-font-8"], &["code-font-7"]),
            expect(Some(1), &["code-font-9"], &["code-font-8"]),
            expect(Some(1), &["code-font-10"], &["code-font-9"]),
            expect(Some(0), &[], &[]),
        ],
        Some(6),
    );

    // Blackletter font codes can be turned off with other font codes or 23m
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[3mitalic\x1b[20mfont10blacklatter\x1b[23mitalicAndBlackletterOff\x1b[20mFont10Again\x1b[11mFont1\x1b[10mdefaultFont",
        vec![
            expect(Some(1), &["code-italic"], &[]),
            expect(Some(2), &["code-italic", "code-font-10"], &[]),
            expect(Some(0), &[], &[]),
            expect(Some(1), &["code-font-10"], &[]),
            expect(Some(1), &["code-font-1"], &["code-font-10"]),
            expect(Some(0), &[], &[]),
        ],
        Some(6),
    );

    // italic can be turned on/off with affecting font codes 1-9  (italic off will clear 'blackletter'(font 23) as per spec)
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[3mitalic\x1b[12mfont2\x1b[23mitalicOff\x1b[3mitalicFont2\x1b[10mjustitalic\x1b[23mnothing",
        vec![
            expect(Some(1), &["code-italic"], &[]),
            expect(Some(2), &["code-italic", "code-font-2"], &[]),
            expect(Some(1), &["code-font-2"], &["code-italic"]),
            expect(Some(2), &["code-italic", "code-font-2"], &[]),
            expect(Some(1), &["code-italic"], &["code-font-2"]),
            expect(Some(0), &[], &[]),
        ],
        Some(6),
    );

    // Reverse video reverses Foreground/Background colors WITH both SET and can called in sequence
    let fg = || rgb(10.0, 20.0, 30.0);
    let bg = || rgb(167.0, 168.0, 169.0);
    let reversed: fn() -> ChildAssertion = || {
        Box::new(|child| {
            expect(Some(2), &[BG, FG], &[])(child);
            inline(child, ColorKind::Foreground, rgb(167.0, 168.0, 169.0));
            inline(child, ColorKind::Background, rgb(10.0, 20.0, 30.0));
        })
    };
    let restored: fn() -> ChildAssertion = || {
        Box::new(|child| {
            expect(Some(2), &[BG, FG], &[])(child);
            inline(child, ColorKind::Background, rgb(167.0, 168.0, 169.0));
            inline(child, ColorKind::Foreground, rgb(10.0, 20.0, 30.0));
        })
    };
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[38;2;10;20;30mfg10,20,30\x1b[48;2;167;168;169mbg167,168,169\x1b[7m8ReverseVideo\x1b[7mDuplicateReverseVideo\x1b[27mReverseOff\x1b[27mDupReverseOff",
        vec![
            Box::new(move |child| {
                expect(Some(1), &[FG], &[])(child);
                inline(child, ColorKind::Foreground, fg());
            }),
            Box::new(move |child| {
                expect(Some(2), &[BG, FG], &[])(child);
                inline(child, ColorKind::Background, bg());
                inline(child, ColorKind::Foreground, fg());
            }),
            reversed(),
            reversed(),
            restored(),
            restored(),
        ],
        Some(6),
    );

    // Reverse video reverses Foreground/Background colors WITH ONLY foreground color SET
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[38;2;10;20;30mfg10,20,30\x1b[7m8ReverseVideo\x1b[27mReverseOff",
        vec![
            Box::new(|child| {
                expect(Some(1), &[FG], &[])(child);
                inline(child, ColorKind::Foreground, rgb(10.0, 20.0, 30.0));
            }),
            Box::new(|child| {
                expect(Some(1), &[BG], &[FG])(child);
                inline(child, ColorKind::Background, rgb(10.0, 20.0, 30.0));
            }),
            Box::new(|child| {
                expect(Some(1), &[FG], &[BG])(child);
                inline(child, ColorKind::Foreground, rgb(10.0, 20.0, 30.0));
            }),
        ],
        Some(3),
    );

    // Reverse video reverses Foreground/Background colors WITH ONLY background color SET
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[48;2;167;168;169mbg167,168,169\x1b[7m8ReverseVideo\x1b[27mReverseOff",
        vec![
            Box::new(|child| {
                expect(Some(1), &[BG], &[])(child);
                inline(child, ColorKind::Background, rgb(167.0, 168.0, 169.0));
            }),
            Box::new(|child| {
                expect(Some(1), &[FG], &[BG])(child);
                inline(child, ColorKind::Foreground, rgb(167.0, 168.0, 169.0));
            }),
            Box::new(|child| {
                expect(Some(1), &[BG], &[FG])(child);
                inline(child, ColorKind::Background, rgb(167.0, 168.0, 169.0));
            }),
        ],
        Some(3),
    );

    // Underline color Different types of color codes still cancel each other
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[58;2;101;102;103m24bitUnderline101,102,103\x1b[58;5;3m8bitsimpleUnderline\x1b[58;2;104;105;106m24bitUnderline104,105,106\x1b[58;5;101m8bitadvanced\x1b[58;2;200;200;200munderline200,200,200\x1b[59mUnderlineColorResetToDefault",
        vec![
            Box::new(|child| {
                expect(Some(1), &[UL_COLORED], &[])(child);
                inline(child, ColorKind::Underline, rgb(101.0, 102.0, 103.0));
            }),
            Box::new(|child| {
                expect(Some(1), &[UL_COLORED], &[])(child);
                // changed to simple theme color, don't know exactly what it should be, but it should NO LONGER BE 101,102,103
                assert_inline_color(
                    child,
                    ColorKind::Underline,
                    rgb(101.0, 102.0, 103.0),
                    "Change to theme color SHOULD NOT STILL BE 24-bit RGBA ANSI color code (101,102,103) should add matching color inline style.",
                    false,
                );
            }),
            Box::new(|child| {
                expect(Some(1), &[UL_COLORED], &[])(child);
                inline(child, ColorKind::Underline, rgb(104.0, 105.0, 106.0));
            }),
            Box::new(|child| {
                expect(Some(1), &[UL_COLORED], &[])(child);
                // changed to 8bit advanced color, don't know exactly what it should be, but it should NO LONGER BE 104,105,106
                assert_inline_color(
                    child,
                    ColorKind::Underline,
                    rgb(104.0, 105.0, 106.0),
                    "Change to theme color SHOULD NOT BE 24-bit RGBA ANSI color code (104,105,106) should add matching color inline style.",
                    false,
                );
            }),
            Box::new(|child| {
                expect(Some(1), &[UL_COLORED], &[])(child);
                inline(child, ColorKind::Underline, rgb(200.0, 200.0, 200.0));
            }),
            Box::new(|child| {
                expect(Some(0), &[], &[])(child);
                assert_inline_color(child, ColorKind::Underline, None, "Incorrect color style found", true);
            }),
        ],
        Some(6),
    );

    // Different types of color codes still cancel each other
    assert_multiple_sequence_elements(
        &setup,
        "\x1b[34msimple\x1b[38;2;101;102;103m24bit\x1b[38;5;3m8bitsimple\x1b[38;2;104;105;106m24bitAgain\x1b[38;5;101m8bitadvanced",
        vec![
            expect(Some(1), &[FG], &[]),
            Box::new(|child| {
                expect(Some(1), &[FG], &[])(child);
                inline(child, ColorKind::Foreground, rgb(101.0, 102.0, 103.0));
            }),
            Box::new(|child| {
                expect(Some(1), &[FG], &[])(child);
                //color is theme based, so we can't check what it should be but we know it should NOT BE 101,102,103 anymore
                assert_inline_color(
                    child,
                    ColorKind::Foreground,
                    rgb(101.0, 102.0, 103.0),
                    "SHOULD NOT LONGER BE 24-bit RGBA ANSI color code (101,102,103) after simple color change.",
                    false,
                );
            }),
            Box::new(|child| {
                expect(Some(1), &[FG], &[])(child);
                inline(child, ColorKind::Foreground, rgb(104.0, 105.0, 106.0));
            }),
            Box::new(|child| {
                expect(Some(1), &[FG], &[])(child);
                // color should NO LONGER BE 104,105,106
                assert_inline_color(
                    child,
                    ColorKind::Foreground,
                    rgb(104.0, 105.0, 106.0),
                    "SHOULD NOT LONGER BE 24-bit RGBA ANSI color code (104,105,106) after advanced color change.",
                    false,
                );
            }),
        ],
        Some(5),
    );
    setup.teardown();
}

/// Assert that the provided ANSI sequence exactly matches the text content of the resulting span.
fn assert_sequence_strict_equal_to_content(setup: &Setup, sequence: &str) {
    let child = get_sequence_output(setup, sequence);
    assert!(child.text_content() == sequence);
}

#[test]
fn invalid_codes_treated_as_regular_text() {
    let setup = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Individual components of ANSI code start are printed
    assert_sequence_strict_equal_to_content(&setup, "\x1b");
    assert_sequence_strict_equal_to_content(&setup, "[");

    // Unsupported sequence prints both characters
    assert_sequence_strict_equal_to_content(&setup, "\x1b[");

    // Random strings are displayed properly
    for _ in 0..50 {
        let uuid = generate_uuid();
        assert_sequence_strict_equal_to_content(&setup, &uuid);
    }
    setup.teardown();
}

/// Assert that a given ANSI sequence maintains added content following the ANSI code, and that the expression itself is
/// thrown away.
fn assert_empty_output(setup: &Setup, sequence: &str) {
    let child = get_sequence_output(setup, &format!("{sequence}content"));
    assert_eq!("content", child.text_content());
    assert_eq!(0, class_list(&child).len());
}

#[test]
fn empty_sequence_output() {
    let setup = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sequences = [
        // No colour codes
        "",
        "\x1b[;m",
        "\x1b[1;;m",
        "\x1b[m",
        "\x1b[99m",
    ];
    for sequence in sequences {
        assert_empty_output(&setup, sequence);
    }

    // Check other possible ANSI terminators
    for terminator in "ABCDHIJKfhmpsu".chars() {
        assert_empty_output(&setup, &format!("\x1b[content{terminator}"));
    }
    setup.teardown();
}

#[test]
fn calc_ansi_8bit_color_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Invalid values
    // Negative (below range), simple range, decimals
    let mut i = -10.0;
    while i <= 15.0 {
        assert!(calc_ansi_8bit_color(i).is_none());
        i += 0.5;
    }

    // In-range range decimals
    let mut i = 16.5;
    while i < 254.0 {
        assert!(calc_ansi_8bit_color(i).is_none());
        i += 1.0;
    }

    // Above range
    let mut i = 256.0;
    while i < 300.0 {
        assert!(calc_ansi_8bit_color(i).is_none());
        i += 0.5;
    }

    // All valid colors
    let round = |value: f64| (value + 0.5).floor();
    for red in 0..=5 {
        for green in 0..=5 {
            for blue in 0..=5 {
                let color_out = calc_ansi_8bit_color(f64::from(16 + red * 36 + green * 6 + blue)).unwrap();
                assert_eq!(f64::from(color_out.r), round(f64::from(red) * (255.0 / 5.0)));
                assert_eq!(f64::from(color_out.g), round(f64::from(green) * (255.0 / 5.0)));
                assert_eq!(f64::from(color_out.b), round(f64::from(blue) * (255.0 / 5.0)));
            }
        }
    }

    // All grays
    for i in 232..=255 {
        let gray_out = calc_ansi_8bit_color(f64::from(i)).unwrap();
        assert!(gray_out.r == gray_out.g);
        assert!(gray_out.r == gray_out.b);
        assert_eq!(f64::from(gray_out.r), round(f64::from(i - 232) / 23.0 * 255.0));
    }
}
