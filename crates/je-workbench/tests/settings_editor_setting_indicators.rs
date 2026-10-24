//! Porte de `src/vs/workbench/contrib/preferences/test/browser/settingsEditorSettingIndicators.test.ts` (suíte
//! `SettingsTreeIndicatorsLabel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso; o `store` é o [`LeakGuard`].
//! - O `IHoverService` parcial (`upcastPartial<IHoverService>({ setupDelayedHover })` e o objeto `hoverService as never`) é o
//!   [`RecordingHoverService`], que implementa o [`IHoverDelegate2`] e só responde ao `setup_delayed_hover`; o resto do
//!   serviço faz o que o delegado base nulo faz.
//! - `document.createElement('div')` é um nó da árvore do `je-ui`, e `container.textContent` é o texto dos nós da
//!   subárvore, na ordem do documento.
//! - O `SettingsTreeSettingElement` parcial é o [`PartialElement`], que implementa o [`SettingIndicatorElement`]: o campo que o
//!   original não define (`undefined`) falha se for lido, como o `undefined` falharia; `element.policyValueDescription =
//!   undefined` é [`PartialElement::policy_value_description`] trocado por `None`.
//! - Os serviços `undefined!` do construtor (configuração, sincronização, idioma e comandos) são `None`, o `Emitter`
//!   de `ISettingOverrideClickEvent` e o de `string` são os mesmos do original.
//! - `assert.deepStrictEqual` do objeto literal é o `assert_eq!` sobre o [`Outcome`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::event::Emitter;
use je_base::lifecycle::{DisposableRef, IDisposable, none, to_disposable};
use je_platform::configuration::registry::ConfigurationDefaultValueSource;
use je_render::Rgba;
use je_ui::{Content, NodeId, Role, Ui};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::hover_api::{
    HoverContent, HoverTarget, IDelayedHoverOptions, IHoverDelegate, IHoverDelegate2, IHoverLifecycleOptions, IHoverOptions, IHoverWidget, IManagedHover,
    IManagedHoverOptions, ManagedHoverContentOrFactory,
};
use je_workbench::contrib::preferences::preferences_widgets::SettingsTarget;
use je_workbench::contrib::preferences::settings_editor_setting_indicators::{ISettingOverrideClickEvent, SettingIndicatorElement, SettingsTreeIndicatorsLabel};

type SetupDelayedHover = Box<dyn Fn(NodeId, &Rc<dyn Fn() -> IDelayedHoverOptions>) -> DisposableRef>;

struct NullManagedHover;

impl IDisposable for NullManagedHover {
    fn dispose(&self) {}
}

impl IManagedHover for NullManagedHover {
    fn show(&self, _focus: bool) {}
    fn hide(&self) {}
    fn update(&self, _tooltip: ManagedHoverContentOrFactory, _options: Option<IManagedHoverOptions>) {}
}

struct RecordingHoverService {
    setup_delayed_hover: SetupDelayedHover,
}

impl IHoverDelegate2 for RecordingHoverService {
    fn show_delayed_hover(&self, _: IDelayedHoverOptions, _: HoverTarget, _: IHoverLifecycleOptions) -> Option<Rc<dyn IHoverWidget>> {
        None
    }
    fn setup_delayed_hover(&self, target: NodeId, options: Rc<dyn Fn() -> IDelayedHoverOptions>, _: Option<IHoverLifecycleOptions>) -> DisposableRef {
        (self.setup_delayed_hover)(target, &options)
    }
    fn setup_delayed_hover_at_mouse(&self, _: NodeId, _: Rc<dyn Fn() -> IDelayedHoverOptions>, _: Option<IHoverLifecycleOptions>) -> DisposableRef {
        none()
    }
    fn show_instant_hover(&self, _: IHoverOptions, _: bool) -> Option<Rc<dyn IHoverWidget>> {
        None
    }
    fn hide_hover(&self, _: bool) {}
    fn show_and_focus_last_hover(&self) {}
    fn setup_managed_hover(&self, _: Rc<dyn IHoverDelegate>, _: NodeId, _: ManagedHoverContentOrFactory, _: Option<IManagedHoverOptions>) -> Rc<dyn IManagedHover> {
        Rc::new(NullManagedHover)
    }
    fn show_managed_hover(&self, _: NodeId) {}
}

/// `upcastPartial<SettingsTreeSettingElement>({ ... })`: só os campos que o teste define.
#[derive(Default)]
struct PartialElement {
    has_policy_value: bool,
    policy_value_description: RefCell<Option<String>>,
    tags: Option<HashSet<String>>,
}

fn undefined<T>(field: &str) -> T {
    panic!("Cannot read properties of undefined (reading '{field}')")
}

impl SettingIndicatorElement for PartialElement {
    fn tags(&self) -> Option<HashSet<String>> {
        self.tags.clone()
    }
    fn is_untrusted(&self) -> bool {
        false
    }
    fn has_policy_value(&self) -> bool {
        self.has_policy_value
    }
    fn policy_value_description(&self) -> Option<String> {
        self.policy_value_description.borrow().clone()
    }
    fn is_agents_window_read_only(&self) -> bool {
        false
    }
    fn settings_target(&self) -> SettingsTarget {
        undefined("settingsTarget")
    }
    fn setting_key(&self) -> String {
        undefined("setting")
    }
    fn is_configured(&self) -> bool {
        false
    }
    fn overridden_scope_list(&self) -> Vec<String> {
        undefined("overriddenScopeList")
    }
    fn overridden_defaults_language_list(&self) -> Vec<String> {
        undefined("overriddenDefaultsLanguageList")
    }
    fn default_value_source(&self) -> Option<ConfigurationDefaultValueSource> {
        None
    }
}

fn styles() -> HighlightedLabelStyles {
    HighlightedLabelStyles {
        foreground: Rgba::opaque(204, 204, 204),
        highlight_foreground: None,
        highlight_weight: 400,
        font_size: 13.0,
        font_weight: 400,
        line_height: Some(18.0),
        italic: false,
        strikethrough: false,
    }
}

/// `document.createElement('div')`.
fn create_container(ui: &Ui) -> NodeId {
    let mut tree = ui.tree_mut();
    let root = tree.root();
    tree.create(root, Role::Generic)
}

/// `container.textContent`.
fn text_content(ui: &Ui, node: NodeId) -> String {
    let tree = ui.tree();
    tree.subtree(node)
        .into_iter()
        .filter_map(|n| match &tree.style(n).content {
            Content::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    specific: Option<String>,
    fallback: Option<String>,
    indicator: bool,
}

#[test]
fn uses_a_setting_specific_policy_explanation_with_the_existing_managed_indicator() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let descriptions: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let recorded = descriptions.clone();
    let hover_service = Rc::new(RecordingHoverService {
        setup_delayed_hover: Box::new(move |_target, options| {
            if let HoverContent::Text(content) = options().content {
                recorded.borrow_mut().push(content);
            }
            none()
        }),
    });
    let ui = Ui::new();
    let container = create_container(&ui);
    let label = store.add_rc(Rc::new(SettingsTreeIndicatorsLabel::new(&ui, styles(), container, None, hover_service, None, None, None)));
    let clicks = store.add_rc(Emitter::<ISettingOverrideClickEvent>::new());
    let filters = store.add_rc(Emitter::<String>::new());
    let element = PartialElement { has_policy_value: true, policy_value_description: RefCell::new(Some("Organization default explanation".to_owned())), ..PartialElement::default() };
    label.update_scope_overrides(&element, &clicks, &filters);
    let specific = descriptions.borrow().last().cloned();
    *element.policy_value_description.borrow_mut() = None;
    label.update_scope_overrides(&element, &clicks, &filters);
    assert_eq!(
        Outcome { specific, fallback: descriptions.borrow().last().cloned(), indicator: text_content(&ui, container).contains("Managed by organization") },
        Outcome {
            specific: Some("Organization default explanation".to_owned()),
            fallback: Some("This setting is managed by your organization and its actual value cannot be changed.".to_owned()),
            indicator: true,
        }
    );
}

#[test]
fn replaces_the_preview_hover_when_updated() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let hover_disposables: Rc<RefCell<Vec<Rc<Cell<bool>>>>> = Rc::new(RefCell::new(Vec::new()));
    let recorded = hover_disposables.clone();
    let hover_service = Rc::new(RecordingHoverService {
        setup_delayed_hover: Box::new(move |_target, _options| {
            let entry = Rc::new(Cell::new(false));
            recorded.borrow_mut().push(entry.clone());
            to_disposable(move || entry.set(true))
        }),
    });
    let ui = Ui::new();
    let label = SettingsTreeIndicatorsLabel::new(&ui, styles(), create_container(&ui), None, hover_service, None, None, None);

    label.update_preview_indicator(&PartialElement { tags: Some(HashSet::from(["preview".to_owned()])), ..PartialElement::default() });
    let first_hover = hover_disposables.borrow().last().cloned().unwrap();
    label.update_preview_indicator(&PartialElement { tags: Some(HashSet::from(["experimental".to_owned()])), ..PartialElement::default() });
    let second_hover = hover_disposables.borrow().last().cloned().unwrap();

    assert!(first_hover.get());
    assert!(!second_hover.get());

    label.dispose();
    assert!(second_hover.get());
}
